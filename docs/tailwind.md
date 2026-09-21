# Tailwind-style utilities

Widget and component builders support inherent utility methods without a trait import,
stylesheet registration, Node.js, or a build step:

```rust
use voidui::{div, text};

let panel = div()
    .flex_col().w_full().p_4().gap_2()
    .border_r(1).border_l_2().border_gray_200()
    .bg_gray_50().rounded_lg()
    .child(text("Native utility styles").text_lg().font_semibold());
```

The same methods work on component invocations, including builders with children:

```rust
use voidui::{Children, component, div, text};

#[component]
fn panel(children: Children) { div().children(children) }

let view = panel().w_full().p_4().bg_gray_50()
    .child(|| text("Component content"))
    .border_r_1().cursor_col_resize();
```

The styles override the component's actual root, with no wrapper node. See
[component root styles](composition.md#root-styles) for precedence, memoization,
and component inputs that share a style method's name.

`w(value)`, `h(value)`, `p(value)`, `px(value)`, `m(value)`, `gap_x(value)`,
`border(value)`, and `border_r(value)` are aliases for the typed style setters.
Numbers are logical pixels: `p(4)` is 4px. Presets use the spacing scale:
`p_4()` is `calc(var(--spacing, 0.25rem) * 4)`. The root font size determines rem;
voidui does not install a browser reset or force a 16px root font size.
The existing default is `content-box`: add `box_border()` (or `box-border`)
when width/height should include padding and borders.
`border_l_2()` means 2px, independently of the spacing scale.

### Values shown in method documentation

IDE hover text and Rustdoc show each preset's CSS properties, default values,
and relative-unit examples. Pixel examples assume a **16px root font size** and
unchanged theme variables. `px` always means logical pixels; display scaling
determines physical pixels.

| Method | CSS value with the default theme | Pixel interpretation |
| --- | --- | --- |
| `border_r_1()` | `border-right-width: 1px` | Always 1 logical px |
| `border_l_2()` | `border-left-width: 2px` | Always 2 logical px |
| `w_16()` | `width: 4rem` (`16 × --spacing`) | 64px at a 16px root font; 80px at a 20px root font |
| `w(16)` | `width: 16px` | Always 16 logical px |
| `p_4()` | `padding: 1rem` | 16px on each side at a 16px root font |
| `p_0p5()` | `padding: 0.125rem` | 2px on each side at a 16px root font |
| `rounded_lg()` | `border-radius: 0.5rem` | 8px at a 16px root font |
| `text_sm()` | `font-size: 0.875rem`; line height `1.25 / 0.875` | 14px text with a 20px line height at a 16px root font |
| `leading_loose()` | `line-height: 2` | Twice the element's font size: 32px for 16px text |
| `w_full()` | `width: 100%` | Containing block width; no fixed pixel value |
| `w_1_2()` | `width: 50%` | 200px when the containing block is 400px wide |
| `w_screen()` | `width: 100vw` | Logical window width; updates when resized |

For example, overriding `--spacing` to `0.5rem` makes `w_16()` use `8rem`
(128px at a 16px root font size). Width describes the CSS box selected by
`box-sizing`; use `box_border()` to include padding and borders in that width.

## CSS classes

Compile the classes your application uses and register the sheet once:

```rust,no_run
use voidui::{Application, WindowOptions, div, text};
use voidui::style::tailwind;

let sheet = tailwind::stylesheet(
    "flex flex-col w-full p-4 gap-2 bg-gray-50 hover:bg-gray-100 text-lg font-semibold"
)?;
let app = Application::new()
    .stylesheet(sheet)
    .window(WindowOptions::default(),
        div().class("flex flex-col w-full p-4 gap-2 bg-gray-50 hover:bg-gray-100")
            .child(text("Native classes").class("text-lg font-semibold")));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `tailwind::all()?` to register all base classes if names are composed at
runtime. State variants still need an explicit `tailwind::stylesheet(...)` sheet.
`tailwind::classes()` lists the supported base classes. Unknown classes and
unsupported variants return errors when compiling an explicit list. Ordinary
`.class(...)` remains a selector identity API; it does not compile utility names.

Supported variants are `hover:`, `focus:`, `focus-within:`, `active:`, `disabled:`,
`enabled:`, `read-only:`, `read-write:`, and `placeholder-shown:`. Combined variants
require every state to match. Append `!` for an important class, e.g. `p-4!`.
Native pointer/focus handling drives these states; disabled state uses the
widget's `disabled` attribute. These rules use voidui's existing state semantics.

## Precedence and themes

Fluent utility methods are inline styles. The last setter wins for each property,
including when mixing long names (`padding_left`) and utilities (`pl_2`). Normal
CSS classes sit below inline styles; CSS `!important` sits above them.
Class-string order does not control precedence. Catalog order is stable: an
all-edge spacing utility precedes axis utilities, which precede side utilities.
Within a family, later scale entries win. Do not rely on conflicting classes to
express an override; use one class or an explicit fluent setter.

Both APIs share theme variables, resolved through the normal CSS cascade:

```rust,no_run
use voidui::{Application, div};
let app = Application::new().css(
    ":root { --spacing: 0.5rem; --color-gray-50: #f4f6f8; --radius-lg: 1rem; }"
)?;
let panel = div().p_4().bg_gray_50().rounded_lg();
# Ok::<(), Box<dyn std::error::Error>>(())
```

Colors, font weights, text sizes and line heights, corner radii, and shadows use
the corresponding `--color-*`, `--font-weight-*`, `--text-*`,
`--text-*--line-height`, `--radius-*`, and `--shadow-*` variables with built-in
fallbacks. Defaults are pinned to Tailwind CSS v4.1.13; its MIT license accompanies
the catalog. No theme values are written globally, so scoped overrides inherit.

## Coverage and limits

- Sizes: `w`, `h`, `min-w`, `min-h`, `max-w`, `max-h`, `basis`, and `size`, with
  common spacing steps, fractions, full/auto and supported intrinsic sizes.
  Fractions use underscores in Rust: `w_1_2()`; half steps use `p_0p5()`.
- Spacing: padding, margin, gaps, insets, axes and individual sides; auto margins.
- Layout: block/flex/grid, flex directions and sizing, alignment, grid rows/columns
  and spans 1–12, positioning, box sizing, visibility and overflow.
- Paint: border widths by side/axis, uniform border colors/radii, all default
  color palettes at shades 50–950, black/white/current/transparent, box shadows,
  opacity and common z-index values.
- Text: sizes xs–9xl with line heights, font weights, italic, alignment and wrapping.
- Interaction: all 36 standard cursor keyword utilities, user selection, pointer events.

### Cursor utilities

Every standard keyword from the [Tailwind cursor reference](https://tailwindcss.com/docs/cursor)
has a CSS class and a Rust method. Replace hyphens with underscores in Rust:
`cursor-col-resize` becomes `cursor_col_resize()`.

```rust
use voidui::div;

let column_handle = div().w_1().h_full().cursor_col_resize();
let row_handle = div().h_1().w_full().cursor_row_resize();
let corner_handle = div().size_4().cursor_nwse_resize();
```

The complete keyword set is `auto`, `default`, `pointer`, `wait`, `text`, `move`,
`help`, `not-allowed`, `none`, `context-menu`, `progress`, `cell`, `crosshair`,
`vertical-text`, `alias`, `copy`, `no-drop`, `grab`, `grabbing`, `all-scroll`,
`col-resize`, `row-resize`, `n-resize`, `e-resize`, `s-resize`, `w-resize`,
`ne-resize`, `nw-resize`, `se-resize`, `sw-resize`, `ew-resize`, `ns-resize`,
`nesw-resize`, `nwse-resize`, `zoom-in`, and `zoom-out`.

These methods set pointer appearance. Resize/drag behavior needs event handlers;
the operating system supplies the native cursor shape. `cursor_none()` hides
the pointer. Cursor values inherit, and later setters override earlier ones.
State classes such as `hover:cursor-col-resize` work through `tailwind::stylesheet`.
Image cursors and arbitrary-value cursor classes are not implemented.

### Builder naming and remaining limits

Existing `flex_row()`/`flex_col()` also enable flex display for compatibility with
voidui's builder API. Their CSS classes only set direction, as in Tailwind.
Use `flex_wrap_on()` for the `flex-wrap` preset; `flex_wrap(value)` remains the
typed setter. Use `border_1()`/`border_r_1()` for the `border`/`border-r` classes.

This is a native utility subset, not a loader for arbitrary Tailwind compiler
output. It does not implement Preflight, `@theme`, layers, media/container queries,
responsive/dark/group variants, arbitrary-value class syntax, negative class names,
plugins, generated content, or per-corner radii/per-side border colors. Use existing
typed setters for custom and negative lengths, and ordinary CSS for custom classes.
Unsupported capabilities are not silently approximated.
