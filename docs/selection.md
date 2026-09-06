# Text Selection

Read-only text can be selected continuously across the document scope of a single `WidgetTree`, without being limited by individual `text()` widgets or nested container boundaries. For example:

```cpp
voidui::column(
    voidui::text("VoidNote"),
    voidui::text("Hello, VoidUI!")
)
```

Dragging from the first paragraph to the second highlights both. The start and end points may fall in the middle of either text run. Reverse dragging, changing direction during a drag, crossing whitespace between elements, and moving beyond text bounds all update the same selection. After double-clicking to select a word, you can continue dragging by word across elements.

When dragging across non-selectable elements such as buttons or across container whitespace, the framework looks for a text endpoint in the nearest content container under the pointer. This prevents selection from jumping to an adjacent column merely because its text is at a similar vertical position. Dragging directly onto selectable text in another container still extends the selection.

Dragging near the edge of a scrollable region starts automatic scrolling. Scrolling and selection continue even while the pointer remains still, and the speed increases with the distance beyond the edge. Both vertical and horizontal scrolling are supported. Nested scrollable regions give priority to the inner region; once it reaches its limit, the outer region takes over if the pointer is also near the outer edge. Inputs and textareas scroll only their own editable content.

Automatic scrolling stops when the pointer returns to the middle of the region, the mouse button is released, Escape is pressed, the window loses focus, or the selection is cleared. The window also stops receiving continuous wake-ups after the scroll limit is reached. Pressing text without dragging does not start scrolling.

Ctrl+A (or Command+A) selects all selectable text in the current document without requiring an existing selection. Ctrl+C, Command+C, and the Copy key copy the same range; Escape clears the selection. Copied text is concatenated in component declaration order. A newline is inserted between vertically separated text runs, text on the same line is joined directly, and existing newlines are preserved. `WidgetTree::selected_text()` returns the same UTF-8 text.

The VSS `user-select` property controls which content participates in selection:

```css
.no-select { user-select: none; }
.selectable { user-select: text; }
.atomic { user-select: all; }
```

- Text with `none` is neither highlighted nor copied, but it does not block a selection that crosses it. Descendants can explicitly set `text` to restore selection.
- `all` treats all selectable text in the container as a unit. Clicking one of its text runs or dragging into it from outside includes the entire group; the selection can still continue beyond the group.
- Hidden content and button labels that are non-selectable by default do not participate in document selection.
- `input` and `textarea` retain independent editing selections. Dragging out of an input does not select the surrounding document, and Ctrl+A applies only to that input.
- Each overlay has an independent selection scope and is not joined to the underlying page. The selection is cleared when the overlay closes, a selection endpoint is removed, or an endpoint becomes non-selectable.

These behaviors follow the [CSS UI `user-select` rules](https://www.w3.org/TR/css-ui-4/#content-selection). VoidUI does not implement the complete HTML layout or DOM Range model: newlines between elements are inferred from layout rows when copying, and Shift+click selection extension is not currently implemented.

Selection reuses the generated text layout, so dragging does not reshape text. Run `xmake run voidui_selection_selftest` to verify cross-node highlighting, copied text, UTF-8 handling, nested transforms, selection policies, and input/overlay boundaries.

Run `xmake run voidui_selection_autoscroll_selftest` to verify automatic scrolling, selection updates with a stationary pointer, handoff between nested scroll regions, and stopping conditions. The test advances the window clock and does not depend on real waiting or mouse input.
