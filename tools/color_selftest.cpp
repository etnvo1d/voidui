// Exercise of the colour system: the CSS colour syntaxes, the spaces they name,
// how a pair of them is mixed, and how a gradient carries an interpolation
// method through parsing, equality and animation.
#include "voidui/core/style.h"

#include <cmath>
#include <cstdio>
#include <vector>

using namespace voidui;

namespace {

int failures = 0;

void check(bool condition, const char *what) {
  if (!condition) {
    ++failures;
    std::printf("  FAIL  %s\n", what);
  } else {
    std::printf("  ok    %s\n", what);
  }
}

bool near(float a, float b, float epsilon = 0.001f) {
  return std::abs(a - b) <= epsilon;
}

Color parse(const char *text) {
  Color color = Color::TRANSPARENT;
  if (!parse_style_value(text, color))
    return Color::from_float(-1.0f, -1.0f, -1.0f, -1.0f);
  return color;
}

bool bytes(const char *text, std::uint8_t r, std::uint8_t g, std::uint8_t b,
           std::uint8_t a = 255) {
  return parse(text).to_rgba8() == Rgba8{r, g, b, a};
}

Brush parse_brush(const char *text) {
  Brush brush = Color::TRANSPARENT;
  if (!parse_style_value(text, brush))
    return Color::from_float(-1.0f, -1.0f, -1.0f, -1.0f);
  return brush;
}

} // namespace

int main() {
  std::puts("colour self-test");

  // -- Legacy syntaxes -------------------------------------------------------

  check(bytes("#e5e5e5", 229, 229, 229), "six-digit hex");
  check(bytes("#0f08", 0, 255, 0, 136), "four-digit hex doubles each nibble");
  check(bytes("rebeccapurple", 102, 51, 153),
        "the full CSS named-colour table is present");
  check(bytes("transparent", 0, 0, 0, 0), "transparent is a named colour");
  check(bytes("DarkSlateGrey", 47, 79, 79),
        "named colours are case-insensitive and include the -grey spellings");
  Color scratch(0, 0, 0);
  Brush scratch_brush = Color::TRANSPARENT;
  check(!parse_style_value("notacolour", scratch),
        "an unknown keyword is not a colour");

  check(bytes("rgb(255, 0, 0)", 255, 0, 0), "legacy comma-separated rgb()");
  check(bytes("rgba(255, 0, 0, 0.5)", 255, 0, 0, 128),
        "legacy rgba() reads its fourth argument as a 0..1 alpha");
  check(bytes("rgb(255 0 0 / 50%)", 255, 0, 0, 128),
        "modern rgb() with a slash-separated percentage alpha");
  check(bytes("rgb(100% 0% 0%)", 255, 0, 0), "rgb() percentages");
  check(bytes("hsl(120 100% 50%)", 0, 255, 0), "hsl()");
  check(bytes("hsl(120deg, 100%, 50%)", 0, 255, 0),
        "hsl() accepts an angle unit and the legacy comma form");
  check(bytes("hwb(0 0% 0%)", 255, 0, 0), "hwb()");
  check(bytes("hwb(0 50% 0%)", 255, 128, 128), "hwb() whiteness");

  check(parse("#f00").space == ColorSpace::LegacyRgb &&
            parse("hsl(0 100% 50%)").space == ColorSpace::LegacyRgb,
        "every legacy syntax is tagged as legacy sRGB");

  // -- Modern syntaxes -------------------------------------------------------

  {
    const Color modern = parse("color(srgb 1 0 0)");
    check(modern.space == ColorSpace::Srgb && modern.to_rgba8() == Rgba8{255, 0, 0, 255},
          "color(srgb ...) is red, tagged as modern syntax");
    check(!(modern == parse("#f00")),
          "the same numbers written two ways are not the same colour");

    const Color linear = parse("color(srgb-linear 1 0 0)");
    check(linear.space == ColorSpace::SrgbLinear &&
              linear.to_rgba8() == Rgba8{255, 0, 0, 255},
          "color(srgb-linear 1 0 0) encodes back to full red");
    check(near(parse("color(srgb-linear 0.5 0.5 0.5)").r, srgb_encode(0.5f)),
          "a linear component is stored transfer-encoded");

    const Color wide = parse("color(display-p3 1 0 0)");
    check(!wide.in_gamut() && wide.r > 1.0f,
          "display-p3 red sits outside sRGB and is kept, not clipped");
    check(wide.to_rgba8() == Rgba8{255, 0, 0, 255},
          "it clips only on the way to the device");
  }

  {
    // sRGB red is Oklab(0.6280, 0.2249, 0.1258).
    const Color red = parse("oklab(0.6279554 0.2248631 0.1258463)");
    check(red.space == ColorSpace::Oklab && red.to_rgba8() == Rgba8{255, 0, 0, 255},
          "oklab() round-trips sRGB red");
    check(bytes("oklab(62.79554% 0.2248631 0.1258463)", 255, 0, 0),
          "oklab() lightness accepts a percentage");

    const std::array<float, 3> lab =
        Color(255, 0, 0).to(ColorInterpolationSpace::Oklab);
    check(near(lab[0], 0.62796f) && near(lab[1], 0.22486f) &&
              near(lab[2], 0.12585f),
          "Color::to(Oklab) matches the reference conversion");

    const Color teal = parse("oklch(0.7 0.15 200)");
    const std::array<float, 3> lch = teal.to(ColorInterpolationSpace::Oklch);
    check(teal.space == ColorSpace::Oklch && near(lch[0], 0.7f) &&
              near(lch[1], 0.15f) && near(lch[2], 200.0f, 0.01f),
          "oklch() survives the round trip through extended sRGB");
    check(!teal.in_gamut() && teal.to_rgba8() == Rgba8{0, 185, 195, 255},
          "an out-of-gamut oklch() is clipped once, at the device");
    check(bytes("oklch(0.7 37.5% 200)", 0, 185, 195),
          "oklch() chroma accepts a percentage of 0.4");
    check(bytes("oklch(0.7 0.15 200 / 0.5)", 0, 185, 195, 128),
          "a slash alpha applies to every modern syntax");
  }

  // -- Interpolation ---------------------------------------------------------

  {
    const Color black = parse("#000");
    const Color white = parse("#fff");
    check(mix_colors(black, white, 0.5f).to_rgba8() == Rgba8{128, 128, 128, 255},
          "two legacy colours mix in sRGB, as the web does");
    check(mix_colors(black, white, 0.5f).space == ColorSpace::LegacyRgb,
          "and the result stays legacy, so a chain of them does not drift");

    // Oklab's midpoint between black and white is L = 0.5, which is a good deal
    // darker than the sRGB midpoint -- the whole reason the space exists.
    check(mix_colors(black, parse("color(srgb 1 1 1)"), 0.5f).to_rgba8() ==
              Rgba8{99, 99, 99, 255},
          "one modern endpoint moves the default mixing space to Oklab");
    check(mix_colors(black, white, 0.5f,
                     {ColorInterpolationSpace::Oklab,
                      HueInterpolation::Shorter, true})
                  .to_rgba8() == Rgba8{99, 99, 99, 255},
          "an explicit space overrides the default");

    check(default_interpolation_space(black, white) ==
              ColorInterpolationSpace::Srgb &&
          default_interpolation_space(black, parse("oklch(0.7 0.1 30)")) ==
              ColorInterpolationSpace::Oklab,
          "the default space follows CSS Color 4");

    // Premultiplied mixing: fading red out must not drag it towards the black
    // that `transparent` happens to be.
    const Color faded = mix_colors(parse("red"), parse("transparent"), 0.5f);
    check(faded.to_rgba8() == Rgba8{255, 0, 0, 128},
          "colours are mixed premultiplied by alpha");
  }

  {
    check(near(adjust_hue(350.0f, 10.0f, HueInterpolation::Shorter), 370.0f) &&
              near(adjust_hue(350.0f, 10.0f, HueInterpolation::Longer), 10.0f) &&
              near(adjust_hue(350.0f, 10.0f, HueInterpolation::Increasing),
                   370.0f) &&
              near(adjust_hue(350.0f, 10.0f, HueInterpolation::Decreasing),
                   10.0f),
          "hue interpolation methods pick their arc by unwrapping the angle");

    // Mixing white into a hue in a polar space must not sweep the wheel: white
    // has no hue of its own and borrows its partner's.
    const Color pink = mix_colors(parse("white"), parse("oklch(0.6 0.2 30)"),
                                  0.5f,
                                  {ColorInterpolationSpace::Oklch,
                                   HueInterpolation::Shorter, true});
    check(near(pink.to(ColorInterpolationSpace::Oklch)[2], 30.0f, 0.5f),
          "an achromatic endpoint borrows the hue it is mixing with");
  }

  // -- color-mix() -----------------------------------------------------------

  check(bytes("color-mix(in srgb, red 25%, blue)", 64, 0, 191),
        "color-mix() percentages weight the two sides");
  check(bytes("color-mix(in srgb, red, blue)", 128, 0, 128),
        "omitted percentages mean an even mix");
  check(bytes("color-mix(in srgb, red 25%, blue 25%)", 128, 0, 128, 128),
        "a total below 100% scales the result's alpha");
  check(bytes("color-mix(in oklab, white, black)", 99, 99, 99),
        "color-mix() honours the space it names");
  check(!parse_style_value("color-mix(red, blue)", scratch),
        "color-mix() requires an interpolation method");

  // -- Gradients -------------------------------------------------------------

  {
    const Brush plain = parse_brush("linear-gradient(to right, red, blue)");
    const auto *plain_gradient = std::get_if<LinearGradient>(&plain);
    check(plain_gradient &&
              !plain_gradient->interpolation().specified &&
              plain_gradient->effective_interpolation().space ==
                  ColorInterpolationSpace::Srgb,
          "an all-legacy gradient still ramps through sRGB");

    const Brush modern =
        parse_brush("linear-gradient(to right, oklch(0.7 0.15 200), blue)");
    const auto *modern_gradient = std::get_if<LinearGradient>(&modern);
    check(modern_gradient && modern_gradient->effective_interpolation().space ==
                                 ColorInterpolationSpace::Oklab,
          "one modern stop moves the whole ramp to Oklab");

    const Brush polar =
        parse_brush("linear-gradient(in oklch longer hue, red, blue)");
    const auto *polar_gradient = std::get_if<LinearGradient>(&polar);
    check(polar_gradient &&
              polar_gradient->interpolation() ==
                  ColorInterpolationMethod{ColorInterpolationSpace::Oklch,
                                           HueInterpolation::Longer, true},
          "in <space> <hue-method> hue parses on its own");

    const Brush both =
        parse_brush("linear-gradient(45deg in oklab, red, blue)");
    const auto *both_gradient = std::get_if<LinearGradient>(&both);
    check(both_gradient &&
              both_gradient->geometry() == LinearGradient::Geometry::CssAngle &&
              near(both_gradient->angle(), 0.7853982f) &&
              both_gradient->interpolation().space ==
                  ColorInterpolationSpace::Oklab,
          "an angle and an interpolation method share the first argument");

    const Brush corner =
        parse_brush("linear-gradient(to top right in oklch, red, blue)");
    const auto *corner_gradient = std::get_if<LinearGradient>(&corner);
    check(corner_gradient &&
              corner_gradient->direction() ==
                  LinearGradient::Direction::TopRight &&
              corner_gradient->interpolation().space ==
                  ColorInterpolationSpace::Oklch,
          "so do a corner keyword and an interpolation method");

    const Brush reversed =
        parse_brush("linear-gradient(in oklch shorter hue to right, red, blue)");
    const auto *reversed_gradient = std::get_if<LinearGradient>(&reversed);
    check(reversed_gradient &&
              reversed_gradient->direction() ==
                  LinearGradient::Direction::Right &&
              reversed_gradient->interpolation().space ==
                  ColorInterpolationSpace::Oklch,
          "CSS joins the two with ||, so either order parses");

    const Brush conic_reversed =
        parse_brush("conic-gradient(in oklch from 90deg, red, blue)");
    const auto *conic_reversed_gradient =
        std::get_if<ConicGradient>(&conic_reversed);
    check(conic_reversed_gradient &&
              near(conic_reversed_gradient->angle(), 1.5707964f) &&
              conic_reversed_gradient->interpolation().space ==
                  ColorInterpolationSpace::Oklch,
          "including on a conic gradient");

    check(!parse_style_value("linear-gradient(in nonsense, red, blue)",
                             scratch_brush),
          "an unknown space is rejected rather than silently ignored");
    check(!parse_style_value("linear-gradient(in oklab longer hue, red, blue)",
                             scratch_brush),
          "a hue method is meaningless in a rectangular space");

    check(plain_gradient && polar_gradient &&
              !style_value_equals(*plain_gradient, *polar_gradient) &&
              style_value_hash(*plain_gradient) !=
                  style_value_hash(*polar_gradient),
          "the interpolation method is part of a gradient's identity");
  }

  {
    const Brush wheel = parse_brush(
        "conic-gradient(from 90deg in oklch, red 0deg, blue 180deg, red 1turn)");
    const auto *conic = std::get_if<ConicGradient>(&wheel);
    std::array<Color, kMaxGradientStops> colors{
        Color::TRANSPARENT, Color::TRANSPARENT, Color::TRANSPARENT,
        Color::TRANSPARENT, Color::TRANSPARENT, Color::TRANSPARENT,
        Color::TRANSPARENT, Color::TRANSPARENT};
    std::array<float, kMaxGradientStops> positions{};
    const std::size_t count =
        conic ? conic->resolve_stops(colors, positions) : 0;
    check(conic && near(conic->angle(), 1.5707964f) && count == 3 &&
              near(positions[0], 0.0f) && near(positions[1], 0.5f) &&
              near(positions[2], 1.0f),
          "conic stops take angles, and `from` and `in` share one argument");
    check(conic && conic->interpolation().space ==
                       ColorInterpolationSpace::Oklch,
          "a conic gradient carries its interpolation method too");

    const Brush bare = parse_brush("conic-gradient(red, lime, blue)");
    const auto *bare_conic = std::get_if<ConicGradient>(&bare);
    check(bare_conic && bare_conic->angle() == 0.0f &&
              bare_conic->stops().size() == 3,
          "the whole prelude is optional");

    const Brush percent =
        parse_brush("conic-gradient(red 0%, blue 25% 75%, red)");
    const auto *percent_conic = std::get_if<ConicGradient>(&percent);
    check(percent_conic && percent_conic->stops().size() == 4,
          "a conic stop takes percentages and expands a double position");
  }

  {
    // Animating a gradient keeps whatever space it was authored in.
    Brush from = parse_brush("linear-gradient(in oklch, red, blue)");
    Brush to = parse_brush("linear-gradient(in oklch, blue, red)");
    Brush mixed = from;
    check(interpolate_style_value(from, to, 0.5f, mixed) &&
              std::get<LinearGradient>(mixed).interpolation().space ==
                  ColorInterpolationSpace::Oklch,
          "an animated gradient keeps its interpolation method");

    Brush other = parse_brush("linear-gradient(in oklab, blue, red)");
    check(!interpolate_style_value(from, other, 0.5f, mixed),
          "two gradients mixing in different spaces do not interpolate");
  }

  if (failures == 0)
    std::puts("OK");
  else
    std::printf("FAILED (%d failures)\n", failures);
  return failures == 0 ? 0 : 1;
}
