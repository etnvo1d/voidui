# Text widgets

```rust
use voidui::{div, text};
use voidui::style::color::Rgba8;

let label_color = Rgba8::from_rgb8(40, 80, 160);
let panel = div()
    .child("text")
    .child(text("text").color(label_color).font("Arial"));
```

Strings become the same `Text` widget as the explicit `text(...)` constructor.
Borrowed `&str` and `&String`, owned `String`, `SharedString`, `Arc<str>`, and
`Cow<str>` are accepted as children. Borrowed content is copied or shared into the
node, so the original string does not need to outlive the tree.

## Typography and inheritance

```rust
use voidui::{div, text};
use voidui::render::{TextAlign, font};
use voidui::style::{color::Rgba8, text::LineHeight};

let panel = div()
    .font("Arial")
    .font_size(18.0)
    .line_height(LineHeight::Relative(1.5))
    .color(Rgba8::from_rgb8(40, 40, 40))
    .child("Inherits the panel's typography")
    .child(
        text("Custom label")
            .font_options(font("Arial").bold().italic())
            .font_size(24.0)
            .line_height(32.0)
            .text_align(TextAlign::Center)
            .wrap(false),
    );
```

Colors, fonts, font sizes, line heights, and text alignment inherit through nested
containers. An explicit child value overrides the parent. Root defaults are the
backend's system UI font, black, 16 logical pixels, and a 1.2 line-height multiplier.
`LineHeight::Pixels` stays fixed when inherited; `LineHeight::Relative` scales with
the child's font size. Font size and line height must be finite and positive.

Inherited declarations use `CssValue<T>`: `Unset` follows normal inheritance,
`Inherit` explicitly takes the parent value, and `Initial` resets it. Paint
properties use the same defaulting type but do not inherit automatically.
`.font(name)` updates only font-family; weight and style inherit independently.
LineHeight::Percent/Em compute to pixels before inheritance, while Relative remains
unitless. See [Fluent styles](style.md) for the full behavior and direct methods.
`tree.text_style(id)` exposes typography after the latest style update, including
sampled transitions. After editing `tree.style_mut(id)`, call `update_styles(now)`
and call `layout_computed` if its `layout` flag is set. Calling `layout` directly still performs
the required style update. Native windows coordinate these stages automatically.

## Layout and drawing

Text participates in Block, Flexbox, and Grid through Taffy's leaf measurement
callback. Min-content width uses Unicode line-break opportunities; max-content
width preserves only explicit line breaks. Finite widths use Parley's paragraph line breaking. Flexbox and Grid receive first/last text baselines, including the top
padding and border inset.

`wrap(false)` disables soft wrapping but preserves explicit newlines. Empty text
occupies no content area. Spaces and explicit blank/trailing lines are preserved;
CRLF is treated as a line break. Text size and line spacing use logical pixels.

Use `.padding(12)`, `.border_width(1)`, `.margin_top(8)`, `.width(200)`, and the
other direct style methods just like on `div`. Text measures and draws inside the content box;
padding and border are counted exactly once. Glyphs are clipped to this content
box. `text_align` moves glyphs inside it and does not align child boxes.

Call `tree.layout(available_space, &text_cache)` followed by
`tree.draw(&mut painter)?`. Use the same `TextSystem` for the cache and painter.
The backend must have an available font; headless tests load the repository's
bundled font explicitly. Missing font families use the backend's fallback rules;
if no font or fallback can be resolved, its existing API panics.

`Widget::draw` now returns `render::Result<()>`, so atlas/rasterization errors reach
the caller. Drawing before a full layout or with unresolved style changes returns an error.
An independent subtree layout does not make the whole tree ready for drawing.
The layout callback remains infallible: a backend shaping error is reported as a
panic with context rather than silently producing an empty label.

Each widget retains native Parley layout data. Width-only probes temporarily
rebreak that paragraph and restore its final width, so later intrinsic queries
cannot overwrite final line breaks. A weak `TextLayoutCache` can share identical
paragraphs; call `finish_frame()` after each frame to prune dead entries. Color and
selection changes reuse the existing paragraph. See [Parley](parley.md).

## Selection

Text is selectable by default. Use `.user_select(...)`, standard `user-select` CSS,
and `::selection` colors to control it. See [Text selection](selection.md) for
gestures, clipboard access, inheritance, performance, and browser-layout boundaries.

## Scope

Each string child is a separate text box; adjacent string children do not become
a browser-style inline formatting context. Use `rich_text(span(...).child(...))`
for shared inline layout and `rich_editor(&editor)` for editable rich content; see
[Rich text foundations](rich-text.md). Ellipsis, CSS whitespace modes, and full CSS
text wrapping rules are not implemented. Font coverage and soft wrapping follow the existing renderer.
The tree paints shared backgrounds, solid borders, rounded box corners, and
rectangular per-axis overflow clips before invoking widget drawing. Generic scrolling is supported; see
[Scrolling](scrolling.md). Rounded overflow clipping is not implemented.

The reusable `core::text::Text` remains available for custom widgets. `shape`
returns independent `PreparedText` with size, baseline, and paint methods;
`measure`/`paint` provide the existing convenience API. Their `line_clamp` option
counts visible rows across hard and soft breaks, without adding an ellipsis.

Run `cargo test --test text` for real-font layout and CPU glyph scene tests.
