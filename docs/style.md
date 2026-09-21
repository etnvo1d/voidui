# Fluent styles and CSS inheritance

`WidgetBuilder`, `ComponentElement`, and named-property component builders share
the same inherent style methods; no trait import or style callback is required.
Component styles apply to the final rendered root without adding a layout node.
Builder `layout(|style| ...)` has been removed.

[Tailwind-style utilities](tailwind.md) add shortcuts such as `w_full()`, `p_4()`,
`border_r(1)`, `border_l_2()`, and `bg_gray_50()` to the same builders.

```rust
use voidui::{div, text};
use voidui::style::{pct, AUTO, color::Rgba8};

let panel = div()
    .flex_col()
    .width(pct(100.0))
    .padding(16)
    .margin_top(12)
    .gap(8)
    .color(Rgba8::from_rgb8(40, 60, 80))
    .child("Inherits the panel's color")
    .child(
        div().flex_row().items_center().gap(12)
            .child(text("Label").bold())
            .child(text("Value").margin_left(AUTO)),
    );
```

Pixel values accept integer and floating-point inputs. `pct(50.0)` means 50%;
`AUTO` is available only where the property supports auto. Negative margins and
positioning offsets are allowed; negative padding, gaps, and dimensions are rejected.
`core::layout::Dimension::percent(50.0)` also means 50%. This is a breaking
change from fraction-based percentage inputs: migrate `pct(0.5)` to `pct(50.0)`
and `Dimension::percent(0.5)` to `Dimension::percent(50.0)`. Values above 100%
remain valid. `Dimension::fit_content_percent` follows the same convention.

`Dimension` is a framework-owned input type. Convert it with `.into()` when
assigning a raw Taffy `LayoutStyle` field. Low-level Taffy types and helpers keep
their original fractional convention, including `LengthPercentage::percent(0.1)`,
`LengthPercentageAuto::percent(0.1)`, and `core::layout::percent(0.1)`, which mean 10%.
Prefer `pct(10.0)` with fluent setters to use the CSS-number convention throughout.
Use concrete values, `pct`, or `AUTO` for fluent length arguments: generic Taffy
helpers such as `length()` cannot infer their return type through a numeric setter.
Taffy helpers remain convenient for typed grid tracks and low-level style structs.

## Property methods

| Group | Methods |
| --- | --- |
| Container | `block`, `flow_root`, `flex`, `flex_row`, `flex_col`, `flex_row_reverse`, `flex_col_reverse`, `grid`, `hidden`, `display` |
| Size | `width`, `height`, `size`, `min_width`, `min_height`, `max_width`, `max_height`, `aspect_ratio`, `box_sizing` |
| Spacing | `margin`, `margin_x`, `margin_y`, `margin_top/right/bottom/left`; matching `padding` methods; `margin_edges`, `padding_edges` |
| Borders | `border_width`, `border_top/right/bottom/left_width`, `border_widths`, `border_color`, `border_radius` |
| Flex | `flex_direction`, `flex_wrap`, `flex_grow`, `flex_shrink`, `flex_basis`, `gap`, `row_gap`, `column_gap` |
| Alignment | `align_items`, `align_self`, `align_content`, `justify_content`, `justify_items`, `justify_self`, `items_center`, `justify_center`, `justify_between` |
| Grid | `grid_template_columns/rows`, `grid_auto_columns/rows`, `grid_auto_flow`, `grid_column`, `grid_row` |
| Transform | `transform`, `transform_origin` |
| Position/overflow | `relative`, `absolute`, `fixed`, `sticky`, `static_position`, `position`, `top/right/bottom/left`, `inset`, `inset_edges`, `overflow`, `overflow_x/y`, `overflow_axes`, `scrollbar_width`, `scrollbar_color`, `scrollbar_gutter`, `overscroll_behavior` |
| Typography | `color`, `font`, `font_family`, `font_options`, `font_weight`, `font_style`, `font_features`, `font_fallbacks`, `font_size`, `line_height`, `text_align`, `direction`, `wrap`, `bold`, `italic` |
| Stacking/input | `z_index`, `isolation`, `visibility`, `pointer_events` |
| Paint | `background`, `background_image`, `box_shadow`, `border_color`, `border_radius` |
| Transitions | `transition`, `transition_property`, `transition_duration`, `transition_delay`, `transition_timing_function` |

`flex_row` and `flex_col` set both display and direction. `flex_direction` changes
only direction. Spacing shorthands expand immediately: `margin(12).margin_top(4)`
sets only the top side to 4; reversing the calls makes every side 12.
`.layout_style(LayoutStyle)` remains available for complete reusable Taffy presets,
but ordinary styling uses the direct methods above.

## Inheritance and defaulting

Color, each font longhand, font size, line height, text alignment, direction, and
soft wrapping inherit by default. Layout/spacing, backgrounds, borders, and corner
radii do not. Child declarations override inherited values without changing siblings.

```rust
use voidui::{div, text};
use voidui::style::{CssValue, LayoutProperty, color::Rgba8};

let panel = div().color(Rgba8::from_rgb8(180, 30, 30)).padding_top(12)
    .child(text("Inherited").color(CssValue::Inherit))
    .child(text("Initial foreground").color(CssValue::Initial))
    .child(text("Inherited again").color(CssValue::Unset))
    .child(div().inherit(LayoutProperty::PaddingTop).child("Explicit padding inheritance"));
```

For inherited and paint properties, setters accept `CssValue::Inherit`, `Initial`,
and `Unset`. Inherit takes the parent's computed value; Initial uses the property
initial value; Unset chooses inheritance only for an inherited property. At the
root, inheritance falls back to the initial value. Foreground defaults to the UI's
black/system-font policy; backgrounds are transparent and borders use currentColor.

For supported non-inherited layout longhands use `inherit(LayoutProperty::...)`,
`initial(...)`, and `unset(...)`. A subsequent concrete setter removes that
longhand's defaulting override. The property registry defines each mapping once,
so inherited values and direct setters address the same Taffy field. Percentages
remain percentages when they are CSS computed values: inherited percentage margins
resolve against the receiving element's containing block, not the parent's used
pixel margin. `layout_style` replaces all layout values and defaulting overrides.

`Color::CurrentColor` on color itself uses inherited color. In a background or
border it resolves against the receiving element's foreground. It remains a
keyword through explicit paint-property inheritance. It is never sent unresolved
to the renderer.

`.font("Arial")` is a family setter and preserves inherited weight, style,
features, and fallbacks. `.font_options(Font)` explicitly expands a whole
renderer font descriptor into independent longhands. Calling `.font(...)` later
changes only the family. Neither method is a parser for CSS's `font` shorthand.

FontSize::Em/Percent resolve against the parent's computed font size. Descendants
inherit that computed pixel size unless they declare their own relative size.
LineHeight::Relative is unitless and stays a multiplier through inheritance;
LineHeight::Percent/Em resolve against the declaring element's font size and pass
down as pixels. Normal uses the UI's existing 1.2 spacing policy.

`TextAlignment::Start` and `End` remain logical through inheritance and resolve
against each child's own Direction. `Direction` also reaches Taffy's box layout.
This does not force the renderer's paragraph bidi embedding level.

The specified `Style` is retained separately from resolved properties. Inspect
`tree.text_style`, `tree.paint_style`, and `tree.layout_style` after layout to read
computed values; `tree.style` returns declarations. After low-level mutation via
`tree.style_mut`, run layout again. If changing a raw Taffy field previously set
with explicit defaulting, also call `clear_layout_keyword` for that field; the
fluent setters perform this step automatically.

## Scope

Typed declarations now integrate with [CSS stylesheets and selectors](css.md),
including specificity, source order and !important. Cascade origins/layers beyond
author sheets and fluent properties, revert/revert-layer, and the full CSS
text/white-space model are not implemented. Position now defaults to CSS static;
use relative explicitly when insets should offset an in-flow box. Static, relative,
absolute, fixed and sticky are supported at the widget-tree layer. See
[Sticky positioning and 2D transforms](spatial.md) for transforms and coordinate APIs. Taffy-specific properties retain that
engine's defaults; supported
inheritance rules do not imply complete browser CSS conformance. Font size and
line height currently require finite positive values in the text renderer.

See the [inheritance/defaulting specification](https://www.w3.org/TR/css-cascade-5/#inheritance),
[font selection properties](https://www.w3.org/TR/css-fonts-4/#font-family-prop),
[line-height computed values](https://www.w3.org/TR/CSS22/visudet.html#propdef-line-height),
and [currentColor](https://www.w3.org/TR/css-color-4/#currentcolor-color).

```compile_fail
// The style callback API is intentionally unavailable.
voidui::div().layout(|style| style.padding.top = voidui::core::layout::length(10));
```

```compile_fail
// CSS padding does not accept auto.
voidui::div().padding(voidui::style::AUTO);
```

See [CSS effects](effects.md) for transition timing, gradient/color-space APIs, shadow lists and supported-value limits.

See [Stacking and overlays](overlays.md) for stacking contexts, Rust top-layer lifecycle and CSS-only tooltips.

For standard CSS scrolling and the Rust-only `scrollbar_mode` preference, see
[Scrolling](scrolling.md).
