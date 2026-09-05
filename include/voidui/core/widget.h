#pragma once

#include <bitset>
#include <concepts>
#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

#include <string>

#include "voidui/core/event.h"
#include "voidui/core/layout.h"
#include "voidui/core/selection.h"
#include "voidui/core/style.h"
#include "voidui/core/style/declaration.h"
#include "voidui/paint/painter.h"

#define VOIDUI_FLUENT_METHOD(NAME, PARAMS, ...)                                \
  auto &NAME PARAMS & {                                                        \
    __VA_ARGS__                                                                \
    return *this;                                                              \
  }                                                                            \
                                                                               \
  auto &&NAME PARAMS && {                                                      \
    __VA_ARGS__                                                                \
    return std::move(*this);                                                   \
  }

#define VOIDUI_FLUENT_SETTER(NAME, MEMBER, ...)                                \
  VOIDUI_FLUENT_METHOD(NAME, (__VA_ARGS__ value), MEMBER = std::move(value);)

/// Maps the C++ sizing API to the same inline style layer used by VSS.
#define VOIDUI_WIDGET_SIZE_STYLE                                               \
  VOIDUI_FLUENT_METHOD(size, (Size<Length> value),                             \
                       set_style<styles::Width>(std::move(value.width));       \
                       set_style<styles::Height>(std::move(value.height));)    \
  VOIDUI_FLUENT_METHOD(width, (Length value),                                  \
                       set_style<styles::Width>(std::move(value));)            \
  VOIDUI_FLUENT_METHOD(height, (Length value),                                 \
                       set_style<styles::Height>(std::move(value));)           \
  VOIDUI_FLUENT_METHOD(margin, (Margin value),                                 \
                       set_style<styles::MarginTop>(value.top);                \
                       set_style<styles::MarginRight>(value.right);            \
                       set_style<styles::MarginBottom>(value.bottom);          \
                       set_style<styles::MarginLeft>(value.left);)

namespace voidui {

/// The visible line an IME is editing, plus the caret's horizontal offset
/// from that line. Backends use it to keep native composition and candidate
/// windows beside the text being entered.
struct TextInputArea {
  Rect<float> rect;
  float cursor = 0.0f;
};

class Registrar;
class LayoutContext;
class StyleSheet;
struct DrawContext;
struct OverlayOptions;

class WidgetKey {
public:
  explicit WidgetKey(std::uint64_t value) : value_(value) {}
  explicit WidgetKey(std::string value) : value_(std::move(value)) {}

  bool operator==(const WidgetKey &) const = default;

  std::size_t hash() const {
    return std::visit(
        []<class T>(const T &value) { return std::hash<T>{}(value); }, value_);
  }

private:
  std::variant<std::uint64_t, std::string> value_;
};

class WidgetStatus {
public:
  inline static uint8_t Hovered = 1 << 0;
  inline static uint8_t Active = 1 << 1;
  inline static uint8_t Focused = 1 << 2;

  bool is_hovered() const { return flags_[0]; }
  bool is_active() const { return flags_[1]; }
  bool is_focused() const { return flags_[2]; }
  bool matches(uint8_t flags) const {
    return (flags_.to_ulong() & flags) == flags;
  }

  /// The raw bits, as selectors test them.
  uint8_t bits() const { return static_cast<uint8_t>(flags_.to_ulong()); }

  WidgetStatus &set_hovered(bool hovered) {
    flags_[0] = hovered;
    return *this;
  }
  WidgetStatus &set_active(bool active) {
    flags_[1] = active;
    return *this;
  }
  WidgetStatus &set_focused(bool focused) {
    flags_[2] = focused;
    return *this;
  }
  WidgetStatus &set(uint8_t flags) {
    flags_ = flags;
    return *this;
  }

private:
  std::bitset<4> flags_;
};

/// The base class for all widgets.
/// The copy constructor and assignment operator are deleted. Widget should
/// only be deep copied by implementing the clone() method.
class Widget {
public:
  Widget() = default;
  virtual ~Widget() = default;

  Widget(const Widget &) = delete;
  Widget &operator=(const Widget &) = delete;

  Widget(Widget &&) noexcept = default;
  Widget &operator=(Widget &&) noexcept = default;

  // C++23 preserves the concrete widget type and value category at every step.
#define VOIDUI_COMMON_STYLE_SETTER(name, property)                             \
  template <typename Self>                                                     \
  Self &&name(this Self &&self, styles::property::Value value) {               \
    self.template set_style<styles::property>(std::move(value));               \
    return std::forward<Self>(self);                                           \
  }
  VOIDUI_COMMON_STYLE_SETTER(position, Position)
  VOIDUI_COMMON_STYLE_SETTER(z_index, ZIndex)
  VOIDUI_COMMON_STYLE_SETTER(left, Left)
  VOIDUI_COMMON_STYLE_SETTER(right, Right)
  VOIDUI_COMMON_STYLE_SETTER(top, Top)
  VOIDUI_COMMON_STYLE_SETTER(bottom, Bottom)
  VOIDUI_COMMON_STYLE_SETTER(visibility, Visibility)
  VOIDUI_COMMON_STYLE_SETTER(pointer_events, PointerEvents)
  VOIDUI_COMMON_STYLE_SETTER(white_space, WhiteSpace)
  VOIDUI_COMMON_STYLE_SETTER(opacity, Opacity)
  VOIDUI_COMMON_STYLE_SETTER(transform, Transform)
  VOIDUI_COMMON_STYLE_SETTER(transition_property, TransitionProperty)
  VOIDUI_COMMON_STYLE_SETTER(transition_duration, TransitionDuration)
  VOIDUI_COMMON_STYLE_SETTER(transition_delay, TransitionDelay)
  VOIDUI_COMMON_STYLE_SETTER(transition_timing_function,
                             TransitionTimingFunction)
#undef VOIDUI_COMMON_STYLE_SETTER

  template <typename Self>
  Self &&inset(this Self &&self, Spacing<Inset> edges) {
    self.template set_style<styles::Top>(edges.top);
    self.template set_style<styles::Right>(edges.right);
    self.template set_style<styles::Bottom>(edges.bottom);
    self.template set_style<styles::Left>(edges.left);
    return std::forward<Self>(self);
  }

  template <typename Self>
  Self &&transition(this Self &&self, TransitionShorthand value) {
    self.template set_style<styles::TransitionProperty>(
        std::move(value.properties));
    self.template set_style<styles::TransitionDuration>(
        std::move(value.durations));
    self.template set_style<styles::TransitionDelay>(std::move(value.delays));
    self.template set_style<styles::TransitionTimingFunction>(
        std::move(value.easings));
    self.template set_style<styles::TransitionBehavior>(
        std::move(value.behaviors));
    return std::forward<Self>(self);
  }

  virtual void register_children(Registrar &registrar) = 0;
  virtual Size<float> layout(Constraints constraints, LayoutContext &ctx) = 0;
  virtual void draw(const DrawContext &ctx, Painter &painter) = 0;
  virtual void draw_foreground(const DrawContext &, Painter &) {}
  virtual EventResult on_event(Event &e) = 0;
  virtual std::unique_ptr<Widget> clone() const = 0;
  virtual bool focusable() const { return false; }
  /// Semantic states supplied by controls, in addition to pointer/focus state.
  virtual std::uint8_t style_status() const { return 0; }
  virtual bool exposes_style_descendants() const { return false; }
  virtual void focus_lost(Event &) {}
  virtual bool clips_children() const { return false; }
  /// Retain a collapsed viewport's children while excluding them from paint,
  /// input, portals and keyboard focus. The viewport can still expose an opener.
  virtual bool children_visible() const { return true; }
  /// Opt-in portal boundary; ordinary widgets pay no per-instance storage.
  virtual const OverlayOptions *overlay_options() const { return nullptr; }
  virtual bool is_flex_container() const { return false; }
  virtual Rect<float> children_clip(Rect<float> bounds) const { return bounds; }
  virtual bool foreground_hit_test(Point<float>, Rect<float>) const {
    return false;
  }

  /// Cold-path hooks for read-only text selection. They reuse Widget's vtable
  /// instead of adding an interface pointer to every Text object.
  virtual bool supports_text_selection() const { return false; }
  virtual std::uint32_t selection_hit_test(Point<float>, Rect<float>) const {
    return 0;
  }
  virtual std::pair<std::uint32_t, std::uint32_t>
  selection_word_at(std::uint32_t) const {
    return {0, 0};
  }
  virtual std::string_view selection_text() const { return {}; }
  virtual void selection_changed(std::uint32_t, std::uint32_t) {}
  virtual std::pair<std::uint32_t, std::uint32_t> text_selection() const {
    return {0, 0};
  }
  /// Optional viewport for selection auto-scroll, in untransformed tree space.
  virtual std::optional<Rect<float>> selection_scroll_viewport(Rect<float>) const {
    return std::nullopt;
  }
  /// Consume a scroll delta in logical pixels; None means an edge was reached.
  virtual Invalidation selection_scroll_by(Point<float>) {
    return Invalidation::None;
  }
  virtual bool accepts_text_input() const { return false; }
  virtual std::optional<TextInputArea> text_input_area(Rect<float>) const {
    return std::nullopt;
  }
  virtual void text_input_stopped() {}

  /// Carries runtime-only data from the previous declaration of the same
  /// concrete widget type. Most widgets keep all runtime state in Node and do
  /// not need to override this.
  virtual void inherit_runtime(const Widget &) {}

  // -- Reconciliation -------------------------------------------------------
  template <typename Self> Self &&key(this Self &&self, std::string value) {
    self.reconciliation_key_ = std::make_unique<WidgetKey>(std::move(value));
    return std::forward<Self>(self);
  }

  template <typename Self, std::integral T>
  Self &&key(this Self &&self, T value) {
    self.reconciliation_key_ =
        std::make_unique<WidgetKey>(static_cast<std::uint64_t>(value));
    return std::forward<Self>(self);
  }

  const WidgetKey *reconciliation_key() const {
    return reconciliation_key_.get();
  }

  // -- Styling
  // -------------------------------------------------------------

  /// Style classes, as `.name` in a stylesheet.
  template <typename Self>
  Self &&classes(this Self &&self, std::vector<std::string> classes) {
    self.style_classes_ = std::move(classes);
    return std::forward<Self>(self);
  }

  template <typename Self>
  Self &&add_class(this Self &&self, std::string name) {
    self.style_classes_.push_back(std::move(name));
    return std::forward<Self>(self);
  }

  template <typename Self> Self &&id(this Self &&self, std::string value) {
    self.style_id_ = std::move(value);
    return std::forward<Self>(self);
  }

  const std::vector<std::string> &style_classes() const {
    return style_classes_;
  }
  const std::string &style_id() const { return style_id_; }

  /// Rules this widget class ships with. Unlike a single declaration, a
  /// stylesheet can style states and descendants (`button:hover`,
  /// `button text`, ...). The resolver always places these rules at the
  /// WidgetDefault origin, beneath themes and application styles.
  virtual std::shared_ptr<const StyleSheet> default_stylesheet() const {
    return {};
  }

  /// Values set on this instance alone, above every stylesheet rule. This is
  /// what a fluent setter such as `padding(...)` writes into, so that setting
  /// a property in C++ and setting it in a stylesheet go through one cascade
  /// rather than two competing mechanisms.
  template <class P> void set_style(typename P::Value value) {
    inline_style_.set<P>(std::move(value));
  }

  /// Null when nothing was set, so the resolver can skip the step entirely.
  const StyleDeclaration *inline_style() const {
    return inline_style_.empty() ? nullptr : &inline_style_;
  }

  /// Copies styling and reconciliation identity onto a clone.
  ///
  /// clone() is implemented per widget and only ever copies that widget's own
  /// fields, so without this the base class's identity would be lost every
  /// time a widget was passed by lvalue. The resolved style is deliberately
  /// not copied: it belongs to a node in a tree, not to a widget in the air.
  void copy_style_identity_from(const Widget &other) {
    style_classes_ = other.style_classes_;
    style_id_ = other.style_id_;
    inline_style_ = other.inline_style_;
    reconciliation_key_ =
        other.reconciliation_key_
            ? std::make_unique<WidgetKey>(*other.reconciliation_key_)
            : nullptr;
  }

private:
  std::vector<std::string> style_classes_;
  std::string style_id_;
  StyleDeclaration inline_style_;
  std::unique_ptr<WidgetKey> reconciliation_key_;
};

class Registrar {
public:
  Registrar(std::vector<std::unique_ptr<Widget>> &children,
            std::vector<std::unique_ptr<Widget>> &internal_children,
            std::vector<std::string> &internal_parts)
      : children_(children), internal_children_(internal_children),
        internal_parts_(internal_parts) {}

  void take_child(std::unique_ptr<Widget> child) {
    children_.push_back(std::move(child));
  }

  /// An internal child is invisible to outside selectors. Naming it exposes it
  /// as `host::part(name)` -- the only way through the boundary, and a
  /// deliberate part of the component's public surface.
  void take_internal_child(std::unique_ptr<Widget> child,
                           std::string part_name = {}) {
    internal_children_.push_back(std::move(child));
    internal_parts_.push_back(std::move(part_name));
  }

private:
  std::vector<std::unique_ptr<Widget>> &children_;
  std::vector<std::unique_ptr<Widget>> &internal_children_;
  std::vector<std::string> &internal_parts_;
};

template <class T>
concept WidgetClass = std::derived_from<std::remove_cvref_t<T>, Widget>;

/// Deep-copies a widget, identity included.
///
/// Always prefer this to calling clone() directly: clone() is the per-widget
/// half of the copy, this is the whole of it.
inline std::unique_ptr<Widget> clone_widget(const Widget &widget) {
  std::unique_ptr<Widget> copy = widget.clone();
  if (copy)
    copy->copy_style_identity_from(widget);
  return copy;
}

/// Transfers a widget declaration into a unique_ptr<Widget>. If the widget is
/// an lvalue reference, it will be cloned. If it is an rvalue reference, it
/// will be moved into a new unique_ptr<Widget>.
template <WidgetClass T> std::unique_ptr<Widget> transfer_widget(T &&widget) {
  using Concrete = std::remove_cvref_t<T>;

  if constexpr (std::is_lvalue_reference_v<T &&>) {
    return clone_widget(widget);
  } else {
    return std::make_unique<Concrete>(std::forward<T>(widget));
  }
}

} // namespace voidui
