# Parley text backend

Parley is the production text backend. Fontique discovers/selects fonts, Parley
shapes and lays out paragraphs, and Swash rasterizes glyphs into the existing WGPU
atlas. The application retains its demand-driven window/runtime, CSS cascade,
Taffy box layout and document selection policy.

## Ownership and invalidation

The application shares one `ParleyTextSystem` across windows. It contains the
font collection, reusable Parley layout workspace, and font-face/raster caches.
Font registration is transactional: an invalid batch leaves the collection and
its revision unchanged. After adding fonts at runtime, invalidate the affected
window's layout to update existing text. Registered fonts can supply fallbacks without system
font discovery.

Each text widget owns its source string and a retained `PreparedText`. That value
holds an `Arc<Paragraph>` containing a native Parley `Layout`, not a converted
GPUI glyph vector. A per-window weak pool shares identical content/style/width and reuses shaped
data for other widths without retaining obsolete paragraphs. `finish_frame()` prunes dead entries.

Width changes rebreak existing native layout data. Intrinsic Taffy probes restore
the final width before returning, so a late min/max-content query cannot change
what is painted. Min/max-content sizes and baselines are cached as small numeric records, so
viewport changes do not repeatedly recompute intrinsic layouts. No second glyph
layout is retained for these records. Font/content/line-height changes rebuild the
paragraph. Color,
alignment and selection changes do not invoke the shaper. The renderer keeps at
most one pending raster image between a bounds query and its atlas upload.

`TextSystem::stats()` exposes shaping calls and interned raster font-face counts.
These counters are diagnostic; asking for previously unresolved font metrics can
also shape a short font probe. No selection timer or idle text-layout task is
created.

## Public APIs

The widget APIs remain unchanged:

```rust
use voidui::{div, text};
use voidui::style::{color::Rgba8, selection::UserSelect};

let content = div().font("Arial").color(Rgba8::from_rgb8(30, 40, 50))
    .child("Inherited text")
    .child(text("Select this value").user_select(UserSelect::All));
```

Custom font registration:

```rust,no_run
use std::{borrow::Cow, sync::Arc};
use voidui::render::{ParleyTextSystem, TextSystem};

let backend = ParleyTextSystem::new_without_system_fonts("My UI Font");
backend.add_fonts(vec![Cow::Owned(std::fs::read("MyUIFont.ttf")?)])?;
let system = Arc::new(TextSystem::new(Arc::new(backend)));
# Ok::<(), anyhow::Error>(())
```

For widget-independent text, `core::text::Text::shape` returns `PreparedText`.
Its measure/paint convenience API remains available. The renderer's lower-level
`TextSystem::shape_paragraph(text, runs, font_size, line_height, width, clamp)`
accepts styled UTF-8 runs and returns a `Paragraph`; `Paragraph::paint` writes it
to a `Painter`. `TextLayoutCache::prepare` provides weak reuse for uniform text.
`TextLayoutCache::prepare_runs` provides exact-key weak sharing for rich runs.
`Paragraph::layout()` exposes immutable native glyphs/clusters and horizontal
geometry. For mixed absolute line heights, use `row_bounds`, `caret_bounds`,
`first_baseline` / `last_baseline`, and selection APIs for display coordinates;
the native layout uses isolated vertical bands. See [Rich text foundations](rich-text.md).

For editor flows and embedded objects, use
`shape_inline_paragraph(text, runs, InlineTextStyle { font, font_size, line_height },
width, clamp, objects, indent)`. Unlike the low-level text-only shaper, this entry
point receives the container typography explicitly and establishes a CSS font
strut. `InlineTextBox::align` resolves supported parent-relative alignments using
the style at the source placeholder. The raw text-only API retains its native
text-layout contract without inventing a container font from the first span.

The conformance suite in `crates/voidui_gpui_wgpu/tests/inline_layout.rs` covers
empty/typed/deleted object rows, two bundled fonts, font registration, per-span
half-leading, negative leading, wrapping, selections and cached font metrics.
The browser fixture can be replayed by serving
`crates/voidui_gpui_wgpu/tests` and opening `fixtures/inline-layout.html`.
`fixtures/inline-layout-browser.json` records 48 measured browser cases. Browser
pixel quantization is allowed in that comparison; input stability is checked
separately at a 0.001 logical-pixel tolerance.

The old `CosmicTextSystem`, `PlatformTextSystem` shaping interface, `WindowTextSystem`,
`LineLayout`, `ShapedLine`, `WrappedLine`, `LineWrapper`, force-cell-width and
hash-only line-layout APIs have been removed. `TextLayoutCache` is now its own
paragraph cache rather than an alias. Renderer callers should migrate to the
paragraph APIs; no compatibility adapter reconstructs the old glyph arrays.
Font descriptors, explicit fallback lists, OpenType features, glyph raster IDs,
metrics, colored emoji and run decorations remain supported. Parley's feature
setting values are limited to `u16`; larger values return a layout error.

## Source indices and selection

Rust selection points remain UTF-8 byte offsets into the original widget string.
CRLF is normalized to one line break for Parley, with a sparse source-offset map;
lone CR is treated as a line break too. LF-only strings need no normalized copy.
`Paragraph::layout_index` / `source_index` convert between original and native
indices; `layout_text()` returns the text addressed by native cursors/clusters.

Parley supplies hit testing, visual cursor movement, word boundaries and selection
rectangles. Visual affinity and the preferred column are retained for Shift-arrow
navigation. Document ordering, `user-select`, modal/inert restrictions, and live
node mutation rules remain above paragraph layout. Programmatic ranges can address
character boundaries; highlight geometry expands a partial grapheme to the whole
grapheme. A boundary-only UAX #29 guard also prevents font-fallback clusters from
splitting combining/ZWJ sequences during user gestures; it does not create a
second caret-cell array.

Line-box height uses line advances, while Parley's expanded ink bounds remain
available for glyph/selection geometry. Hanging line-end whitespace is excluded
from alignment offsets. Paragraph base direction follows Parley's Unicode
analysis; CSS `direction` continues to resolve logical box/text alignment.

## Platforms and scope

Complex-script segmentation is enabled, including Parley's ICU dictionary-backed
word/line analysis. Linux loads fontconfig dynamically; system-font lookup needs
the runtime service, while bundled-font operation and cross-compilation do not
require host fontconfig headers. macOS/Windows use Fontique's native font services.

Full browser DOM inline formatting, the full CSS whitespace model, contenteditable,
touch selection handles, continuous drag autoscroll and vertical writing remain
outside this backend. Each string child is still a text box; rich inline builders
now compose spans inside one text box. [Editable controls](input.md)
now reuse the backend for independent editor sessions, IME and caret rendering.
See [Text widgets](text.md) and [Text selection](selection.md) for ordinary text. Video and the renderer's existing limitations are unchanged.

## Verification and memory

See the [production migration measurements](../experiments/text-memory/production-results.md)
for native-window memory, the retained-document workload, idle verification and
the remaining large-document reflow performance difference.

## Named font resources and positioned glyphs

`TextSystem::add_fonts_once(key, load)` shares lazily loaded application font
resources across all handles to a backend. Use a unique, namespaced key for an
immutable resource and select its faces by family name. This API preserves the
generic-family defaults, so loading icon or math fonts cannot change the default
UI face. The loader executes once after a successful registration, must not
re-enter the font service, and may return borrowed or owned bytes. Failed loads
can retry. A new resource increments the font revision; a cache hit does not.

`Paragraph::paint_glyphs(painter, origin, color)` places the first line's baseline
at `origin`. It uses the same shaping, fallback fonts, raster cache and atlas as
ordinary paragraph painting. It paints glyph foregrounds only; paragraph
selection, backgrounds, and decorations are omitted. Only the caller's clip
applies, allowing large mathematical operators and italic overhangs to extend
outside the line box. Shape and paint with the same text service.

Use `TextSystem::add_font_face_once(key, descriptor, load)` for a bundled face
whose declared family, weight, or style must override font-file metadata, as
with CSS `@font-face`. The descriptor is a `Font`; features and fallback lists
remain shaping inputs, not registration metadata. This API shares the same
immutable resource cache, revision handling, and generic-family preservation
as `add_fonts_once`. The font data itself is not edited. Use distinct keys for
distinct immutable face definitions.
