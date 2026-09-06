#pragma once

#include <cstdint>
#include <string_view>

#include "voidui/core/style/property.h"
#include "voidui/core/style/value.h"

/// The CSS table properties, as VSS sees them.
///
/// Everything a table needs beyond the ordinary box properties is declared
/// here, with the same macro as `background`, in the same registry, so a
/// stylesheet styles a table with the vocabulary it already knows:
///
///   table { table-layout: fixed; border-collapse: collapse; }
///   table { border-spacing: 8 4; caption-side: bottom; empty-cells: hide; }
///   td    { vertical-align: middle; text-align: right; }
///
/// The five that control the whole grid -- `border-collapse`, `border-spacing`,
/// `caption-side`, `empty-cells` and `table-layout` -- are read off the table's
/// own node. The first four inherit, exactly as CSS defines them, so a rule
/// written on an ancestor reaches every table beneath it; a table nested inside
/// a cell can still override any of them for itself.
///
/// `vertical-align` does not inherit and is read per cell. Its `baseline`
/// value aligns the first text baselines of the cells in a row against each
/// other, which is why Widget exposes `first_baseline()`: a cell asks its
/// content where its first baseline sits and the row lines them up. Content
/// that has no baseline to report is aligned as if `top` had been written.

namespace voidui {

/// `auto` sizes columns from their content; `fixed` takes the widths written
/// on the columns (or on the first row's cells) and never measures the rest.
enum class TableLayout : std::uint8_t { Auto, Fixed };

/// `separate` gives every cell its own border, with `border-spacing` between
/// them; `collapse` merges adjacent borders into one shared grid line.
enum class BorderCollapse : std::uint8_t { Separate, Collapse };

enum class CaptionSide : std::uint8_t { Top, Bottom };

/// `hide` leaves a cell with no content unpainted -- no background, no border
/// -- in `separate` mode. Ignored under `collapse`, as in CSS.
enum class EmptyCells : std::uint8_t { Show, Hide };

enum class VerticalAlign : std::uint8_t { Baseline, Top, Middle, Bottom };

/// The horizontal and vertical gaps of `border-spacing`. One value in a
/// stylesheet sets both, as in CSS.
struct BorderSpacing {
  float horizontal = 0.0f;
  float vertical = 0.0f;

  constexpr BorderSpacing() = default;
  constexpr BorderSpacing(float uniform)
      : horizontal(uniform), vertical(uniform) {}
  constexpr BorderSpacing(float horizontal, float vertical)
      : horizontal(horizontal), vertical(vertical) {}

  constexpr bool operator==(const BorderSpacing &) const = default;
};

bool parse_style_value(std::string_view text, TableLayout &out);
bool parse_style_value(std::string_view text, BorderCollapse &out);
bool parse_style_value(std::string_view text, CaptionSide &out);
bool parse_style_value(std::string_view text, EmptyCells &out);
bool parse_style_value(std::string_view text, VerticalAlign &out);
bool parse_style_value(std::string_view text, BorderSpacing &out);

inline BorderSpacing interpolate_style_value(const BorderSpacing &a,
                                             const BorderSpacing &b, float t) {
  return {a.horizontal + (b.horizontal - a.horizontal) * t,
          a.vertical + (b.vertical - a.vertical) * t};
}

namespace styles {

VOIDUI_GLOBAL_STYLE_PROPERTY(TableLayout, voidui::TableLayout,
                             "table-layout", NotInherited, Layout,
                             voidui::TableLayout::Auto);

VOIDUI_GLOBAL_STYLE_PROPERTY(BorderCollapse, voidui::BorderCollapse,
                             "border-collapse", Inherited, Layout,
                             voidui::BorderCollapse::Separate);

VOIDUI_GLOBAL_STYLE_PROPERTY(BorderSpacing, voidui::BorderSpacing,
                             "border-spacing", Inherited, Layout,
                             voidui::BorderSpacing{});

VOIDUI_GLOBAL_STYLE_PROPERTY(CaptionSide, voidui::CaptionSide,
                             "caption-side", Inherited, Layout,
                             voidui::CaptionSide::Top);

VOIDUI_GLOBAL_STYLE_PROPERTY(EmptyCells, voidui::EmptyCells, "empty-cells",
                             Inherited, Paint, voidui::EmptyCells::Show);

VOIDUI_GLOBAL_STYLE_PROPERTY(VerticalAlign, voidui::VerticalAlign,
                             "vertical-align", NotInherited, Layout,
                             voidui::VerticalAlign::Baseline);

} // namespace styles

} // namespace voidui
