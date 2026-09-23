# Performance and resource ownership

VoidUI retains layout, source-backed editor geometry, and completed GPU frames.
Voidnote uses the same services for its live Markdown preview, attachments, tables,
and virtual file tree. The investigation and original measurements are in
[the September audit](performance-audit-2026-09-22.md).

## Bulk styles

Use `StyleSpanBuilder` when a parser emits many overlapping inline spans:

```rust
use voidui::{InlineStyle, StyleSpan, StyleSpanBuilder};
let mut styles = StyleSpanBuilder::new();
styles.push(StyleSpan::new(0..12, InlineStyle::new().bold()));
styles.push(StyleSpan::new(4..8, InlineStyle::new().italic()));
let canonical = styles.finish();
```

Later entries override only properties they explicitly set. Metadata merges by
key. The builder sweeps interval endpoints and updates an insertion-ordered
composition tree; it does not merge the accumulated array for each insertion.
Validate UTF-8 source boundaries when publishing the result. `Projection::style`
remains available for small, immediate updates.

Extensions can implement `selection_affects_projection`. Return false only when
old and new selections produce the same presentation. Other document, highlight,
font, composition, and explicit projection revisions still invalidate layout.
The default returns true. Single-hard-line revelation updates reuse the height
index; multiline folds use the full transactional indexing path. Compatibility
snapshots without a document identity never use revision-only shortcuts.

Voidnote also maps ordinary word edits inside existing parser text events. Edits
near syntax, links, references, line boundaries, or math retain a full-parse
fallback. This deliberately does not claim a fully incremental Markdown parser.
Full parsing now builds styles in a batch rather than repeatedly normalizing them.

## Source-backed cells

`BlockArrangement::cell_overscan` opts into cell virtualization. Return complete
cell geometry and the full block extent. The editor retains geometry for source
navigation, but materializes glyph flows near the viewport and requested caret.
Focused/captured views and IME retain their ownership; Tab uses complete source
geometry even for an unmaterialized destination. Width changes and registry changes
invalidate arrangements. Cell metrics and glyph layouts have separate caches.

`ViewportOptions::cell_shapes` and `cell_metrics` are `CacheBudget` values. Their
defaults are 4 MiB / 128 shapes and 16 MiB / 32,768 metric entries. Shape byte costs
are conservative estimates, not allocator or RSS measurements. `LayoutStats`
reports cached cells and these retained-byte estimates.

Voidnote tables keep exact global auto-fit column sizing. The initial pass still
measures every cell once, and edits still scan lightweight structure/metrics to
compute exact column maxima. Unchanged cells reuse their metrics and a bounded
working set of width-independent shaping. Only visible cell flows remain mounted;
painting skips offscreen rows before drawing decorations. Large table metadata is
still proportional to row count. The 128-paragraph limit is not presented as a
hard bound on table geometry or active viewport content.

## File-backed images

`ImageAsset::open` reads dimensions and file revision, without decoding bitmap
pixels. `asset_img(asset)` reserves geometry and requests a physical-size variant
on the shared compute executor when painted. It retains the displayed variant
while a resize/DPI replacement is prepared. Object-fit cover requests enough pixels
for the unclipped image rectangle. `img(Image)` keeps its existing eager contract.

Bitmap variants preserve aspect ratio and do not upscale stored pixels. The worker
currently uses full decode followed by thumbnail resampling, so large source
images still have a bounded transient decode cost. Serial decode admission plus
`MediaLimits` limits concurrent source-buffer peaks. Decoder-specific subsampling
is not assumed. SVG metadata can still require parsing its small document tree.

`ImageAsset::set_cache_budget` controls the shared recent-variant cache. The default
is 16 MiB / 1,024 entries. Active views hold strong references; live variants are
weakly interned and remain usable after eviction. File length and modification time
are part of asset identity. Errors stay with the view until its source or requested
size changes. The asset description can be reopened to discover a modified file.
`cache_stats` reports retained estimates, hits, misses, and evictions.

Hosted `WidgetView` trees inherit their owner's task runtime. Worker completion
therefore wakes the actual native host instead of waiting in an unpumped private
executor. Removing a view cancels its local wait; a compute closure already running
may finish and publish into the bounded resource cache.

## Frame and glyph retention

`Scene::finish` assigns a publication revision. Treat a finished scene as immutable;
after changing public primitive arrays, finish it again. The renderer can retain
completed pixels rather than repeat uploads and path passes on exposure.

Caret ink remains in its original stacking and clipping position. A blink changes
presentation visibility without rebuilding the scene. Up to two completed frame
variants are populated lazily. This avoids drawing the caret above a popover or
assuming swapchain pixels survive presentation. Each variant costs one physical
BGRA texture, and one full-screen copy pass is still required per presentation.

Configure `WindowOptions::render_cache` (`RenderCacheOptions`) to set `frame_bytes`
and `glyph_bytes`. Defaults are 32 MiB and 16 MiB per window. If completed-frame
retention would exceed its budget, the renderer uses ordinary scene rendering.
Zero disables that cache. Large/high-DPI windows can therefore trade repeated GPU
work for lower retained memory. Resize, resource recovery, transparency changes,
and scene revision changes invalidate the relevant backing state.

Native path targets cover the clipped union of the scene's paths, with explicit
screen origin in shader uniforms. Text-only scenes release those targets. The
formula canvas retains local tessellated geometry; scrolling translates geometry
instead of running curve tessellation again. Small SVG icons continue to use the
raster image route and do not imply a native path target.

Glyph tiles are pinned by retained scenes. Budget eviction removes only unpinned
tiles at frame boundaries; WGPU retains resources referenced by already-submitted
commands. Atlas payload can exceed the budget for an active working set, and page
fragmentation can make texture allocation larger than tile payload. Low-level
atlas clients that construct sprites directly should retain `PlatformAtlas::pin`
leases; `Painter` does this automatically. CPU raster bounds use a separate bounded
LRU and can be safely recomputed after eviction.

`FrameStats::renderer` includes scene submissions, retained presentations, instance
upload bytes, path passes, frame/path texture payloads, glyph payload, and atlas
page payload. These are not GPU timestamps or RSS. `last_present_time` remains a
CPU duration. Explicit snapshots intentionally render/read back the scene and must
be excluded from ordinary presentation-performance measurements.

## Virtual lists and trees

`virtual_list(count, options, render_row)` mounts the visible fixed-height rows,
overscan, and explicitly pinned rows. The callback returns a stably keyed element.
Row height includes margins. Keep row state outside recycled rows, or pin active
editors/captured rows with `VirtualListOptions::pinned`. Large spacer blocks retain
the total scroll extent. Viewport notifications defer state publication until the
layout traversal ends.

Voidnote flattens expanded directory snapshots and uses this primitive. It retains
metadata for expanded folders, reloads explicit directory revisions, and drops
unreachable collapsed snapshots. Renaming rows are pinned; file selection lives
outside row widgets. File I/O runs through the bounded blocking worker service.

## Note persistence

Note opening reads directly when its path-keyed session mounts, without prefetch,
a global source cache, or a separate Loading phase. Autosave retains
one cancellable debounce timer per session and writes immutable text snapshots on
a worker. Per-path sequencing prevents an older delayed write from overwriting a
newer completed write. Temporary files are created exclusively, synced, and renamed
in the same directory; cleanup removes only a file created by that save attempt.
Pending saves are window-scoped and survive switching notes. Window/process exit
still requires an application-level flush policy if guaranteed shutdown persistence
is desired; the runtime does not block shutdown indefinitely.

## Verification

```sh
cargo check --workspace --all-targets --all-features
cargo check --workspace --no-default-features
cargo test --workspace --all-features
cargo test --manifest-path ../voidnote/Cargo.toml --all-targets
```

Regression coverage includes batch/sequential style equivalence, nested metadata,
virtual row state, offscreen cell navigation, image cache budgets and worker
publication, formula identity beyond cache capacity, glyph pinning, projection
fallbacks, scroll anchors, and stale-save ordering. Reproduce synthetic timing with
the isolated probe described in `target/perf-audit/README.md`; original and derived
results are stored separately under `target/perf-implementation/`.
