#include "voidui/core/style/animation.h"

#include "voidui/core/style.h"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <limits>

namespace voidui {
namespace {

bool is_space(char c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' ||
         c == '\v';
}

std::vector<std::string_view> split_top_level(std::string_view text,
                                              char separator) {
  std::vector<std::string_view> result;
  int depth = 0;
  std::size_t begin = 0;
  for (std::size_t i = 0; i < text.size(); ++i) {
    if (text[i] == '(')
      ++depth;
    else if (text[i] == ')')
      depth = std::max(depth - 1, 0);
    else if (text[i] == separator && depth == 0) {
      result.push_back(style_trim(text.substr(begin, i - begin)));
      begin = i + 1;
    }
  }
  result.push_back(style_trim(text.substr(begin)));
  return result;
}

std::vector<std::string_view> split_words(std::string_view text) {
  std::vector<std::string_view> result;
  std::size_t begin = std::string_view::npos;
  int depth = 0;
  for (std::size_t i = 0; i < text.size(); ++i) {
    if (text[i] == '(')
      ++depth;
    else if (text[i] == ')')
      depth = std::max(depth - 1, 0);

    if (depth == 0 && is_space(text[i])) {
      if (begin != std::string_view::npos) {
        result.push_back(text.substr(begin, i - begin));
        begin = std::string_view::npos;
      }
    } else if (begin == std::string_view::npos) {
      begin = i;
    }
  }
  if (begin != std::string_view::npos)
    result.push_back(text.substr(begin));
  return result;
}

bool read_function(std::string_view text, std::size_t &position,
                   std::string_view &name, std::string_view &arguments) {
  while (position < text.size() && is_space(text[position]))
    ++position;
  const std::size_t name_begin = position;
  while (position < text.size() &&
         ((text[position] >= 'a' && text[position] <= 'z') ||
          (text[position] >= 'A' && text[position] <= 'Z') ||
          text[position] == '-'))
    ++position;
  name = text.substr(name_begin, position - name_begin);
  if (name.empty() || position >= text.size() || text[position] != '(')
    return false;

  const std::size_t args_begin = ++position;
  int depth = 1;
  while (position < text.size() && depth > 0) {
    if (text[position] == '(')
      ++depth;
    else if (text[position] == ')')
      --depth;
    ++position;
  }
  if (depth != 0)
    return false;
  arguments = text.substr(args_begin, position - args_begin - 1);
  return true;
}

bool parse_angle(std::string_view text, float &radians) {
  text = style_trim(text);
  float scale = 1.0f;
  if (text.ends_with("deg")) {
    text.remove_suffix(3);
    scale = 0.01745329251994329577f;
  } else if (text.ends_with("rad")) {
    text.remove_suffix(3);
  } else {
    return false;
  }
  float value = 0.0f;
  if (!parse_style_value(text, value))
    return false;
  radians = value * scale;
  return true;
}

/// A `<time>`: seconds or milliseconds, with the unit required. `transition-
/// delay` is the one place CSS allows a negative, where it means the
/// transition starts already partway through.
bool parse_seconds(std::string_view text, bool allow_negative, float &seconds) {
  float scale = 0.0f;
  if (text.ends_with("ms")) {
    text.remove_suffix(2);
    scale = 0.001f;
  } else if (text.ends_with('s')) {
    text.remove_suffix(1);
    scale = 1.0f;
  } else {
    return false;
  }
  // A bare "s" or "ms" is a keyword, not a time.
  if (style_trim(text).empty())
    return false;
  float value = 0.0f;
  if (!parse_style_value(text, value) || !std::isfinite(value) ||
      (!allow_negative && value < 0.0f))
    return false;
  seconds = value * scale;
  return true;
}

bool parse_iterations(std::string_view text, float &iterations) {
  if (text == "infinite") {
    iterations = std::numeric_limits<float>::infinity();
    return true;
  }
  const std::string copy(text);
  char *end = nullptr;
  const float value = std::strtof(copy.c_str(), &end);
  if (end == copy.c_str() || *end != '\0' || !std::isfinite(value) ||
      value <= 0.0f)
    return false;
  iterations = value;
  return true;
}

bool parse_behavior(std::string_view text, TransitionBehavior &behavior) {
  if (text == "normal")
    behavior = TransitionBehavior::Normal;
  else if (text == "allow-discrete")
    behavior = TransitionBehavior::AllowDiscrete;
  else
    return false;
  return true;
}

template <class T>
T cycle(const StyleValueList<T> &list, std::size_t index, T fallback) {
  if (list.empty())
    return fallback;
  return list[index % list.size()];
}

} // namespace

Transform VisualTransform::matrix() const {
  return Transform::translate(translate_x, translate_y)
      .concat(Transform::rotate(rotation))
      .concat(Transform::scale(scale_x, scale_y));
}

bool parse_style_value(std::string_view text, VisualTransform &out) {
  text = style_trim(text);
  if (text == "none") {
    out = {};
    return true;
  }

  VisualTransform result;
  std::size_t position = 0;
  while (position < text.size()) {
    std::string_view name;
    std::string_view arguments;
    if (!read_function(text, position, name, arguments))
      return false;
    std::vector<std::string_view> values = split_top_level(arguments, ',');
    if (values.size() == 1) {
      const auto words = split_words(values.front());
      if (words.size() > 1)
        values = words;
    }

    float a = 0.0f;
    float b = 0.0f;
    if (name == "translate" && values.size() == 2 &&
        parse_style_value(values[0], a) && parse_style_value(values[1], b)) {
      result.translate_x = a;
      result.translate_y = b;
    } else if (name == "translateX" && values.size() == 1 &&
               parse_style_value(values[0], a)) {
      result.translate_x = a;
    } else if (name == "translateY" && values.size() == 1 &&
               parse_style_value(values[0], a)) {
      result.translate_y = a;
    } else if (name == "scale" && !values.empty() && values.size() <= 2 &&
               parse_style_value(values[0], a) &&
               (values.size() == 1 || parse_style_value(values[1], b))) {
      result.scale_x = a;
      result.scale_y = values.size() == 1 ? a : b;
    } else if (name == "rotate" && values.size() == 1 &&
               parse_angle(values[0], a)) {
      result.rotation = a;
    } else {
      return false;
    }
  }
  out = result;
  return true;
}

namespace {

/// A `<shadow>` as css-backgrounds-3 states it:
///
///   inset? && <length>{2,4} && <color>?
///
/// `&&` means the three parts may appear in any order, which is why this reads
/// the words rather than indexing fixed positions. The old code took the last
/// word as the colour and required exactly four or five words, so it rejected
/// `inset 0 2px 4px #0003` and `#0003 0 2px 4px` alike, and had no way to
/// express a shadow with no blur radius at all.
bool parse_shadow(std::string_view text, Shadow &out) {
  const auto words = split_words(text);
  if (words.empty())
    return false;

  // CSS says an omitted colour is `currentColor`. There is no such value here,
  // so the shadow takes the same opaque black an unstyled foreground would.
  Color color(0, 0, 0);
  float lengths[4] = {0.0f, 0.0f, 0.0f, 0.0f};
  std::size_t length_count = 0;
  bool inset = false;
  bool has_color = false;

  for (std::string_view word : words) {
    if (word == "inset") {
      if (inset)
        return false;
      inset = true;
      continue;
    }
    // Lengths are tried first: no colour notation parses as a bare number, and
    // no length parses as a colour, so the two never contend.
    float value = 0.0f;
    if (length_count < 4 && parse_style_value(word, value)) {
      lengths[length_count++] = value;
      continue;
    }
    if (has_color || !parse_style_value(word, color))
      return false;
    has_color = true;
  }

  // The two offsets are the only required part, and a negative blur radius is
  // invalid rather than clamped -- it invalidates the whole declaration.
  if (length_count < 2 || lengths[2] < 0.0f)
    return false;

  // CSS states the blur as the width the shadow's edge fades over, roughly
  // twice the Gaussian's standard deviation. Shadow carries the deviation, so
  // the two differ by a factor of two: `0 1px 2px` is a one-unit sigma, not the
  // two-unit one this used to hand the shader.
  out = Shadow(color, Point<float>(lengths[0], lengths[1]), lengths[2] * 0.5f,
               lengths[3], inset);
  return true;
}

/// The identity a list is padded with when it is shorter than the one it
/// interpolates against: invisible, sized to nothing, and on the same side of
/// the box as its counterpart.
Shadow transparent_like(const Shadow &other) {
  return Shadow(Color::TRANSPARENT, {}, 0.0f, 0.0f, other.inset);
}

} // namespace

bool parse_style_value(std::string_view text, Shadow &out) {
  text = style_trim(text);
  if (text == "none") {
    out = Shadow(Color::TRANSPARENT, {}, 0.0f);
    return true;
  }
  return parse_shadow(text, out);
}

bool parse_style_value(std::string_view text, ShadowList &out) {
  text = style_trim(text);
  if (text.empty())
    return false;
  if (text == "none") {
    out = ShadowList::make({});
    return true;
  }

  std::vector<Shadow> shadows;
  for (std::string_view item : split_top_level(text, ',')) {
    Shadow shadow;
    if (!parse_shadow(item, shadow))
      return false;
    shadows.push_back(shadow);
  }
  out = ShadowList::make(std::move(shadows));
  return true;
}

bool style_value_equals(const VisualTransform &a, const VisualTransform &b) {
  return a.translate_x == b.translate_x && a.translate_y == b.translate_y &&
         a.scale_x == b.scale_x && a.scale_y == b.scale_y &&
         a.rotation == b.rotation;
}

std::uint64_t style_value_hash(const VisualTransform &value) {
  return style_hash_bytes(&value, sizeof(value));
}

bool style_value_equals(const Shadow &a, const Shadow &b) {
  return style_value_equals(a.color, b.color) && a.offset.x == b.offset.x &&
         a.offset.y == b.offset.y && a.blur == b.blur &&
         a.spread == b.spread && a.inset == b.inset;
}

std::uint64_t style_value_hash(const Shadow &value) {
  // Field by field, not as bytes: Shadow leads with a Color, which pads after
  // its space tag.
  const auto hash_float = [](float number) {
    const float canonical = number == 0.0f ? 0.0f : number;
    return style_hash_bytes(&canonical, sizeof(canonical));
  };
  std::uint64_t seed = style_value_hash(value.color);
  seed = style_hash_combine(seed, hash_float(value.offset.x));
  seed = style_hash_combine(seed, hash_float(value.offset.y));
  seed = style_hash_combine(seed, hash_float(value.blur));
  seed = style_hash_combine(seed, hash_float(value.spread));
  return style_hash_combine(seed, value.inset ? 1u : 0u);
}

VisualTransform interpolate_style_value(const VisualTransform &a,
                                        const VisualTransform &b, float t) {
  VisualTransform result;
  result.translate_x = interpolate_style_value(a.translate_x, b.translate_x, t);
  result.translate_y = interpolate_style_value(a.translate_y, b.translate_y, t);
  result.scale_x = interpolate_style_value(a.scale_x, b.scale_x, t);
  result.scale_y = interpolate_style_value(a.scale_y, b.scale_y, t);
  result.rotation = interpolate_style_value(a.rotation, b.rotation, t);
  return result;
}

Shadow interpolate_style_value(const Shadow &a, const Shadow &b, float t) {
  return Shadow(
      interpolate_style_value(a.color, b.color, t),
      Point<float>(interpolate_style_value(a.offset.x, b.offset.x, t),
                   interpolate_style_value(a.offset.y, b.offset.y, t)),
      interpolate_style_value(a.blur, b.blur, t),
      interpolate_style_value(a.spread, b.spread, t),
      // Not interpolable on its own; the list above refuses to pair two
      // shadows that disagree, so this only ever flips at the midpoint of a
      // deliberately discrete transition.
      t < 0.5f ? a.inset : b.inset);
}

bool interpolate_style_value(const ShadowList &a, const ShadowList &b, float t,
                             ShadowList &out) {
  const std::size_t count = std::max(a.size(), b.size());
  if (count == 0) {
    out = b;
    return true;
  }

  std::vector<Shadow> result;
  result.reserve(count);
  for (std::size_t i = 0; i < count; ++i) {
    const Shadow from = i < a.size() ? a[i] : transparent_like(b[i]);
    const Shadow to = i < b.size() ? b[i] : transparent_like(a[i]);
    if (from.inset != to.inset)
      return false;
    result.push_back(interpolate_style_value(from, to, t));
  }
  out = ShadowList::make(std::move(result));
  return true;
}

std::uint64_t style_value_hash(const AnimationSpec &spec) {
  const auto hash_float = [](float number) {
    const float canonical = number == 0.0f ? 0.0f : number;
    return style_hash_bytes(&canonical, sizeof(canonical));
  };
  std::uint64_t seed = style_hash_bytes(spec.name.data(), spec.name.size());
  seed = style_hash_combine(seed, hash_float(spec.duration));
  seed = style_hash_combine(seed, hash_float(spec.delay));
  seed = style_hash_combine(seed, hash_float(spec.iterations));
  seed = style_hash_combine(seed, style_value_hash(spec.easing));
  return style_hash_combine(seed, spec.alternate ? 1u : 0u);
}

bool parse_style_value(std::string_view text, AnimationList &out) {
  text = style_trim(text);
  if (text == "none") {
    out = AnimationList::make({});
    return true;
  }

  std::vector<AnimationSpec> animations;
  for (std::string_view item : split_top_level(text, ',')) {
    AnimationSpec spec;
    bool saw_duration = false;
    bool saw_delay = false;
    for (std::string_view word : split_words(item)) {
      float seconds = 0.0f;
      Easing easing;
      float iterations = 0.0f;
      // Duration first, then delay -- and only the delay may be negative,
      // where it means the animation starts already partway through.
      if (parse_seconds(word, /*allow_negative=*/saw_duration, seconds)) {
        if (!saw_duration) {
          spec.duration = seconds;
          saw_duration = true;
        } else if (!saw_delay) {
          spec.delay = seconds;
          saw_delay = true;
        } else {
          return false;
        }
      } else if (parse_style_value(word, easing)) {
        spec.easing = easing;
      } else if (word == "alternate") {
        spec.alternate = true;
      } else if (parse_iterations(word, iterations)) {
        spec.iterations = iterations;
      } else if (spec.name.empty()) {
        spec.name = std::string(word);
      } else {
        return false;
      }
    }
    if (spec.name.empty() || !saw_duration || spec.duration <= 0.0f)
      return false;
    animations.push_back(std::move(spec));
  }
  out = AnimationList::make(std::move(animations));
  return true;
}

// -- Transitions -------------------------------------------------------------

PropertyIndex style_find_property(std::string_view name,
                                  std::string_view scope) {
  const PropertyRegistry &registry = PropertyRegistry::instance();
  if (!scope.empty()) {
    std::string scoped(scope);
    scoped += '.';
    scoped += name;
    const PropertyIndex scoped_index = registry.find(scoped);
    if (scoped_index != kInvalidPropertyIndex)
      return scoped_index;
  }
  return registry.find(name);
}

bool style_is_timing_property(PropertyIndex property) {
  // Resolved once. The indices are stable for the life of the process, and
  // this is on the path that decides whether a property may transition.
  static const PropertyIndex timing[] = {
      styles::Animation::index(),
      styles::TransitionProperty::index(),
      styles::TransitionDuration::index(),
      styles::TransitionDelay::index(),
      styles::TransitionTimingFunction::index(),
      styles::TransitionBehavior::index(),
  };
  for (const PropertyIndex candidate : timing)
    if (candidate == property)
      return true;
  return false;
}

TransitionSpec TransitionSettings::at(std::size_t index) const {
  TransitionSpec spec;
  spec.property = properties.declared() ? properties[index]
                                        : kAllTransitionProperties;
  // A negative duration is invalid CSS; treating it as zero keeps one bad
  // value in a list from taking the whole declaration down.
  spec.duration = std::max(cycle(durations, index, 0.0f), 0.0f);
  spec.delay = cycle(delays, index, 0.0f);
  spec.easing = cycle(easings, index, Easing{});
  spec.behavior = cycle(behaviors, index, TransitionBehavior::Normal);
  return spec;
}

bool TransitionSettings::idle() const {
  if (none())
    return true;
  const std::size_t entries = count();
  for (std::size_t i = 0; i < entries; ++i) {
    const TransitionSpec spec = at(i);
    if (spec.property != kInvalidPropertyIndex &&
        spec.combined_duration() > 0.0f)
      return false;
  }
  return true;
}

bool TransitionSettings::resolve(PropertyIndex property,
                                 TransitionSpec &out) const {
  if (none() || property == kInvalidPropertyIndex ||
      property == kAllTransitionProperties)
    return false;

  bool found = false;
  const std::size_t entries = count();
  for (std::size_t i = 0; i < entries; ++i) {
    const PropertyIndex entry =
        properties.declared() ? properties[i] : kAllTransitionProperties;
    if (entry != kAllTransitionProperties && entry != property)
      continue;
    // Keep scanning: when a property is mentioned twice, CSS gives the last
    // mention the win.
    out = at(i);
    out.property = property;
    found = true;
  }
  return found;
}

bool parse_transition_property_list(std::string_view text,
                                    std::string_view scope,
                                    TransitionPropertyList &out) {
  text = style_trim(text);
  if (text.empty())
    return false;
  if (text == "none") {
    out = TransitionPropertyList::make({});
    return true;
  }

  std::vector<PropertyIndex> properties;
  for (std::string_view item : split_top_level(text, ',')) {
    if (item.empty())
      return false;
    if (item == "all") {
      properties.push_back(kAllTransitionProperties);
      continue;
    }
    if (item == "none")
      return false;
    // An unknown name stays in the list as an invalid index. CSS treats a
    // custom-ident it does not recognise as valid-but-inert, and dropping it
    // would shift every later entry's pairing with the other longhands.
    properties.push_back(style_find_property(item, scope));
  }
  out = TransitionPropertyList::make(std::move(properties));
  return true;
}

bool parse_style_value(std::string_view text, TransitionPropertyList &out) {
  return parse_transition_property_list(text, {}, out);
}

bool parse_style_value(std::string_view text, StyleTimeList &out) {
  text = style_trim(text);
  if (text.empty())
    return false;
  std::vector<float> times;
  for (std::string_view item : split_top_level(text, ',')) {
    float seconds = 0.0f;
    if (!parse_seconds(item, /*allow_negative=*/true, seconds))
      return false;
    times.push_back(seconds);
  }
  out = StyleTimeList::make(std::move(times));
  return true;
}

bool parse_style_value(std::string_view text, EasingList &out) {
  text = style_trim(text);
  if (text.empty())
    return false;
  std::vector<Easing> easings;
  for (std::string_view item : split_top_level(text, ',')) {
    Easing easing;
    if (!parse_style_value(item, easing))
      return false;
    easings.push_back(easing);
  }
  out = EasingList::make(std::move(easings));
  return true;
}

bool parse_style_value(std::string_view text, TransitionBehaviorList &out) {
  text = style_trim(text);
  if (text.empty())
    return false;
  std::vector<TransitionBehavior> behaviors;
  for (std::string_view item : split_top_level(text, ',')) {
    TransitionBehavior behavior = TransitionBehavior::Normal;
    if (!parse_behavior(item, behavior))
      return false;
    behaviors.push_back(behavior);
  }
  out = TransitionBehaviorList::make(std::move(behaviors));
  return true;
}

bool parse_transition_shorthand(std::string_view text, std::string_view scope,
                                TransitionShorthand &out) {
  text = style_trim(text);
  if (text.empty())
    return false;

  const std::vector<std::string_view> items = split_top_level(text, ',');

  // `transition: none` disables every transition on the node, and CSS only
  // allows it on its own -- inside a list it would have no position to take.
  if (items.size() == 1 && items.front() == "none") {
    out.properties = TransitionPropertyList::make({});
    out.durations = StyleTimeList::make({});
    out.delays = StyleTimeList::make({});
    out.easings = EasingList::make({});
    out.behaviors = TransitionBehaviorList::make({});
    return true;
  }

  std::vector<PropertyIndex> properties;
  std::vector<float> durations;
  std::vector<float> delays;
  std::vector<Easing> easings;
  std::vector<TransitionBehavior> behaviors;
  properties.reserve(items.size());
  durations.reserve(items.size());
  delays.reserve(items.size());
  easings.reserve(items.size());
  behaviors.reserve(items.size());

  for (std::string_view item : items) {
    if (item.empty())
      return false;

    PropertyIndex property = kAllTransitionProperties;
    float duration = 0.0f;
    float delay = 0.0f;
    Easing easing;
    TransitionBehavior behavior = TransitionBehavior::Normal;
    bool saw_property = false;
    bool saw_duration = false;
    bool saw_delay = false;
    bool saw_easing = false;
    bool saw_behavior = false;

    for (std::string_view word : split_words(item)) {
      float seconds = 0.0f;
      Easing parsed_easing;
      TransitionBehavior parsed_behavior = TransitionBehavior::Normal;

      // The two <time> slots are positional -- the first is the duration, the
      // second the delay -- so only the delay may be negative.
      if (parse_seconds(word, /*allow_negative=*/saw_duration, seconds)) {
        if (!saw_duration) {
          duration = seconds;
          saw_duration = true;
        } else if (!saw_delay) {
          delay = seconds;
          saw_delay = true;
        } else {
          return false;
        }
      } else if (!saw_easing && parse_style_value(word, parsed_easing)) {
        easing = parsed_easing;
        saw_easing = true;
      } else if (!saw_behavior && parse_behavior(word, parsed_behavior)) {
        behavior = parsed_behavior;
        saw_behavior = true;
      } else if (!saw_property) {
        if (word == "none")
          return false;
        property = word == "all" ? kAllTransitionProperties
                                 : style_find_property(word, scope);
        saw_property = true;
      } else {
        return false;
      }
    }

    properties.push_back(property);
    durations.push_back(duration);
    delays.push_back(delay);
    easings.push_back(easing);
    behaviors.push_back(behavior);
  }

  out.properties = TransitionPropertyList::make(std::move(properties));
  out.durations = StyleTimeList::make(std::move(durations));
  out.delays = StyleTimeList::make(std::move(delays));
  out.easings = EasingList::make(std::move(easings));
  out.behaviors = TransitionBehaviorList::make(std::move(behaviors));
  return true;
}

} // namespace voidui
