# Extensible editor framework

The editor separates source storage, display projection, viewport layout and input.
Text, hiding, replacements, embedded views and source-backed compound blocks use
one transaction/selection model. Extensions describe presentation and return edit
transactions; they do not write document bytes during layout.

## Ownership and coordinates

```text
Document + transactions + anchors
             |
     immutable snapshot
             |
 extension projections + highlights + preedit
             |
       source position map
             |
 viewport block index + height index + bounded text cache
             |
 native text flows / embedded views / custom block cells
```

All document and platform input indices remain UTF-8 bytes. `PositionMap` converts
between source and projected bytes, with explicit `Bias::Before` / `Bias::After`
at collapsed or inserted ranges. Hide/replacement operations never rewrite source.
Mouse hits and visual movement return source positions. Copy uses source text;
atomic replacement deletion removes its represented source range. Rich exports and
undo contain committed text/styles only. A component can implement another clipboard
format using the existing range/transaction APIs.

`EditorState::create_anchor` returns a stable `AnchorId`; its byte position maps
through transactions and undo/redo. Release unused anchors with `remove_anchor`.
Anchors are session references, not undoable edits or collaboration identifiers.

## Source storage and snapshots

`Document::read(range)` and the `TextRead` trait return borrowed text where possible
and copy only a range crossing storage chunks. `chunk(position)` supports streaming
Unicode operations. `EditorState::snapshot()` contains a cheap text snapshot,
committed style spans, selection and the most recent delta. Rope snapshots share
unchanged storage. Style-span snapshots share immutable canonical spans too.

Short controls keep a String. `BufferOptions::inline_bytes` controls one-way rope
promotion. `Document::with_buffer_options` and `EditorState::from_document` expose
that policy. The byte-indexed Ropey dependency is pinned to `2.0.0-beta.1`; all usage
is isolated in `buffer.rs`. No dependency source is modified. `text()` and the legacy
borrowed `slice()` remain explicit compatibility materializations: large-document
extensions should use range reads and snapshots instead. Materialized text is
invalidated by edits. Selection validation, normal insertion, undo and grapheme
navigation do not require an entire contiguous document.

`last_change()` is one delta, not an unlimited journal. An incremental parser checks
its indexed revision against `change.before_revision`; skipped revisions or grouped
undo require rebuilding the affected parser state from the snapshot. Async results
must publish the captured revision with `set_projection` / `set_highlights`; stale
results are rejected before presentation or history changes.

## Projection primitives

```rust
# #[cfg(feature = "editing")]
# fn main() -> Result<(), voidui::editing::EditError> {
use voidui::editing::{EditorState, Projection, Replacement, ViewId};

let mut state = EditorState::new("**hello** and [photo]");
let plan = Projection::new()
    .replace(Replacement::text(ViewId(1), 0..9, "hello").reveal_on_selection())
    .replace(Replacement::object(ViewId(2), 14..21));
state.set_projection(state.revision(), plan)?;
assert!(!state.can_undo());
# Ok(())
# }
# #[cfg(not(feature = "editing"))]
# fn main() {}
```

- `Replacement::hide` collapses a source range.
- `Replacement::text` substitutes visible text. An empty source range inserts a
  display-only annotation anchored between source bytes.
- `Replacement::object` displays a registered inline view instead of a range.
- `reveal_on_selection` exposes the source while selection touches that replacement.
  An extension can coordinate several markers using the complete selection.
- `Projection::styles` adds derived style spans. User formatting remains separate.
- `Projection::paragraph` applies paragraph geometry and decorations.
- `Projection::block` delegates a whole source block to a registered view or layout.

Ranges and style values are validated atomically. Structural replacements cannot
overlap each other. Inline replacements may be contained within a custom block,
but cannot cross its boundary. IDs must be unique in a composed projection.
Block views represent complete source paragraphs or groups of paragraphs; use an
inline replacement for a partial paragraph. Projection plans are cleared on text
edits and recomputed by extensions. They are not persisted in undo or rich exports.

Pointer text selection captures the current effective projection on button-down.
Selection-dependent reveal/hide updates are deferred until release or cancellation,
so the same drag cannot target two different arrangements of source markers.
The source caret/selection updates immediately; release changes presentation without
hit-testing the pointer again. Keyboard selection reveals source immediately.
Focus loss, source edits, IME, and extension replacement end the captured gesture.
Scrolling and width reflow still use the captured plan while the pointer is held.

Preedit uses the same composed-source coordinate space as the platform input client.
Replacements intersecting composition are temporarily revealed; surviving ranges
are mapped around preedit. Composition retains its input style. Commit still makes
one ordinary history operation.

## Register an extension

```rust
# #[cfg(feature = "editing")]
# fn main() {
use voidui::{Editor, rich_editor};
use voidui::editing::*;

struct View;
impl EditorExtension for View {
    fn project(&mut self, cx: ExtensionContext<'_>) -> Result<Projection, EditError> {
        // A real parser can keep an incremental index in this instance.
        let mut plan = Projection::new();
        if cx.snapshot.text.read(0..2).as_deref() == Some("# ") {
            plan.replacements.push(
                Replacement::hide(ViewId(10), 0..2).reveal_on_selection(),
            );
        }
        Ok(plan)
    }
}
let editor = Editor::new("# Title");
let control = rich_editor(&editor).extensions(
    EditorExtensions::new().register(ViewId(1), 0, || Box::new(View)),
);
# }
# #[cfg(not(feature = "editing"))]
# fn main() {}
```

Extension instances survive component reconciliation while their factory identity
is unchanged. `mounted` receives a weak widget invalidator; `unmounted` runs on
replacement/removal. Extensions own and cancel any async work in their lifecycle,
using the existing task APIs. Immutable snapshots can be sent to background work;
UI handles and factories remain on the UI thread.

Projection layers run in ascending priority and then stable extension ID order.
Later style fields override earlier ones. Conflicting structural replacements
produce an error, retaining the previous prepared view. Commands run in reverse
priority; the first returned transaction wins. Commands are disabled during IME
and in readonly controls. Transactions still pass revision and document constraints.
The existing `on_key` and `on_change` hooks remain available.

Automatic highlighters use `set_highlights` or projection styles. Both feed the
same projected text/layout pipeline, and neither enters undo or clears redo.

## Inline images and widgets

Emit widgets together with their source ranges. A projection owns a lightweight,
comparable description; the editor creates the actual view only when layout needs
it. Formula counts can grow or shrink without any registration or ID-to-data map.

Use `Replacement::widget(range, description)` for inline content, including an
empty range for an inserted annotation. For a whole block use
`Projection::block_view(BlockView::widget(range, description))`.

Existing VoidUI components can use `WidgetView::describe(props, render)`:

```rust
use voidui::{editing::{Projection, Replacement, WidgetView}, text, IntoElement};

let source = "[preview]";
let preview = "Current preview".to_owned();
let projection = Projection::new().replace(Replacement::widget(
    0..source.len(),
    WidgetView::describe(preview, |value| {
        text(value.clone()).width(160.0).height(40.0).into_element()
    }),
));
```

The props implement `Clone + PartialEq`. The render function receives them and
returns an `Element`; pass changing inputs through props instead of capturing
hidden inputs. Equal props reuse the mounted subtree. Changed props reconcile it,
preserving matching component state and embedded input focus.

For native rendering, implement `ViewDescription` on a `PartialEq` data type.
Its associated `View` implements `EmbeddedView`, and `create(&self)` constructs
that view. Equality must include all rendering and event-handling inputs. When a
description changes, the default behavior replaces the view. Implement
`update(&self, view: &mut Self::View) -> bool` to update it in place and return
`true`; returning `false` requests replacement and must leave the view unchanged.
Apply the complete current inputs: after failed layout, the editor may restore the
previous description. Updating a view invalidates its layout automatically, even
if only its baseline changed. Descriptions should not mutate document state.

Unkeyed widgets match by description type and source range. Consecutive document
edits map these ranges forward; when intervening revisions are unavailable, the
editor recreates unkeyed views rather than guessing their identity. Widgets at the
same range match in their existing order. Add `.key("stable-name")` to a widget
replacement or `BlockView` when it should keep identity across moves or reordering.
Keys are local to each extension; base-projection keys form a separate scope.
Duplicate keys within a scope are rejected. A key establishes identity, while the
description's equality and `update` method determine whether its instance survives.

Descriptions and mounted UI stay on the UI thread. Background parsers can consume
`ProjectionSnapshot` and return parsed data for the UI to turn into descriptions.
`Replacement::id` and `BlockView::id` on described widgets are runtime handles
assigned by validation/layout, not persistent application identifiers.

An `EmbeddedView` provides `measure`, `paint` and optional input/IME handling.
Its `mouse_scroll` hook receives native wheel units at the hovered view without
taking keyboard focus. `WidgetView` forwards this event through its subtree's
normal mouse handlers and scrolling defaults. Consumed events prevent duplicate
scrolling in the enclosing editor; unhandled events can scroll the editor instead.
`ViewMetrics` includes size, baseline and inline alignment. Parley includes the
box width during line breaking; the shared inline layout combines its aligned
extents with the text. A view may report `needs_layout` after model changes.

`WidgetView::new(factory)` adapts an existing `Element` subtree, including `img`,
buttons and text controls. It shares the font service and parent invalidation
wakeup. Its subtree has its own style cascade; put required inherited typography
on its root. Source-backed table cells use the block-layout API below and do not
create a separate Editor for each cell.

### Inline alignment and font struts

Each editor text flow has an explicit parent font, font size and line height.
Following [CSS 2.2 section 10.8](https://www.w3.org/TR/CSS22/visudet.html#line-height),
a zero-width font strut contributes to every visual row, even when replacements
leave only widgets. Typing and deleting ordinary text in the same font therefore
do not change a small widget's baseline. The parent's line height is a minimum;
a smaller styled span cannot shrink the entire row below that minimum.

Use `.inline_align(render::InlineAlignment::Middle)` on `WidgetView::describe`
or `WidgetView::new` for checkbox-like widgets. Native `EmbeddedView`
implementations can return `ViewMetrics::new(width, height).align(...)`.
Supported parent-relative values are `Baseline` (the default), `Middle`,
`TextTop`, and `TextBottom`. `Middle` uses half the surrounding font's x-height;
it does not center the widget in the entire row. `Baseline` uses the view's
reported baseline, which may lie outside its bounds. Formula renderers can
continue to report their own baseline without knowing the surrounding font.

Use `.inline_offset_em(-0.1)` for an optical adjustment equivalent to relative
`top: -0.1em`. The offset uses the surrounding text's computed font size, including
span overrides. Painting and hit bounds move together, while line height, text
baselines and neighboring content keep their original layout positions. This
also avoids resolving `em` against a hosted widget's independent style cascade.

Object-only styled spans retain their surrounding typography. Font metrics are
resolved by the same backend as visible glyphs, cached by font and size, and
invalidated when fonts are registered. No sentinel characters or extra caret
stops are inserted into the source. Each text run contributes its own half-leading
before line extents are combined with the objects. Glyph painting, widget bounds,
selection and hit testing all use that resulting display geometry.

This API implements these horizontal, parent-relative inline rules. It does not
implement the full CSS inline tree, vertical writing, or every `vertical-align`
value. As in CodeMirror, source replacement and event handling remain independent
of widget appearance; changing alignment does not add document edits.

Only visible embedded views are mounted. Offscreen views unmount, and a returning
view is measured before painting even when its paragraph geometry was cached.
A focused or pointer-captured embedded control is retained until its interaction
ends. Focused embedded inputs receive text, IME, clipboard queries and candidate
geometry. Escape returns focus to the source editor without changing its selection.
Views can keep persistent application state outside their mounted instances.

### Widget events and source selection

Widgets receive input before the source editor performs its default action.
`EmbeddedView::ignore_event` defaults to `true`, following CodeMirror's
`WidgetType.ignoreEvent` contract: events inside a widget do not implicitly select
its source. The widget's `input` still runs. Returning `true` from `input` consumes
an event even when `ignore_event` returns `false`.

For a passive preview that should allow normal source selection, override
`ignore_event` or use `WidgetView::describe(...).ignore_events(|_| false)`.
Interactive previews can move the source caret explicitly on a completed click.
Scrollbar gestures preserve the complete existing selection, including multiple
carets. No temporary selection or preview-visibility exception is needed.

A consumed pointer press captures its move, release and cancel events. Crossing
another widget does not transfer that gesture. Conversely, text selection drags
remain owned by the source editor when crossing widgets. `InputContext::bounds`
is the widget's current paint rectangle, including before its first paint.

Keyboard focus is separate from pointer capture. Custom views report it through
`has_focus` (by default, whether they expose a text input client) and release it
through `blur`. Implement `cancel_pointer` to end gestures without synthesizing
clicks; it runs on cancellation, focus loss and removal. `WidgetView` handles these
lifecycle details for its subtree. Merely scrolling a passive view never redirects
subsequent typing or IME input away from the source editor.

`EmbeddedView::pointer_cursor` supplies the cursor over hosted content. Its position
and bounds use the same coordinates as painting; a captured gesture can receive a
position outside its bounds or `None` after window exit. Return `None` to retain the
source editor's CSS cursor. `WidgetView` resolves its own scrollbar, drag and child
cursors, including nested editors. Wrappers around a `WidgetView` should forward
this hook along with input and painting.

Cursor queries do not synthesize mouse events or change selections. They place
hosted content at its current bounds, so hovering works before the first click or
paint and after scrolling. Native hosts use `WidgetTree::pointer_cursor_at` with
their captured text client to keep a drag's cursor outside the host control.

### Explicit registrations

`EditorViews::register(id, factory)`, `Replacement::object(id, range)` and
`Projection::block(id, range)` remain available for externally managed views.
`register_dynamic(|id| ...)` is a fallback for applications that already own an ID
registry: fixed factories take precedence, then dynamic factories run in order
until one returns a view. `contains(id)` only checks explicit registrations.
Described widgets resolve directly and do not consult these factories.

Clone a registry across component renders to preserve its registered instances.
Changing the registry unmounts registered views but preserves described widgets.
For explicit block records, use `BlockView::new(id, range)`; struct literals also
need the new `widget: None` field. Source-backed block providers use the separate
arrangement API below.

## Paragraphs and custom blocks

`ParagraphStyle` supplies alignment, left/right insets, first-line indentation,
spacing before/after, a background and a leading rule. Negative first-line indent
creates hanging text inside the leading gutter. These properties participate in
measurement, paint, hit testing and caret placement.

Use `ParagraphStyle::font`, `font_size` and `line_height` for line-container
typography, analogous to styling a
[CodeMirror line decoration](https://codemirror.net/examples/decoration/). Unset fields
inherit from the editor; inline spans inherit from that resolved paragraph and
can override individual properties. The paragraph supplies the baseline and
minimum line box even when replacements conceal every character. Its typography
also applies to empty lines and each wrapped row. Taller inline text or widgets
can expand a row, so `line_height` is not a clipping height.

```rust
use voidui::editing::{ParagraphStyle, Projection};

let code = "let answer = 42;";
let code_size = 14.0;
let projection = Projection::new().paragraph(
    0..code.len(),
    ParagraphStyle {
        font: Some(voidui::render::font("monospace")),
        font_size: Some(code_size),
        // None keeps the editor's configured line height.
        line_height: None,
        ..Default::default()
    },
);
```

Attach the style to source line starts, including a zero-length range at an
empty final line. Hiding inline syntax does not remove the line style. For
overlapping paragraph ranges the last matching record in validated range order
wins. Source-backed custom blocks pass their typography to measured cells;
paragraph styles at a cell's source start can override it. Height estimates,
resize reflow and layout-cache invalidation use these same resolved settings.

For compound source-backed content implement `BlockLayout::layout` and register it
with `EditorViews::block`, or resolve runtime IDs (one per parsed table, for
example) with `EditorViews::block_dynamic`. The provider receives a `BlockMeasure` and returns a
`BlockArrangement` containing positioned `TextCell` source ranges and decoration
rectangles. `measure_text` uses the editor's native text engine, style spans and
inline views. Those cells share outer-document selection, transactions and IME.
Mark the projected `BlockView` with `.source_backed()` to retain its layout during
composition; preedit maps its source range instead of replacing it with plain text.
Arrow navigation and Tab/Shift-Tab can move between cells; deleting one character
in a cell never deletes its containing block.

`GridBlock` is a reusable table arrangement with explicit row/cell source ranges,
column weights, padding, gaps and rules. Schemas/parsers supply ranges; table
serialization, merge/split commands, validation and rectangular-selection policies
belong to the table extension. The example implements a small source-backed table
provider without adding table syntax to the editor core.

### Interactive source-backed decorations

Return `arrangement.with_decoration(description)` from a `BlockLayout` to attach
an ordinary `ViewDescription` / `EmbeddedView` to its source-backed cells. Include
the computed cell geometry and source inputs in the description's equality. The
arrangement owns the block size; the decoration is measured for its internal
controls and painted behind the cell text and inline child views.

Decorations use the existing embedded-view mounting, focus and pointer capture
protocol. Their bounds use the same coordinate system as paint and input.
Return `false` from `ignore_event` and from unhandled `input` calls to preserve
normal source caret placement and text selection. Inline child widgets take hit
priority; a decoration is never an atomic source replacement.

Use `SelectionSet::structured` for an object/cell selection that has a source
navigation anchor but is not a text insertion point. It suppresses the text caret
and automatic `RevealOnSelection` behavior. Set a normal text selection when the
user explicitly enters source editing. Inline views that should delegate pointer
gestures to the block can return `false` from `EmbeddedView::pointer_events`.

A source-backed decoration can expose `SourceViewport` to clip and scroll its
cell text and inline views without changing the block's document extent. Return
the viewport bounds and retained offset in block-local coordinates. Implement
`reveal_source` to follow the active caret after keyboard/IME edits. Passive caret
queries never move the viewport. Wheel input reaches the decoration after inline
views; its controls remain fixed outside the scrolling content.

A view with a structured selection can override `selected_text()` and `has_focus()`
without implementing a second `TextInputClient`. This supports table clipboard
formats while retaining the host's IME and source geometry. Document transactions
issued during embedded input trigger the editor's `on_change` notification.

## Virtualization and performance contracts

Built-in controls pass their viewport to `EditorLayout::prepare_snapshot`.
Unseen blocks retain source ranges and estimated heights, not native glyph layouts.
A prefix-height index updates and finds scroll positions logarithmically. Measured
block heights survive eviction of shaped paragraphs. Projection updates
compare local source, styles and embedded-view descriptions, so changing a marker
elsewhere does not discard an unchanged block's geometry.

Before a layout update, the editor captures a source position near the viewport
top and its screen offset. Consecutive edits map that position forward; folding,
widget measurements and width changes restore the same source anchor after
layout. A viewport at the bottom follows the document end. Explicit scrolling
replaces the anchor, while keyboard/IME navigation retains its caret-reveal policy.

`prepare_snapshot`, `set_viewport` and `reflow` return the corrected vertical
scroll offset. Custom hosts must apply it before supplying their next viewport.
The built-in controls handle this automatically. Overscan and retained cache size
are configurable through `ViewportOptions`; visible working-set blocks are kept
even if that set exceeds the requested cache target.

Source edits with consecutive revisions update only the changed hard-line window
when structural replacement boundaries are unchanged. Prefix/suffix metadata is
remapped without scanning its text. The metadata vectors are still contiguous, so
updating their indices has linear cost. Width-only updates rebreak retained native
glyphs. Evicted blocks reshape when revisited. Legacy `prepare` / `prepare_styled`
measure all blocks for callers that require exact whole-document intrinsic size.

A custom block may set `BlockArrangement::viewport_dependent` and emit only visible
children while reporting its full extent. It receives a block-local viewport and a
`required_position` for offscreen caret queries. Providers are rerun as these inputs
change. This supports virtual tables and application-defined paged content without
retaining all child glyphs. The default native text flow still shapes one complete
hard paragraph to preserve Unicode shaping/bidi context; it does not automatically
split a huge unbroken paragraph into artificial lines. Applications needing bounded
layout for such content must supply a segmented block provider and its boundary policy.

## Verification and example

```sh
cargo run --example editor_extensions
cargo test --test editor_framework --test input --test rich_layout --test rich_editing
cargo test --workspace
cargo test --doc -p voidui
cargo check --no-default-features --lib
cargo bench --bench rich_text -- 1000 500
```

Tests cover source/display mapping, multi-paragraph folds, source-backed grids,
custom baselines, real inline image scenes, embedded control input/IME, extension
commands/lifecycle, viewport eviction/remounting, virtual compound blocks and
remote carets, incremental index equivalence, rope snapshots and chunked graphemes.
The 20,000-paragraph workload asserts bounded shaping/cache counts, not timing.

See the [benchmark guide](../benches/README.md) for current workload commands and
measurement limits. Source-block metadata remains vector-backed: edits can remap
prefix/suffix indices in linear time even when glyph layout remains localized.
Cache targets retain the visible working set if it exceeds the requested count.

### Composition geometry and captured selection scrolling

`ProjectionSnapshot::composing` distinguishes transient display text from a
committed snapshot without changing document identity. Source-backed layouts can
use `BlockMeasure::composition_cells()` to preserve widths and minimum heights
across IME candidates. The viewport measures its source anchor before resolving
visible indices, including when the anchor is inside a tall compound block.

An embedded view can opt into timed edge scrolling with `selection_drag()` and
implement `scroll_source(delta)` for its local viewport. The host scrolls the
surrounding document and redispatches pointer movement using current geometry.
Only active edge selection gestures schedule frames; release, cancellation,
focus loss and reaching a scroll limit stop the loop. Custom hosts can drive the
same schedule through `WidgetTree::next_input_frame` and `tick_input`.
