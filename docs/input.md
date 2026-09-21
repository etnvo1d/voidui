# Input, textarea, and reusable editing

Ordinary controls bind directly to `State<String>`. They own their cursor,
selection, IME preedit and undo history internally.

```rust
use voidui::{component, div, input, state, textarea, IntoElement};

#[component]
fn fields() -> impl IntoElement {
    let title = state(String::new);
    let notes = state(|| String::from("Initial notes"));

    div()
        .child(input(title).placeholder("Title"))
        .child(textarea(notes).rows(5))
}
```

`state(|| "")` creates `State<&str>`. Editable text needs an owned string; use
`state(String::new)` or `state(|| "Initial value".to_owned())`. Both `input(value)`
and `input(&value)` accept String state handles. Borrow the handle or clone it
when another callback also needs it.

The binding subscribes the retained input directly, without subscribing its
parent component. Typing does not rerun the parent unless the parent explicitly
reads the value. Only committed edits update String state; preedit remains local.
An equal value echoed by application code preserves selection, composition and
history. A different external value replaces the displayed text, cancels preedit,
clamps the caret and clears that control's undo history. Input removes line breaks
from its display; textarea normalizes CRLF/CR to LF. The next user edit publishes
the normalized value. Application writes do not fire `on_change`.

State belongs to its owning component. Lift it to a surviving ancestor when its
value should persist after hiding/unmounting an input. Cursor and history survive
ordinary reconciliation with the same widget identity and binding; changing the
binding creates a fresh editing session.

## CSS and groups

Use ordinary CSS selectors and declarations. `InputGroup` is a `div` with a
`role="group"` attribute and an `input-group` class. Its user-agent default is
flex layout; author CSS can replace it with grid, block or another layout.
No private CSS properties or parser syntax are required.

```rust
use voidui::{component, div, input, input_group, state, text, IntoElement};

#[component]
fn search() -> impl IntoElement {
    let query = state(String::new);
    let clear = query.clone();
    input_group()
        .child(text("Search").class("addon"))
        .child(input(query).placeholder("Find a document"))
        .child(div().tag("button").child("Clear")
            .on_click(move || clear.set(String::new())))
}
```

```css
.input-group {
    display: flex;
    align-items: center;
    gap: 8px;
    border: 1px solid #cbd5e1;
    border-radius: 6px;
}
.input-group:focus-within { border-color: #2563eb; }
.input-group > input { flex-grow: 1; border: none; }
input, textarea { padding: 8px; caret-color: #2563eb; }
input::placeholder { color: #64748b; font-style: italic; }
input:read-only { color: #64748b; }
input:disabled { cursor: not-allowed; }
input::selection, textarea::selection {
    color: white;
    background-color: #2563eb;
}
```

New standard CSS support includes `caret-color` (`auto`, colors, CSS-wide
keywords), `caret-animation` (`auto`, `manual`, CSS-wide keywords), `::placeholder`
text styling, `:placeholder-shown`, `:read-only` and `:read-write`. Existing
`:focus`, `:focus-within`, `:disabled`, `::selection`, typography, borders, shadows,
flex/grid and box sizing also apply. This remains the documented CSS subset in
[CSS](css.md), not a complete browser engine. In particular, this change does not
implement the full `white-space` model; `text-wrap-mode: nowrap` disables textarea
soft wrapping while preserving explicit line breaks.

Group background/addon clicks focus the first enabled input. Buttons preserve
their own focus and activation. Addons are ordinary sibling widgets; their text
never enters the editor's value, copied text, or undo history. Use normal layout
properties for prefixes, suffixes and textarea headers/footers.

## Input behavior

- Input is single-line, with horizontal caret-following scroll. Textarea supports
  soft wrapping, explicit paragraphs, vertical scroll and optional unwrapped text.
  Textareas use shared CSS scrollbars and chain wheel input at their boundary; see
  [Scrolling](scrolling.md).
- `rows`/`columns` set intrinsic dimensions. CSS width/height/min/max sizes take
  precedence. Content changes do not automatically grow the control.
- Arrows, Shift-selection, word movement/deletion, Home/End, Page Up/Down,
  select-all, copy/cut/paste and undo/redo are supported. Platform command keys are
  Cmd on macOS and Ctrl elsewhere. Tab moves focus by default.
- Click places a caret, double-click selects a word, triple-click selects a hard
  paragraph; Shift extends selection and Alt-click adds another caret.
  Pointer selection preserves the viewport; an out-of-bounds drag notification
  scrolls toward the pointer. Keyboard navigation and typing reveal the caret.
  Scroll extents describe actual content: a caret on the right edge is drawn
  inward, with matching IME and completion bounds, without adding horizontal overflow.
- IME preedit is separate from committed document text. Commit replaces all
  selections in one history group; only the primary selection displays preedit.
  Blur cancels preedit. Composition confirmation does not submit the control.
- `.read_only(true)` permits selection/copy but blocks default editing commands.
  `.disabled(true)` also excludes the control from focus and input dispatch.
- The caret schedules one deadline only while focused, eligible and drawable.
  `caret-animation: manual` disables automatic blinking; unfocused/hidden windows
  have no repeating editor task.

There is no form dependency. `on_submit` is an application callback, not automatic
form submission. Password masking, maxlength/validation UI, touch handles, continuous out-of-bounds drag autoscroll, native accessibility trees
and drag-resize handles are not included in this first implementation.

## Explicit editing sessions

Use `Editor` when the application needs document transactions, multiple selections,
custom keymaps or direct editor geometry. String state binding is optional.

```rust
use voidui::{Editor, textarea};
use voidui::editing::{EditKind, Selection, SelectionSet};

let editor = Editor::new("alpha alpha");
editor.update(|session| {
    session.select(SelectionSet::new(
        [Selection::caret(5), Selection::caret(11)], 1,
    )?)?;
    session.replace_selections("_suffix", EditKind::Command)?;
    Ok::<_, voidui::editing::EditError>(())
}).unwrap();
let view = textarea(&editor);
```

Clone an Editor handle to share its session (including selections). In components,
`state(|| Editor::new(...)).get()` creates that handle once. The ordinary String
binding's editor is owned by its view; choose an explicit Editor when sharing
advanced session state across views.

`EditorState` and `Document` are independent of widget trees. `Transaction` uses
UTF-8 byte ranges from one explicit revision; overlapping edits, invalid UTF-8
boundaries, invalid resulting selections and stale revisions fail before mutation.
`ChangeSet` maps selection/decoration positions to the next revision. Retain the
`Change` values returned by transactions or undo/redo when maintaining external
indexes. `SelectionSet` normalizes overlapping ranges and retains a primary range.
The single-selection representation has no extra range-vector allocation.

Undo stores edited fragments, not full document snapshots. `HistoryOptions`
controls the retained history payload budget and typing/deletion merge interval.
`max_bytes: 0` disables history allocation. Programmatic commands and IME commits
are separate groups. The budget includes fragment and record storage, but excludes
allocator metadata and spare capacity of the outer history containers.

`EditorState::constrain` installs a transaction validator while its returned guard
lives. Input uses this to reject newline edits to an explicit Editor, including
undo of older multiline history. An explicit single-line Editor must initially
contain no line breaks. Applications can use constraints for their own invariants.

## Extending behavior or implementing a Widget

`.on_key` runs before the default keymap, except during IME composition. Return
true when the command is consumed. The supplied `KeyContext` exposes `editor`,
`layout`, `move_selection`, and global `caret_bounds` for popup positioning.

```rust
use voidui::{Editor, textarea};
use voidui::editing::Motion;
use winit::keyboard::Key;

let editor = Editor::new("First line\nSecond line");
let view = textarea(&editor).on_key(|key, context| {
    if key.key == Key::Character("j".into()) {
        context.move_selection(Motion::Down, false).unwrap();
        true
    } else {
        false
    }
});
```

This is a command-extension example, not an installed Vim mode. A modal keymap
can keep mode/operator state in its own captures. Completion accepts a candidate
with a transaction; menus can use `KeyContext::caret_bounds` or
`WidgetTree::input_bounds_for_range` with the existing overlay system.

`EditorLayout` exposes retained paragraphs, hit testing, caret geometry, visual
navigation and multiple-selection painting for custom views. The default view
also accepts sparse inline styles from the document, with rich transactions,
format history, and preedit projection; see [Rich text foundations](rich-text.md).
Projection, folding, embedded views, source-backed custom blocks and viewport layout
are described in [Extensible editor framework](editor-framework.md). No Markdown, Vim, language server or completion algorithm is
hardcoded into the document core.

A custom `Widget` can implement `TextInputClient` without adopting EditorState.
The client protocol separates key commands, committed text and preedit, and exposes
platform-visible UTF-8 text/range queries including composition. The Winit adapter
handles focused routing, clipboard and candidate-window placement. A platform
adapter requiring UTF-16 must convert explicitly. The current native adapter uses
Winit's available IME API; full platform surrounding-text/accessibility integration
still requires additional platform work.

External widget models use `WidgetInvalidator` and `Widget::update_model`.
`State::subscribe_widget` returns a guard for direct state-to-widget invalidation;
use `with_untracked` in the retained widget to avoid subscribing its parent.
Subscriptions and invalidators are weak and stale node IDs are ignored.

## Performance and verification

TextEdit has fixed intrinsic row/column sizing, so typing normally invalidates
painting only. A separate label or layout-dependent CSS selector that reads the
value can still legitimately cause layout. Paragraphs retain native Parley glyph
data; local edits rebuild changed hard paragraphs, width changes only rebreak, and
color/caret/selection changes do not shape text. Painting skips offscreen blocks.
Placeholder layout is prepared only when it is displayed.

The document keeps short strings inline and promotes larger sources to a rope.
Built-in controls retain viewport layouts and use source-range snapshots for IME.
Legacy whole-text reads remain available explicitly; see the framework guide for
incremental indexing, cache limits and large single-paragraph boundaries.
String bindings publish an owned value only on committed edits; use an explicit
Editor to avoid whole-value copies for large documents. No new text/font service
or dependency is created per input.

```sh
cargo test --test editing --test input --test state
cargo test --workspace
cargo test --doc -p voidui
cargo check --no-default-features --lib
cargo run --example input
cargo run --example input -- --smoke target/input-smoke
cargo bench --bench editing -- 10000 1000 1000
```

The default `editing` Cargo feature enables built-in controls and the reusable
editor module. Disabling it retains the generic custom-widget input protocol.
The benchmark reports requested heap bytes, not RSS. Native smoke checks no extra
box layout during typing, multi-cursor edits, and a two-second parked caret.

## General pointer and keyboard handlers

Inputs support the same sync/async `on_mouse_*`, `on_click`, `on_drag`, and
`on_key_down`/`on_key_up` methods as other widget builders. These methods preserve
the concrete input builder, so `placeholder`, `read_only`, and other input options
remain available after registration. General key handlers run before the editor's
specialized key handler and defaults; synchronous `PREVENT_DEFAULT` can suppress
editing/Tab traversal. IME text remains a separate input protocol.

An ancestor's automatic drag does not steal selection from an input. Attaching
`on_drag` directly to the input explicitly opts it into that gesture. See
[Events and dragging](events.md) for capture and async callback lifetime rules.
