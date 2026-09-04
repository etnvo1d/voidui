#pragma once

#include <algorithm>
#include <concepts>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <new>
#include <string>
#include <string_view>
#include <type_traits>
#include <typeindex>
#include <utility>

#include "voidui/core/border.h"
#include "voidui/core/color.h"
#include "voidui/core/geometry.h"
#include "voidui/core/layout.h"
#include "voidui/core/typography.h"
#include "voidui/render/brush.h"

namespace voidui {

/// 64-bit mixing step, used everywhere a style hash is folded together.
constexpr std::uint64_t style_hash_combine(std::uint64_t seed,
                                           std::uint64_t value) noexcept {
  value *= 0xff51afd7ed558ccdULL;
  value ^= value >> 32;
  seed ^= value + 0x9e3779b97f4a7c15ULL + (seed << 6) + (seed >> 2);
  return seed;
}

inline std::uint64_t style_hash_bytes(const void *data,
                                      std::size_t size) noexcept {
  const auto *bytes = static_cast<const unsigned char *>(data);
  std::uint64_t hash = 0xcbf29ce484222325ULL;
  for (std::size_t i = 0; i < size; ++i) {
    hash ^= static_cast<std::uint64_t>(bytes[i]);
    hash *= 0x100000001b3ULL;
  }
  return hash;
}

// -- Customisation points ----------------------------------------------------
//
// A value type used in a style property must answer three questions: are two
// of me equal, what is my hash, and how do I read myself out of text. All
// three are found by ADL, so a component author living in their own namespace
// can supply them next to their type without touching this header.
//
// Equality and hashing are what make ComputedStyle de-duplication and cheap
// repaint diffing possible; parsing is what lets the value appear in a .vss
// file. A type that never appears in text may leave the parser out -- the
// property is then simply reported as unparseable if a stylesheet names it.

template <class T>
concept StyleEqualityComparable = requires(const T &a, const T &b) {
  { a == b } -> std::convertible_to<bool>;
};

namespace style_detail {

template <class T> bool default_equals(const T &a, const T &b) {
  if constexpr (StyleEqualityComparable<T>) {
    return a == b;
  } else if constexpr (std::is_trivially_copyable_v<T>) {
    return std::memcmp(&a, &b, sizeof(T)) == 0;
  } else {
    static_assert(sizeof(T) == 0,
                  "style value type needs operator== or a style_value_equals() "
                  "overload found by ADL");
    return false;
  }
}

template <class T> std::uint64_t default_hash(const T &value) {
  if constexpr (std::is_trivially_copyable_v<T>) {
    return style_hash_bytes(&value, sizeof(T));
  } else {
    static_assert(sizeof(T) == 0,
                  "style value type needs a style_value_hash() overload found "
                  "by ADL");
    return 0;
  }
}

} // namespace style_detail

// -- Built-in value types ----------------------------------------------------

inline bool style_value_equals(const Color &a, const Color &b) { return a == b; }

/// Hashed field by field rather than as bytes: Color carries a tag after four
/// floats and therefore trailing padding, whose contents are not guaranteed to
/// be anything.
inline std::uint64_t style_value_hash(const Color &color) {
  const auto channel = [](float value) {
    const float canonical = value == 0.0f ? 0.0f : value;
    return style_hash_bytes(&canonical, sizeof(canonical));
  };
  std::uint64_t seed = static_cast<std::uint64_t>(color.space);
  seed = style_hash_combine(seed, channel(color.r));
  seed = style_hash_combine(seed, channel(color.g));
  seed = style_hash_combine(seed, channel(color.b));
  return style_hash_combine(seed, channel(color.a));
}

inline std::uint64_t
style_value_hash(const ColorInterpolationMethod &method) {
  std::uint64_t seed = static_cast<std::uint64_t>(method.space);
  seed = style_hash_combine(seed, static_cast<std::uint64_t>(method.hue));
  return style_hash_combine(seed,
                            static_cast<std::uint64_t>(method.specified));
}

inline bool style_value_equals(const LinearGradient &a,
                               const LinearGradient &b) {
  return a == b;
}

inline std::uint64_t style_value_hash(const LinearGradient &gradient) {
  const auto hash_float = [](float value) {
    const float canonical = value == 0.0f ? 0.0f : value;
    return style_hash_bytes(&canonical, sizeof(canonical));
  };
  std::uint64_t seed = static_cast<std::uint64_t>(gradient.geometry());
  const Point<float> start = gradient.start();
  const Point<float> end = gradient.end();
  seed = style_hash_combine(seed, hash_float(start.x));
  seed = style_hash_combine(seed, hash_float(start.y));
  seed = style_hash_combine(seed, hash_float(end.x));
  seed = style_hash_combine(seed, hash_float(end.y));
  const float angle = gradient.angle();
  seed = style_hash_combine(seed, hash_float(angle));
  seed = style_hash_combine(seed,
                            static_cast<std::uint64_t>(gradient.direction()));
  seed = style_hash_combine(seed, style_value_hash(gradient.interpolation()));
  for (const GradientStop &stop : gradient.stops()) {
    seed = style_hash_combine(seed, style_value_hash(stop.color));
    seed = style_hash_combine(seed, hash_float(stop.position));
    seed = style_hash_combine(seed, static_cast<std::uint64_t>(stop.unit));
  }
  return seed;
}

inline bool style_value_equals(const ConicGradient &a, const ConicGradient &b) {
  return a == b;
}

inline std::uint64_t style_value_hash(const ConicGradient &gradient) {
  const auto hash_float = [](float value) {
    const float canonical = value == 0.0f ? 0.0f : value;
    return style_hash_bytes(&canonical, sizeof(canonical));
  };
  std::uint64_t seed = hash_float(gradient.angle());
  seed = style_hash_combine(seed, style_value_hash(gradient.interpolation()));
  for (const GradientStop &stop : gradient.stops()) {
    seed = style_hash_combine(seed, style_value_hash(stop.color));
    seed = style_hash_combine(seed, hash_float(stop.position));
    seed = style_hash_combine(seed, static_cast<std::uint64_t>(stop.unit));
  }
  return seed;
}

inline bool style_value_equals(const Radius &a, const Radius &b) {
  return a.left_top == b.left_top && a.right_top == b.right_top &&
         a.right_bottom == b.right_bottom && a.left_bottom == b.left_bottom;
}

inline bool style_value_equals(const Length &a, const Length &b) {
  if (a.value.index() != b.value.index())
    return false;
  if (const auto *fixed = std::get_if<Length::Fixed>(&a.value))
    return fixed->value == std::get<Length::Fixed>(b.value).value;
  if (const auto *flex = std::get_if<Length::Flex>(&a.value))
    return flex->value == std::get<Length::Flex>(b.value).value;
  return true;
}

inline std::uint64_t style_value_hash(const Length &length) {
  std::uint64_t seed = static_cast<std::uint64_t>(length.value.index());
  if (const auto *fixed = std::get_if<Length::Fixed>(&length.value)) {
    const float value = fixed->value == 0.0f ? 0.0f : fixed->value;
    return style_hash_combine(seed, style_hash_bytes(&value, sizeof(value)));
  }
  if (const auto *flex = std::get_if<Length::Flex>(&length.value))
    return style_hash_combine(seed, flex->value);
  return seed;
}

inline std::uint64_t style_value_hash(const MarginValue &margin) {
  if (margin.is_auto())
    return 0x9e3779b97f4a7c15ULL;
  const float value =
      margin.fixed_or_zero() == 0.0f ? 0.0f : margin.fixed_or_zero();
  return style_hash_bytes(&value, sizeof(value));
}

template <class T>
bool style_value_equals(const Spacing<T> &a, const Spacing<T> &b) {
  return a.left == b.left && a.top == b.top && a.right == b.right &&
         a.bottom == b.bottom;
}

inline bool style_value_equals(const Brush &a, const Brush &b) {
  if (a.index() != b.index())
    return false;
  if (const auto *color = std::get_if<Color>(&a))
    return style_value_equals(*color, std::get<Color>(b));
  if (const auto *linear = std::get_if<LinearGradient>(&a))
    return style_value_equals(*linear, std::get<LinearGradient>(b));
  return style_value_equals(std::get<ConicGradient>(a),
                            std::get<ConicGradient>(b));
}

inline std::uint64_t style_value_hash(const Brush &brush) {
  std::uint64_t seed = static_cast<std::uint64_t>(brush.index());
  if (const auto *color = std::get_if<Color>(&brush))
    return style_hash_combine(seed, style_value_hash(*color));
  if (const auto *linear = std::get_if<LinearGradient>(&brush))
    return style_hash_combine(seed, style_value_hash(*linear));
  return style_hash_combine(seed,
                            style_value_hash(std::get<ConicGradient>(brush)));
}

inline bool style_value_equals(const std::string &a, const std::string &b) {
  return a == b;
}

inline std::uint64_t style_value_hash(const std::string &text) {
  return style_hash_bytes(text.data(), text.size());
}

inline float interpolate_style_value(float a, float b, float t) {
  return a + (b - a) * t;
}

/// CSS picks the space from the two endpoints -- sRGB while both are legacy
/// colours, Oklab as soon as either is written in a modern syntax -- so an
/// animation between two `#hex` values costs exactly the four lerps it always
/// did, and only a stylesheet that asked for `oklch()` pays for the conversion.
inline Color interpolate_style_value(Color a, Color b, float t) {
  return mix_colors(a, b, t);
}

inline Radius interpolate_style_value(const Radius &a, const Radius &b,
                                      float t) {
  return Radius(interpolate_style_value(a.left_top, b.left_top, t),
                interpolate_style_value(a.right_top, b.right_top, t),
                interpolate_style_value(a.right_bottom, b.right_bottom, t),
                interpolate_style_value(a.left_bottom, b.left_bottom, t));
}

inline Spacing<float> interpolate_style_value(const Spacing<float> &a,
                                              const Spacing<float> &b,
                                              float t) {
  return Spacing<float>(interpolate_style_value(a.left, b.left, t),
                        interpolate_style_value(a.top, b.top, t),
                        interpolate_style_value(a.right, b.right, t),
                        interpolate_style_value(a.bottom, b.bottom, t));
}

inline bool interpolate_style_value(const Length &a, const Length &b, float t,
                                    Length &out) {
  const auto *from = std::get_if<Length::Fixed>(&a.value);
  const auto *to = std::get_if<Length::Fixed>(&b.value);
  if (!from || !to)
    return false;
  out = Length::Fixed{interpolate_style_value(from->value, to->value, t)};
  return true;
}

inline bool interpolate_style_value(const MarginValue &a, const MarginValue &b,
                                    float t, MarginValue &out) {
  if (a.is_auto() || b.is_auto())
    return false;
  out = MarginValue(
      interpolate_style_value(a.fixed_or_zero(), b.fixed_or_zero(), t));
  return true;
}

inline bool interpolate_style_value(const LinearGradient &a,
                                    const LinearGradient &b, float t,
                                    LinearGradient &out) {
  const std::span<const GradientStop> from = a.stops();
  const std::span<const GradientStop> to = b.stops();
  if (a.geometry() != b.geometry() || from.size() != to.size() ||
      !(a.interpolation() == b.interpolation()) ||
      (a.geometry() == LinearGradient::Geometry::CssDirection &&
       a.direction() != b.direction()))
    return false;

  std::array<GradientStop, kMaxGradientStops> stops{
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT)};
  for (std::size_t i = 0; i < from.size(); ++i) {
    if (from[i].unit != to[i].unit)
      return false;
    stops[i].color = interpolate_style_value(from[i].color, to[i].color, t);
    stops[i].unit = from[i].unit;
    stops[i].position =
        from[i].unit == GradientStopUnit::Unspecified
            ? 0.0f
            : interpolate_style_value(from[i].position, to[i].position, t);
  }
  const std::span<const GradientStop> values(stops.data(), from.size());

  if (a.geometry() == LinearGradient::Geometry::CssAngle) {
    out = LinearGradient::css_angle(
              interpolate_style_value(a.angle(), b.angle(), t), values)
              .with_interpolation(a.interpolation());
    return true;
  }
  if (a.geometry() == LinearGradient::Geometry::CssDirection) {
    out = LinearGradient::css_direction(a.direction(), values)
              .with_interpolation(a.interpolation());
    return true;
  }

  const Point<float> a_start = a.start();
  const Point<float> b_start = b.start();
  const Point<float> a_end = a.end();
  const Point<float> b_end = b.end();
  out = LinearGradient(
            Point<float>(interpolate_style_value(a_start.x, b_start.x, t),
                         interpolate_style_value(a_start.y, b_start.y, t)),
            Point<float>(interpolate_style_value(a_end.x, b_end.x, t),
                         interpolate_style_value(a_end.y, b_end.y, t)),
            values)
            .with_interpolation(a.interpolation());
  return true;
}

inline bool interpolate_style_value(const ConicGradient &a,
                                    const ConicGradient &b, float t,
                                    ConicGradient &out) {
  const std::span<const GradientStop> from = a.stops();
  const std::span<const GradientStop> to = b.stops();
  if (from.size() != to.size() || !(a.interpolation() == b.interpolation()))
    return false;

  const float angle = interpolate_style_value(a.angle(), b.angle(), t);

  // Only the angle moving is the case this primitive exists for -- a spinner
  // ring -- and `with_angle` copies a shared pointer and a float rather than
  // rebuilding the stops, so it is worth spotting.
  bool same_stops = true;
  for (std::size_t i = 0; i < from.size(); ++i)
    same_stops &= style_value_equals(from[i].color, to[i].color) &&
                  from[i].position == to[i].position &&
                  from[i].unit == to[i].unit;
  if (same_stops) {
    out = a.with_angle(angle);
    return true;
  }

  std::array<GradientStop, kMaxGradientStops> stops{
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT)};
  for (std::size_t i = 0; i < from.size(); ++i) {
    if (from[i].unit != to[i].unit)
      return false;
    stops[i].color = interpolate_style_value(from[i].color, to[i].color, t);
    stops[i].unit = from[i].unit;
    stops[i].position =
        from[i].unit == GradientStopUnit::Unspecified
            ? 0.0f
            : interpolate_style_value(from[i].position, to[i].position, t);
  }

  out = ConicGradient(angle,
                      std::span<const GradientStop>(stops.data(), from.size()))
            .with_interpolation(a.interpolation());
  return true;
}

inline bool interpolate_style_value(const Brush &a, const Brush &b, float t,
                                    Brush &out) {
  if (a.index() != b.index())
    return false;
  if (const auto *from = std::get_if<Color>(&a)) {
    out = interpolate_style_value(*from, std::get<Color>(b), t);
    return true;
  }
  if (const auto *from = std::get_if<LinearGradient>(&a)) {
    LinearGradient value = *from;
    if (!interpolate_style_value(*from, std::get<LinearGradient>(b), t, value))
      return false;
    out = std::move(value);
    return true;
  }
  ConicGradient value;
  if (!interpolate_style_value(std::get<ConicGradient>(a),
                               std::get<ConicGradient>(b), t, value))
    return false;
  out = std::move(value);
  return true;
}

// -- Generic entry points ----------------------------------------------------

template <class T> bool style_equals(const T &a, const T &b) {
  if constexpr (requires { style_value_equals(a, b); }) {
    return style_value_equals(a, b);
  } else {
    return style_detail::default_equals(a, b);
  }
}

template <class T> std::uint64_t style_hash(const T &value) {
  if constexpr (requires { style_value_hash(value); }) {
    return style_value_hash(value);
  } else {
    return style_detail::default_hash(value);
  }
}

// -- Text parsers for the built-in value types -------------------------------

/// Trims ASCII whitespace from both ends.
std::string_view style_trim(std::string_view text);

bool parse_style_value(std::string_view text, float &out);
bool parse_style_value(std::string_view text, bool &out);
bool parse_style_value(std::string_view text, Length &out);
bool parse_style_value(std::string_view text, MarginValue &out);
bool parse_style_value(std::string_view text, Color &out);
bool parse_style_value(std::string_view text, Radius &out);
bool parse_style_value(std::string_view text, Spacing<float> &out);
bool parse_style_value(std::string_view text, Spacing<MarginValue> &out);
bool parse_style_value(std::string_view text, Brush &out);
bool parse_style_value(std::string_view text, std::string &out);

/// True when `T` can be read from a stylesheet.
template <class T>
concept StyleParseable = requires(std::string_view text, T &out) {
  { parse_style_value(text, out) } -> std::same_as<bool>;
};

// -- PropertyValue -----------------------------------------------------------

/// A type-erased style value.
///
/// Deliberately not std::any: the cascade needs equality and hashing, which
/// any cannot provide, and those two operations are what buy the ComputedStyle
/// sharing in resolver.h. Values up to 32 bytes -- which covers every built-in
/// type, gradients and shared_ptr included -- live inline, so resolving a tree
/// performs no allocation at all.
class PropertyValue {
public:
  static constexpr std::size_t kInlineSize = 32;
  static constexpr std::size_t kInlineAlign = alignof(std::max_align_t);

  PropertyValue() noexcept : heap_(nullptr) {}

  template <class T, class Decayed = std::remove_cvref_t<T>,
            class = std::enable_if_t<!std::is_same_v<Decayed, PropertyValue>>>
  explicit PropertyValue(T &&value) : heap_(nullptr) {
    construct_<Decayed>(std::forward<T>(value));
  }

  PropertyValue(const PropertyValue &other) : heap_(nullptr) {
    copy_from_(other);
  }

  PropertyValue(PropertyValue &&other) noexcept : heap_(nullptr) {
    move_from_(std::move(other));
  }

  PropertyValue &operator=(const PropertyValue &other) {
    if (this != &other) {
      reset();
      copy_from_(other);
    }
    return *this;
  }

  PropertyValue &operator=(PropertyValue &&other) noexcept {
    if (this != &other) {
      reset();
      move_from_(std::move(other));
    }
    return *this;
  }

  ~PropertyValue() { reset(); }

  void reset() noexcept {
    if (vtable_) {
      const VTable *vtable = vtable_;
      vtable->destroy(storage_());
      if (!vtable->inlined && heap_)
        vtable->deallocate(heap_);
      vtable_ = nullptr;
      heap_ = nullptr;
    }
  }

  bool has_value() const noexcept { return vtable_ != nullptr; }
  explicit operator bool() const noexcept { return has_value(); }

  std::type_index type() const noexcept {
    return vtable_ ? vtable_->type : std::type_index(typeid(void));
  }

  /// Returns nullptr when the stored type is not `T`. Callers treat that as
  /// "fall back to the default" rather than as an error, which is what keeps a
  /// type-mismatched token in a stylesheet from taking the window down.
  template <class T> const T *as() const noexcept {
    if (!vtable_ || vtable_->type != std::type_index(typeid(T)))
      return nullptr;
    return static_cast<const T *>(storage_());
  }

  bool operator==(const PropertyValue &other) const {
    if (!vtable_ || !other.vtable_)
      return vtable_ == other.vtable_;
    if (vtable_->type != other.vtable_->type)
      return false;
    return vtable_->equals(storage_(), other.storage_());
  }

  bool operator!=(const PropertyValue &other) const {
    return !(*this == other);
  }

  std::uint64_t hash() const {
    if (!vtable_)
      return 0;
    return style_hash_combine(
        static_cast<std::uint64_t>(vtable_->type.hash_code()),
        vtable_->hash(storage_()));
  }

private:
  struct VTable {
    std::type_index type;
    bool inlined;
    void (*copy)(const void *source, void *destination);
    void (*move_inline)(void *source, void *destination) noexcept;
    void *(*allocate_copy)(const void *source);
    void (*destroy)(void *object) noexcept;
    void (*deallocate)(void *block) noexcept;
    bool (*equals)(const void *a, const void *b);
    std::uint64_t (*hash)(const void *object);
  };

  template <class T> static constexpr bool fits_inline() {
    return sizeof(T) <= kInlineSize && alignof(T) <= kInlineAlign &&
           std::is_nothrow_move_constructible_v<T>;
  }

  template <class T> static const VTable *vtable_for() {
    static const VTable table{
        std::type_index(typeid(T)),
        fits_inline<T>(),
        [](const void *source, void *destination) {
          ::new (destination) T(*static_cast<const T *>(source));
        },
        [](void *source, void *destination) noexcept {
          if constexpr (fits_inline<T>()) {
            if constexpr (std::is_trivially_copyable_v<T>) {
              std::memcpy(destination, source, sizeof(T));
            } else {
              T *value = static_cast<T *>(source);
              ::new (destination) T(std::move(*value));
              value->~T();
            }
          }
        },
        [](const void *source) -> void * {
          void *block = ::operator new(sizeof(T), std::align_val_t{alignof(T)});
          ::new (block) T(*static_cast<const T *>(source));
          return block;
        },
        [](void *object) noexcept { static_cast<T *>(object)->~T(); },
        [](void *block) noexcept {
          ::operator delete(block, std::align_val_t{alignof(T)});
        },
        [](const void *a, const void *b) {
          return style_equals(*static_cast<const T *>(a),
                              *static_cast<const T *>(b));
        },
        [](const void *object) {
          return style_hash(*static_cast<const T *>(object));
        },
    };
    return &table;
  }

  template <class T, class Arg> void construct_(Arg &&argument) {
    if constexpr (fits_inline<T>()) {
      ::new (static_cast<void *>(buffer_)) T(std::forward<Arg>(argument));
    } else {
      void *block = ::operator new(sizeof(T), std::align_val_t{alignof(T)});
      ::new (block) T(std::forward<Arg>(argument));
      heap_ = block;
    }
    vtable_ = vtable_for<T>();
  }

  void copy_from_(const PropertyValue &other) {
    if (!other.vtable_)
      return;
    if (other.vtable_->inlined) {
      other.vtable_->copy(other.storage_(), buffer_);
    } else {
      // The allocation size is not recoverable from the vtable, so a heap
      // value is round-tripped through its own copy constructor into a block
      // the vtable knows how to release.
      void *block = other.vtable_->allocate_copy(other.storage_());
      heap_ = block;
    }
    vtable_ = other.vtable_;
  }

  void move_from_(PropertyValue &&other) noexcept {
    if (!other.vtable_)
      return;
    if (other.vtable_->inlined) {
      other.vtable_->move_inline(other.buffer_, buffer_);
    } else {
      heap_ = other.heap_;
    }
    vtable_ = other.vtable_;
    other.vtable_ = nullptr;
    other.heap_ = nullptr;
  }

  void *storage_() noexcept {
    return vtable_ && vtable_->inlined ? static_cast<void *>(buffer_) : heap_;
  }

  const void *storage_() const noexcept {
    return vtable_ && vtable_->inlined ? static_cast<const void *>(buffer_)
                                       : heap_;
  }

  const VTable *vtable_ = nullptr;
  union {
    alignas(kInlineAlign) unsigned char buffer_[kInlineSize];
    void *heap_;
  };
};

} // namespace voidui
