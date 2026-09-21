# CSS stylesheets, selectors, and optional live reload

```rust,no_run
use voidui::{Application, WindowOptions, div, text};

fn main() -> anyhow::Result<()> {
    Application::new()
        .css_file("ui.css")?
        .window(WindowOptions::default(),
            div().id("app")
                .child(text("Hello CSS").class("title"))
                .child(div().class("card").attr("tabindex", "0").child("A card")))
        .run()
}
```

```css
#app {
    padding: 24px;
    color: #243444;
    font-family: "Arial", "Helvetica";
}
.title { font-size: 28px; margin-bottom: 16px; }
#app > .card {
    background: #e8f0f5;
    padding: 16px;
    border: 1px solid #789;
    border-radius: 8px;
}
.card:hover { border-color: #28a; }
.card:focus-within { color: #067; }
```

`Application::css(source)` parses source once; `css_file(path)` reads it once.
`stylesheet(Stylesheet::parse(source)?)` accepts an already-compiled stylesheet.
Multiple calls preserve source order. Widgets in every application window receive
those sheets. Use `AppWindow::set_stylesheets` or `WidgetTree::set_stylesheets`
for explicit replacement; old rules are removed rather than merged indefinitely.

## Selectors and cascade

The adapter uses Servo's `selectors` crate and `cssparser`, not string matching
or regular expressions. It supports type, universal, ID, class, attribute selectors
(including matching operators and case flags), descendant/child/adjacent/general
sibling combinators, selector lists, :root, :scope, :empty, structural nth/first/last/
only selectors, :not(), :is(), :where(), :has(), and nth-child(... of ...).

Widget names default to `div`, `text`, or `widget` for custom widgets. A custom
widget can override `Widget::tag_name`; `.tag("button")` provides an explicit name.
`.class("a b")` adds two classes and deduplicates them. `.attr(...)` exposes other
attributes. ID/class attribute selectors use the same canonical values as `.id`
and `.class`. Call `tree.set_id`, `set_classes`, `set_tag`, or `set_attribute` to
change selector identity and invalidate matches.

This is a widget tree, not an HTML DOM: each Text widget is an element named `text`
and counts among element siblings. Its nonempty text content prevents it from
matching :empty. The stateless ::backdrop and ::selection pseudo-elements are implemented. Other pseudo-elements,
namespaces with custom bindings, shadow DOM,
HTML form semantics, and generated content are not implemented.

Cascade precedence, from low to high, is stylesheet normal declarations, explicit
fluent/inline properties, then stylesheet !important declarations. Within a
stylesheet origin, specificity wins before source order. Selector-list rules use
the strongest matching selector; :where contributes zero specificity. Shorthands
expand to longhands before cascading. Inheritance is computed after cascade, so
an explicitly declared child property wins over a value inherited from a parent,
even when the parent's declaration was important.

Fluent setters track explicit writes, including default-valued `width(AUTO)`,
`padding(0)`, and `color(CssValue::Unset)`. Raw non-default `Style` field values also
overlay CSS. When assigning a default value directly to a raw field, call
`style.mark(Property::...)` to distinguish that write from an absent declaration.
Use the fluent API when building elements to get tracking automatically. Cascade
results never overwrite the authored style returned by `tree.style(id)`.

## State and invalidation

Supported states include :hover, :active, :focus, :focus-within, :enabled, :disabled,
:modal, :popover-open, :read-only, :read-write and :placeholder-shown. See [Stacking and overlays](overlays.md) for the Rust
lifecycle behind modal/popover state and ::backdrop styling.
Enabled/disabled use the widget's `disabled` attribute. Native pointer movement
updates the hit widget and its ancestors; moving within the same hit widget does
not recascade. Primary-button press/release drives active state. Widgets tagged
button/input/textarea or carrying `tabindex` can receive pointer focus. Programmatic focus
uses `tree.set_focused`; `set_status` is available for custom state integration.
Native Tab/Shift+Tab traversal supports modal focus scope. General pointer/key
handlers can prevent these defaults; see [Events and dragging](events.md).
Full HTML form-control behavior remains an application responsibility.

Only referenced state bits invalidate stylesheet matching. Focus-only sheets do
not recascade when hover changes; attribute-only disabled rules do not start mouse
hit testing. Focus-within uses maintained ancestor counts rather than scanning
subtrees during each match.

Selectors are compiled and indexed by one mandatory rightmost ID, class, or type.
Selectors without such a key go in a fallback bucket. For one node the engine
checks only its buckets, then delegates full matching and specificity to Servo.
Per-pass nth/:has caches are reused across nodes. Matching is conservative at tree
level after identity/state/stylesheet changes, preserving sibling and relational
selector correctness. Fine-grained dependency invalidation is not yet implemented.

`tree.cascade_stats()` reports cumulative passes, candidate tests and successful
selector matches. An unchanged layout, exposure, or resize does not rematch
selectors. Percentage geometry still reflows through Taffy on resize. The retained
window scene means idle windows do not layout, paint, parse, or match CSS at all.

## Hot reload: off by default

Enable hot reload with `.css_hot_reload(true)` when using `css_file` sources.
There is no Cargo feature or separate build configuration.

```rust,no_run
use voidui::{Application, WindowOptions, div};
fn main() -> anyhow::Result<()> {
    Application::new()
        .css_file("ui.css")?
        .css_hot_reload(true)
        .window(WindowOptions::default(), div().class("app"))
        .run()
}
```

```sh
cargo run --example css
cargo run --example css -- --watch
```

The runtime switch defaults to false. Disabled applications create no watcher,
worker thread, wake channel, or debounce wait, and do not repeatedly access files.
The watcher dependency is always available in the build; this does not start it
or add work to rendering. CSS files are still read and compiled once during setup.

When enabled, one native watcher subscribes non-recursively to parent directories
of the selected CSS files. Unrelated files and read-access events are filtered.
Directory watching preserves atomic editor saves. A bounded wake channel coalesces
notifications while a set retains every changed source; events for different files
are not dropped. The worker blocks when idle and debounces actual events for
120 ms, then reads only affected sources. Identical bytes are not reprocessed.
Startup performs a re-read after subscribing to avoid missing a change between
initial load and watcher setup.

File IO runs off-thread. Source text arrives through Winit's event-loop proxy;
parsing/cascade stay on the UI thread because Taffy's compact values are not Send.
A successfully compiled, semantically changed sheet replaces the previous one and
invalidates affected application windows. Equal rules do not redraw. Edits confined
to `::placeholder` or `::selection` never reach a node's computed style, so the text
control compares its own presentation and reports the change; those reloads repaint
immediately instead of waiting for a later edit or a focus change. Invalid CSS
or read errors keep the last good sheet and report a diagnostic. Closing the app
drops the watcher, signals the worker and joins it. No background watcher is left
behind. This uses native notifications, not polling or per-frame metadata checks.

Symlink-target replacement in a different directory, deletion/recreation of the
watched directory itself, and unreliable network filesystem notifications are not
guaranteed; re-register/restart watching in those cases.

## Supported CSS values and limits

The current property adapter covers the implemented UI layout and painting model:

- Display block/flow-root/flex/grid/none, box-sizing, dimensions/min/max, margins,
  padding, border widths, gaps, static/relative/absolute/fixed positioning and insets, overflow, aspect ratio,
  Flexbox sizing/direction/wrapping/alignment, Grid tracks and placements.
- Foreground/background/border colors, uniform border-radius, solid/none border
  shorthand, font-family lists, font-size, numeric/normal/bold font-weight,
  normal/italic font-style, line-height, text-align, direction, and text-wrap.
- Lengths in px, %, em, rem, vw, and vh, plus unitless zero. Relative units
  also work in supported grid tracks, shadows, gradients, transforms, and borders.
  A bare line-height number remains a multiplier when inherited.
- `calc()`, `min()`, `max()`, and `clamp()` for layout lengths and constant
  pixel/number properties, as described below.
- z-index, isolation, visibility and pointer-events; see [Stacking and overlays](overlays.md).
- Standard overflow, scrollbar-width/color/gutter and overscroll-behavior; see [Scrolling](scrolling.md).
- CSS Color 4 spaces, gradients, box-shadow, and transitions; see [CSS effects](effects.md).
- inherit/initial/unset where the corresponding typed property supports them.

Unsupported syntax returns a line/column error and does not partially replace the
stylesheet. This strict loading policy differs from a browser's error recovery.
At-rules (including @media, @import, @property and @font-face), nested rules,
keyframe animations, cascade layers/revert, generated content,
per-side border colors/styles, elliptical corner radii, full
white-space processing and inline formatting are not implemented. The adapter does
not fetch URLs or install fonts. Typography and position limitations documented in
[Text widgets](text.md) and [CSS layout](layout.md) still apply. Border widths use
the UI's existing solid-border model.

### Custom properties and relative units

Custom properties cascade by specificity, source order, and `!important`, inherit
by default, and retain case-sensitive names. Use `var(--name, fallback)` in any
supported property, including shorthands and math expressions:

```css
:root {
    --space: 0.5rem;
    --accent: #2878dd;
    font-size: 16px;
}
.card {
    font-size: 1.25em;
    width: min(80vw, 48rem);
    min-height: 20vh;
    padding: var(--space);
    border: 1px solid var(--accent, blue);
    margin-bottom: calc(1em + var(--space));
}
```

`em` uses the element's computed font size. In `font-size` itself it uses the
parent's font size. `rem` uses the root's computed font size; the root's own
`font-size` resolves `rem` against the initial font size. `vw` and `vh` are 1% of
the logical window width and height. These units are converted before the existing
property parser runs, so each property's existing grammar and limits still apply.
Unitless line heights remain multipliers; length-based line heights become pixels
before inheritance.

Variables are substituted after cascade, before property validation. An undefined
or cyclic variable uses its fallback if one is supplied. Without a usable fallback,
the winning declaration behaves as `unset`; an earlier declaration does not win
again. A defined value of the wrong type does not trigger the fallback. For example,
`--size: red; width: var(--size, 20px)` computes to the initial width, not `20px`.
Cycles include references inside fallbacks. Substitution preserves token boundaries:
`var(--number)px` cannot construct a length; use `calc(var(--number) * 1px)`.

Inherited custom properties already have their variable references substituted.
Relative units inside them remain relative to the element where the resulting
value is used. `initial` clears a custom property, while `inherit` and `unset` use
the parent's value. Empty custom values and empty fallbacks are supported. Strings
are not interpolated. Values have bounded nesting (64), input/expansion size
(1 MiB), and component counts (16,384 per list).

Ordinary styles, SVG styles, `::selection`, `::placeholder`, and `::backdrop` share
this value resolution. Backdrops inherit custom properties from their originating
element while retaining independent defaults for ordinary properties.

Native windows publish their logical viewport before sampling styles. For a custom
host, call `WidgetTree::set_viewport_size` before `update_styles`. A full-tree layout
with definite available space supplies the viewport when no explicit viewport has
been set. Explicit viewports remain independent of layout constraints. Intrinsic measurements
and independent subtree layouts retain the last viewport. Before a viewport is
provided, its dimensions are zero. Resizing recomputes contextual values without
rematching ordinary selectors. Clean idle trees do not resolve values again.

### Math functions

Layout length longhands accept `calc()`, `min()`, `max()`, and `clamp()` with
`px`, `%`, nested functions, parentheses, and `+`, `-`, `*`, `/`. This includes
width/height, min/max sizes, flex-basis, padding, margins, borders, gaps, and
positioning insets. Their existing shorthands expand math values into longhands.
Multiplication requires a unitless number on at least one side; division requires
a nonzero unitless divisor. Addition, subtraction, and comparisons require
compatible types. `px` and `%` are compatible in layout lengths; a unitless zero
inside an expression is still a number. Put whitespace on both sides of `+` and `-`.

```css
.editor {
    box-sizing: border-box;
    width: 100%;
    height: 100%;
    /* Keep the scroll host full-width and center a 720px text column. */
    padding: 32px max(48px, (100% - 720px) / 2);
    overflow-y: auto;
}
.panel { width: clamp(240px, 60%, 900px); }
.item { flex-basis: calc((100% - 32px) / 3); }
```

Percentages resolve during layout against the property's containing-block basis,
so resize reflows expressions without reparsing or recascading CSS. Expressions
without percentages are reduced to pixel values before layout, preserving
intrinsic sizing. A percentage expression retains its percentage dependency even
when its coefficient is zero. Taffy's existing percentage/intrinsic sizing limits
still apply; see [CSS layout](layout.md).

Properties that forbid negative sizes clamp the **final** math result to zero;
negative intermediate values are allowed. Margins and positioning insets remain
signed. `clamp(minimum, preferred, maximum)` gives the minimum priority if the
limits conflict. Pixel-only expressions also work in pixel properties such as
border-radius and font-size. Number expressions work in numeric properties such
as flex-grow, flex-shrink, and font-weight, with their existing range restrictions.

This is not full CSS Values Level 4 support. Trigonometric/rounding functions, `none` clamp bounds, unit cancellation,
and non-finite constants are not supported. Percentage
math in typography, grid track definitions, gradients, and media sizing/position
does not use this layout expression path; existing media position support remains
limited to additive `calc()`. Invalid types, zero divisors, numeric overflow, and
excessive expression complexity produce stylesheet errors. Expressions are limited
to 64 levels of nesting and 1,024 terms.

Layout math expressions are owned by stylesheets and specified/computed styles;
replacing a stylesheet releases unused expressions. If a custom layout tree uses
raw Taffy lengths copied from a style, keep that originating style alive and forward
`LayoutPartialTree::resolve_calc_value` to `voidui::core::layout::resolve_calc`.
Raw `LayoutStyle` clones contain non-owning calculation handles. No Taffy or
cssparser source modifications are required.

Semantics follow [CSS mathematical expressions](https://www.w3.org/TR/css-values-4/#math)
within the limits above, using [Taffy's calculation resolver](https://docs.rs/taffy/0.14.0/taffy/trait.LayoutPartialTree.html#method.resolve_calc_value).

## Verify

```sh
cargo test --workspace
cargo bench --bench css -- 1000 1000
cargo run --example css -- --smoke target/css-smoke
```

The candidate-count test uses 1,000 class rules and checks exactly one candidate
per matching node, rather than all 1,000 rules per node. Repeating layout adds zero
selector tests. Timing output is diagnostic, not a cross-platform benchmark.
The native smoke test saves an invalid file, verifies the last good color remains,
then atomically replaces the file and verifies the new color is presented. It
captures initial.png and reloaded.png. Native hot reload has been exercised on
macOS; Windows/Linux builds still require their own runtime verification.

Primary API references: [Servo selectors](https://docs.rs/selectors/0.40.0/selectors/),
[cssparser](https://docs.rs/cssparser/0.37.0/cssparser/),
[notify](https://docs.rs/notify/8.2.0/notify/).

## Selection styling

`user-select` controls selection constraints; `cursor` controls the native pointer.
`::selection` supports foreground/background colors with highlight inheritance.
See [Text selection](selection.md) for supported values, Rust APIs, and scope.

## Editable text

[Input and textarea](input.md) support `::placeholder` typography, `::selection`,
`caret-color` and `caret-animation` using standard CSS syntax. InputGroup is an
ordinary CSS container; its addons use existing flex/grid layout and selectors.
