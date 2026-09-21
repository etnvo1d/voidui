# CSS layout

voidui uses Taffy 0.14 directly. `core::layout` re-exports its style, geometry,
measurement, and tree traits, plus reusable layout functions. The public
`Dimension` input wraps Taffy and uses CSS percentage numbers: `percent(50.0)`
means 50%; convert it with `.into()` for raw layout fields. The optional use of
[CSS stylesheets](css.md) compiles into these same typed properties; no second copy
of the widget tree is involved.

## Build a container

```rust
use voidui::core::layout::{AlignItems, FlexWrap, fr, length};
use voidui::style::pct;
use voidui::widgets::div::div;

let panel = div()
    .grid()
    .width(pct(100.0))
    .gap(12.0)
    .padding(16)
    .grid_template_columns(vec![length(180), fr(1.0)])
    .child(div().height(240))
    .child(
        div()
            .flex_row()
            .gap(8.0)
            .flex_wrap(FlexWrap::Wrap)
            .align_items(AlignItems::CENTER)
            .child(div().width(80).height(24))
            .child(div().width(120).height(32)),
    );
```

`div()` starts with `display: block` and `box-sizing: content-box`. Block children
stack vertically, auto widths fill the containing block, and adjoining vertical
margins collapse. Use `Display::FlowRoot` to establish a separate block formatting
context. `gap` applies to Flexbox and Grid, not ordinary block flow.

Style changes use direct fluent methods: `.flex_row()`, `.flex_col()`, `.margin(12)`,
`.margin_top(4)`, `.padding(16)`, `.min_width(0)`, and the alignment/grid/positioning
methods listed in [Fluent styles](style.md). Numeric values are logical pixels;
use `pct(50.0)` for 50%, `AUTO` for auto, and `fr(1.0)` for typed grid tracks.

`.layout_style(style)` replaces the complete Taffy layout value for reusable
presets. A standalone `LayoutStyle::default()` retains Taffy's Flexbox/border-box
defaults. There is no builder style-closure API.

## Run and inspect layout

```rust
# use voidui::core::{layout::{AvailableSpace, Size}, widget_tree::WidgetTree};
# use voidui::render::TextLayoutCache;
# use voidui::widgets::div::div;
# fn example(text_cache: &TextLayoutCache) {
let mut tree = WidgetTree::new();
let root = tree.build_root(div().child(div()));
tree.layout(
    Size {
        width: AvailableSpace::Definite(800.0),
        height: AvailableSpace::Definite(600.0),
    },
    text_cache,
);
let global_bounds = tree.bounds(root);
let css_layout = tree.layout_result(root);
# }
```

Available space is a CSS sizing/wrapping input, not a forced size or maximum.
Constrain the root using its `size`, `min_size`, and `max_size`. Use `MinContent`
and `MaxContent` for intrinsic queries; do not substitute infinity or zero.
Percentage heights resolve only when the containing block height is definite.

`layout_result` holds unrounded parent-relative border-box geometry, resolved
padding/borders, and overflow information. `bounds` holds global border-box
geometry. Global coordinates are recomputed from the local result on every run.
`layout_subtree` treats a subtree as an independent root at a global origin; use
`layout` to reflow siblings and ancestors after a size change.

Taffy's measurements are retained across frames. Local geometry, typography and
widget-model changes invalidate the affected node and its ancestors; unchanged
subtrees reuse measurements when their layout inputs still match. Scrollbar gutter
changes invalidate the affected scroll container and its ancestors between passes.
Changes to selector inputs, tree structure or layout ownership conservatively
invalidate all layout caches, as do changes to the text system, font revision or
independent layout root. Custom widgets with mutable models must request
`WidgetInvalidator::relayout()` when their intrinsic size changes.

Inline style edits recompute the edited element and propagate inherited changes
until computed styles stabilize. They do not rematch selectors throughout the
tree. `style_resolutions()` counts computed-style work separately from
`cascade_stats()` so headless tests can verify reuse without timing assertions.

Run a complete headless example with `cargo run --example layout`.
For resize regression measurements, run `cargo run --example resize_bench -- 1000 200`.
It reports CPU update costs and checks component, style and shaping reuse; native
event delivery, painting and GPU presentation are excluded.

## Reuse the functions in another widget

Container widgets can implement `Widget::layout` with:

```rust
# use voidui::core::{context::LayoutContext, layout::{LayoutInput, LayoutOutput}};
# struct Container;
# impl Container {
fn layout(&mut self, inputs: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput {
    ctx.layout_children(inputs)
}
# }
```

Intrinsic leaf widgets use `layout_leaf(ctx.layout_style(), inputs, measure)`.
The callback measures content, while Taffy applies CSS sizing, padding, and borders.
Text widgets can obtain the shaping cache from `ctx.text_layout()` and resolved
typography from `ctx.text_style()`. Measurements
may run multiple times; honor the available width and distinguish min-content
from max-content. `LayoutInput` and `LayoutOutput` retain Taffy's baseline,
percentage-resolution, margin-collapse, and measurement-phase information.

`layout_container`, `layout_block`, `layout_flex`, `layout_grid`, `layout_leaf`,
`layout_hidden`, and `layout_root` are independent of `WidgetTree` and rendering.
The tree-based functions accept Taffy's low-level traits. `LayoutContext`
implements those traits directly over the existing generational widget IDs.

The old `Constraints`, `Length`, and `MarginValue` types have been removed.
Use Taffy's `AvailableSpace`, `Dimension`, `LengthPercentage`, and
`LengthPercentageAuto` instead. Layout properties now live under `Style::layout`.
`Widget::layout` accepts `LayoutInput` and returns `LayoutOutput` so parent and
child layout algorithms can exchange complete CSS sizing information.

## Scope and known limitations

Supported modes are Block, FlowRoot, Flexbox, Grid, and None. The low-level layout
functions retain Taffy's relative/absolute model. WidgetTree adds CSS static/fixed
positioning and a containing-block projection without changing DOM ownership.
See [Stacking and overlays](overlays.md) for APIs, behavior changes and positioning
limits, including automatic static-position fallback for relocated boxes.
Inline formatting, tables, and floats remain unsupported. Sticky positioning and
2D transforms are handled after layout; see [Spatial styles](spatial.md).
CSS layout lengths support `calc()`, `min()`, `max()`, and `clamp()` with pixels
and percentages; see [CSS math functions](css.md#math-functions) for syntax and
limits. Custom trees that copy stylesheet calculation handles must retain the
originating style and forward `resolve_calc_value` to `layout::resolve_calc`.
CSS parsing is supplied by the separate stylesheet adapter.

The widget tree now paints backgrounds/borders and rectangular overflow clips from
these layout results. Generic scrolling and scrollbars are provided by the retained
scroll adapter; see [Scrolling](scrolling.md). Rounded overflow clipping is not implemented.

### Taffy 0.14: percentage padding in block flow

A block parent of content size 200 × 100 with a child of width 25%, height 50%, and
10% top/bottom padding should give the child a border-box height of 90:
`100 * 0.5 + 200 * 0.1 * 2`. Taffy 0.14 produces 70 and reports 10-pixel vertical
padding. The same input reproduces with the unmodified `TaffyTree`, independently
of voidui's adapter. Flexbox and Grid pass this case.

The cause is `generate_item_list` in Taffy's `src/compute/block.rs`: it resolves
child padding against the two-axis `node_inner_size`, so vertical percentages
use the parent's height. CSS requires the containing block's width for all four
padding percentages. See the [CSS box model specification](https://www.w3.org/TR/CSS22/box.html#padding-properties).

The dependency is not patched. The CSS expectation is retained as an explicitly
ignored conformance test, with a passing reference comparison beside it. Re-run
it when upgrading Taffy:

```sh
cargo test --test layout css_block_vertical_percentage_padding_uses_containing_width -- --ignored
cargo test --test layout upstream_reference_reproduces_block_percentage_padding_limitation -- --nocapture
```

Until upstream fixes it, use fixed vertical padding for children in block flow,
or use Flexbox/Grid when their formatting rules fit the container.

Text widgets and inherited typography are described in [Text widgets](text.md).
