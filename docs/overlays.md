# Stacking, top layer, and CSS-only tooltips

Run `cargo run --example overlays` to try nested modals, manual popovers and a
CSS-only tooltip. The library supplies no Modal, Dialog or Tooltip component.
Build ordinary `div`/`text` elements and use Rust to manage their lifecycle.

## CSS stacking and positioning

```css
.surface { position: relative; isolation: isolate; }
.panel { position: absolute; inset: 12px; z-index: 2; }
.panel.behind { z-index: -1; }
.viewport-cover { position: fixed; inset: 0; z-index: 10; }
```

Supported properties are standard CSS:

- `z-index: auto | <integer>`, including negative values;
- `position: static | relative | absolute | fixed`;
- `isolation: auto | isolate`;
- `visibility: visible | hidden`;
- `pointer-events: auto | none`.

They participate in cascade and inherit/initial/unset. Visibility and pointer-events
inherit normally, and descendants can explicitly override them. Other properties
in this list are non-inherited. Integer z-index and visibility also participate in
transitions; z-index:auto is not interpolated into an integer.

The initial position is now CSS `static`. Insets do not move static boxes. Code
that previously relied on Taffy's implicit relative positioning must say
`.relative()` or `position: relative`. The fluent API has `position`, `relative`,
`absolute`, `fixed`, `static_position`, `z_index`, `isolation`, `visibility`, and
`pointer_events` setters. `z_index(AUTO)` means auto. CSS enums live in
`voidui::style::layer`; the old `core::layout::Position` remains Taffy's two-value
low-level type and is accepted by the `position` setter for compatibility.

A positioned box with integer z-index creates an atomic stacking context. Fixed
boxes and isolation:isolate also create contexts. Integer z-index applies to flex
and grid items even without positioning, but is ignored on ordinary static blocks.
Negative contexts paint after their context's background and before normal flow;
zero/auto positioned groups and positive contexts follow normal content. Equal
levels use DOM order. A high-z descendant can escape a z-index:auto group, but
cannot escape its parent's actual stacking context. Sorting only siblings, or
sorting all elements by one global z value, would not implement these rules.

Selectors and inheritance retain DOM parents. A separate lightweight layout
projection attaches absolute boxes to their nearest positioned ancestor and fixed
boxes to the viewport. A containing block uses its padding box. Intervening static
overflow ancestors do not clip an absolute descendant whose containing block lies
outside them. Viewport-fixed boxes escape ancestor overflow. A transformed ancestor instead
contains and clips its fixed descendants. Fixed positioning does not escape
stacking contexts.

**Positioning limits:** use explicit insets on both axes when a positioned box is
relocated to a different layout parent. The original DOM static-position rectangle
for fully automatic insets across intervening static ancestors is not implemented;
Taffy's fallback then uses the projected containing block's child sequence.
Filter/contain-created containing blocks, CSS anchor
positioning, automatic tooltip flipping, floats, inline formatting and order-modified
flex/grid painting are not implemented. Overflow clipping remains rectangular.
In standalone unbounded layout, the root's size substitutes for an unspecified
viewport dimension; native windows always supply definite viewport dimensions.

## Top layer is managed by Rust

HTML's top layer is separate from numeric z-index. It is not an author-settable
CSS positioning mode. There is no `position: overlay`, `overlay: modal`, custom
selector or reserved high-z band in voidui.

```rust,no_run
use voidui::{div, AppWindow};

fn open_another(window: &mut AppWindow) -> anyhow::Result<()> {
    let parent = window.tree().active_modal()
        .or_else(|| window.tree().root()).unwrap();
    let id = window.tree_mut().append_child(parent,
        div().tag("dialog").class("panel")
            .child("A modal, built from ordinary elements"))?;
    window.show_modal(id)?;
    Ok(())
}
```

```css
dialog.panel {
    position: fixed;
    inset: 0;
    margin: auto;
    width: 320px;
    height: 200px;
    padding: 24px;
    box-sizing: border-box;
    color: #e4edf5;
    background: #243847;
    border-radius: 14px;
}
dialog.panel:modal { border: 1px solid #83a89a; }
dialog.panel::backdrop { background: rgb(0 0 0 / .5); }
```

`AppWindow` and `WidgetTree` expose:

| API | Operation |
| --- | --- |
| `show_modal(id)` | Enter the top layer, set :modal and the open attribute, establish modality and focus. |
| `show_popover(id)` | Enter as a nonmodal, manually managed popover, matching :popover-open. |
| `close_top_layer(id)` | Remove this entry and DOM-descendant entries; restore eligible focus. |
| `top_layer()` / `top_layer_kind(id)` | Inspect entries in opening order. |
| `active_modal()` / `is_inert(id)` | Inspect the active modal interaction scope. |
| `focused()` / `set_focused(...)` / `focus_next(reverse)` | Manage focus without a visual component. |
| `hit_test(point)` | Return an element or an originating element's backdrop target in logical pixels. |
| `append_child` / `remove_subtree` | Change content while retaining surviving generational IDs. |
| `find_by_id` / `attribute` | Look up an element or read canonical attributes for application event dispatch. |

The window exposes lifecycle methods directly; inspection and structural/focus
methods are on `window.tree()` / `tree_mut()`. The window schedules work after
mutations. Standalone trees call `update_styles` and `layout_computed` when needed.

Opening an already-open entry in the same mode is a no-op and does not reorder it.
Close before changing modes. Reopening appends it after all existing top-layer
entries. Each backdrop is painted immediately before its owner, above all earlier
entries and ordinary content. Even z-index:2147483647 cannot cover it. Nested
modals do not consume numeric z-index levels or hit a configured nesting limit;
practical depth is bounded by memory and execution resources.

Top-layer roots use the viewport/initial containing block, bypass ancestor clips,
and preserve their DOM selector/inheritance relationships. Their DOM ancestors'
display:none still suppresses rendering. Use the Rust close operation to release
modality; merely hiding a modal with CSS does not remove its modal state.

Minimal UA defaults hide closed `dialog` elements and closed elements with a
`popover` attribute, and provide centered sizing defaults. Author CSS can override
those defaults, so prefer `dialog:modal { display: flex; }` over an unconditional
`dialog { display: flex; }` if closed dialogs must stay hidden. Generic divs can
also be promoted; author CSS controls their closed appearance.

`:modal`, `:popover-open`, `[open]`, and `::backdrop` are standard syntax. Backdrop
styles match the originating element but start from independent initial values;
they do not inherit that element's foreground, typography or inline styles. Existing
box paint properties and layout lengths style the backdrop. Normal element rules
cannot leak into it. Popover backdrops have UA-important pointer-events:none, as
in HTML, so a nonmodal popover does not acquire a modal hit shield. Authors can
still paint a non-interactive popover backdrop. A modal remains inert outside its
scope even if its own backdrop has pointer-events:none or display:none.

Only the most recently opened modal permits pointer targeting and focus in its
subtree. Nested top-layer popovers in that subtree remain interactive. Ancestors'
explicit inert attribute does not suppress a promoted modal; explicit inert on
the modal itself still applies. Native Tab/Shift+Tab navigation stays inside the
active modal and respects positive/zero tabindex order. Closing or removing a modal
restores focus when the saved target remains valid. The example implements Escape,
backdrop dismissal and action routing in Rust; these are application policies, not
new CSS declarations.

This is a native document runtime, not the complete HTML dialog/popover API. It
does not implement auto/hint popover dismissal policies, invoker anchoring, close
watchers/cancel events, dialog return values, overlay/display exit transitions,
ARIA/accessibility integration, or general HTML button/form event semantics.
`::backdrop` is the supported stateless pseudo-element; ::before/::after and
pseudo-element interaction states remain unsupported.

## Tooltip with CSS alone

```css
.trigger { position: relative; }
.tooltip {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 6px;
    z-index: 10;
    visibility: hidden;
    pointer-events: none;
    background: #edf7f0;
    color: #18352a;
    padding: 10px;
    border-radius: 6px;
    transition: visibility 120ms;
}
.trigger:hover > .tooltip,
.trigger:focus-within > .tooltip { visibility: visible; }
```

```rust
use voidui::div;
let element = div().class("trigger")
    .child(div().tag("button").child("Help"))
    .child(div().class("tooltip").child("Additional information"));
```

This needs no tooltip component or Rust open operation. It obeys normal clipping
and stacking. When it must escape ancestor clipping/stacking, promote the ordinary
element with `show_popover` and use CSS fixed insets or Rust-computed coordinates;
CSS anchor positioning and collision avoidance are separate, unimplemented features.

## Caching and verification

Drawing and hit testing share a retained paint order. Color-only changes reuse it;
geometry changes refresh clips without sorting contexts again. Order changes,
structural mutations and top-layer operations rebuild it. Empty div content phases
are omitted, while custom widgets can report whether they paint content. No GPU
pipeline, polling thread, refresh loop or perpetual timer is added for overlays.
Pointer targeting is refreshed after geometry/order changes without rematching
selectors on unchanged frames.

```sh
cargo test --workspace
cargo run --example overlays
cargo run --example overlays -- --smoke target/overlay-smoke
cargo run --example overlays -- --pixels
cargo run --release --example stacking_bench -- 1000 200
```

Tests cover stacking-context escape/containment, negative and static z-index,
flex/grid items, containing blocks, fixed clipping, input order, backdrop cascade,
focus, runtime insertion/removal and 96 nested modals. Native pixel checks verify
that top-layer backdrops cover maximum-z content and a negative-z nested modal
still paints above its parent. The smoke test captures layers, opens/closes nested
modals, removes their nodes and checks an idle interval. Native runtime validation
is on macOS; Windows/Linux compilation does not establish platform runtime behavior.

References: [CSS stacking order](https://www.w3.org/TR/CSS22/zindex.html),
[CSS Positioned Layout](https://www.w3.org/TR/css-position-3/),
[Top layer and backdrop](https://www.w3.org/TR/css-position-4/#top-layer),
[HTML rendering defaults](https://html.spec.whatwg.org/multipage/rendering.html),
[Pointer events](https://www.w3.org/TR/css-ui-4/#pointer-events-control).

See [Sticky positioning and 2D transforms](spatial.md) for post-layout
coordinates, transformed clipping, and containing blocks.
