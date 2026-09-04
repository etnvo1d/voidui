#include "voidui/core/style/value.h"

#include "voidui/core/typography.h"

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <cstdlib>
#include <limits>
#include <span>
#include <string>
#include <vector>

namespace voidui {
namespace {

bool is_space(char c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' ||
         c == '\v';
}

bool is_digit(char c) { return c >= '0' && c <= '9'; }

/// The characters an unquoted font family name may use: CSS identifier
/// characters, plus every non-ASCII byte -- a family is often written in the
/// script it belongs to, and `font-family: 微软雅黑` has to parse unquoted.
bool is_family_char(char c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || is_digit(c) ||
         c == '-' || c == '_' || static_cast<unsigned char>(c) >= 0x80;
}

/// Splits on top-level occurrences of `separator`, ignoring anything inside
/// parentheses so that `rgb(1, 2, 3), #fff` splits into two, not four.
std::vector<std::string_view> split_top_level(std::string_view text,
                                              char separator) {
  std::vector<std::string_view> parts;
  int depth = 0;
  std::size_t start = 0;
  for (std::size_t i = 0; i < text.size(); ++i) {
    const char c = text[i];
    if (c == '(')
      ++depth;
    else if (c == ')')
      depth = depth > 0 ? depth - 1 : 0;
    else if (c == separator && depth == 0) {
      parts.push_back(style_trim(text.substr(start, i - start)));
      start = i + 1;
    }
  }
  parts.push_back(style_trim(text.substr(start)));
  return parts;
}

std::vector<std::string_view> split_whitespace(std::string_view text) {
  std::vector<std::string_view> parts;
  int depth = 0;
  std::size_t start = std::string_view::npos;
  for (std::size_t i = 0; i < text.size(); ++i) {
    const char c = text[i];
    if (c == '(')
      ++depth;
    else if (c == ')')
      depth = depth > 0 ? depth - 1 : 0;

    if (depth == 0 && is_space(c)) {
      if (start != std::string_view::npos) {
        parts.push_back(text.substr(start, i - start));
        start = std::string_view::npos;
      }
    } else if (start == std::string_view::npos) {
      start = i;
    }
  }
  if (start != std::string_view::npos)
    parts.push_back(text.substr(start));
  return parts;
}

bool parse_hex_digit(char c, int &out) {
  if (c >= '0' && c <= '9')
    out = c - '0';
  else if (c >= 'a' && c <= 'f')
    out = c - 'a' + 10;
  else if (c >= 'A' && c <= 'F')
    out = c - 'A' + 10;
  else
    return false;
  return true;
}

bool parse_hex_color(std::string_view text, Color &out) {
  if (text.empty() || text[0] != '#')
    return false;
  std::string_view digits = text.substr(1);
  int values[8]{};
  if (digits.size() != 3 && digits.size() != 4 && digits.size() != 6 &&
      digits.size() != 8)
    return false;
  for (std::size_t i = 0; i < digits.size(); ++i)
    if (!parse_hex_digit(digits[i], values[i]))
      return false;

  auto byte = [](int value) { return static_cast<std::uint8_t>(value); };
  if (digits.size() <= 4) {
    // #RGB / #RGBA -- each digit doubled, as in CSS.
    const std::uint8_t alpha =
        digits.size() == 4 ? byte(values[3] * 17) : std::uint8_t{255};
    out = Color(byte(values[0] * 17), byte(values[1] * 17),
                byte(values[2] * 17), alpha);
    return true;
  }
  const std::uint8_t alpha =
      digits.size() == 8 ? byte(values[6] * 16 + values[7]) : std::uint8_t{255};
  out =
      Color(byte(values[0] * 16 + values[1]), byte(values[2] * 16 + values[3]),
            byte(values[4] * 16 + values[5]), alpha);
  return true;
}

/// The raw text between a function's parentheses.
///
/// Unlike parse_function() this does not split on commas: a modern colour
/// function separates its components with spaces and its alpha with a slash,
/// and which form was written is decided afterwards.
bool parse_function_body(std::string_view text, std::string_view name,
                         std::string_view &body) {
  if (text.size() <= name.size() + 1)
    return false;
  if (text.compare(0, name.size(), name) != 0)
    return false;
  const std::string_view rest = style_trim(text.substr(name.size()));
  if (rest.size() < 2 || rest.front() != '(' || rest.back() != ')')
    return false;
  body = rest.substr(1, rest.size() - 2);
  return true;
}

bool parse_function(std::string_view text, std::string_view name,
                    std::vector<std::string_view> &arguments) {
  if (text.size() <= name.size() + 2)
    return false;
  if (text.compare(0, name.size(), name) != 0)
    return false;
  std::string_view rest = style_trim(text.substr(name.size()));
  if (rest.size() < 2 || rest.front() != '(' || rest.back() != ')')
    return false;
  arguments = split_top_level(rest.substr(1, rest.size() - 2), ',');
  return true;
}

bool parse_finite_number(std::string_view text, float &out) {
  text = style_trim(text);
  if (text.empty())
    return false;
  const std::string buffer(text);
  char *end = nullptr;
  const float value = std::strtof(buffer.c_str(), &end);
  if (end == buffer.c_str() || *end != '\0' || !std::isfinite(value))
    return false;
  out = value;
  return true;
}

bool parse_css_angle(std::string_view text, float &radians) {
  text = style_trim(text);
  constexpr float kRadiansPerDegree = 0.01745329251994329577f;
  constexpr float kRadiansPerGrad = 0.01570796326794896619f;
  constexpr float kTwoPi = 6.28318530717958647692f;

  struct Unit {
    std::string_view suffix;
    float scale;
  };
  constexpr Unit units[]{{"deg", kRadiansPerDegree},
                         {"grad", kRadiansPerGrad},
                         {"rad", 1.0f},
                         {"turn", kTwoPi}};
  for (const Unit &unit : units) {
    if (!text.ends_with(unit.suffix))
      continue;
    float value = 0.0f;
    if (!parse_finite_number(
            style_trim(text.substr(0, text.size() - unit.suffix.size())),
            value))
      return false;
    radians = value * unit.scale;
    return std::isfinite(radians);
  }

  // CSS permits a unitless zero angle and no other unitless angle.
  float zero = 0.0f;
  if (!parse_finite_number(text, zero) || zero != 0.0f)
    return false;
  radians = 0.0f;
  return true;
}

constexpr float kDegreesPerRadian = 57.295779513082320876f;
constexpr float kTwoPiRadians = 6.28318530717958647692f;

/// `<number> | <percentage>`. `number_scale` converts a plain number into the
/// component's own units -- 1/255 for an `rgb()` channel -- and `percent_value`
/// is what 100% denotes there.
bool parse_component(std::string_view text, float number_scale,
                     float percent_value, float &out) {
  text = style_trim(text);
  if (text.empty())
    return false;
  if (text.back() == '%') {
    float value = 0.0f;
    if (!parse_finite_number(text.substr(0, text.size() - 1), value))
      return false;
    out = value / 100.0f * percent_value;
    return true;
  }
  float value = 0.0f;
  if (!parse_finite_number(text, value))
    return false;
  out = value * number_scale;
  return true;
}

/// `<alpha-value>`, or 1 when the slash and everything after it was omitted.
bool parse_alpha_value(std::string_view text, float &out) {
  if (style_trim(text).empty()) {
    out = 1.0f;
    return true;
  }
  float value = 0.0f;
  if (!parse_component(text, 1.0f, 1.0f, value))
    return false;
  out = std::clamp(value, 0.0f, 1.0f);
  return true;
}

/// `<hue>`: an angle, or a bare number read as degrees.
bool parse_color_hue(std::string_view text, float &degrees) {
  float radians = 0.0f;
  if (parse_css_angle(text, radians)) {
    degrees = radians * kDegreesPerRadian;
    return true;
  }
  return parse_finite_number(text, degrees);
}

/// Splits a colour function's arguments into three components plus an alpha,
/// accepting both the legacy `a, b, c, alpha` form and the modern
/// `a b c / alpha` one.
bool split_color_arguments(std::string_view body,
                           std::array<std::string_view, 3> &components,
                           std::string_view &alpha) {
  alpha = {};
  const std::vector<std::string_view> halves = split_top_level(body, '/');
  if (halves.size() > 2)
    return false;
  if (halves.size() == 2) {
    // A slash with nothing after it is not an omitted alpha, it is a typo.
    if (halves[1].empty())
      return false;
    alpha = halves[1];
  }

  std::vector<std::string_view> parts = split_top_level(halves[0], ',');
  if (parts.size() == 1)
    parts = split_whitespace(style_trim(halves[0]));
  if (parts.size() == 4) {
    if (!alpha.empty())
      return false;
    alpha = parts[3];
    parts.resize(3);
  }
  if (parts.size() != 3)
    return false;
  components = {parts[0], parts[1], parts[2]};
  return true;
}

/// CSS Color 4's hsl-to-rgb, written as the spec writes it.
Color hsl_to_color(float hue_degrees, float saturation, float lightness,
                   float alpha) {
  const float wrapped =
      std::fmod(std::fmod(hue_degrees, 360.0f) + 360.0f, 360.0f) / 30.0f;
  saturation = std::clamp(saturation, 0.0f, 1.0f);
  lightness = std::clamp(lightness, 0.0f, 1.0f);
  const float amplitude =
      saturation * std::min(lightness, 1.0f - lightness);
  const auto channel = [&](float offset) {
    const float k = std::fmod(offset + wrapped, 12.0f);
    return lightness -
           amplitude * std::max(-1.0f, std::min({k - 3.0f, 9.0f - k, 1.0f}));
  };
  return Color::from_float(channel(0.0f), channel(8.0f), channel(4.0f), alpha);
}

Color hwb_to_color(float hue_degrees, float whiteness, float blackness,
                   float alpha) {
  whiteness = std::clamp(whiteness, 0.0f, 1.0f);
  blackness = std::clamp(blackness, 0.0f, 1.0f);
  if (whiteness + blackness >= 1.0f) {
    const float grey = whiteness / (whiteness + blackness);
    return Color::from_float(grey, grey, grey, alpha);
  }
  const Color pure = hsl_to_color(hue_degrees, 1.0f, 0.5f, alpha);
  const float span = 1.0f - whiteness - blackness;
  return Color::from_float(pure.r * span + whiteness, pure.g * span + whiteness,
                           pure.b * span + whiteness, alpha);
}

/// `in <space>` or `in <polar-space> <hue-method> hue`, reading from the front
/// of `words` and reporting how many of them it took -- a gradient may write
/// its geometry on either side of the method, so the caller needs to know
/// where the method ended.
bool parse_interpolation_method(std::span<const std::string_view> words,
                                ColorInterpolationMethod &out,
                                std::size_t &consumed) {
  if (words.size() < 2 || words[0] != "in")
    return false;

  ColorInterpolationMethod method;
  method.specified = true;
  const std::string_view space = words[1];
  if (space == "srgb")
    method.space = ColorInterpolationSpace::Srgb;
  else if (space == "srgb-linear")
    method.space = ColorInterpolationSpace::SrgbLinear;
  else if (space == "display-p3")
    method.space = ColorInterpolationSpace::DisplayP3;
  else if (space == "oklab")
    method.space = ColorInterpolationSpace::Oklab;
  else if (space == "oklch")
    method.space = ColorInterpolationSpace::Oklch;
  else
    return false;

  // A hue-interpolation method is spelled out in full -- `longer hue`, not
  // `longer` -- so the word `hue` in fourth place is what tells one from a
  // gradient geometry that simply follows the space name.
  if (words.size() < 4 || words[3] != "hue") {
    out = method;
    consumed = 2;
    return true;
  }
  if (!is_polar(method.space))
    return false;
  if (words[2] == "shorter")
    method.hue = HueInterpolation::Shorter;
  else if (words[2] == "longer")
    method.hue = HueInterpolation::Longer;
  else if (words[2] == "increasing")
    method.hue = HueInterpolation::Increasing;
  else if (words[2] == "decreasing")
    method.hue = HueInterpolation::Decreasing;
  else
    return false;

  out = method;
  consumed = 4;
  return true;
}

/// What a gradient's first argument may hold before the stops begin.
struct GradientPrelude {
  /// The `to right` or `45deg` or `from 90deg` half, if it was written.
  std::string_view geometry;
  ColorInterpolationMethod method;
  bool has_method = false;
  /// False when the argument held neither, which means it is the first stop.
  bool present = false;
};

/// Reads `[<geometry>] || <color-interpolation-method>` -- CSS joins the two
/// with `||`, so they may appear in either order -- out of a gradient's first
/// argument. False only when a method was written and what surrounds it is
/// malformed; an argument that is simply a colour stop comes back `present`
/// false.
bool parse_gradient_prelude(std::string_view argument, GradientPrelude &out) {
  argument = style_trim(argument);
  const std::vector<std::string_view> words = split_whitespace(argument);

  std::size_t at = words.size();
  for (std::size_t i = 0; i < words.size(); ++i) {
    if (words[i] == "in") {
      at = i;
      break;
    }
  }

  if (at == words.size()) {
    out.geometry = argument;
    out.present = !argument.empty();
    return true;
  }

  std::size_t consumed = 0;
  if (!parse_interpolation_method(
          std::span<const std::string_view>(words.data() + at,
                                            words.size() - at),
          out.method, consumed))
    return false;
  out.has_method = true;
  out.present = true;

  // The words are views into `argument`, so either side of the method is a
  // substring of it.
  const auto offset_of = [argument](std::string_view word) {
    return static_cast<std::size_t>(word.data() - argument.data());
  };
  const std::string_view head =
      style_trim(argument.substr(0, offset_of(words[at])));
  const std::size_t tail_at = at + consumed;
  const std::string_view tail =
      tail_at < words.size()
          ? style_trim(argument.substr(offset_of(words[tail_at])))
          : std::string_view();

  // The geometry sits on one side or the other, never split across both.
  if (!head.empty() && !tail.empty())
    return false;
  out.geometry = head.empty() ? tail : head;
  return true;
}

bool parse_css_direction(std::string_view text,
                         LinearGradient::Direction &out) {
  const std::vector<std::string_view> words =
      split_whitespace(style_trim(text));
  if (words.size() < 2 || words.size() > 3 || words[0] != "to")
    return false;

  bool top = false, right = false, bottom = false, left = false;
  for (std::size_t i = 1; i < words.size(); ++i) {
    if (words[i] == "top")
      top = true;
    else if (words[i] == "right")
      right = true;
    else if (words[i] == "bottom")
      bottom = true;
    else if (words[i] == "left")
      left = true;
    else
      return false;
  }
  if ((top && bottom) || (left && right) ||
      static_cast<int>(top) + static_cast<int>(right) +
              static_cast<int>(bottom) + static_cast<int>(left) !=
          static_cast<int>(words.size() - 1))
    return false;

  if (top && right)
    out = LinearGradient::Direction::TopRight;
  else if (right && bottom)
    out = LinearGradient::Direction::BottomRight;
  else if (bottom && left)
    out = LinearGradient::Direction::BottomLeft;
  else if (left && top)
    out = LinearGradient::Direction::TopLeft;
  else if (top)
    out = LinearGradient::Direction::Top;
  else if (right)
    out = LinearGradient::Direction::Right;
  else if (bottom)
    out = LinearGradient::Direction::Bottom;
  else if (left)
    out = LinearGradient::Direction::Left;
  else
    return false;
  return true;
}

bool parse_gradient_stop_position(std::string_view text, Color color,
                                  GradientStop &out) {
  text = style_trim(text);
  float value = 0.0f;
  if (text.ends_with("%")) {
    if (!parse_finite_number(text.substr(0, text.size() - 1), value))
      return false;
    out = GradientStop(color, value / 100.0f);
    return true;
  }
  if (text.ends_with("px")) {
    if (!parse_finite_number(text.substr(0, text.size() - 2), value))
      return false;
    out = GradientStop::length(color, value);
    return true;
  }
  if (!parse_finite_number(text, value) || value != 0.0f)
    return false;
  out = GradientStop(color, 0.0f);
  return true;
}

/// `<color> [<angle> | <percentage>]{0,2}` around the turn. Two positions on
/// one colour expand into a hard band, exactly as in the linear case.
bool parse_conic_stop_position(std::string_view text, Color color,
                               GradientStop &out) {
  text = style_trim(text);
  if (text.ends_with("%")) {
    float value = 0.0f;
    if (!parse_finite_number(text.substr(0, text.size() - 1), value))
      return false;
    out = GradientStop(color, value / 100.0f);
    return true;
  }
  float radians = 0.0f;
  if (!parse_css_angle(text, radians))
    return false;
  out = GradientStop(color, radians / kTwoPiRadians);
  return true;
}

/// Reads the comma-separated `<color-stop-list>` that both gradient functions
/// end with. `position` differs between them -- a fraction of the line, or a
/// fraction of the turn -- so it arrives as a parser.
bool parse_color_stops(const std::vector<std::string_view> &arguments,
                       std::size_t first,
                       bool (*position)(std::string_view, Color,
                                        GradientStop &),
                       std::array<GradientStop, kMaxGradientStops> &stops,
                       std::size_t &count) {
  count = 0;
  for (std::size_t i = first; i < arguments.size(); ++i) {
    const std::vector<std::string_view> words = split_whitespace(arguments[i]);
    if (words.empty() || words.size() > 3)
      return false;

    Color color = Color::TRANSPARENT;
    if (!parse_style_value(words[0], color))
      return false;
    if (words.size() == 1) {
      if (count >= stops.size())
        return false;
      stops[count++] = GradientStop(color);
      continue;
    }

    for (std::size_t at = 1; at < words.size(); ++at) {
      if (count >= stops.size() || !position(words[at], color, stops[count]))
        return false;
      ++count;
    }
  }
  return count >= 2;
}

bool parse_css_linear_gradient(const std::vector<std::string_view> &arguments,
                               LinearGradient &out) {
  if (arguments.size() < 2)
    return false;

  GradientPrelude prelude;
  if (!parse_gradient_prelude(arguments[0], prelude))
    return false;

  std::size_t first_stop = prelude.present ? 1 : 0;
  float angle = 0.0f;
  LinearGradient::Direction direction = LinearGradient::Direction::Bottom;
  enum class Geometry { Direction, Angle } geometry = Geometry::Direction;
  if (!prelude.geometry.empty()) {
    if (parse_css_direction(prelude.geometry, direction)) {
      // Nothing else to do: `direction` is already set.
    } else if (parse_css_angle(prelude.geometry, angle)) {
      geometry = Geometry::Angle;
    } else if (prelude.has_method) {
      // A method was written, so this argument is a prelude and whatever sits
      // beside the space name has to be a direction or an angle.
      return false;
    } else {
      // Neither: the argument was the first colour stop all along.
      first_stop = 0;
    }
  }

  std::array<GradientStop, kMaxGradientStops> stops{
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT)};
  std::size_t count = 0;
  if (!parse_color_stops(arguments, first_stop, &parse_gradient_stop_position,
                         stops, count))
    return false;

  const std::span<const GradientStop> values(stops.data(), count);
  out = geometry == Geometry::Angle
            ? LinearGradient::css_angle(angle, values)
            : LinearGradient::css_direction(direction, values);
  if (prelude.has_method)
    out = out.with_interpolation(prelude.method);
  return true;
}

bool parse_css_conic_gradient(const std::vector<std::string_view> &arguments,
                              ConicGradient &out) {
  if (arguments.size() < 2)
    return false;

  GradientPrelude prelude;
  if (!parse_gradient_prelude(arguments[0], prelude))
    return false;

  std::size_t first_stop = prelude.present ? 1 : 0;
  float angle = 0.0f;
  if (!prelude.geometry.empty()) {
    const std::vector<std::string_view> geometry =
        split_whitespace(prelude.geometry);
    if (geometry.size() == 2 && geometry[0] == "from" &&
        parse_css_angle(geometry[1], angle)) {
      // Nothing else to do: `angle` is already set.
    } else if (prelude.has_method) {
      return false;
    } else {
      first_stop = 0;
    }
  }

  std::array<GradientStop, kMaxGradientStops> stops{
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT)};
  std::size_t count = 0;
  if (!parse_color_stops(arguments, first_stop, &parse_conic_stop_position,
                         stops, count))
    return false;

  out = ConicGradient(angle,
                      std::span<const GradientStop>(stops.data(), count));
  if (prelude.has_method)
    out = out.with_interpolation(prelude.method);
  return true;
}

} // namespace

std::string_view style_trim(std::string_view text) {
  std::size_t begin = 0;
  std::size_t end = text.size();
  while (begin < end && is_space(text[begin]))
    ++begin;
  while (end > begin && is_space(text[end - 1]))
    --end;
  return text.substr(begin, end - begin);
}

bool parse_style_value(std::string_view text, float &out) {
  text = style_trim(text);
  if (text.empty())
    return false;

  // A trailing unit is accepted and ignored: lengths are logical pixels
  // throughout, and writing `px` is a habit worth tolerating.
  if (text.size() > 2 && text.compare(text.size() - 2, 2, "px") == 0)
    text = style_trim(text.substr(0, text.size() - 2));
  if (!text.empty() && text.back() == '%')
    text = text.substr(0, text.size() - 1);
  if (text.empty())
    return false;

  const std::string buffer(text);
  char *end = nullptr;
  const float value = std::strtof(buffer.c_str(), &end);
  if (end == buffer.c_str() || *end != '\0')
    return false;
  out = value;
  return true;
}

bool parse_style_value(std::string_view text, bool &out) {
  text = style_trim(text);
  if (text == "true" || text == "yes" || text == "1") {
    out = true;
    return true;
  }
  if (text == "false" || text == "no" || text == "0") {
    out = false;
    return true;
  }
  return false;
}

bool parse_style_value(std::string_view text, FontWeight &out) {
  text = style_trim(text);
  if (text == "normal") {
    out = FontWeight::Normal;
    return true;
  }
  if (text == "bold") {
    out = FontWeight::Bold;
    return true;
  }
  if (text == "thin") {
    out = FontWeight::Thin;
    return true;
  }
  if (text == "extra-light" || text == "ultra-light") {
    out = FontWeight::ExtraLight;
    return true;
  }
  if (text == "light") {
    out = FontWeight::Light;
    return true;
  }
  if (text == "medium") {
    out = FontWeight::Medium;
    return true;
  }
  if (text == "semi-bold" || text == "demi-bold") {
    out = FontWeight::SemiBold;
    return true;
  }
  if (text == "extra-bold" || text == "ultra-bold") {
    out = FontWeight::ExtraBold;
    return true;
  }
  if (text == "black" || text == "heavy") {
    out = FontWeight::Black;
    return true;
  }

  unsigned value = 0;
  const auto parsed =
      std::from_chars(text.data(), text.data() + text.size(), value);
  if (text.empty() || parsed.ec != std::errc{} ||
      parsed.ptr != text.data() + text.size() || value < 1 || value > 1000)
    return false;
  out = static_cast<FontWeight>(value);
  return true;
}

// -- FontFamilyList ----------------------------------------------------------

FontFamilyList FontFamilyList::of(std::vector<std::string> families) {
  std::erase_if(families,
                [](const std::string &name) { return name.empty(); });
  if (families.empty())
    return {};

  auto data = std::make_shared<Data>();
  data->hash = 0xcbf29ce484222325ULL;
  for (const std::string &name : families)
    data->hash = style_hash_combine(data->hash,
                                    style_hash_bytes(name.data(), name.size()));
  data->families = std::move(families);

  FontFamilyList list;
  list.data_ = std::move(data);
  return list;
}

const std::string &FontFamilyList::primary() const {
  static const std::string none;
  return data_ ? data_->families.front() : none;
}

bool FontFamilyList::operator==(const FontFamilyList &other) const {
  if (data_ == other.data_)
    return true;
  if (!data_ || !other.data_ || data_->hash != other.data_->hash)
    return false;
  return data_->families == other.data_->families;
}

std::string FontFamilyList::to_string() const {
  std::string out;
  for (const std::string &name : families()) {
    if (!out.empty())
      out += ", ";
    // A name needing quotes is one that would not read back as identifiers.
    const bool bare =
        !name.empty() && !is_digit(name.front()) &&
        std::all_of(name.begin(), name.end(), [](char c) {
          return is_family_char(c) || c == ' ';
        });
    if (bare)
      out += name;
    else
      out += '"' + name + '"';
  }
  return out;
}

bool parse_style_value(std::string_view text, FontFamilyList &out) {
  std::vector<std::string> families;

  std::size_t at = 0;
  while (at <= text.size()) {
    // Find the comma ending this name, ignoring commas inside quotes.
    std::size_t end = at;
    char quote = 0;
    while (end < text.size()) {
      const char c = text[end];
      if (quote) {
        if (c == quote)
          quote = 0;
      } else if (c == '"' || c == '\'') {
        quote = c;
      } else if (c == ',') {
        break;
      }
      ++end;
    }
    if (quote)
      return false; // unterminated string

    std::string_view item = style_trim(text.substr(at, end - at));
    if (item.empty())
      return false; // an empty slot -- `a, , b` or a trailing comma

    if (item.front() == '"' || item.front() == '\'') {
      if (item.size() < 2 || item.back() != item.front())
        return false;
      // Taken verbatim: a quoted family name is a string, and CSS escapes are
      // not part of the VSS subset.
      const std::string_view inner = item.substr(1, item.size() - 2);
      if (inner.empty() || inner.find(item.front()) != std::string_view::npos)
        return false;
      families.emplace_back(inner);
    } else {
      // A run of identifiers. Internal whitespace folds to one space, so
      // `Helvetica   Neue` and `Helvetica Neue` are the same family.
      std::string name;
      bool space = false;
      for (const char c : item) {
        if (is_space(c)) {
          space = !name.empty();
          continue;
        }
        if (!is_family_char(c))
          return false;
        if (space) {
          name += ' ';
          space = false;
        }
        name += c;
      }
      if (name.empty() || is_digit(name.front()))
        return false;
      families.push_back(std::move(name));
    }

    if (end == text.size())
      break;
    at = end + 1;
  }

  if (families.empty())
    return false;

  out = FontFamilyList::of(std::move(families));
  return true;
}

bool parse_style_value(std::string_view text, Length &out) {
  text = style_trim(text);
  if (text == "auto") {
    out = Length::Auto{};
    return true;
  }
  if (text == "fill") {
    out = Length::Fill{};
    return true;
  }

  if (text.size() > 2 && text.substr(text.size() - 2) == "fr") {
    const std::string_view weight = style_trim(text.substr(0, text.size() - 2));
    unsigned value = 0;
    const auto parsed =
        std::from_chars(weight.data(), weight.data() + weight.size(), value);
    if (weight.empty() || parsed.ec != std::errc{} ||
        parsed.ptr != weight.data() + weight.size() || value == 0 ||
        value > std::numeric_limits<std::uint16_t>::max())
      return false;
    out = Length::Flex{static_cast<std::uint16_t>(value)};
    return true;
  }

  if (!text.empty() && text.back() == '%')
    return false;
  float value = 0.0f;
  if (!parse_style_value(text, value) || !std::isfinite(value) || value < 0.0f)
    return false;
  out = Length::Fixed{value};
  return true;
}

bool parse_style_value(std::string_view text, MarginValue &out) {
  text = style_trim(text);
  if (text == "auto") {
    out = MarginValue::Auto{};
    return true;
  }

  float value = 0.0f;
  if (!parse_style_value(text, value) || !std::isfinite(value))
    return false;
  out = MarginValue(value);
  return true;
}

namespace {

/// One operand of color-mix(): a colour with an optional percentage on either
/// side of it.
bool parse_mix_operand(std::string_view text, Color &out, float &percentage) {
  std::vector<std::string_view> words = split_whitespace(style_trim(text));
  percentage = -1.0f;

  const auto read_percentage = [&percentage](std::string_view value) {
    if (!value.ends_with("%"))
      return false;
    return parse_finite_number(value.substr(0, value.size() - 1), percentage) &&
           percentage >= 0.0f;
  };

  if (words.size() == 2) {
    if (read_percentage(words[1]))
      words.pop_back();
    else if (read_percentage(words[0]))
      words.erase(words.begin());
    else
      return false;
  } else if (words.size() != 1) {
    return false;
  }
  return parse_style_value(words[0], out);
}

/// `color-mix(in <space>, <color> [<percentage>], <color> [<percentage>])`.
bool parse_color_mix(std::string_view body, Color &out) {
  const std::vector<std::string_view> parts = split_top_level(body, ',');
  if (parts.size() != 3)
    return false;

  ColorInterpolationMethod method;
  const std::vector<std::string_view> words = split_whitespace(parts[0]);
  std::size_t consumed = 0;
  if (!parse_interpolation_method(words, method, consumed) ||
      consumed != words.size())
    return false;

  Color first = Color::TRANSPARENT;
  Color second = Color::TRANSPARENT;
  float first_percent = -1.0f;
  float second_percent = -1.0f;
  if (!parse_mix_operand(parts[1], first, first_percent) ||
      !parse_mix_operand(parts[2], second, second_percent))
    return false;

  // CSS normalises the pair: an omitted percentage is whatever is left over,
  // and a total below 100% scales the result's alpha rather than its colour.
  if (first_percent < 0.0f && second_percent < 0.0f) {
    first_percent = 50.0f;
    second_percent = 50.0f;
  } else if (first_percent < 0.0f) {
    first_percent = std::max(100.0f - second_percent, 0.0f);
  } else if (second_percent < 0.0f) {
    second_percent = std::max(100.0f - first_percent, 0.0f);
  }
  const float total = first_percent + second_percent;
  if (total <= 0.0f)
    return false;

  out = mix_colors(first, second, second_percent / total, method);
  if (total < 100.0f)
    out.a *= total / 100.0f;
  return true;
}

} // namespace

bool parse_style_value(std::string_view text, Color &out) {
  text = style_trim(text);
  if (text.empty())
    return false;

  if (text[0] == '#')
    return parse_hex_color(text, out);

  std::array<std::string_view, 3> components{};
  std::string_view alpha_text;
  std::string_view body;

  if (parse_function_body(text, "rgba", body) ||
      parse_function_body(text, "rgb", body)) {
    float channels[3]{};
    float alpha = 1.0f;
    if (!split_color_arguments(body, components, alpha_text) ||
        !parse_alpha_value(alpha_text, alpha))
      return false;
    for (std::size_t i = 0; i < 3; ++i)
      if (!parse_component(components[i], 1.0f / 255.0f, 1.0f, channels[i]))
        return false;
    out = Color::from_float(std::clamp(channels[0], 0.0f, 1.0f),
                            std::clamp(channels[1], 0.0f, 1.0f),
                            std::clamp(channels[2], 0.0f, 1.0f), alpha);
    return true;
  }

  if (parse_function_body(text, "hsla", body) ||
      parse_function_body(text, "hsl", body)) {
    float hue = 0.0f;
    float saturation = 0.0f;
    float lightness = 0.0f;
    float alpha = 1.0f;
    if (!split_color_arguments(body, components, alpha_text) ||
        !parse_color_hue(components[0], hue) ||
        !parse_component(components[1], 0.01f, 1.0f, saturation) ||
        !parse_component(components[2], 0.01f, 1.0f, lightness) ||
        !parse_alpha_value(alpha_text, alpha))
      return false;
    out = hsl_to_color(hue, saturation, lightness, alpha);
    return true;
  }

  if (parse_function_body(text, "hwb", body)) {
    float hue = 0.0f;
    float whiteness = 0.0f;
    float blackness = 0.0f;
    float alpha = 1.0f;
    if (!split_color_arguments(body, components, alpha_text) ||
        !parse_color_hue(components[0], hue) ||
        !parse_component(components[1], 0.01f, 1.0f, whiteness) ||
        !parse_component(components[2], 0.01f, 1.0f, blackness) ||
        !parse_alpha_value(alpha_text, alpha))
      return false;
    out = hwb_to_color(hue, whiteness, blackness, alpha);
    return true;
  }

  // The a and b axes of Oklab run to roughly +-0.4, which is what CSS pins
  // 100% to; chroma is on the same scale.
  if (parse_function_body(text, "oklab", body)) {
    float lightness = 0.0f;
    float a_axis = 0.0f;
    float b_axis = 0.0f;
    float alpha = 1.0f;
    if (!split_color_arguments(body, components, alpha_text) ||
        !parse_component(components[0], 1.0f, 1.0f, lightness) ||
        !parse_component(components[1], 1.0f, 0.4f, a_axis) ||
        !parse_component(components[2], 1.0f, 0.4f, b_axis) ||
        !parse_alpha_value(alpha_text, alpha))
      return false;
    out = Color::oklab(lightness, a_axis, b_axis, alpha);
    return true;
  }

  if (parse_function_body(text, "oklch", body)) {
    float lightness = 0.0f;
    float chroma = 0.0f;
    float hue = 0.0f;
    float alpha = 1.0f;
    if (!split_color_arguments(body, components, alpha_text) ||
        !parse_component(components[0], 1.0f, 1.0f, lightness) ||
        !parse_component(components[1], 1.0f, 0.4f, chroma) ||
        !parse_color_hue(components[2], hue) ||
        !parse_alpha_value(alpha_text, alpha))
      return false;
    out = Color::oklch(lightness, std::max(chroma, 0.0f), hue, alpha);
    return true;
  }

  if (parse_function_body(text, "color-mix", body))
    return parse_color_mix(body, out);

  if (parse_function_body(text, "color", body)) {
    // `color()` names its space first, so the name is peeled off and the three
    // components that follow are read the same way whichever space it was.
    const std::string_view trimmed = style_trim(body);
    const std::size_t name_end = trimmed.find_first_of(" \t\n\r\f\v");
    if (name_end == std::string_view::npos)
      return false;
    const std::string_view space = trimmed.substr(0, name_end);
    float channels[3]{};
    float alpha = 1.0f;
    if (!split_color_arguments(trimmed.substr(name_end), components,
                               alpha_text) ||
        !parse_alpha_value(alpha_text, alpha))
      return false;
    for (std::size_t i = 0; i < 3; ++i)
      if (!parse_component(components[i], 1.0f, 1.0f, channels[i]))
        return false;

    if (space == "srgb")
      out = Color::srgb(channels[0], channels[1], channels[2], alpha);
    else if (space == "srgb-linear")
      out = Color::srgb_linear(channels[0], channels[1], channels[2], alpha);
    else if (space == "display-p3")
      out = Color::display_p3(channels[0], channels[1], channels[2], alpha);
    else
      return false;
    return true;
  }

  return find_named_color(text.data(), text.size(), out);
}

bool parse_style_value(std::string_view text, Radius &out) {
  const std::vector<std::string_view> parts =
      split_whitespace(style_trim(text));
  float values[4]{};
  if (parts.empty() || parts.size() > 4)
    return false;
  for (std::size_t i = 0; i < parts.size(); ++i)
    if (!parse_style_value(parts[i], values[i]))
      return false;

  switch (parts.size()) {
  case 1:
    out = Radius(values[0]);
    return true;
  case 2:
    // top-left/bottom-right, then top-right/bottom-left, as in CSS.
    out = Radius(values[0], values[1], values[0], values[1]);
    return true;
  case 3:
    out = Radius(values[0], values[1], values[2], values[1]);
    return true;
  default:
    out = Radius(values[0], values[1], values[2], values[3]);
    return true;
  }
}

bool parse_style_value(std::string_view text, Spacing<float> &out) {
  const std::vector<std::string_view> parts =
      split_whitespace(style_trim(text));
  float values[4]{};
  if (parts.empty() || parts.size() > 4)
    return false;
  for (std::size_t i = 0; i < parts.size(); ++i)
    if (!parse_style_value(parts[i], values[i]))
      return false;

  // CSS order -- top right bottom left -- mapped onto Spacing's
  // left/top/right/bottom fields.
  float top = values[0], right = values[0], bottom = values[0],
        left = values[0];
  if (parts.size() == 2) {
    top = bottom = values[0];
    left = right = values[1];
  } else if (parts.size() == 3) {
    top = values[0];
    left = right = values[1];
    bottom = values[2];
  } else if (parts.size() == 4) {
    top = values[0];
    right = values[1];
    bottom = values[2];
    left = values[3];
  }
  out = Spacing<float>(left, top, right, bottom);
  return true;
}

bool parse_style_value(std::string_view text, Spacing<MarginValue> &out) {
  const std::vector<std::string_view> parts =
      split_whitespace(style_trim(text));
  MarginValue values[4]{};
  if (parts.empty() || parts.size() > 4)
    return false;
  for (std::size_t i = 0; i < parts.size(); ++i)
    if (!parse_style_value(parts[i], values[i]))
      return false;

  MarginValue top = values[0];
  MarginValue right = values[0];
  MarginValue bottom = values[0];
  MarginValue left = values[0];
  if (parts.size() == 2) {
    top = bottom = values[0];
    left = right = values[1];
  } else if (parts.size() == 3) {
    top = values[0];
    left = right = values[1];
    bottom = values[2];
  } else if (parts.size() == 4) {
    top = values[0];
    right = values[1];
    bottom = values[2];
    left = values[3];
  }
  out = Spacing<MarginValue>(left, top, right, bottom);
  return true;
}

bool parse_style_value(std::string_view text, Brush &out) {
  text = style_trim(text);

  std::vector<std::string_view> arguments;
  if (parse_function(text, "conic-gradient", arguments)) {
    ConicGradient gradient;
    if (!parse_css_conic_gradient(arguments, gradient))
      return false;
    out = std::move(gradient);
    return true;
  }

  if (parse_function(text, "linear-gradient", arguments)) {
    LinearGradient gradient(Color::TRANSPARENT, Color::TRANSPARENT);
    if (!parse_css_linear_gradient(arguments, gradient))
      return false;
    out = std::move(gradient);
    return true;
  }

  Color color(0, 0, 0);
  if (!parse_style_value(text, color))
    return false;
  out = color;
  return true;
}

bool parse_style_value(std::string_view text, std::string &out) {
  text = style_trim(text);
  if (text.size() >= 2 && (text.front() == '"' || text.front() == '\'') &&
      text.back() == text.front())
    text = text.substr(1, text.size() - 2);
  out.assign(text);
  return true;
}

} // namespace voidui
