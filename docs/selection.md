# Text selection

Selection is enabled for ordinary text by default, including `div().child("text")`.
Run `cargo run --example selection` for the native demonstration.

```css
.article { user-select: text; cursor: text; }
.toolbar { user-select: none; }
.copy-code { user-select: all; }
.selection-scope { user-select: contain; }

:root::selection {
    color: #112b22;
    background-color: #bcebcf;
}
.warning::selection { background-color: #ffdda0; }
```

The same element properties are available without a style closure:

```rust
use voidui::{div, text};
use voidui::style::selection::{Cursor, UserSelect};
use winit::window::CursorIcon;

let content = div()
    .child(text("Selectable by default"))
    .child(div().user_select(UserSelect::None).child("Toolbar label"))
    .child(text("Select this whole value")
        .user_select(UserSelect::All)
        .cursor(Cursor::Icon(CursorIcon::Text)));
```

## CSS behavior

`user-select` accepts `auto`, `text`, `none`, `all`, and `contain`, plus the
engine's existing `initial`, `inherit`, and `unset` keywords. It is not normally
inherited. Its `auto` used value propagates a parent's used `none` or `all`;
otherwise it is `text`. A descendant may explicitly opt back into selection.

`none` prevents starting a user selection and preserves an existing selection
when clicked. A range crossing it excludes its text from both highlighting and
plain-text copying. `all` selects the relevant ancestor atomically, with the
non-all-descendant exception when the range stays entirely inside that descendant.
`contain` confines gestures started inside; a gesture from outside cannot end
inside, but can cross the whole element. These rules follow
[CSS UI 4](https://www.w3.org/TR/css-ui-4/#content-selection), including the
specified `-webkit-user-select` compatibility alias. No private CSS property is
needed. The `contain` keyword is defined by this draft; browser support varies.

`cursor` inherits and accepts standard keyword cursors, including `text`, `auto`,
`default`, `pointer`, `none`, and the resize/grab/zoom keywords. `auto` uses an
I-beam over selectable text. During selection, `auto` keeps the cursor from the
press location until release, so pressing empty space or container padding does
not switch to an I-beam. Explicit cursor styles at the pointer still take priority.
Image cursors are not implemented.
`pointer-events: none` does not itself disable text selection.

`::selection` supports `color` and solid `background-color` (`background` can set
that color). Rules use the ordinary specificity/source-order/`!important` cascade,
but highlight inheritance is separate from element inheritance: a child's
highlight can inherit its parent's highlight background. `currentColor` resolves
against the originating element. Any authored/inherited highlight color disables
both paired UA defaults; an unspecified foreground then uses `currentColor`, and
an unspecified background is transparent. See
[CSS Pseudo 4](https://www.w3.org/TR/css-pseudo-4/#highlight-cascade).

Use `:root::selection` to supply an inherited document default. A universal
`::selection` rule matches every element and can override inherited values.
Non-applicable supported properties, such as padding and font size, do not change
layout when written in a highlight rule. Highlight text decorations and
`text-shadow` are not implemented. Unsupported declarations retain the engine's
strict parse-error behavior.

## Interaction and Rust access

Native windows handle dragging, Shift-click extension, double-click word selection,
triple-click paragraph selection, and Cmd/Ctrl+A/C. Shift+Left/Right and Home/End
extend a read-only range; word modifiers use Option on macOS and Ctrl elsewhere.
Native arrows follow Parley's visual order, including bidi affinity; Up/Down retain
the preferred column. The explicit Rust Backward/Forward variants still use
logical grapheme order. Mouse geometry handles visually disjoint bidi ranges.

A window has one anchor/focus range in document order, independent of z-index.
Hit testing follows painted stacking/clipping. Hidden and inert content cannot
start selection; a modal constrains interaction to its active scope. The default
UA styling makes `button`, `meter`, `progress`, and `select` non-selectable.

```rust
use voidui::{div, text};
use voidui::core::{selection::SelectionPoint, widget_tree::WidgetTree};

let mut tree = WidgetTree::new();
tree.build_root(div().child(text("Hello world").id("message")));
let id = tree.find_by_id("message").unwrap();
tree.set_selection(SelectionPoint::text(id, 0), SelectionPoint::text(id, 5))?;
assert!(tree.selection().is_some());
// Layout is required before querying visible selected_text/selection_rectangles.
# Ok::<(), anyhow::Error>(())
```

Offsets are UTF-8 bytes, not JavaScript UTF-16 offsets. Text points must be valid
character boundaries; mouse/keyboard gestures use extended Unicode graphemes.
`SelectionPoint::children` addresses logical child boundaries, including a widget's
own text as its first virtual child. Programmatic `set_selection` bypasses
`user-select`, as in the Selection API; this CSS property is not copy protection.
It does not bypass visibility or modal inertness for rendering/copying.

`selection`, `selection_direction`, `selected_range(id)`, `selected_text`, and
`selection_rectangles(id)` expose the current state. `text_caret_position(id, byte)`
uses logical window coordinates; at ambiguous wrap/bidi boundaries it prefers the
downstream caret. `replace_text`, `append_child`, and `remove_subtree` update live
boundaries without replacing surviving widget IDs.

`on_select_start` registers a document-level cancellation guard for a new
non-collapsed user selection. `take_selection_change()` consumes a coalesced
boundary-change notification; it can be called through `window.tree()` without
scheduling another frame. These are Rust counterparts to the relevant
[Selection API rules](https://www.w3.org/TR/selection-api/), not a DOM event
bubbling system. Custom runtimes can call `selection_pointer_down`,
`selection_pointer_move`, and `end_selection_drag` directly after layout.

`AppWindow::copy_selection()` writes plain text to the system clipboard and returns
false for an empty selection without overwriting existing clipboard contents.
The native backend is opened only on the first copy and is shared across windows.
Wayland uses Winit's display, retained until the clipboard backend is released.
No automatic X11/Wayland PRIMARY selection export or rich HTML clipboard format
is provided. Clipboard service failures are returned by the Rust API and logged
by the built-in shortcut handler.

`voidui::clipboard::read_text()` and `write_text(text)` reach the same backend from
event callbacks and components, which have no `AppWindow`. They need a running
application on the calling thread and return an error without one, so an editor
context menu can implement cut, copy and paste with the framework's shortcuts.

## Layout boundaries and cost

Each string child is currently a separate text box. Copying inserts a newline
between selected text boxes, normalizes CRLF, and never inserts soft-wrap newlines.
The existing layout preserves spaces and explicit line breaks. This is **not a
complete HTML text engine**: browser inline formatting, CSS whitespace collapsing,
contenteditable, touch selection handles, continuous drag autoscroll, and vertical
writing are not implemented. [Input and textarea](input.md) have independent
editing selections, IME and insertion carets, with `caret-color` and
`caret-animation` support. They do not join the ordinary document-selection range.

Glyph positions, selection rectangles and cursor affinity come from the retained
Parley paragraph. A UAX #29 boundary guard prevents missing-font fallback from
splitting a combining/ZWJ sequence. CRLF preprocessing has a sparse original-byte
map; source offsets remain stable. `selection_focus_affinity()` exposes the current
visual side of a wrap/bidi boundary.

Dragging updates document range fragments and painting without Taffy layout or
font reshaping. The document index is lazy, and short ranges use binary searches
over its spans. Highlight state is absent on nodes without authored/inherited
highlight rules. There is no selection timer, caret blink, polling task, or
clipboard connection for an idle selection. `WindowOptions::selection` configures
UA highlight colors and multi-click timing/distance. See [Parley](parley.md) for
layout ownership and low-level API migration details.

Verification commands:

```sh
cargo test --test selection
cargo run --example selection -- --pixels
cargo run --example selection -- --smoke target/selection-smoke
cargo run --release --example selection_bench
```

The pixel test checks that transparent selected foreground replaces the original
glyphs; the native smoke checks retained layout and idle frames. The benchmark
measures CPU hit testing and range fragments only, excluding GPU and first-use
font/layout costs.
