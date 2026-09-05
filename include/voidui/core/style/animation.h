#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <string_view>
#include <type_traits>
#include <vector>

#include "voidui/core/style/easing.h"
#include "voidui/core/style/property.h"
#include "voidui/core/style/value.h"
#include "voidui/core/transform.h"
#include "voidui/paint/paint.h"

namespace voidui {

/// Decomposed transforms interpolate without matrix skew artifacts. Rendering
/// composes them around the widget center in CSS order.
struct VisualTransform {
  float translate_x = 0.0f;
  float translate_y = 0.0f;
  float scale_x = 1.0f;
  float scale_y = 1.0f;
  float rotation = 0.0f;
  float translate_x_percent = 0.0f;
  float translate_y_percent = 0.0f;
  // transform: translate(0) still creates a CSS containing/stacking block.
  bool specified = false;

  Transform matrix(Size<float> reference = {}) const;
  bool establishes_containing_block() const {
    return specified || translate_x != 0 || translate_y != 0 ||
           translate_x_percent != 0 || translate_y_percent != 0 ||
           scale_x != 1 || scale_y != 1 || rotation != 0;
  }
};

static_assert(sizeof(VisualTransform) <= PropertyValue::kInlineSize);

bool parse_style_value(std::string_view text, VisualTransform &out);

/// One `<shadow>`: two to four lengths, an optional colour and an optional
/// `inset`, in any order CSS permits. `box-shadow` itself is a list -- see
/// ShadowList below -- and this is the entry for a single one.
bool parse_style_value(std::string_view text, Shadow &out);

bool style_value_equals(const VisualTransform &a, const VisualTransform &b);
std::uint64_t style_value_hash(const VisualTransform &value);
bool style_value_equals(const Shadow &a, const Shadow &b);
std::uint64_t style_value_hash(const Shadow &value);

VisualTransform interpolate_style_value(const VisualTransform &a,
                                        const VisualTransform &b, float t);
Shadow interpolate_style_value(const Shadow &a, const Shadow &b, float t);

// -- Comma-separated value lists ---------------------------------------------

/// The shape every `animation-*` and `transition-*` property value has: a
/// comma-separated list, shared between the nodes that declare it.
///
/// A struct rather than the bare `shared_ptr<const vector<T>>` for two
/// reasons. It gives the list a type in this namespace, which is what lets
/// ADL find the equality, hash and parse hooks the style system looks for on
/// a value type. And it keeps `transition-duration` from silently assigning
/// to `transition-delay`, which the raw alias would have allowed.
///
/// A null `values` means the property was never declared, and stands for its
/// CSS initial value. That distinction matters for `transition-property`,
/// where "not declared" is `all` and an explicit empty list is `none`.
template <class T> struct StyleValueList {
  std::shared_ptr<const std::vector<T>> values;

  bool declared() const noexcept { return static_cast<bool>(values); }
  bool empty() const noexcept { return !values || values->empty(); }
  std::size_t size() const noexcept { return values ? values->size() : 0; }
  const T &operator[](std::size_t index) const { return (*values)[index]; }

  const T *begin() const { return values ? values->data() : nullptr; }
  const T *end() const {
    return values ? values->data() + values->size() : nullptr;
  }

  static StyleValueList make(std::vector<T> items) {
    return StyleValueList{
        std::make_shared<const std::vector<T>>(std::move(items))};
  }
};

/// True for the element types that are their own comparison and their own
/// hash. Scalars are routed around style_equals/style_hash deliberately: those
/// entry points look for an ADL overload, and a scalar will happily convert
/// into one written for some other value type.
template <class T>
concept StyleScalarValue = std::is_arithmetic_v<T> || std::is_enum_v<T>;

template <class T>
bool style_value_equals(const StyleValueList<T> &a,
                        const StyleValueList<T> &b) {
  if (a.values == b.values)
    return true;
  if (a.declared() != b.declared() || a.size() != b.size())
    return false;
  for (std::size_t i = 0; i < a.size(); ++i) {
    if constexpr (StyleScalarValue<T>) {
      if (!(a[i] == b[i]))
        return false;
    } else if (!style_equals(a[i], b[i])) {
      return false;
    }
  }
  return true;
}

template <class T>
std::uint64_t style_value_hash(const StyleValueList<T> &list) {
  if (!list.declared())
    return 0;
  std::uint64_t seed = 0x9e3779b97f4a7c15ULL;
  for (std::size_t i = 0; i < list.size(); ++i) {
    if constexpr (std::is_floating_point_v<T>) {
      // Negative zero compares equal to zero but does not hash equal to it,
      // which would leave two identical styles in different cache buckets.
      const T canonical = list[i] == T{} ? T{} : list[i];
      seed = style_hash_combine(
          seed, style_hash_bytes(&canonical, sizeof(canonical)));
    } else if constexpr (StyleScalarValue<T>) {
      seed = style_hash_combine(seed, static_cast<std::uint64_t>(list[i]));
    } else {
      seed = style_hash_combine(seed, style_hash(list[i]));
    }
  }
  return seed;
}

// -- Keyframe animations -----------------------------------------------------

struct AnimationSpec {
  std::string name;
  float duration = 0.0f;
  float delay = 0.0f;
  float iterations = 1.0f;
  Easing easing{};
  bool alternate = false;

  friend bool operator==(const AnimationSpec &,
                         const AnimationSpec &) = default;
};

std::uint64_t style_value_hash(const AnimationSpec &spec);

using AnimationList = StyleValueList<AnimationSpec>;

bool parse_style_value(std::string_view text, AnimationList &out);

// -- Box shadows -------------------------------------------------------------

/// `box-shadow` is a comma-separated list, and the order is load-bearing: the
/// first entry paints on top of the ones after it.
using ShadowList = StyleValueList<Shadow>;

bool parse_style_value(std::string_view text, ShadowList &out);

/// Two shadow lists interpolate entry by entry, the shorter one padded with
/// transparent zero-sized shadows the way css-backgrounds-3 specifies. A pair
/// that disagrees about `inset` has no interpolation at all -- the two are
/// different shapes -- so this reports failure and the transition falls back to
/// the discrete behaviour every other unanimatable value gets.
bool interpolate_style_value(const ShadowList &a, const ShadowList &b, float t,
                             ShadowList &out);

// -- Transitions -------------------------------------------------------------

/// What `transition-property: all` stores in place of a real index.
///
/// Distinct from kInvalidPropertyIndex, which marks a name this build does not
/// know: CSS keeps such a name in the list rather than dropping it, so the
/// positional pairing with the other four longhands has to survive it.
inline constexpr PropertyIndex kAllTransitionProperties =
    static_cast<PropertyIndex>(-2);

/// `transition-behavior`. A property with no interpolation -- an enum, a font
/// stack, a gradient whose stop count changed -- only takes part in a
/// transition when the author opts in, and then flips at the halfway point of
/// its eased progress instead of interpolating.
enum class TransitionBehavior : std::uint8_t {
  Normal,
  AllowDiscrete,
};

using TransitionPropertyList = StyleValueList<PropertyIndex>;
using StyleTimeList = StyleValueList<float>;
using EasingList = StyleValueList<Easing>;
using TransitionBehaviorList = StyleValueList<TransitionBehavior>;

/// One `transition-property` entry together with the four values that go with
/// it, after the CSS list-cycling rule has been applied.
struct TransitionSpec {
  PropertyIndex property = kInvalidPropertyIndex;
  float duration = 0.0f;
  float delay = 0.0f;
  Easing easing{};
  TransitionBehavior behavior = TransitionBehavior::Normal;

  /// CSS's "combined duration". A transition can only start while this is
  /// positive, which is what makes a `transition-property` declaration with no
  /// duration cost nothing at run time.
  float combined_duration() const { return duration + delay; }
};

/// A node's five `transition-*` longhands, read together.
///
/// Nothing is materialised: `at()` applies the rule that the shorter lists
/// repeat up to the length of transition-property, so reading a node's
/// transitions costs five shared-pointer copies and no allocation.
struct TransitionSettings {
  TransitionPropertyList properties;
  StyleTimeList durations;
  StyleTimeList delays;
  EasingList easings;
  TransitionBehaviorList behaviors;

  /// `transition-property: none` -- the one declaration that switches a node's
  /// transitions off no matter what the other four say.
  bool none() const { return properties.declared() && properties.empty(); }

  /// Undeclared transition-property is `all`, which is a single entry.
  std::size_t count() const {
    return properties.declared() ? properties.size() : 1;
  }

  TransitionSpec at(std::size_t index) const;

  /// True when no entry could ever start a transition. The animator checks
  /// this before it allocates any per-node state.
  bool idle() const;

  /// The entry that governs `property`, honouring both `all` and the CSS rule
  /// that a repeated property name is won by its last mention. False when the
  /// property does not transition on this node.
  bool resolve(PropertyIndex property, TransitionSpec &out) const;
};

bool parse_style_value(std::string_view text, TransitionPropertyList &out);
bool parse_style_value(std::string_view text, StyleTimeList &out);
bool parse_style_value(std::string_view text, EasingList &out);
bool parse_style_value(std::string_view text, TransitionBehaviorList &out);

/// The expansion of one `transition` shorthand declaration. Every list is
/// always filled in: a shorthand resets the longhands it does not mention to
/// their initial values, exactly as CSS requires.
struct TransitionShorthand {
  TransitionPropertyList properties;
  StyleTimeList durations;
  StyleTimeList delays;
  EasingList easings;
  TransitionBehaviorList behaviors;
};

/// Reads a `transition` shorthand. `scope` is the rule's key widget name, so
/// `transition: caret-color 1s` inside `input { ... }` finds
/// `input.caret-color`; pass an empty view outside a scoped rule.
bool parse_transition_shorthand(std::string_view text, std::string_view scope,
                                TransitionShorthand &out);

/// Reads a `transition-property` value under the same scoping rule.
bool parse_transition_property_list(std::string_view text,
                                    std::string_view scope,
                                    TransitionPropertyList &out);

/// Resolves a property name the way a declaration in that rule would:
/// `caret-color` with scope `input` finds `input.caret-color`, falling back to
/// the unprefixed name.
PropertyIndex style_find_property(std::string_view name,
                                  std::string_view scope);

/// True for the properties that describe motion rather than appearance. They
/// never transition and never animate -- letting them would allow a
/// declaration to rewrite its own timing mid-flight.
bool style_is_timing_property(PropertyIndex property);

} // namespace voidui
