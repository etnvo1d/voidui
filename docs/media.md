# Images and inline SVG

Use `img` for an immutable image resource and `svg` for a vector document whose
nodes participate in the UI's CSS cascade. No custom CSS properties are required.

## Images

```rust,no_run
use voidui::{img, media::Image};
use voidui::style::media::ObjectFit;

// Load once before mounting; clone the handle for additional instances.
let logo = Image::from_file("assets/logo.svg")?;
let view = img(logo)
    .alt("Company logo")
    .width(160)
    .height(80)
    .object_fit(ObjectFit::Contain);
# Ok::<(), anyhow::Error>(())
```

`Image::from_bytes` detects PNG, JPEG, GIF, WebP and UTF-8 SVG. Bitmap decoding
happens during construction, so malformed input returns an error before mounting.
Animated formats currently display their first frame. `Image::from_rgba` accepts
tightly packed, straight-alpha RGBA8. All clones share one pixel allocation.

`img` takes an `Image` handle, not a path that is reopened while drawing. For a
large file, load it inside an existing scoped task and publish the resulting
handle through state. Image handles are `Send + Sync`. Network requests, URL
resolution, file watching and loading placeholders remain application policy.
`alt` is stored as the standard attribute; this does not add a native accessibility
bridge or a broken-image fallback renderer.

```css
img {
    width: 160px;
    height: 80px;
    object-fit: contain;
    object-position: right 8px bottom 4px;
    image-rendering: auto;
    border-radius: 8px;
}
```

The content box clips the image, including `cover` and displaced `object-position`.
Borders and padding remain outside the image's content. An automatic dimension is
derived from the intrinsic aspect ratio; min/max constraints are transferred
between automatic dimensions before ordinary box layout.

## Inline SVG

```rust
use voidui::{svg, svg_from_str};

let check = svg()
    .view_box(0, 0, 24, 24)
    .width(24)
    .height(24)
    .class("check")
    .fill("none")
    .stroke("currentColor")
    .stroke_width(2)
    .child(svg::path().class("mark").d("M5 12 L10 17 L19 7"));

let imported = svg_from_str(
    r#"<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="8"/></svg>"#,
)?;
# Ok::<(), anyhow::Error>(())
```

`SvgDocument::parse`, `from_file`, and `from_node` create reusable documents;
`svg().document(document.clone())` imports one into a widget. Documents retain
compiled UI CSS values and are shared on the UI thread; read source bytes in a
background task if needed. `SvgNode` graph descriptions can cross threads. `to_xml` exports the
source graph with escaped text and attributes. Builder attributes retain SVG's
case-sensitive spelling. `SvgNode::new` and `.attr` allow standard SVG elements
and attributes beyond the named conveniences.

Available node constructors include `path`, `rect`, `circle`, `ellipse`, `line`,
`polyline`, `polygon`, `g`, `defs`, `linear_gradient`, `radial_gradient`, `stop`,
`clip_path`, `mask`, `filter`, `text`, `tspan`, `image`, `symbol`, `use_node`, `title`
and `desc`. Only the outer viewport allocates a widget, layout cache and event
state. SVG coordinates and group transforms do not pass through Flexbox or Grid.

```css
.toolbar { color: #2874d9; }
.toolbar > svg.check { width: 24px; height: 24px; }
.toolbar svg .mark {
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
}
.toolbar:hover svg .mark { stroke: #d04070; }
svg circle { cx: 12px; cy: 12px; r: 8px; fill: currentColor; }
svg path.dynamic { d: path("M5 12 L10 17 L19 7"); }
```

The shared selector engine sees SVG descendants, including combinators,
attribute selectors, `:nth-child`, `:is`, `:where` and `:has`. Presentation
attributes have lower precedence than stylesheet declarations; inline styles,
specificity, source order and `!important` participate in the cascade. Imported
`<style>` sheets are scoped to that SVG document and follow application sheets.
They can match the SVG's UI ancestors. They do not inject rules into unrelated UI
widgets. Root `:hover`, `:active` and focus state work in descendant selectors.
Individual SVG shapes do not yet have pointer hit testing or event handlers.

`currentColor` inherits through the outer UI and SVG groups. Explicit path colors
remain independent. SVG loaded through `img` has a separate style context: outer
`color`, `fill` and descendant selectors cannot recolor its contents.

`viewBox` and `preserveAspectRatio` are SVG **attributes**, not CSS properties.
Use `.view_box(...)` and `.preserve_aspect_ratio("xMidYMid meet")`. `none`, `meet`
and `slice` are supported. Width and height are the displayed viewport size;
viewBox defines the internal coordinate system. An SVG image also retains its
preserveAspectRatio behavior when `object-fit: fill` changes the image viewport.

## Implemented CSS

| Area | Properties / values |
| --- | --- |
| Image fitting | `object-fit: fill / contain / cover / none / scale-down` |
| Image position | `object-position`: one to four coordinates, edge keywords, px, %, additive `calc()` with px/% |
| Image sampling | `image-rendering: auto / smooth / high-quality / crisp-edges / pixelated` |
| Paint | `fill`, `stroke`, `fill-opacity`, `stroke-opacity`, `fill-rule`, `clip-rule` |
| Stroke | `stroke-width`, `stroke-linecap`, `stroke-linejoin`, `stroke-miterlimit`, `stroke-dasharray`, `stroke-dashoffset` |
| Effects | `clip-path` and `mask` with `url(...)`/`none`, `filter`, `opacity`, `paint-order`, `shape-rendering` |
| Paint servers | `stop-color`, `stop-opacity`, `flood-color`, `flood-opacity` |
| SVG geometry | `x`, `y`, `cx`, `cy`, `r`, `rx`, `ry`, `d: path(...) / none`; width/height use the existing size properties |
| Existing styles | Color, supported typography, visibility, display; ordinary size, spacing and border CSS on the outer widget |

New longhands accept `inherit`, `initial` and `unset`. Stroke lengths support SVG
units and percentages; geometry CSS is lowered to attributes after the cascade.
Opacity accepts numbers and percentages and is clamped to [0, 1]. SVG group
opacity composites the group once, so overlapping children do not darken twice.

This is a documented static SVG/CSS subset, not browser-wide SVG 2 conformance.
New opacity/filter/clip/mask paint behavior applies to SVG content (opacity also
to `img`); it does not implement opacity or filter compositing for ordinary `div`
subtrees. Unsupported CSS is rejected by `Stylesheet::parse`, rather than mapped
to invented syntax. Advanced CSS masking/basic-shape clip functions, CSS
transforms, `vector-effect`, shape events, SVG scripting/SMIL, `foreignObject`,
image orientation/resolution CSS and media transitions are not implemented.
Attribute `transform` is supported by the SVG renderer. Existing `color`
transitions can drive `currentColor` but require a rasterization for each changed
SVG appearance; use static assets for large frequently changing graphics.

## Fonts and resource limits

SVG text uses the explicit shared `SvgOptions::fontdb`. Its default database is
empty: icon-only applications do not scan system fonts or create a second font
catalog. Supply the same bundled font bytes you use for ordinary text:

```rust,no_run
use std::sync::Arc;
use voidui::{svg, media::{fontdb, SvgOptions}};
let mut fonts = fontdb::Database::new();
fonts.load_font_data(std::fs::read("assets/Example-Regular.ttf")?);
let view = svg().options(SvgOptions {
    fontdb: Arc::new(fonts),
    ..Default::default()
});
# Ok::<(), anyhow::Error>(())
```

`MediaLimits` defaults to 16 MiB of input, 16 million output pixels, an 8192-pixel
maximum dimension, 100,000 XML nodes and 256 nesting levels. Custom options can
lower or raise these limits. GPU texture limits still apply. External SVG image
references cannot open files or make network requests; embedded raster data URLs
are allowed and checked. Nested SVG data-image resources are disabled.

## Responsive SVG resizing

Both inline `svg` and SVG-backed `img` accept an optional rendering policy:

```rust,no_run
use voidui::{svg, media::SvgRenderPolicy};
let background = svg()
    .render_policy(SvgRenderPolicy::ScaleWhileRendering)
    .view_box(0, 0, 100, 50)
    .child(svg::rect().width(100).height(50).fill("blue"));
```

The default, `SvgRenderPolicy::Exact`, synchronously renders the current appearance
and physical size on an atlas miss. Keep it for graphics that must match every
animation frame exactly.

`ScaleWhileRendering` displays the last completed texture at the new bounds while
the shared CPU worker pool parses and rasterizes the newest request. Inline CSS
resolution and source serialization still happen on the UI thread. The first
frame has no SVG content until the worker finishes. During resizing, temporary
scaling can soften the image or change its aspect ratio; the completed result
restores the exact viewport, `preserveAspectRatio`, and DPI semantics. CSS changes
also display the previous appearance until their replacement is ready. Bitmap
images ignore this policy.

Each view keeps at most one compute operation in flight and one replaceable latest
request. Queued obsolete requests are skipped; an already running resvg call cannot
be interrupted, but its obsolete result is discarded before publication. Completion
invalidates the mounted widget and wakes rendering without polling every frame.
Unmounting or replacing the resource cancels its scoped task. Worker admission and
rendering failures are returned by the next draw; an unchanged failed request does
not continuously retry. Headless hosts must drive `tree.task_runtime().tick()` when
its waker fires, just as for other framework tasks.

Background mode retains the displayed BGRA pixel buffer for atlas/device recovery,
so an upload never falls back to synchronous SVG rendering. The old displayed
buffer and a worker's replacement can coexist during a refresh. Per-view work also
obeys the application's `TaskOptions` compute concurrency and queue limits.

## Caching and memory

- Image clones share decoded premultiplied BGRA pixels; each atlas uploads a
  bitmap once. Retained CPU pixels permit GPU recovery without reopening files.
- SVG documents share compact immutable nodes. Equivalent resolved SVGs share
  parsed rendering trees and live raster identities across instances.
- Viewport/style changes invalidate SVG appearance; DPI changes choose a new
  physical raster size. An unchanged scene does not parse or rasterize again.
- In exact mode, SVG pixel buffers are temporary and released after upload. Background
  mode retains only the displayed buffer and in-flight work. Size-variant caches
  hold weak references, so repeatedly resizing does not retain every old raster.
- Widgets and retained/replayed scenes own image texture lifetimes. Before a GPU
  frame, the atlas collects unreferenced images and frees empty texture pages.
  This scan visits managed images only, not every glyph in the font atlas.
- Sparse image/SVG style storage allocates nothing for ordinary unstyled widgets.
  Internal SVG style resolution uses a depth-bounded ancestor stack, not a full
  UI computed-style record retained for every path.

`voidui::media::stats()` reports actual bitmap decodes, SVG rendering-tree parses
and SVG rasterizations. These counters exclude cache hits and SVG source-graph
construction. Do not load or reconstruct large sources inside every component
render; keep reusable handles/documents in state or application resources.

`svg_from_str` parses its source graph on every call, before widget reconciliation
can compare the result. For repeated fixed icons, parse a `SvgDocument` once and
build views with `svg().document(document.clone())`. The document is shared; each
view keeps its own CSS appearance and viewport. Unrelated inline style edits no
longer expire that view's resolved SVG source cache.

## Verification

```sh
cargo test --test media
cargo test --workspace
cargo run --example media
cargo run --example media -- --snapshot target/media-example.png
cargo run --release --example media_bench -- 1000 200
```

The snapshot command checks GPU color, premultiplied alpha, pixelated sampling and
cache reuse. The headless benchmark asserts that repeated icons share one atlas
entry and that cached frames add no parsing/rasterization work. These are static
icon measurements, not throughput guarantees for animated complex SVGs.

Semantics follow [CSS Images 3](https://www.w3.org/TR/css-images-3/) and
[SVG coordinate systems](https://www.w3.org/TR/SVG/coords.html). Static rendering
is provided by [resvg](https://github.com/linebender/resvg).
