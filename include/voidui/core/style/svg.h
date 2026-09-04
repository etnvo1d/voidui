#pragma once

#include <array>
#include <cstdint>
#include <span>
#include <string_view>

#include "voidui/core/color.h"
#include "voidui/core/style/property.h"
#include "voidui/core/style/value.h"
#include "voidui/paint/paint.h"
#include "voidui/paint/path.h"

/// The SVG presentation properties, as VSS sees them.
///
/// SVG's paint attributes are not a separate language: `fill`, `stroke` and
/// their companions are ordinary CSS properties that happen to be defined by
/// the SVG specification, they cascade and inherit exactly like `color` does,
/// and a stylesheet may set them on anything. So they are declared here with
/// the same macro as `background` and land in the same registry -- which is
/// what makes
///
///   .icon       { fill: $text.muted; transition: fill 120ms ease; }
///   .icon:hover { fill: $text.primary; }
///
/// work without the SVG widget knowing that hovering, theming or transitions
/// exist. A document that names none of them inherits the widget's computed
/// values, which is why one icon file can be tinted per site of use rather
/// than duplicated per colour.
///
/// All of them are inherited except `vector-effect`, matching SVG 1.1 and 2.

namespace voidui {

/// An SVG paint value: `none | currentColor | <color> | url(#id)`.
///
/// Twenty-four bytes, so it rides inside a PropertyValue's inline buffer and a
/// resolved style holds no allocation for it.
///
/// `Server` -- a `url(#id)` reference to a gradient -- only ever arises inside
/// a parsed document, where the fragment has something to resolve against; the
/// index names an entry in that document's brush table. A stylesheet naming a
/// URL is reported as unparseable rather than silently painting black, because
/// a VSS rule has no document to look the fragment up in.
class SvgPaint {
public:
  enum class Kind : std::uint8_t {
    None,
    CurrentColor,
    Solid,
    Server,
  };

  constexpr SvgPaint() = default;

  static constexpr SvgPaint none() { return SvgPaint(); }

  static constexpr SvgPaint current_color() {
    SvgPaint paint;
    paint.kind_ = Kind::CurrentColor;
    return paint;
  }

  static constexpr SvgPaint solid(Color color) {
    SvgPaint paint;
    paint.kind_ = Kind::Solid;
    paint.color_ = color;
    return paint;
  }

  static constexpr SvgPaint server(std::uint16_t index) {
    SvgPaint paint;
    paint.kind_ = Kind::Server;
    paint.server_ = index;
    return paint;
  }

  constexpr Kind kind() const { return kind_; }
  constexpr bool is_none() const { return kind_ == Kind::None; }

  /// Whether this paint puts anything on the screen at all. The one test the
  /// drawing path actually wants: `fill: none` is by far the most common value
  /// in a stroked icon, and skipping it skips a whole command.
  constexpr bool paints() const { return kind_ != Kind::None; }

  /// Meaningful for `Solid`.
  constexpr Color color() const { return color_; }

  /// Meaningful for `Server`.
  constexpr std::uint16_t server_index() const { return server_; }

  constexpr bool operator==(const SvgPaint &other) const {
    if (kind_ != other.kind_)
      return false;
    if (kind_ == Kind::Solid)
      return color_ == other.color_;
    if (kind_ == Kind::Server)
      return server_ == other.server_;
    return true;
  }

private:
  Color color_ = Color::TRANSPARENT;
  std::uint16_t server_ = 0;
  Kind kind_ = Kind::None;
};

/// The most dash lengths a `stroke-dasharray` may carry.
///
/// Six covers every pattern anyone writes -- a dash, a dot, and the two- and
/// three-part rhythms in between -- and keeps the value inside a
/// PropertyValue's inline buffer, which is worth more than an unbounded list
/// nobody would fill. A longer list is reported as unparseable rather than
/// silently truncated.
inline constexpr std::size_t kMaxSvgDashes = 6;

struct SvgDashArray {
  std::array<float, kMaxSvgDashes> lengths{};
  std::uint8_t count = 0;

  constexpr bool empty() const { return count == 0; }

  std::span<const float> span() const {
    return std::span<const float>(lengths.data(), count);
  }
};

/// `paint-order`. SVG allows any permutation of fill, stroke and markers;
/// without markers only two of them differ, so only two are stored -- `normal`,
/// `fill` and `fill stroke` all mean the first, and anything naming stroke
/// before fill means the second.
enum class SvgPaintOrder : std::uint8_t {
  FillStroke,
  StrokeFill,
};

/// `vector-effect`. `non-scaling-stroke` keeps the pen a fixed width on screen
/// however the shape around it is scaled -- what a hairline in an icon drawn at
/// several sizes actually wants.
enum class SvgVectorEffect : std::uint8_t {
  None,
  NonScalingStroke,
};

// -- Value-type hooks --------------------------------------------------------

inline std::uint64_t style_value_hash(const SvgPaint &paint) {
  std::uint64_t seed = static_cast<std::uint64_t>(paint.kind());
  if (paint.kind() == SvgPaint::Kind::Solid)
    return style_hash_combine(seed, style_value_hash(paint.color()));
  if (paint.kind() == SvgPaint::Kind::Server)
    return style_hash_combine(seed, paint.server_index());
  return seed;
}

/// Hashed and compared field by field: the trailing count leaves padding, and
/// the unused tail of `lengths` is not part of the value.
inline bool style_value_equals(const SvgDashArray &a, const SvgDashArray &b) {
  if (a.count != b.count)
    return false;
  for (std::uint8_t i = 0; i < a.count; ++i)
    if (a.lengths[i] != b.lengths[i])
      return false;
  return true;
}

inline std::uint64_t style_value_hash(const SvgDashArray &dashes) {
  std::uint64_t seed = dashes.count;
  for (std::uint8_t i = 0; i < dashes.count; ++i) {
    const float value = dashes.lengths[i] == 0.0f ? 0.0f : dashes.lengths[i];
    seed = style_hash_combine(seed, style_hash_bytes(&value, sizeof(value)));
  }
  return seed;
}

/// Two solid paints interpolate as their colours do, so `transition: fill` on
/// an icon behaves the way `transition: color` does on a label. Everything else
/// is discrete -- `none` has no colour to move towards, and `currentColor` is
/// already following whatever `color` is doing.
inline bool interpolate_style_value(const SvgPaint &from, const SvgPaint &to,
                                    float progress, SvgPaint &out) {
  if (from.kind() != SvgPaint::Kind::Solid ||
      to.kind() != SvgPaint::Kind::Solid)
    return false;
  out = SvgPaint::solid(mix_colors(from.color(), to.color(), progress));
  return true;
}

/// Patterns of the same length interpolate elementwise. A change in the number
/// of dashes is discrete, as CSS defines it.
inline bool interpolate_style_value(const SvgDashArray &from,
                                    const SvgDashArray &to, float progress,
                                    SvgDashArray &out) {
  if (from.count != to.count)
    return false;
  out.count = from.count;
  for (std::uint8_t i = 0; i < from.count; ++i)
    out.lengths[i] =
        from.lengths[i] + (to.lengths[i] - from.lengths[i]) * progress;
  return true;
}

// -- Parsers -----------------------------------------------------------------
//
// LineCap and LineJoin are framework-wide types, but `butt | round | square`
// and `miter | round | bevel` are SVG's spellings for them, so their readers
// live beside the properties that need them.

bool parse_style_value(std::string_view text, SvgPaint &out);
bool parse_style_value(std::string_view text, SvgDashArray &out);
bool parse_style_value(std::string_view text, SvgPaintOrder &out);
bool parse_style_value(std::string_view text, SvgVectorEffect &out);
bool parse_style_value(std::string_view text, LineCap &out);
bool parse_style_value(std::string_view text, LineJoin &out);
bool parse_style_value(std::string_view text, FillRule &out);

namespace styles {

VOIDUI_GLOBAL_STYLE_PROPERTY(Fill, SvgPaint, "fill", Inherited, Paint,
                             SvgPaint::solid(Color(0, 0, 0)));

VOIDUI_GLOBAL_STYLE_PROPERTY(FillOpacity, float, "fill-opacity", Inherited,
                             Paint, 1.0f);

VOIDUI_GLOBAL_STYLE_PROPERTY(FillRule, voidui::FillRule, "fill-rule", Inherited,
                             Paint, voidui::FillRule::NonZero);

VOIDUI_GLOBAL_STYLE_PROPERTY(Stroke, SvgPaint, "stroke", Inherited, Paint,
                             SvgPaint::none());

VOIDUI_GLOBAL_STYLE_PROPERTY(StrokeOpacity, float, "stroke-opacity", Inherited,
                             Paint, 1.0f);

VOIDUI_GLOBAL_STYLE_PROPERTY(StrokeWidth, float, "stroke-width", Inherited,
                             Paint, 1.0f);

VOIDUI_GLOBAL_STYLE_PROPERTY(StrokeLinecap, voidui::LineCap, "stroke-linecap",
                             Inherited, Paint, voidui::LineCap::Butt);

VOIDUI_GLOBAL_STYLE_PROPERTY(StrokeLinejoin, voidui::LineJoin,
                             "stroke-linejoin", Inherited, Paint,
                             voidui::LineJoin::Miter);

VOIDUI_GLOBAL_STYLE_PROPERTY(StrokeMiterlimit, float, "stroke-miterlimit",
                             Inherited, Paint, 4.0f);

VOIDUI_GLOBAL_STYLE_PROPERTY(StrokeDasharray, SvgDashArray, "stroke-dasharray",
                             Inherited, Paint, SvgDashArray{});

VOIDUI_GLOBAL_STYLE_PROPERTY(StrokeDashoffset, float, "stroke-dashoffset",
                             Inherited, Paint, 0.0f);

VOIDUI_GLOBAL_STYLE_PROPERTY(PaintOrder, SvgPaintOrder, "paint-order",
                             Inherited, Paint, SvgPaintOrder::FillStroke);

VOIDUI_GLOBAL_STYLE_PROPERTY(VectorEffect, SvgVectorEffect, "vector-effect",
                             NotInherited, Paint, SvgVectorEffect::None);

} // namespace styles

} // namespace voidui
