#pragma once

#include <cstdint>
#include <deque>
#include <mutex>
#include <string>
#include <string_view>
#include <typeindex>
#include <unordered_map>
#include <vector>

#include "voidui/core/invalidation.h"
#include "voidui/core/style/value.h"

namespace voidui {

/// A dense, process-wide index for a style property.
///
/// Indices are handed out in registration order and are therefore not stable
/// across runs -- they are never serialised. The canonical, stable identity of
/// a property is its name; the index exists so the hot paths (cascade merge,
/// ComputedStyle lookup) compare small integers instead of strings.
using PropertyIndex = std::uint32_t;

inline constexpr PropertyIndex kInvalidPropertyIndex =
    static_cast<PropertyIndex>(-1);

/// Everything the framework needs to know about a property it has never heard
/// of. Filled in by StyleProperty::index() from the tag type, so a component
/// author writes a declaration and gets all of this for free.
struct PropertyDescriptor {
  /// Canonical name: "background" for a framework property, or
  /// "input.caret-color" for one owned by a component. The scope prefix is
  /// what keeps two unrelated third-party libraries from colliding.
  std::string name;

  std::type_index tag = std::type_index(typeid(void));
  std::type_index value_type = std::type_index(typeid(void));

  /// Whether an unset value is taken from the parent node. Only a handful of
  /// properties should say yes -- text colour, font, and the like.
  bool inherited = false;

  /// The least work needed when the effective value changes.
  Invalidation invalidation = Invalidation::Paint;

  PropertyValue (*make_default)() = nullptr;

  /// Null when the value type has no parse_style_value() overload. Such a
  /// property still works from C++; a stylesheet naming it gets a diagnostic.
  bool (*parse)(std::string_view text, PropertyValue &out) = nullptr;

  /// Null for discrete values. Interpolators write into caller-owned storage,
  /// which lets active animations reuse their PropertyValue allocations.
  bool (*interpolate)(const PropertyValue &from, const PropertyValue &to,
                      float progress, PropertyValue &out) = nullptr;
};

/// The open set of style properties.
///
/// This is the piece that lets a component author outside the framework define
/// new styleable properties: registration happens from their own translation
/// unit, keyed by name, and the framework never needs a central list.
class PropertyRegistry {
public:
  static PropertyRegistry &instance();

  /// Idempotent by name. Registering the same name twice with the same value
  /// type returns the original index, which is what makes the registry safe
  /// when a header is instantiated in several translation units or DLLs where
  /// type_index identities may not be shared.
  PropertyIndex register_property(PropertyDescriptor descriptor);

  /// kInvalidPropertyIndex when unknown. Callers report and skip rather than
  /// fail -- an unknown property usually means a plugin is not loaded.
  PropertyIndex find(std::string_view name) const;

  const PropertyDescriptor &describe(PropertyIndex index) const;

  std::size_t size() const;

  /// Snapshot of every registered name, for diagnostics and tooling.
  std::vector<std::string> names() const;

private:
  PropertyRegistry() = default;

  mutable std::mutex mutex_;
  // describe() returns references retained across third-party registration.
  std::deque<PropertyDescriptor> descriptors_;
  std::unordered_map<std::string, PropertyIndex> by_name_;
};

/// Maps a widget type onto the name selectors use for it, in both directions.
class WidgetTypeRegistry {
public:
  static WidgetTypeRegistry &instance();

  void register_type(std::string name, std::type_index type);

  /// Null when no widget by that name has been registered.
  const std::type_index *find(std::string_view name) const;

  std::string_view name_of(std::type_index type) const;

private:
  WidgetTypeRegistry() = default;

  mutable std::mutex mutex_;
  std::unordered_map<std::string, std::type_index> by_name_;
  std::unordered_map<std::type_index, std::string> by_type_;
};

inline constexpr bool inherit_Inherited = true;
inline constexpr bool inherit_NotInherited = false;

/// Base for a style property tag.
///
/// `Self` is the tag type itself (CRTP), which is how index() reaches the
/// scope name, local name and default value the macros generate.
template <class Self, class T, bool Inherit = false,
          Invalidation Invalidates = Invalidation::Paint>
struct StyleProperty {
  using Value = T;

  static constexpr bool is_inherited = Inherit;

  /// Registers on first call and caches the result. A function-local static,
  /// not a global, so there is no static initialisation order problem.
  static PropertyIndex index() {
    static const PropertyIndex resolved = register_();
    return resolved;
  }

  static const T &default_value_ref() {
    static const T value = Self::default_value();
    return value;
  }

  static std::string canonical_name() {
    std::string_view scope = Self::style_scope_name();
    if (scope.empty())
      return std::string(Self::local_name);
    std::string name(scope);
    name += '.';
    name += Self::local_name;
    return name;
  }

private:
  static PropertyIndex register_() {
    PropertyDescriptor descriptor;
    descriptor.name = canonical_name();
    descriptor.tag = std::type_index(typeid(Self));
    descriptor.value_type = std::type_index(typeid(T));
    descriptor.inherited = Inherit;
    descriptor.invalidation = Invalidates;
    descriptor.make_default = []() {
      return PropertyValue(Self::default_value());
    };
    if constexpr (StyleParseable<T>) {
      descriptor.parse = [](std::string_view text, PropertyValue &out) {
        T value{Self::default_value()};
        if (!parse_style_value(text, value))
          return false;
        out = PropertyValue(std::move(value));
        return true;
      };
    }
    if constexpr (requires(const T &from, const T &to, float progress, T &out) {
                    interpolate_style_value(from, to, progress, out);
                  }) {
      descriptor.interpolate = [](const PropertyValue &from,
                                  const PropertyValue &to, float progress,
                                  PropertyValue &out) {
        const T *a = from.as<T>();
        const T *b = to.as<T>();
        if (!a || !b)
          return false;
        // Seed from the source so interpolation also works for deliberately
        // non-default-constructible value types such as Brush.
        T value{*a};
        if (!interpolate_style_value(*a, *b, progress, value))
          return false;
        out = PropertyValue(std::move(value));
        return true;
      };
    } else if constexpr (requires(const T &from, const T &to, float progress) {
                           {
                             interpolate_style_value(from, to, progress)
                           } -> std::same_as<T>;
                         }) {
      descriptor.interpolate = [](const PropertyValue &from,
                                  const PropertyValue &to, float progress,
                                  PropertyValue &out) {
        const T *a = from.as<T>();
        const T *b = to.as<T>();
        if (!a || !b)
          return false;
        out = PropertyValue(interpolate_style_value(*a, *b, progress));
        return true;
      };
    }
    return PropertyRegistry::instance().register_property(
        std::move(descriptor));
  }
};

/// Forces a property to register during static initialisation.
///
/// Without this a property would only appear in the registry once some C++ code
/// mentioned its tag, and a stylesheet loaded before that would report the name
/// as unknown. The macros below plant one of these per property so a component
/// author never has to think about it.
struct PropertyWarmup {
  explicit PropertyWarmup(PropertyIndex (*registrar)()) { registrar(); }
};

struct WidgetTypeWarmup {
  WidgetTypeWarmup(const char *name, std::type_index type) {
    WidgetTypeRegistry::instance().register_type(name, type);
  }
};

} // namespace voidui

/// Declares the selector name and property namespace of a widget.
///
///   class Input : public Widget {
///   public:
///     VOIDUI_STYLE_SCOPE(Input, "input")
///     ...
///   };
///
/// Gives `input { ... }` in a stylesheet and prefixes this widget's properties
/// with `input.`.
#define VOIDUI_STYLE_SCOPE(WidgetType, ScopeName)                              \
  static const char *style_scope_name() { return ScopeName; }                  \
  static ::voidui::PropertyIndex voidui_register_scope_() {                    \
    ::voidui::WidgetTypeRegistry::instance().register_type(                    \
        ScopeName, std::type_index(typeid(WidgetType)));                       \
    return 0;                                                                  \
  }                                                                            \
  inline static const ::voidui::PropertyWarmup voidui_scope_warmup_{           \
      &voidui_register_scope_};

/// Declares one styleable property inside a widget class.
///
///   VOIDUI_STYLE_PROPERTY(Input, CaretColor, Color, "caret-color",
///                         NotInherited, Paint, Color(0, 0, 0));
///
/// Produces the tag type `Input::CaretColor` for type-safe C++ access, the
/// text name `input.caret-color` for stylesheets, the default value, the
/// inheritance and invalidation flags, and static-init registration -- from
/// one line.
///
/// `Owner` is named explicitly because the tag is a nested class: its member
/// function bodies are parsed once the enclosing class is complete, so the
/// scope name can be reached, but only through a qualified call.
#define VOIDUI_STYLE_PROPERTY(Owner, Tag, ValueType, TextName, Inheritance,    \
                              InvalidationKind, ...)                           \
  struct Tag                                                                   \
      : ::voidui::StyleProperty<Tag, ValueType,                                \
                                ::voidui::inherit_##Inheritance,               \
                                ::voidui::Invalidation::InvalidationKind> {    \
    static constexpr const char *local_name = TextName;                        \
    static ValueType default_value() { return __VA_ARGS__; }                   \
    static const char *style_scope_name() {                                    \
      return Owner::style_scope_name();                                        \
    }                                                                          \
  };                                                                           \
  inline static const ::voidui::PropertyWarmup voidui_warmup_##Tag {           \
    &Tag::index                                                                \
  }

/// Declares a property that belongs to the framework itself rather than to a
/// widget, at namespace scope. Its canonical name has no prefix.
#define VOIDUI_GLOBAL_STYLE_PROPERTY(Tag, ValueType, TextName, Inheritance,    \
                                     InvalidationKind, ...)                    \
  struct Tag                                                                   \
      : ::voidui::StyleProperty<Tag, ValueType,                                \
                                ::voidui::inherit_##Inheritance,               \
                                ::voidui::Invalidation::InvalidationKind> {    \
    static constexpr const char *local_name = TextName;                        \
    static const char *style_scope_name() { return ""; }                       \
    static ValueType default_value() { return __VA_ARGS__; }                   \
  };                                                                           \
  inline const ::voidui::PropertyWarmup voidui_warmup_##Tag { &Tag::index }
