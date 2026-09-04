#include "voidui/core/color.h"

#include <algorithm>
#include <cmath>
#include <string_view>

namespace voidui {
namespace {

constexpr float kDegreesPerRadian = 57.295779513082320876f;
constexpr float kRadiansPerDegree = 0.01745329251994329577f;

/// Sign-preserving cube root. std::cbrt already is; spelling it out here keeps
/// the intent visible at the two call sites where a negative LMS value is not
/// an error but an ordinary out-of-gamut colour.
float signed_cbrt(float value) { return std::cbrt(value); }

struct Vec3 {
  float x, y, z;
};

Vec3 linear_srgb_to_oklab(Vec3 c) {
  const float l = 0.4122214708f * c.x + 0.5363325363f * c.y +
                  0.0514459929f * c.z;
  const float m = 0.2119034982f * c.x + 0.6806995451f * c.y +
                  0.1073969566f * c.z;
  const float s = 0.0883024619f * c.x + 0.2817188376f * c.y +
                  0.6299787005f * c.z;

  const float l_ = signed_cbrt(l);
  const float m_ = signed_cbrt(m);
  const float s_ = signed_cbrt(s);

  return {0.2104542553f * l_ + 0.7936177850f * m_ - 0.0040720468f * s_,
          1.9779984951f * l_ - 2.4285922050f * m_ + 0.4505937099f * s_,
          0.0259040371f * l_ + 0.7827717662f * m_ - 0.8086757660f * s_};
}

Vec3 oklab_to_linear_srgb(Vec3 lab) {
  const float l_ = lab.x + 0.3963377774f * lab.y + 0.2158037573f * lab.z;
  const float m_ = lab.x - 0.1055613458f * lab.y - 0.0638541728f * lab.z;
  const float s_ = lab.x - 0.0894841775f * lab.y - 1.2914855480f * lab.z;

  const float l = l_ * l_ * l_;
  const float m = m_ * m_ * m_;
  const float s = s_ * s_ * s_;

  return {+4.0767416621f * l - 3.3077115913f * m + 0.2309699292f * s,
          -1.2684380046f * l + 2.6097574011f * m - 0.3413193965f * s,
          -0.0041960863f * l - 0.7034186147f * m + 1.7076147010f * s};
}

/// Display P3 shares the sRGB transfer function, so only the primaries differ
/// and one matrix each way is the whole conversion.
Vec3 linear_p3_to_linear_srgb(Vec3 c) {
  return {1.2249401762f * c.x - 0.2249401762f * c.y,
          -0.0420569547f * c.x + 1.0420569547f * c.y,
          -0.0196375546f * c.x - 0.0786360454f * c.y + 1.0982736000f * c.z};
}

Vec3 linear_srgb_to_linear_p3(Vec3 c) {
  return {0.8224621000f * c.x + 0.1775380000f * c.y,
          0.0331941000f * c.x + 0.9668058000f * c.y,
          0.0170827000f * c.x + 0.0723974000f * c.y + 0.9105199000f * c.z};
}

Vec3 decode(Vec3 c) {
  return {srgb_decode(c.x), srgb_decode(c.y), srgb_decode(c.z)};
}

Vec3 encode(Vec3 c) {
  return {srgb_encode(c.x), srgb_encode(c.y), srgb_encode(c.z)};
}

/// The ColorSpace tag a mix carries away with it. Two legacy colours mixed in
/// sRGB stay legacy, so a chain of transitions between `#hex` values never
/// drifts into the Oklab default partway through.
ColorSpace tag_for_result(ColorInterpolationSpace space, const Color &a,
                          const Color &b) {
  switch (space) {
  case ColorInterpolationSpace::Srgb:
    return a.space == ColorSpace::LegacyRgb && b.space == ColorSpace::LegacyRgb
               ? ColorSpace::LegacyRgb
               : ColorSpace::Srgb;
  case ColorInterpolationSpace::SrgbLinear:
    return ColorSpace::SrgbLinear;
  case ColorInterpolationSpace::DisplayP3:
    return ColorSpace::DisplayP3;
  case ColorInterpolationSpace::Oklab:
    return ColorSpace::Oklab;
  case ColorInterpolationSpace::Oklch:
    return ColorSpace::Oklch;
  }
  return ColorSpace::Srgb;
}

struct NamedColor {
  std::string_view name;
  Color color;
};

/// The CSS named colours, sorted so the lookup is a binary search rather than
/// a walk of 148 string compares.
constexpr NamedColor kNamedColors[] = {
    {"aliceblue", Color::from_rgb_u32(0xF0F8FF)},
    {"antiquewhite", Color::from_rgb_u32(0xFAEBD7)},
    {"aqua", Color::from_rgb_u32(0x00FFFF)},
    {"aquamarine", Color::from_rgb_u32(0x7FFFD4)},
    {"azure", Color::from_rgb_u32(0xF0FFFF)},
    {"beige", Color::from_rgb_u32(0xF5F5DC)},
    {"bisque", Color::from_rgb_u32(0xFFE4C4)},
    {"black", Color::from_rgb_u32(0x000000)},
    {"blanchedalmond", Color::from_rgb_u32(0xFFEBCD)},
    {"blue", Color::from_rgb_u32(0x0000FF)},
    {"blueviolet", Color::from_rgb_u32(0x8A2BE2)},
    {"brown", Color::from_rgb_u32(0xA52A2A)},
    {"burlywood", Color::from_rgb_u32(0xDEB887)},
    {"cadetblue", Color::from_rgb_u32(0x5F9EA0)},
    {"chartreuse", Color::from_rgb_u32(0x7FFF00)},
    {"chocolate", Color::from_rgb_u32(0xD2691E)},
    {"coral", Color::from_rgb_u32(0xFF7F50)},
    {"cornflowerblue", Color::from_rgb_u32(0x6495ED)},
    {"cornsilk", Color::from_rgb_u32(0xFFF8DC)},
    {"crimson", Color::from_rgb_u32(0xDC143C)},
    {"cyan", Color::from_rgb_u32(0x00FFFF)},
    {"darkblue", Color::from_rgb_u32(0x00008B)},
    {"darkcyan", Color::from_rgb_u32(0x008B8B)},
    {"darkgoldenrod", Color::from_rgb_u32(0xB8860B)},
    {"darkgray", Color::from_rgb_u32(0xA9A9A9)},
    {"darkgreen", Color::from_rgb_u32(0x006400)},
    {"darkgrey", Color::from_rgb_u32(0xA9A9A9)},
    {"darkkhaki", Color::from_rgb_u32(0xBDB76B)},
    {"darkmagenta", Color::from_rgb_u32(0x8B008B)},
    {"darkolivegreen", Color::from_rgb_u32(0x556B2F)},
    {"darkorange", Color::from_rgb_u32(0xFF8C00)},
    {"darkorchid", Color::from_rgb_u32(0x9932CC)},
    {"darkred", Color::from_rgb_u32(0x8B0000)},
    {"darksalmon", Color::from_rgb_u32(0xE9967A)},
    {"darkseagreen", Color::from_rgb_u32(0x8FBC8F)},
    {"darkslateblue", Color::from_rgb_u32(0x483D8B)},
    {"darkslategray", Color::from_rgb_u32(0x2F4F4F)},
    {"darkslategrey", Color::from_rgb_u32(0x2F4F4F)},
    {"darkturquoise", Color::from_rgb_u32(0x00CED1)},
    {"darkviolet", Color::from_rgb_u32(0x9400D3)},
    {"deeppink", Color::from_rgb_u32(0xFF1493)},
    {"deepskyblue", Color::from_rgb_u32(0x00BFFF)},
    {"dimgray", Color::from_rgb_u32(0x696969)},
    {"dimgrey", Color::from_rgb_u32(0x696969)},
    {"dodgerblue", Color::from_rgb_u32(0x1E90FF)},
    {"firebrick", Color::from_rgb_u32(0xB22222)},
    {"floralwhite", Color::from_rgb_u32(0xFFFAF0)},
    {"forestgreen", Color::from_rgb_u32(0x228B22)},
    {"fuchsia", Color::from_rgb_u32(0xFF00FF)},
    {"gainsboro", Color::from_rgb_u32(0xDCDCDC)},
    {"ghostwhite", Color::from_rgb_u32(0xF8F8FF)},
    {"gold", Color::from_rgb_u32(0xFFD700)},
    {"goldenrod", Color::from_rgb_u32(0xDAA520)},
    {"gray", Color::from_rgb_u32(0x808080)},
    {"green", Color::from_rgb_u32(0x008000)},
    {"greenyellow", Color::from_rgb_u32(0xADFF2F)},
    {"grey", Color::from_rgb_u32(0x808080)},
    {"honeydew", Color::from_rgb_u32(0xF0FFF0)},
    {"hotpink", Color::from_rgb_u32(0xFF69B4)},
    {"indianred", Color::from_rgb_u32(0xCD5C5C)},
    {"indigo", Color::from_rgb_u32(0x4B0082)},
    {"ivory", Color::from_rgb_u32(0xFFFFF0)},
    {"khaki", Color::from_rgb_u32(0xF0E68C)},
    {"lavender", Color::from_rgb_u32(0xE6E6FA)},
    {"lavenderblush", Color::from_rgb_u32(0xFFF0F5)},
    {"lawngreen", Color::from_rgb_u32(0x7CFC00)},
    {"lemonchiffon", Color::from_rgb_u32(0xFFFACD)},
    {"lightblue", Color::from_rgb_u32(0xADD8E6)},
    {"lightcoral", Color::from_rgb_u32(0xF08080)},
    {"lightcyan", Color::from_rgb_u32(0xE0FFFF)},
    {"lightgoldenrodyellow", Color::from_rgb_u32(0xFAFAD2)},
    {"lightgray", Color::from_rgb_u32(0xD3D3D3)},
    {"lightgreen", Color::from_rgb_u32(0x90EE90)},
    {"lightgrey", Color::from_rgb_u32(0xD3D3D3)},
    {"lightpink", Color::from_rgb_u32(0xFFB6C1)},
    {"lightsalmon", Color::from_rgb_u32(0xFFA07A)},
    {"lightseagreen", Color::from_rgb_u32(0x20B2AA)},
    {"lightskyblue", Color::from_rgb_u32(0x87CEFA)},
    {"lightslategray", Color::from_rgb_u32(0x778899)},
    {"lightslategrey", Color::from_rgb_u32(0x778899)},
    {"lightsteelblue", Color::from_rgb_u32(0xB0C4DE)},
    {"lightyellow", Color::from_rgb_u32(0xFFFFE0)},
    {"lime", Color::from_rgb_u32(0x00FF00)},
    {"limegreen", Color::from_rgb_u32(0x32CD32)},
    {"linen", Color::from_rgb_u32(0xFAF0E6)},
    {"magenta", Color::from_rgb_u32(0xFF00FF)},
    {"maroon", Color::from_rgb_u32(0x800000)},
    {"mediumaquamarine", Color::from_rgb_u32(0x66CDAA)},
    {"mediumblue", Color::from_rgb_u32(0x0000CD)},
    {"mediumorchid", Color::from_rgb_u32(0xBA55D3)},
    {"mediumpurple", Color::from_rgb_u32(0x9370DB)},
    {"mediumseagreen", Color::from_rgb_u32(0x3CB371)},
    {"mediumslateblue", Color::from_rgb_u32(0x7B68EE)},
    {"mediumspringgreen", Color::from_rgb_u32(0x00FA9A)},
    {"mediumturquoise", Color::from_rgb_u32(0x48D1CC)},
    {"mediumvioletred", Color::from_rgb_u32(0xC71585)},
    {"midnightblue", Color::from_rgb_u32(0x191970)},
    {"mintcream", Color::from_rgb_u32(0xF5FFFA)},
    {"mistyrose", Color::from_rgb_u32(0xFFE4E1)},
    {"moccasin", Color::from_rgb_u32(0xFFE4B5)},
    {"navajowhite", Color::from_rgb_u32(0xFFDEAD)},
    {"navy", Color::from_rgb_u32(0x000080)},
    {"oldlace", Color::from_rgb_u32(0xFDF5E6)},
    {"olive", Color::from_rgb_u32(0x808000)},
    {"olivedrab", Color::from_rgb_u32(0x6B8E23)},
    {"orange", Color::from_rgb_u32(0xFFA500)},
    {"orangered", Color::from_rgb_u32(0xFF4500)},
    {"orchid", Color::from_rgb_u32(0xDA70D6)},
    {"palegoldenrod", Color::from_rgb_u32(0xEEE8AA)},
    {"palegreen", Color::from_rgb_u32(0x98FB98)},
    {"paleturquoise", Color::from_rgb_u32(0xAFEEEE)},
    {"palevioletred", Color::from_rgb_u32(0xDB7093)},
    {"papayawhip", Color::from_rgb_u32(0xFFEFD5)},
    {"peachpuff", Color::from_rgb_u32(0xFFDAB9)},
    {"peru", Color::from_rgb_u32(0xCD853F)},
    {"pink", Color::from_rgb_u32(0xFFC0CB)},
    {"plum", Color::from_rgb_u32(0xDDA0DD)},
    {"powderblue", Color::from_rgb_u32(0xB0E0E6)},
    {"purple", Color::from_rgb_u32(0x800080)},
    {"rebeccapurple", Color::from_rgb_u32(0x663399)},
    {"red", Color::from_rgb_u32(0xFF0000)},
    {"rosybrown", Color::from_rgb_u32(0xBC8F8F)},
    {"royalblue", Color::from_rgb_u32(0x4169E1)},
    {"saddlebrown", Color::from_rgb_u32(0x8B4513)},
    {"salmon", Color::from_rgb_u32(0xFA8072)},
    {"sandybrown", Color::from_rgb_u32(0xF4A460)},
    {"seagreen", Color::from_rgb_u32(0x2E8B57)},
    {"seashell", Color::from_rgb_u32(0xFFF5EE)},
    {"sienna", Color::from_rgb_u32(0xA0522D)},
    {"silver", Color::from_rgb_u32(0xC0C0C0)},
    {"skyblue", Color::from_rgb_u32(0x87CEEB)},
    {"slateblue", Color::from_rgb_u32(0x6A5ACD)},
    {"slategray", Color::from_rgb_u32(0x708090)},
    {"slategrey", Color::from_rgb_u32(0x708090)},
    {"snow", Color::from_rgb_u32(0xFFFAFA)},
    {"springgreen", Color::from_rgb_u32(0x00FF7F)},
    {"steelblue", Color::from_rgb_u32(0x4682B4)},
    {"tan", Color::from_rgb_u32(0xD2B48C)},
    {"teal", Color::from_rgb_u32(0x008080)},
    {"thistle", Color::from_rgb_u32(0xD8BFD8)},
    {"tomato", Color::from_rgb_u32(0xFF6347)},
    {"transparent", Color(0, 0, 0, 0)},
    {"turquoise", Color::from_rgb_u32(0x40E0D0)},
    {"violet", Color::from_rgb_u32(0xEE82EE)},
    {"wheat", Color::from_rgb_u32(0xF5DEB3)},
    {"white", Color::from_rgb_u32(0xFFFFFF)},
    {"whitesmoke", Color::from_rgb_u32(0xF5F5F5)},
    {"yellow", Color::from_rgb_u32(0xFFFF00)},
    {"yellowgreen", Color::from_rgb_u32(0x9ACD32)},
};

} // namespace

float srgb_encode(float linear) noexcept {
  const float magnitude = std::abs(linear);
  const float encoded =
      magnitude <= 0.0031308f
          ? magnitude * 12.92f
          : 1.055f * std::pow(magnitude, 1.0f / 2.4f) - 0.055f;
  return linear < 0.0f ? -encoded : encoded;
}

float srgb_decode(float encoded) noexcept {
  const float magnitude = std::abs(encoded);
  const float linear = magnitude <= 0.04045f
                           ? magnitude / 12.92f
                           : std::pow((magnitude + 0.055f) / 1.055f, 2.4f);
  return encoded < 0.0f ? -linear : linear;
}

Color Color::srgb_linear(float red, float green, float blue, float alpha) {
  return Color(srgb_encode(red), srgb_encode(green), srgb_encode(blue), alpha,
               ColorSpace::SrgbLinear);
}

Color Color::display_p3(float red, float green, float blue, float alpha) {
  const Vec3 linear = linear_p3_to_linear_srgb(
      {srgb_decode(red), srgb_decode(green), srgb_decode(blue)});
  const Vec3 out = encode(linear);
  return Color(out.x, out.y, out.z, alpha, ColorSpace::DisplayP3);
}

Color Color::oklab(float lightness, float a_axis, float b_axis, float alpha) {
  const Vec3 out = encode(oklab_to_linear_srgb({lightness, a_axis, b_axis}));
  return Color(out.x, out.y, out.z, alpha, ColorSpace::Oklab);
}

Color Color::oklch(float lightness, float chroma, float hue_degrees,
                   float alpha) {
  const float radians = hue_degrees * kRadiansPerDegree;
  Color result = Color::oklab(lightness, chroma * std::cos(radians),
                              chroma * std::sin(radians), alpha);
  result.space = ColorSpace::Oklch;
  return result;
}

bool Color::in_gamut() const {
  const float slack = 1.0f / 512.0f;
  return r >= -slack && r <= 1.0f + slack && g >= -slack &&
         g <= 1.0f + slack && b >= -slack && b <= 1.0f + slack;
}

Rgba8 Color::to_rgba8() const {
  const auto quantise = [](float value) {
    return static_cast<std::uint8_t>(std::clamp(value, 0.0f, 1.0f) * 255.0f +
                                     0.5f);
  };
  return Rgba8{quantise(r), quantise(g), quantise(b), quantise(a)};
}

std::array<float, 3> Color::to(ColorInterpolationSpace target) const {
  switch (target) {
  case ColorInterpolationSpace::Srgb:
    return {r, g, b};
  case ColorInterpolationSpace::SrgbLinear: {
    const Vec3 linear = decode({r, g, b});
    return {linear.x, linear.y, linear.z};
  }
  case ColorInterpolationSpace::DisplayP3: {
    const Vec3 p3 = encode(linear_srgb_to_linear_p3(decode({r, g, b})));
    return {p3.x, p3.y, p3.z};
  }
  case ColorInterpolationSpace::Oklab: {
    const Vec3 lab = linear_srgb_to_oklab(decode({r, g, b}));
    return {lab.x, lab.y, lab.z};
  }
  case ColorInterpolationSpace::Oklch: {
    const Vec3 lab = linear_srgb_to_oklab(decode({r, g, b}));
    const float chroma = std::hypot(lab.y, lab.z);
    float hue = std::atan2(lab.z, lab.y) * kDegreesPerRadian;
    if (hue < 0.0f)
      hue += 360.0f;
    return {lab.x, chroma, hue};
  }
  }
  return {r, g, b};
}

Color Color::from(const std::array<float, 3> &components, float alpha,
                  ColorInterpolationSpace source, ColorSpace tag) {
  Color result;
  switch (source) {
  case ColorInterpolationSpace::Srgb:
    result = Color(components[0], components[1], components[2], alpha, tag);
    break;
  case ColorInterpolationSpace::SrgbLinear:
    result = Color::srgb_linear(components[0], components[1], components[2],
                                alpha);
    break;
  case ColorInterpolationSpace::DisplayP3:
    result =
        Color::display_p3(components[0], components[1], components[2], alpha);
    break;
  case ColorInterpolationSpace::Oklab:
    result = Color::oklab(components[0], components[1], components[2], alpha);
    break;
  case ColorInterpolationSpace::Oklch:
    result = Color::oklch(components[0], components[1], components[2], alpha);
    break;
  }
  result.space = tag;
  return result;
}

ColorInterpolationSpace default_interpolation_space(const Color &a,
                                                    const Color &b) {
  // CSS Color 4: legacy colours keep interpolating in sRGB for compatibility;
  // the moment either side is written in a modern syntax the pair moves to
  // Oklab, which is the spec's default everywhere it is free to choose.
  return a.space == ColorSpace::LegacyRgb && b.space == ColorSpace::LegacyRgb
             ? ColorInterpolationSpace::Srgb
             : ColorInterpolationSpace::Oklab;
}

ColorInterpolationMethod resolve_interpolation(ColorInterpolationMethod method,
                                               const Color &a,
                                               const Color &b) {
  if (!method.specified) {
    method.space = default_interpolation_space(a, b);
    method.specified = true;
  }
  return method;
}

float adjust_hue(float from, float hue, HueInterpolation method) {
  float delta = std::fmod(hue - from, 360.0f);
  if (delta < 0.0f)
    delta += 360.0f;

  switch (method) {
  case HueInterpolation::Shorter:
    if (delta > 180.0f)
      delta -= 360.0f;
    break;
  case HueInterpolation::Longer:
    if (delta > 0.0f && delta < 180.0f)
      delta -= 360.0f;
    break;
  case HueInterpolation::Increasing:
    break;
  case HueInterpolation::Decreasing:
    if (delta > 0.0f)
      delta -= 360.0f;
    break;
  }
  return from + delta;
}

Color mix_colors(const Color &a, const Color &b, float t,
                 ColorInterpolationMethod method) {
  const ColorInterpolationMethod resolved = resolve_interpolation(method, a, b);
  const ColorSpace tag = tag_for_result(resolved.space, a, b);
  const float alpha = a.a + (b.a - a.a) * t;

  std::array<float, 3> from = a.to(resolved.space);
  std::array<float, 3> to = b.to(resolved.space);

  const bool polar = is_polar(resolved.space);
  if (polar) {
    // An achromatic colour has no hue to speak of, so it borrows its partner's
    // rather than dragging the mix through an arbitrary arc. Without this,
    // white to red in Oklch sweeps the whole wheel whenever white happens to
    // land on hue zero.
    constexpr float kAchromatic = 1e-4f;
    if (from[1] <= kAchromatic)
      from[2] = to[2];
    else if (to[1] <= kAchromatic)
      to[2] = from[2];
    to[2] = adjust_hue(from[2], to[2], resolved.hue);
  }

  // CSS interpolates with the components premultiplied by alpha, which is what
  // keeps a fade to `transparent` from passing through the transparent
  // colour's own hue. Hue itself is never premultiplied -- it is an angle.
  std::array<float, 3> mixed{};
  const bool premultiplied = alpha > 1e-6f;
  for (std::size_t i = 0; i < 3; ++i) {
    if (polar && i == 2) {
      mixed[i] = from[i] + (to[i] - from[i]) * t;
      continue;
    }
    if (!premultiplied) {
      mixed[i] = from[i] + (to[i] - from[i]) * t;
      continue;
    }
    const float scaled_from = from[i] * a.a;
    const float scaled_to = to[i] * b.a;
    mixed[i] = (scaled_from + (scaled_to - scaled_from) * t) / alpha;
  }

  if (resolved.space == ColorInterpolationSpace::Srgb) {
    // The storage form already is this space, so the round trip through
    // Color::from() would be three conversions that cancel out.
    Color result;
    result.r = mixed[0];
    result.g = mixed[1];
    result.b = mixed[2];
    result.a = alpha;
    result.space = tag;
    return result;
  }
  return Color::from(mixed, alpha, resolved.space, tag);
}

bool find_named_color(const char *name, std::size_t length, Color &out) {
  const std::string_view key(name, length);
  if (key.empty() || key.size() > 24)
    return false;

  // The table is lowercase; CSS keywords are case-insensitive, so fold once
  // into a small buffer rather than teaching the comparator about case.
  char folded[24];
  for (std::size_t i = 0; i < length; ++i) {
    const char c = name[i];
    folded[i] = (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
  }
  const std::string_view lowered(folded, length);

  const auto *end = std::end(kNamedColors);
  const auto *found = std::lower_bound(
      std::begin(kNamedColors), end, lowered,
      [](const NamedColor &entry, std::string_view value) {
        return entry.name < value;
      });
  if (found == end || found->name != lowered)
    return false;
  out = found->color;
  return true;
}

} // namespace voidui
