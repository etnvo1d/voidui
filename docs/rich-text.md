# Rich text foundations

Rich labels and editable documents share `RichText`, `InlineStyle`, and UTF-8
`StyleSpan` ranges. Formatting does not create widget boxes. All runs in a hard
paragraph share Unicode analysis, bidi ordering, wrapping, and a baseline on each
visual row. The existing editor owns selection, IME, transactions, and history.

## Compose inline content

```rust
use voidui::{rich_text, span, InlineStyle};
use voidui::style::color::Rgba8;

let label = rich_text(
    span("Hello ")
        .child(span("rich text").bold().color(Rgba8::from_rgb8(80, 50, 160)))
        .child(". ")
        .child(span("Nested ").italic().child(span("styles").bold())),
).font_size(18.0);
```

`span` is a temporary inline builder, not a widget. A parent span's own text comes
before its children. `Inline::build()` validates and flattens nesting once;
`rich_text` also accepts the builder directly. Adjacent equivalent spans coalesce.
Unset style fields inherit the label/editor's typography. Font family, weight,
and posture can be overridden independently; `.bold()` does not select a family.
An explicit inline color survives changes to the container color. Selection
foreground still overrides selected text.

`InlineStyle` supports font descriptors (including fallback and OpenType options),
weight, posture, size, absolute line height, foreground, background, underline,
and strikethrough. Sizes and line heights must be finite and positive. An omitted
inline line height inherits the paragraph's resolved absolute height, independently
of font size. Set both when changing row spacing is intended.

```rust
use voidui::{InlineStyle, RichText, StyleSpan};

let content = RichText::from_spans("Title\nBody", [
    StyleSpan::new(0..5, InlineStyle::new().bold().font_size(28.0).line_height(38.0)),
])?;
let fragment = content.slice(0..5).unwrap();
assert_eq!(fragment.text(), "Title");
# Ok::<(), anyhow::Error>(())
```

`from_spans` sorts ranges, rejects overlaps and invalid UTF-8 boundaries, drops
empty/default intervals, and merges adjacent equal styles. Gaps use the base style.
Use nested builders or editing patches for overlapping formatting. RichText
clones share text and spans; retained storage contains no per-character nodes.

## Edit through a stable handle

```rust
# #[cfg(feature = "editing")]
# fn main() -> Result<(), voidui::editing::EditError> {
use voidui::{Editor, rich_editor, span};
use voidui::editing::{Selection, SelectionSet, StylePatch, Transaction, Edit};

let editor = Editor::from_rich(span("Hello ").child(span("world").bold()));
let view = rich_editor(&editor).rows(8).placeholder("Write something…");

editor.update(|state| {
    state.select(SelectionSet::single(Selection::range(6, 11)))?;
    state.format_selections(StylePatch::new().italic(true))?;
    state.undo()?;
    state.redo()?;
    Ok::<(), voidui::editing::EditError>(())
})?;

editor.update(|state| {
    let transaction = Transaction::new(state.revision(), [Edit::new(0..0, "New ")])
        .format(0..3, StylePatch::new().bold(true));
    state.transact(transaction)
})?;
# Ok(())
# }
# #[cfg(not(feature = "editing"))]
# fn main() {}
```

`rich_editor(&editor)` uses the same multiline view as `textarea(&editor)`.
Existing readonly/disabled rules, scrollbars, key hooks, selection colors, and
change callbacks apply. `Editor::new` and String-bound inputs continue to work.
A String binding only carries plain text; retain an Editor when formatting matters.

Each transaction carries an expected document revision. Text edits use **input**
coordinates; formatting operations and explicit selection use **output** coordinates.
Formats execute in order. Validation and constraints run before any mutation.
Formatting-only operations increment revision and invalidate mounted views, even
when text bytes do not change. They participate in the same undo/redo stack and
break typing groups. No-op patches create no history entry.

`StylePatch` distinguishes keep, set, and clear per property. Fluent methods cover
common operations; public fields use `None`, `Some(Some(value))`, and `Some(None)`
respectively. `StylePatch::clear()` removes all explicit fields, including metadata.
`StylePatch::replace(style)` replaces the complete style. `bold(false)` and
`italic(false)` explicitly choose normal typography even under a styled container.

Formatting a caret sets pending typing style without adding a standalone undo
entry. Moving selection clears that override. Plain insertion inherits formatting
at its insertion point; an explicit `Edit::rich` or `Edit::styled` uses exactly the
supplied overrides, including unformatted gaps. `replace_selections_rich` applies
one fragment at all selections. Deletion and undo preserve surviving styles and
restore removed styles exactly.

IME preedit captures its insertion style and projects spans into display coordinates.
It does not mutate committed text or history. Cancel discards the projection;
commit records one rich edit. Rejected commits retain preedit. `display_spans()`
borrows committed spans when no highlight layer is active; projection allocates only during composition.

## Automatic highlighting without undo entries

Use `EditorState::set_highlights(revision, spans)` for syntax highlighting and
other derived styles. `Transaction::format` and `format_selections` intentionally
remain undoable user edits; migrate automatic highlighter calls to `set_highlights`.
Do not clear history or temporarily disable it to apply syntax colors.

```rust
# #[cfg(feature = "editing")]
# fn main() -> Result<(), voidui::editing::EditError> {
use voidui::{Editor, InlineStyle, StyleSpan, rich_editor};
use voidui::style::color::Rgba8;

let editor = Editor::new("let value = 1;");
// A parser supplies these UTF-8 ranges for the captured document revision.
let revision = editor.with(|state| state.revision());
let tokens = [StyleSpan::new(
    0..3, InlineStyle::new().color(Rgba8::from_rgb8(120, 60, 180)),
)];
editor.update(|state| state.set_highlights(revision, tokens))?;
assert!(!editor.with(|state| state.can_undo()));
let view = rich_editor(&editor);
# Ok(())
# }
# #[cfg(not(feature = "editing"))]
# fn main() {}
```

Highlights form a separate display layer. Their explicit fields override document
formatting; removing them reveals the underlying styles. They never affect the
document revision, rich exports, input-style inheritance, undo/redo entries, redo
availability, or typing-group boundaries. Highlight changes increment `generation`
to repaint subscribed views. `highlight_revision()` is a display-cache key, not a
document revision. Equal canonical highlights are a no-op; `clear_highlights()`
removes the layer. No highlight storage is allocated for plain sessions.

Text changes, including undo/redo, clear outdated highlights. Recompute them for
the resulting revision, for example in the control's `on_change` callback. External
programmatic edits should trigger the application's highlighter too. Formatting-only
edits retain the layer. Async results must submit the captured document revision;
stale results and invalid spans are rejected atomically. During IME, submit ranges
in committed-text coordinates; `display_spans()` projects them around preedit and
keeps the composition's own typing style. Document edit constraints do not run for
this derived layer.

Custom views should use `display_spans()` and include `highlight_revision()` in
their cache key. It borrows document spans when neither highlights nor preedit are
active; otherwise canonical interval streams are merged without copying text.

## Component and integration APIs

- `Document::spans`, `style_at(position, bias)`, and `rich_slice` expose formatting
  without flattening every read into an owned string.
- `ChangeSet::edits` describes text deltas in input coordinates;
  `style_changes` describes exact style replacements in output coordinates.
  `is_empty` includes both. Undo/redo return deltas in execution order.
- `EditorLayout::prepare_styled(text, spans, options, system)` retains paragraphs
  and supplies hit testing, visual navigation, caret/selection geometry and paint.
- `PreparedText::shape_rich(cache, content, options)` provides the same inline
  layout for custom read-only widgets, including intrinsic widths and baselines.
- `InlineStyle::metadata` stores shared key/value attributes. Link targets,
  annotation IDs and application marks travel through rich slices, edits and
  history. Metadata has no built-in activation, fetching or rendering behavior.
  Equivalent visual styles across metadata boundaries share shaping runs.

The OS clipboard adapter currently exchanges plain strings. Component clipboard
adapters can use `rich_slice` / `replace_selections_rich` for their own rich formats;
no HTML parser, unsafe markup execution, or private clipboard encoding is installed.

## Extensible editor layout and storage

The editor now adds source-backed projection, hiding/folding, inline images/widgets,
paragraph geometry, source-backed block providers and viewport layout. See
[Extensible editor framework](editor-framework.md) for APIs, lifecycle, input mapping,
storage policy, virtual compound blocks and precise performance boundaries.

Short controls keep contiguous storage; larger documents promote to a rope.
`Document::read` and snapshots avoid whole-value copies. Compatibility `text()`
materializes a large document explicitly. Rich-text exports remain committed content.
Read-only labels retain the shared inline layout and weak paragraph pool.

## Mixed-height geometry

Parley 0.11.1 assigns some shaped runs the following style's line height and
merges shaping items across line-height-only changes. The regression case with
requested row heights 24, 48, 20 produces incorrect uniform native heights.
The renderer therefore uses a private row adapter when resolved run heights differ.
It leaves Unicode analysis, shaping and horizontal wrapping in Parley, gives native
rows a common height, and computes display advances/baselines from the source
height spans and native ascent/descent. Overlapping native ink bands are separated
through Parley's public line-breaker API before hit testing. No dependency source
or private fields are modified.

The adapter retains coalesced height spans and a reusable compact row vector, not
additional glyphs or per-character caret records. Uniform paragraphs allocate no
adapter. Same-width reflow returns immediately; width changes reuse glyph data.
`Paragraph::layout()` remains useful for native glyphs, logical ranges and visual
horizontal movement. Its mixed-height y coordinates are not display coordinates.
Custom views must use Paragraph's row/caret/selection/baseline APIs or paint method.
Empty editor paragraphs use their separator's explicit line height; the final empty
paragraph inherits the preceding separator. Pending typing style on a completely
empty document affects inserted/preedit text; the empty caret initially uses the
view's base line height.

## Verification

```sh
cargo test --test rich_text --test rich_editing --test rich_layout --test input
cargo test -p voidui_gpui_wgpu --test rich_text
cargo test --workspace
cargo test --doc -p voidui
cargo check --no-default-features --lib
cargo run --example rich_text
cargo run --example rich_text -- --smoke target/rich-text-smoke
cargo bench --bench rich_text -- 1000 500
```

Tests use bundled real fonts and CPU scenes. They cover exact style deltas,
randomized reference-model edits/undo, invalid atomic transactions, metadata,
multiselection, preedit, line geometry, cache isolation and shaping counters.
The benchmark reports requested Rust heap bytes and CPU time; these are not RSS,
GPU memory, or a universal frame-rate claim.
