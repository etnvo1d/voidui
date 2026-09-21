# voidui_gpui_wgpu — standalone voidui edition

This crate includes GPUI's scene primitives, GPU renderer, shaders, texture atlas, text shaping, line wrapping, caches, and glyph rasterization.

You can use it without `gpui`, `gpui_util`, or the Zed workspace. Your application supplies the window and event loop. This derivative is maintained in the voidui project; its source revision and compatibility differences are documented in [UPSTREAM.md](UPSTREAM.md).

## Installation

```toml
[dependencies]
voidui_gpui_wgpu = { path = "/absolute/path/to/voidui_gpui_wgpu" }
winit = "0.30.13"
```

You can copy this directory into another project. Its manifest does not inherit settings from the parent workspace. The crate uses Rust edition 2024 and pins WGPU to version 29.0.4.

The first build normally downloads dependencies. The source archive does not include vendored dependencies. Building this crate does not require cloning Zed or a separate Xcode step to compile Metal shaders.

Run the example or tests from the voidui workspace or from a standalone copy of this directory:

```sh
cargo run -p voidui_gpui_wgpu --example standalone
cargo test -p voidui_gpui_wgpu
```

## Drawing a frame

1. Create a window and event loop, for example with `winit`.
2. Create a renderer with `WgpuRenderer::new(..., &Arc<Window>, ...)`. The surface retains a clone of the window handle source to keep it alive.
3. Create a `ParleyTextSystem`. Load system fonts or use `add_fonts` to supply font bytes, then wrap the backend in an `Arc<TextSystem>`.
4. Create a `TextLayoutCache` for each drawing target. Use `prepare` for cached uniform paragraphs or `TextSystem::shape_paragraph` for styled runs. Both produce native Parley paragraphs without a window.
5. Create a `Scene` and use `Painter` to draw rectangles, paths, and text in logical pixels. The painter handles DPI scaling, rectangular clipping, and glyph atlas entries.
6. Call `scene.finish()`, then `renderer.draw(&scene)`. Call `layout_cache.finish_frame()` after each frame to rotate the text caches.

See [examples/standalone.rs](examples/standalone.rs) for a complete application.

## API reference

| API | Purpose |
| --- | --- |
| `Scene`, `Quad`, `Shadow`, `PathBuilder`, sprite types | Construct primitives and prepare drawing batches. |
| `GradientPaint`, `GradientGeometry`, `GradientStop` | Variable-length color ramps for linear, radial and conic gradients. |
| `Painter`, `fill`, `PaintQuad` | Draw in logical pixels, including backgrounds, borders, and rounded corners. |
| `Paragraph::paint` | Write shaped text and decorations into a painter. |
| `TextSystem`, `TextLayoutCache` | Obtain font metrics, shape and wrap text, and reuse cached layouts. |
| `ParleyTextSystem` | Select fonts with Fontique, lay out with Parley and rasterize with Swash, including fallbacks and OpenType features. |
| `WgpuAtlas` | Allocate texture regions, upload pixels, cache entries, and release them. |
| `WgpuRenderer` | Present frames, resize the drawing surface, recover the device, and read back scene pixels on native targets. |

Low-level scene primitives use `ScaledPixels`, which already include the DPI scale factor. `Painter` accepts logical `Pixels`. Apply DPI scaling only once.

Atlas uploads use R8 for monochrome data and BGRA8 for color data. Convert RGBA8 input before uploading it as color data. The native `render_to_rgba` method returns tightly packed RGBA8 pixels at the current viewport size.

GPU primitive field order and padding must match their WGSL definitions. If you change these types, update the matching shaders and run the layout tests.

## Resource lifetime

Skip drawing when either window dimension is zero, such as while minimized. After a resize, pass the new size in physical pixels to `update_drawable_size` and rebuild the scene.

Use each scene's atlas tiles with the renderer that owns that atlas. Use font IDs with the text system that created them. Device recovery or atlas clearing invalidates cached tiles: rebuild the scene before drawing again.

## Fonts and supported features

Supply your own fonts or load system fonts. The bundled IBM Plex Sans font is used by the example and tests. It does not cover Chinese text or all emoji; load fonts that cover the characters your application needs.

Bold requests use the closest available face. Existing semibold/bold faces and variable-weight instances are not artificially thickened. Synthetic bold is reserved for bold requests that resolve to a non-bold face; a missing medium weight does not trigger it. This policy is shared across macOS, Windows, and Linux.

`Painter` uses grayscale text antialiasing by default, which supports transparent windows. Low-level subpixel sprite types and shaders are included. To use subpixel rendering directly, account for dual-source blending support, RGB/BGR display layout, and an opaque background.

Decode images and SVGs in your application. `Painter::paint_image` uploads
premultiplied BGRA8 through a managed `ImageTexture`, clips to a rounded content
box and supports smooth, crisp-edge and pixelated sampling. Retained scenes and
scene replay preserve texture ownership; the atlas frees unused managed image
allocations before a frame. Ordinary low-level image/glyph uploads keep their
existing caller-managed lifetime and straight-alpha format. This standalone crate
does not include decoding; voidui's [img and svg components](../../docs/media.md)
supply it. The upstream WGPU `PaintSurface` video branch is unimplemented, so video surface import is unavailable.

Your application supplies event dispatch, input methods, widgets, and UI layout. GPUI's application runtime and native platform text implementations are outside this package.

## Platform status

Metal rendering and pixel readback have been tested on macOS with an Apple M3. Linux, Windows, and Web paths have not been tested on their target platforms.

The renderer exposes `set_presents_with_transaction` on macOS and a pre-present
callback. voidui's desktop runtime uses these for synchronous AppKit resize and
Wayland frame notifications. The standalone example does not opt into this native
resize integration. See the root project's `docs/window.md` for actual window tests;
static screenshots alone do not establish frame-by-frame live-resize smoothness.

Font appearance, performance, and feature coverage may differ from GPUI's native Metal/CoreText backend.

## Verification

```sh
cargo test -p voidui_gpui_wgpu
cargo check -p voidui_gpui_wgpu --all-targets
cargo run -p voidui_gpui_wgpu --example standalone -- --snapshot /tmp/voidui-render.png
```

Tests cover scene bounds, shaders, GPU data layouts, atlas release, combining characters, bidirectional paragraphs, and line splitting. Additional standalone tests cover text shaping through scene insertion, DPI clipping and restoration, wrapping caches, and string cache key consistency.

GPU atlas tests require an available graphics device. The screenshot example requires a desktop window environment.

## Licenses

GPUI-derived source is licensed under Apache-2.0; see [LICENSE-APACHE](LICENSE-APACHE).

The gamma correction table retains Microsoft's MIT license; see [LICENSE-MIT-MICROSOFT](LICENSE-MIT-MICROSOFT).

The IBM Plex Sans font used by the example and tests is licensed under the SIL Open Font License; see [tests/fonts/LICENSE.txt](tests/fonts/LICENSE.txt).

## CSS effect integration

`Painter::paint_gradient` records one quad and a variable-size color ramp.
`Painter::paint_shadow` accepts logical-pixel geometry and Gaussian sigma;
`Paragraph::paint` accepts an inherited foreground override without reshaping.
CSS parsing, gamut mapping and timing remain in the root voidui crate, so this
standalone renderer has no stylesheet-parser dependency. Video pipeline/layout
code is retained; the existing Surface draw branch remains unimplemented.

## Rich text and editor geometry

`TextRun::font_size` and `TextRun::line_height` accept optional positive, finite
logical-pixel values. `None` inherits the corresponding paragraph value.
Line height is absolute and does not scale automatically with a run's font size.
Run lengths are UTF-8 byte counts in the original source; CRLF normalization
preserves the styles of surviving bytes, including when CR and LF have separate
runs. An empty run cannot style adjacent text. `TextRun` implements `PartialEq`,
not `Eq`, because its optional dimensions are floating-point values.

Set `TextRun::color_is_explicit` when a run owns its foreground color. The default
is false, preserving existing `paint(..., Some(color), ...)` overrides for plain
text. Explicit foregrounds survive that override. Selection foregrounds override
both inherited and explicit glyph colors. Decorations without their own color
use the resolved run foreground; run backgrounds keep their own colors.

`Paragraph::caret_bounds(index, affinity, width, align)` returns a one-pixel-wide
caret with display vertical geometry in paragraph-local logical pixels.
`caret_position` returns that rectangle's origin. `row_bounds(row, width, align)`
exposes authoritative display row bounds, including trailing whitespace.
`cursor_row(cursor)` accepts a Parley cursor using normalized layout byte offsets
and follows native visual affinity, including the empty row after a final newline.
It can return a clamped-away row; check against `line_count()` before navigating.
`caret_bounds` returns `None` for clamped-away rows or invalid source offsets.
Paragraph height sums visible target line advances; tight leading can make
caret and selection bounds extend beyond those advances.

### Paint-only updates

Paragraph foreground overrides and selection colors do not reshape text.
Changing run colors, backgrounds, decorations or their byte-range boundaries
currently requires a new `shape_paragraph` call. Parley 0.11.1 exposes only
`Layout::styles() -> &[Style<B>]`, with no public mutable style/brush API. A
separate paint overlay would require its own range and partial-ligature handling;
this adapter does not mutate dependency internals or retain a second style table.
Width-only `reflow` continues to reuse shaping.

### Mixed-height adapter and native layout contract

Parley 0.11.1 has a line-height shaping defect. Both public builders reproduce
it; switching builders, OpenType features, font synthesis or zero-size inline
boxes does not fix it. This crate uses a local row-metrics adapter and does not
modify dependencies, vendor Parley, or access private native data.

For mixed effective heights, native styles use one common absolute height.
The adapter retains coalesced source height spans and a compact row table with
only top, advance and baseline. Each row takes the maximum requested height over
its original style intersections, including whitespace and separators. A trailing
empty row inherits the final surviving source byte's height. Baselines use the
native ascent/descent with centered leading; negative leading preserves ink
and caret overflow. For example, a row at top 72 with height 20 and font extent
20.8 has a caret top of 71.6, while its line advance remains 20.

`layout()` still supplies native glyphs, clusters, source ranges and horizontal
geometry. **Its y coordinates and heights are not display coordinates for mixed
heights.** Use `row_bounds`, `caret_bounds`, `height`, `first_baseline`,
`last_baseline`, `selection_rectangles` and Paragraph hit/selection APIs.
Paint translates glyph and decoration baselines, and uses display bounds for
backgrounds, selection and culling. Display hit testing chooses rows by line
advance, then maps to the native row band. If unusually tall fonts overlap even
the common native height, the adapter uses public `BreakLines` positioning to
separate native bands; it only repeats line breaking, never shaping.

Uniform effective heights allocate no adapter. Mixed paragraphs reuse row-vector
capacity on resize. Reflow at an unchanged width returns immediately with no
row traversal or allocation. Empty source still creates no row; editor-owned
empty hard paragraphs and their separator style remain the editor's policy.

For exact source evidence, reproduction commands and the public-API comparison,
see [Parley line-height diagnostics](docs/parley-line-height.md).

## Parley migration

The former GPUI line-layout/wrapper APIs were removed. See [the migration guide](../../docs/parley.md) for the replacement paragraph APIs, cache ownership and source-offset rules.
