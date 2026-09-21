# Scrolling

Every widget can establish a scroll container using standard CSS overflow. Divs,
text, images, custom containers and textareas share layout, clipping, wheel
chaining and scrollbar geometry. A function component's returned elements use the
same behavior; no wrapper ScrollView is required.

```css
.messages {
  height: 320px;
  overflow: auto;
  scrollbar-width: thin;
  scrollbar-color: #718096 transparent;
  scrollbar-gutter: stable;
}
```

Constrain the scrolling axis with height/width, max-height/max-width or a bounded
Flexbox/Grid allocation. An unconstrained block grows with its content. For a
fixed-height list of flex children, use `flex-shrink: 0` on items that should not
shrink to fit the viewport.

## Scrollbar placement is a Rust preference

CSS `scrollbar-gutter` reserves space for **classic** scrollbars. It does not select
classic versus overlay scrollbars; the CSS Overflow specification assigns that
choice to the user agent. VoidUI exposes it through Rust, with overlay as the
default. No `overflow: overlay`, custom CSS property or vendor pseudo-element is
introduced.

```rust
use voidui::{div, Overflow, ScrollbarGutter, ScrollbarMode, ScrollbarWidth};

let list = div()
    .height(320)
    .overflow(Overflow::Auto)
    .scrollbar_mode(ScrollbarMode::Classic)
    .scrollbar_width(ScrollbarWidth::Thin)
    .scrollbar_gutter(ScrollbarGutter::StableBothEdges)
    .child(div().height(800));
```

- `ScrollbarMode::Overlay`: draw above content; do not reserve layout space.
- `ScrollbarMode::Classic`: reserve space for displayed bars, plus any stable gutter.

Set `WindowOptions::scrolling` or `WidgetTree::set_scroll_options` to configure the
host default, logical-pixel widths, minimum thumb length and default colors.
`scrollbar_mode` overrides placement on one element; it does not inherit.
`tree.set_scrollbar_mode(id, Some(mode))` changes it at runtime; `None` restores the
host default. The selected mode survives keyed reconciliation. Palette-only host
changes repaint without layout; changes to gutter dimensions require layout.

The Rust `scrollbar_width` builder now takes `ScrollbarWidth::{Auto, Thin, None}`.
Use `ScrollOptions::{width, thin_width}` for custom application metrics. The old
numeric layout-only builder is replaced; CSS never accepts pixel lengths for this
property. Raw Taffy `LayoutStyle` is still available to low-level layout callers.

## Supported CSS

| Property | Values | Behavior |
| --- | --- | --- |
| `overflow`, `overflow-x`, `overflow-y` | `visible`, `clip`, `hidden`, `auto`, `scroll` | The shorthand accepts one or two axis values. |
| `scrollbar-width` | `auto`, `thin`, `none` | `none` hides bars and gutters without disabling scrolling. |
| `scrollbar-color` | `auto` or two colors | Thumb then track; inherited, with `currentColor` resolved on the declaring element. |
| `scrollbar-gutter` | `auto`, `stable`, `stable both-edges` | Stable inline gutters in classic mode; ignored in overlay mode. |
| `overscroll-behavior`, `overscroll-behavior-x`, `overscroll-behavior-y` | `auto`, `contain`, `none` | Controls chaining at each scroll boundary. |

These longhands support `inherit`, `initial` and `unset`, source-order cascade and
`!important`. CSS-wide keywords must be the entire shorthand. Invalid values fail
stylesheet loading under the framework's existing strict parser.

`visible` does not clip or scroll. `clip` clips without creating a scroll container.
`hidden` clips and permits programmatic scrolling but has no bars or wheel default.
`auto` shows each bar only when that axis overflows; `scroll` keeps it visible even
when content fits. If one axis establishes a scroll container, `visible` on the
other computes to `auto`, and `clip` computes to `hidden`, as specified by CSS.

Stable gutters apply to the inline edges (the current layout engine uses horizontal
writing). `stable both-edges` mirrors the vertical bar's gutter on the other inline
edge; it does not add a second horizontal gutter. The bar is drawn on the right,
including for RTL containers. Horizontal RTL scroll offsets use the CSSOM convention:
zero at the start, negative values toward the end.

## Interaction and geometry

Wheel listeners run before default scrolling. `prevent_default` suppresses the
entire default; stopping propagation alone does not. Pixel deltas are converted
from physical to logical coordinates once by the native adapter. Line deltas use
the hit element's computed line height. Shift converts a vertical-only wheel delta
to horizontal movement.

Each axis consumes distance in the nearest eligible scroll container; unused
distance passes along containing-block ancestry. `contain` and `none` stop that propagation. The
runtime does not implement a rubber-band effect, so both values have the same
visible boundary behavior. Textareas join the same chain at their boundary.

Thumb dragging captures the pointer until release/cancellation, including outside
the element. Track clicks page by one viewport. Scrollbar interaction does not
activate underlying content or start a selection. Clicking a scroll container can
focus it; `tabindex="0"` also includes it in keyboard traversal. Arrows, Page Up/Down,
Home/End and Space scroll after editable and button defaults have been offered the
key. Focusing an offscreen control reveals it in its containing scrollports.

`WidgetTree` exposes `scroll_metrics`, `scroll_to`, `scroll_by`, `scroll_into_view`
and `scrollbar_geometry`. Coordinates are logical pixels; offsets are clamped, and
non-finite requests or stale IDs are ignored. Calls report whether anything changed.
Offsets survive reconciliation and clamp after resize/content changes. Scroll
coordinates follow layout ownership, so fixed and top-layer roots are not moved by
unrelated DOM ancestors. Absolute descendants follow their containing block.

Headless hosts call `dispatch_mouse_scroll` for listeners plus the default, and
`scroll_key` after their own editable/button key defaults. Do not call `scroll_wheel`
again after `dispatch_mouse_scroll`; it is the separate default-only entry point.
Call `update_styles` after changes and `layout_computed` if required, then
`refresh_pointer` to refresh stationary hover before painting. Custom input clients
can expose `TextInputClient::{scroll_content, set_scroll_offset}`; otherwise the
native host retains its legacy input-scroll dispatch. After external editor changes,
headless hosts call `refresh_scroll_content` before drawing, and honor any resulting
layout invalidation. Native windows perform these steps automatically.

## Work and memory

Ordinary nodes have no allocated scroll state or scroll declarations. Retained
state and used-layout data are allocated only for scroll containers. Scrollbar
tracks/thumbs are scene primitives, not extra widgets, tasks or components.

Scrolling repositions the affected layout subtree and refreshes cached clips;
it does not invoke widget layout, reshape text, reconcile components or re-sort
stacking contexts. Geometry work scales with the moved subtree, and clip refresh
with paint entries and ancestor depth. Existing scene/paragraph clipping discards
offscreen primitives and skips offscreen glyph work. This is **not list
virtualization**: all mounted items retain their ordinary widget/layout storage.

Classic `auto` gutters are resolved only during layout. Each layout starts without
auto gutters and adds newly required axes until stable, avoiding a stale scrollbar
that perpetuates its own overflow after content shrinks. Overlay mode needs no
extra gutter pass. Mirrored gutters use an inset layout viewport while preserving
authored CSS and outer box dimensions; the Taffy dependency is not modified.

Wheel/drag updates coalesce through the existing paint waker without pending-node
allocations. There is no scrollbar animation, idle polling or repeating scroll
timer. The warm benchmark checks zero allocations, no layout and retained paint
order; its numbers exclude initial layout and GPU work.

```sh
cargo test --test scroll
cargo run --release --example scroll_bench -- 10000 1000
cargo run --example scrolling
cargo run --example scrolling -- --smoke /tmp/scrolling.png
```

The native smoke test delivers wheel input and thumb dragging, asserts unchanged
layout-pass counts, verifies idle scene reuse and writes a rendered PNG.

## Scope

This provides standard scrolling for the existing widget box model, not a full HTML
browser viewport. Smooth scrolling/`scroll-behavior`, scroll snap, scroll anchoring,
scroll-linked animations, `scroll-padding`/`scroll-margin`,
touch panning, auto-hiding/animated OS scrollbars and rounded overflow clipping are
not implemented. Unsupported CSS is rejected rather than accepted without effect.
Root/body overflow propagation and vertical writing modes are not implemented.

Specifications: [CSS Overflow 3](https://www.w3.org/TR/css-overflow-3/),
[CSS Scrollbars 1](https://www.w3.org/TR/css-scrollbars-1/),
[CSS Overscroll Behavior 1](https://www.w3.org/TR/css-overscroll-1/).

See [Sticky positioning and 2D transforms](spatial.md) for post-layout
coordinates, transformed clipping, and containing blocks.

### Hosted controls and diagonal wheels

A hosted control that handles only one axis should return
`EventResponse::consume_scroll(ScrollAxes::HORIZONTAL)` or `VERTICAL`, using the
original event axes. Ancestor handlers and the shared scrolling default receive
only the remaining components. Reserve `PREVENT_DEFAULT` for consuming the entire
wheel event. This lets an embedded horizontal table scroll without blocking the
page's vertical movement on a trackpad sample that contains both axes.
