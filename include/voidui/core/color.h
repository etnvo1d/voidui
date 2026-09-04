#pragma once

#include <array>
#include <cstddef>
#include <cstdint>

namespace voidui {

/// A colour in the form the framebuffer takes it: eight bits per channel,
/// sRGB-encoded, straight (non-premultiplied) alpha.
///
/// This is the device end of the pipeline and nothing else. Authored colour
/// lives in `Color`, which is wider in every sense -- float components, values
/// outside the sRGB gamut, and the space they were written in -- and collapses
/// to this only on its way into a vertex or a texel.
struct Rgba8 {
  std::uint8_t r = 0;
  std::uint8_t g = 0;
  std::uint8_t b = 0;
  std::uint8_t a = 255;

  friend constexpr bool operator==(const Rgba8 &, const Rgba8 &) = default;
};

/// The syntax a colour was written in.
///
/// Components are always *stored* as extended sRGB, so this changes nothing
/// about what is painted. It exists because CSS picks the default
/// interpolation space from the endpoints: two legacy colours mix in sRGB,
/// anything else mixes in Oklab. Losing the distinction would mean either
/// sending every `#hex` transition through Oklab -- which is not what the web
/// does -- or never honouring an authored `oklch()` at all.
enum class ColorSpace : std::uint8_t {
  /// `#rgb`, `rgb()`, `rgba()`, `hsl()`, `hwb()`, a named colour.
  LegacyRgb,
  /// `color(srgb ...)`.
  Srgb,
  /// `color(srgb-linear ...)`.
  SrgbLinear,
  /// `color(display-p3 ...)`.
  DisplayP3,
  /// `oklab()`.
  Oklab,
  /// `oklch()`.
  Oklch,
};

/// The space a pair of colours is actually mixed in.
///
/// Deliberately a different enum from ColorSpace: `LegacyRgb` and `Srgb` are
/// two ways of writing the same numbers and mix identically, so a mixing space
/// offering both would carry a state that means nothing.
enum class ColorInterpolationSpace : std::uint8_t {
  Srgb,
  SrgbLinear,
  DisplayP3,
  Oklab,
  Oklch,
};

/// True for spaces whose third component is an angle.
constexpr bool is_polar(ColorInterpolationSpace space) {
  return space == ColorInterpolationSpace::Oklch;
}

/// Which way round the hue circle a polar mix travels. Rectangular spaces
/// ignore it.
enum class HueInterpolation : std::uint8_t {
  Shorter,
  Longer,
  Increasing,
  Decreasing,
};

/// A CSS `<color-interpolation-method>` -- the `in oklch longer hue` that may
/// precede a gradient's stops.
struct ColorInterpolationMethod {
  ColorInterpolationSpace space = ColorInterpolationSpace::Oklab;
  HueInterpolation hue = HueInterpolation::Shorter;

  /// False when the author named no space. The space is then taken from the
  /// endpoints rather than from this field, which is why the two cannot be
  /// collapsed into one.
  bool specified = false;

  friend constexpr bool operator==(const ColorInterpolationMethod &,
                                   const ColorInterpolationMethod &) = default;
};

/// sRGB transfer function, extended below zero and above one by odd symmetry.
///
/// The extension is what lets a colour outside the sRGB gamut -- most of the
/// useful range of `oklch()` is -- round-trip through this representation
/// instead of being clipped the moment it is parsed. Clipping happens once, at
/// `Color::to_rgba8()`.
float srgb_encode(float linear) noexcept;
float srgb_decode(float encoded) noexcept;

/// An authored colour.
///
/// Stored as extended sRGB with a float per channel plus a tag saying where
/// the value came from. Every space this framework understands is an analytic
/// transform of that, so one storage form serves all of them and the hot path
/// -- handing a colour to the GPU -- stays a clamp and a multiply rather than
/// a matrix and a cube root.
struct Color {
  /// Extended sRGB, transfer-encoded. Components outside 0..1 are meaningful.
  float r = 0.0f;
  float g = 0.0f;
  float b = 0.0f;
  float a = 1.0f;

  ColorSpace space = ColorSpace::LegacyRgb;

  static const Color TRANSPARENT;

  constexpr Color() = default;

  constexpr Color(std::uint8_t red, std::uint8_t green, std::uint8_t blue,
                  std::uint8_t alpha = 255)
      : r(static_cast<float>(red) / 255.0f),
        g(static_cast<float>(green) / 255.0f),
        b(static_cast<float>(blue) / 255.0f),
        a(static_cast<float>(alpha) / 255.0f) {}

  /// Creates a Color from red, green, blue, and alpha values in the range
  /// [0.0, 1.0].
  constexpr static Color from_float(float red, float green, float blue,
                                    float alpha = 1.0f) {
    return Color(red, green, blue, alpha, ColorSpace::LegacyRgb);
  }

  constexpr static Color from_rgba_u32(std::uint32_t hex) {
    return Color(static_cast<std::uint8_t>(hex >> 24),
                 static_cast<std::uint8_t>((hex >> 16) & 0xFF),
                 static_cast<std::uint8_t>((hex >> 8) & 0xFF),
                 static_cast<std::uint8_t>(hex & 0xFF));
  }

  constexpr static Color from_rgb_u32(std::uint32_t hex) {
    return Color(static_cast<std::uint8_t>(hex >> 16),
                 static_cast<std::uint8_t>((hex >> 8) & 0xFF),
                 static_cast<std::uint8_t>(hex & 0xFF));
  }

  /// `color(srgb r g b / a)` -- the same numbers as from_float(), tagged as
  /// modern syntax so that mixing with it defaults to Oklab.
  constexpr static Color srgb(float red, float green, float blue,
                              float alpha = 1.0f) {
    return Color(red, green, blue, alpha, ColorSpace::Srgb);
  }

  static Color srgb_linear(float red, float green, float blue,
                           float alpha = 1.0f);
  static Color display_p3(float red, float green, float blue,
                          float alpha = 1.0f);
  static Color oklab(float lightness, float a_axis, float b_axis,
                     float alpha = 1.0f);
  static Color oklch(float lightness, float chroma, float hue_degrees,
                     float alpha = 1.0f);

  constexpr Color with_alpha(float alpha) const {
    Color result = *this;
    result.a = alpha;
    return result;
  }

  constexpr Color with_space(ColorSpace tag) const {
    Color result = *this;
    result.space = tag;
    return result;
  }

  /// True when every component already lies inside the sRGB gamut, so
  /// `to_rgba8()` will not have to clip.
  bool in_gamut() const;

  /// Clips into the sRGB gamut and quantises. The one lossy step in the
  /// pipeline, and it happens at the device boundary and nowhere else.
  Rgba8 to_rgba8() const;

  /// The three components this colour has in `target`, alpha excluded. Polar
  /// spaces report hue in degrees, not normalised to any range.
  std::array<float, 3> to(ColorInterpolationSpace target) const;

  static Color from(const std::array<float, 3> &components, float alpha,
                    ColorInterpolationSpace source, ColorSpace tag);

  friend constexpr bool operator==(const Color &, const Color &) = default;

private:
  constexpr Color(float red, float green, float blue, float alpha,
                  ColorSpace tag)
      : r(red), g(green), b(blue), a(alpha), space(tag) {}
};

inline constexpr Color Color::TRANSPARENT{0, 0, 0, 0};

/// The space CSS mixes `a` and `b` in when the author named none: sRGB while
/// both were written in legacy syntax, Oklab as soon as either was not.
ColorInterpolationSpace default_interpolation_space(const Color &a,
                                                    const Color &b);

/// Folds an unspecified method against the endpoints, so the result always
/// names a concrete space.
ColorInterpolationMethod resolve_interpolation(ColorInterpolationMethod method,
                                               const Color &a, const Color &b);

/// `a` at t = 0, `b` at t = 1, mixed the way CSS mixes colours: in the space
/// the method names, with the components premultiplied by alpha so that a fade
/// through `transparent` does not travel through black.
///
/// With the default method this is the CSS rule for a transition and for a
/// gradient that named no space.
Color mix_colors(const Color &a, const Color &b, float t,
                 ColorInterpolationMethod method = {});

/// Rewrites `hue` so that lerping towards it from `from` takes the arc
/// `method` asks for. Degrees; the result may leave 0..360, which is exactly
/// what makes the plain lerp afterwards correct.
float adjust_hue(float from, float hue, HueInterpolation method);

/// One of the 148 CSS named colours, or `transparent`. Case-insensitive.
bool find_named_color(const char *name, std::size_t length, Color &out);

} // namespace voidui
