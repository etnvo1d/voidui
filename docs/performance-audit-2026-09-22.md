# VoidUI performance in Voidnote

## Scope and measurement

The highest-priority problems are repeated style normalization during Markdown
analysis, permanently retained image pixels, and table-wide shaping. The frame
scheduler already sleeps for an unchanged, unfocused editor. Replacing the event
loop or text backend would not address the measured primary CPU bottleneck.

Measured on September 22, 2026: Apple M3, 24 GiB memory, macOS 26.5,
Rust 1.98.1, optimized release profile. VoidUI revision:
`2a745384adbf919aa669ca8bb4fff1661f36ba6a`. Voidnote was read from its current source;
there was no running Voidnote process at the start. The replay compiles its actual
Markdown extension, table implementation, math renderer, and editor CSS.

All fixtures are synthetic. No personal notes were opened or modified. No
production implementation or external dependency was patched. Diagnostics are in
`/Users/peihongyu/Projects/voidui/target/perf-audit/`; their README records commands,
limitations, dependency versions, and the historical fixture rename. Source hashes
are retained alongside raw results.

CPU timings below include the counting allocator, editor input/selection, and
scroll-content preparation, but exclude GPU submission and autosave. Requested
Rust heap bytes are not RSS, physical footprint, or GPU allocations. Native windows
were 1000 x 700 physical pixels at the observed 1x scale; headless scene tests used
800 x 600 logical pixels at 2x. These are different measurement conditions.

## 1. Markdown style construction becomes quadratic

| Synthetic document | Initial layout | Median input | Median selection |
| --- | ---: | ---: | ---: |
| 100 formatted units, 6,190 bytes | 25.91 ms | 18.07 ms | 0.335 ms |
| 1,000 formatted units, 62,890 bytes | 1,789.86 ms | 1,707.18 ms | 4.014 ms |
| 3,000 formatted units, 190,890 bytes | 23,047.75 ms | Not completed | 16.154 ms |
| Same 3,000-unit source, preview disabled | 9.27 ms | 0.204 ms | 0.001 ms |

Each formatted unit contains a heading and bold, italic, and inline-code spans.
A second fresh-process 1,000-unit run reproduced a 1,778.53 ms median input.
The 3,000-unit input run was stopped after collecting its stack sample; it must
not be reported as a completed input benchmark.

The proven path is:

```text
Text input -> document revision -> LivePreview::outline -> Outline::parse
  -> Builder::style, repeatedly
     -> Projection::style
        -> highlights::overlay(all existing spans, one new span)
```

`overlay` walks and copies the existing canonical span sequence every time a new
span is added. With S emitted spans, the accumulated work approaches O(S²).
The two-second sample of the 3,000-unit input contained 1,620 main-thread samples
under `Outline::parse`, with the hot branches in style overlay, vector growth,
copying, and destruction. In the 1,000-unit input loop only one paragraph per edit
was newly shaped. This separates the style-building bottleneck from text shaping.

**Required design change:** provide a bulk style builder with a single finalize
step. Collect raw spans with explicit insertion precedence, then normalize their
endpoints together. Overlapping styles must retain field-wise overlay semantics:
a later color must not erase an earlier font weight. A sweep with ordered active
values per property can approach O(S log S + output). Avoid just sorting and
appending spans, which would break nested formatting. Retain the convenience
single-span API for small updates; make batch behavior explicit for parsers.

After removing quadratic construction, use document changes to update stable
Markdown blocks and their projections. References, fence boundaries, lists, and
other cross-block dependencies need an explicit invalidation region or a correct
full-parse fallback. Background analysis needs immutable snapshots and revision
checks before publication; moving the current quadratic work to a worker alone
would preserve the CPU/memory problem.

## 2. Selection still rebuilds document-wide metadata

Selection-only tests produced zero new shaped paragraphs for the formatted
fixtures, but cost rose from 0.335 ms to 16.154 ms with document size. Markdown's
outline cache avoids parsing at an unchanged document revision. Nevertheless,
`project` clones styles/paragraphs and visits markers; `TextEdit::prepare` includes
the state generation in its key when extensions exist. `prepare_snapshot` then
constructs another engine and either clones or reindexes blocks, resets the height
index, and reuses prior measurements. The incremental indexing fast path explicitly
rejects projections containing replacements or custom blocks.

**Required design change:** separate document-derived analysis, selection-dependent
syntax revelation, and viewport preparation. Represent a selection change as a
small projection delta around the old/new revealed regions. Give blocks stable
identity independent of absolute byte offsets; update the height index locally.
Preserve the existing source-anchor logic and its pointer/scroll regression tests.

This is an API/data-model issue shared by the extension host and editor layout,
not a reason to disable formatting while the user moves the caret.

## 3. Images are eagerly decoded and retained after editor disposal

The generated fixtures are solid-color PNGs with small compressed files but full
2048 x 2048 RGBA payloads. Each requires 16 MiB of decoded pixel storage.

| Fixture | Initial layout | Live requested heap | After dropping the tree |
| --- | ---: | ---: | ---: |
| 1 image | 21.07 ms | 16.70 MiB | 16.63 MiB |
| 10 distinct image paths | 96.55 ms | 160.74 MiB | 160.64 MiB |

A replay explicitly dropped both the tree and editor and still retained
168,434,127 requested bytes, about 160.63 MiB.

`Outline::parse` resolves every image, including offscreen images.
`ImageProps::load` keeps `Option<Image>` in a thread-local, unbounded, strong map.
`Image::from_file` decodes full-resolution pixels synchronously. The 560-pixel
logical display-width cap only changes geometry, not the decoded payload or bitmap
atlas upload size. Consequently, switching away does not release the cached
pixels. The atlas correctly uses weak image keys, but the application's permanent
strong image handles keep those keys alive after upload as well.

**Required design change:** separate an image asset description from decoded
variants. Parse URLs and lightweight dimensions first; decode on a bounded worker
queue for the viewport plus overscan. Keep stable placeholders to preserve scroll
anchors. Share metadata by asset identity and pixel variants by requested physical
size, orientation, and source revision. Apply configurable byte budgets, strong
ownership for active views, weak interning, and eviction of inactive decoded/GPU
variants. Budget limits must also constrain concurrent decode peaks.

Small displayed images should not require the full source raster to stay alive.
Use decoder-supported scaling where available, otherwise release full-resolution
intermediates after resampling. Negative results need file-change invalidation or
expiry; the current map otherwise retains stale failures indefinitely. File probes
before the cache lookup also still perform filesystem work on subsequent parses.

## 4. Tables bypass the paragraph cache's effective bound

| Table | Initial shapes | Initial heap | Median input | Warm scene build |
| --- | ---: | ---: | ---: | ---: |
| 100 body rows x 4 columns | 810 | 2.54 MiB | 3.36 ms | 0.114 ms |
| 1,000 body rows x 4 columns | 8,010 | 16.91 MiB | 37.56 ms | 0.377 ms |
| 5,000 body rows x 4 columns | 40,010 | 82.98 MiB | Not measured | 1.520 ms |

For 1,000 rows, eight edits caused 64,064 new shapes, exactly 8,008 per edit.
The probe inserts into the first/header cell after selecting it.

`TableLayout::layout` measures every cell at infinite width for column sizing and
again at its final column width. `CellMeasure` keys an ephemeral cache by source
range and width and directly shapes for each miss. The engine retains all returned
cell flows. One table counts as one outer block, so the default 128-block cache
limit does not bound cell or glyph memory. Painting also loops through all cells.

Scene counters show an important limit on the claim: all three table sizes emitted
97 quads, approximately 240–320 visible glyph sprites, and two GPU batches. Existing
clipping does prevent offscreen cells from multiplying GPU primitives in this
fixture. The demonstrated growth is CPU traversal, shaping, and heap ownership.

**Required design change:** retain per-cell shaping across edits and width changes;
separate unwrapped intrinsic metrics from line breaking. Cache by local content,
projection, and font inputs rather than absolute source offsets alone. Maintain
column maxima incrementally, including invalidation when the previous maximum
cell shrinks. Retain row heights independently of glyph caches and materialize
only visible cells plus overscan, active composition, and captured selections.
Use the existing `BlockMeasure::viewport` contract and support viewport-dependent
arrangements rather than special-casing tables in the global renderer.

Exact global auto-fit columns require an initial intrinsic scan. Make the sizing
policy explicit: exact full-document fit, bounded/sampled estimation with stable
anchor correction, or user-defined column widths. Virtualization cannot silently
change this semantic requirement. Cache limits should include bytes/glyphs/cells,
not just the number of outer blocks. Very long single paragraphs need the same
budget review, although they were not measured in this audit.

## 5. Formula caching has a reproducible cliff at 513 unique formulas

| Distinct formulas | Median input | Shapes over 12 edits | Allocation calls over 12 edits |
| --- | ---: | ---: | ---: |
| 512 | 5.660 ms | 192 | 104,115 |
| 513 | 22.783 ms | 1,020 | 4,441,971 |

At 512 entries, insertion of the next distinct formula clears the entire global
map. A subsequent full-document parse repeatedly cycles through this undersized
cache. `Math::PartialEq` uses formula `Rc` identity, so newly prepared but unchanged
formulas also invalidate view descriptions and native preparations.

**Required design change:** keep formulas referenced by the active document
reusable independently of a cross-document cache. Use content/mode identity,
byte-aware global eviction, and weak interning of still-live formulas. Ordinary
LRU alone is insufficient when every revision sequentially revisits a working set
larger than the cache. Incremental analysis should request preparation only for
changed formulas. Preserve the already implemented shared font service and lazy
native glyph preparation; the former SVG/font-discovery pipeline is historical.

## 6. Caret frames replay expensive GPU work

An unfocused native editor retained one scene for seven seconds; the only extra
presentation was the probe's explicit shutdown redraw. A focused editor kept one
layout but rebuilt/presented the scene approximately twice per second. This agrees
with the 500 ms blink interval and the `repaint -> scene.clear -> tree.draw` path.
It establishes unnecessary work per caret frame, not sustained high idle CPU:
one native sampling run returned to 0.0% CPU at `ps` precision.

A `cancel`/`overrightarrow` formula fixture generated 30 native paths and seven
batches in the 2x headless scene test. Metal traces of the 1x native windows found:

| Short trace | Maximum recorded Metal allocation | Fragment-stage observations |
| --- | ---: | --- |
| Plain text | 10.45 MiB | 7 main passes; 2.023 ms summed active intervals |
| Vector formulas | 27.14 MiB | 6 main passes and 18 path passes; path intervals alone summed to 6.033 ms |

These are target-process Metal allocations and GPU intervals, not RSS or CPU
presentation timers. The traces are short and instrumented, with different content;
they do not establish an overall GPU-utilization percentage or a controlled speedup.
They directly confirm repeated path work during otherwise idle caret blinking.

Each path batch interrupts the main pass, builds/uploads path vertices, clears a
full-window intermediate target, performs MSAA/resolve when supported, and resumes
the main pass. Vertex arrays are recreated for every submission, including retained
scene exposure. Non-path scenes do not initially allocate the intermediates, but
once allocated they remain until resize/resource invalidation or renderer teardown.

For BGRA8 and 4x MSAA the two path targets have a nominal payload of:

```text
physical_width * physical_height * 4 bytes * (1 + 4 samples)
1000 x 700 at 1x:   13.35 MiB
1000 x 700 at 2x:   53.41 MiB
3840 x 2160:       158.20 MiB
```

These are descriptor-derived payloads, excluding alignment, driver policy,
compression, swapchain images, and atlas storage. They are not measured resident
bytes. The full allocation delta also contains other resources. Simple square
roots/fractions and SVG icons produced no native paths in the control fixture;
they must not be blamed for path targets merely because they look vector-based.

**Required design change:** retain scene segments for stable content and separate
caret/selection/hover overlays. Reuse prepared GPU instance/path data across
unchanged scenes; the current retained CPU scene still gets re-uploaded. Introduce
damage/segment ownership before attempting partial redraw. Swapchain image contents
cannot simply be assumed preserved; use an explicit retained backing strategy and
measure its memory tradeoff. Preserve ordering, clipping, transforms, and in-flight
resource lifetime.

For actual paths, retain tessellated geometry in local coordinates. Use tight
path-batch bounds or a path raster cache with a byte budget instead of unconditional
full-window intermediates. Group only where ordering permits. Release idle path
attachments at safe lifecycle points. Reducing MSAA globally should follow visual
quality tests, not serve as the first fix.

Glyph atlas entries have no normal age/byte eviction path; `before_frame` only
sweeps dead managed images. CPU raster bounds likewise accumulate by glyph/size/DPI
parameters. Long sessions across fonts, sizes, emoji, and DPI settings therefore
retain their distinct glyph working sets. This retention policy is confirmed by
source, but long-session growth was not directly benchmarked. Introduce telemetry
and configurable budgets; pin tiles used by retained scenes and in-flight frames,
and use generations/invalidations so eviction never makes stored tile IDs stale.

## 7. Large expanded file trees remain fully mounted

Voidnote creates one component for every entry in each expanded directory. It does
not virtualize by viewport. The existing generic scroll benchmark demonstrates the
underlying cost even without file icons, text, or application event handlers:

| Rows | Requested retained bytes | Warm wheel dispatch |
| --- | ---: | ---: |
| 1,000 | 2,914,388 | 20.62 us |
| 10,000 | 46,485,908 | 279.54 us |

Both runs allocated zero bytes during warm scrolling and performed no layout or
reconciliation. Zero allocation therefore does not imply constant work or small
retained memory. These are generic framework workloads, not measured full-sidebar
memory totals.

Provide a virtual list/tree primitive with stable keys, a flattened expanded-row
index, and visible-row mounting. Keep selection and rename state outside recycled
rows; pin the active input/captured row. Apply clip/subtree rejection before
expensive widget drawing and use retained scroll transforms where compatible with
sticky descendants. The application should consume this primitive rather than
implementing separate virtualization for every large list.

## Implementation order and acceptance

1. **Bulk style normalization and image ownership.** They explain second-scale
   input stalls and deterministic 160 MiB retention in small fixture documents.
   Verify nested-style equivalence, non-quadratic scaling, and image memory returning
   to a configured budget after document disposal and GPU retirement.
2. **Formula identity and table cell retention.** Test 511/512/513 and larger working
   sets without a cliff. A local table edit should shape changed cells plus explicitly
   affected width dependents; offscreen glyph ownership must follow a byte budget.
3. **Incremental projection and block metadata.** Selection without a preview change
   should not rebuild document-wide arrays or reset the height index. Preserve
   source coordinates, IME, anchors, history, formula clicks, and table selection.
4. **Retained scene segments, GPU uploads, and path targets.** Caret-only frames must
   not reshape text or rebuild static formula geometry. Replay should not upload
   unchanged buffers. Record GPU timestamp intervals and actual allocated bytes at
   both 1x and 2x; do not use `last_present_time` as GPU execution time.
5. **Virtual file tree and unified resource budgets.** Check 1k/10k/100k rows, repeated
   note switches, large images, emoji, font-size/DPI changes, and resource recovery.

Maintain separate counters for parsing, style normalization, projection changes,
block indexing, shaped paragraphs/cells, scene construction, upload bytes, path
passes, atlas bytes, and retained/peak memory. Compare fresh-process cold opens and
warm repetitions separately. Add scale-ratio/operation-count regression tests;
absolute timing thresholds are machine-specific. Run native traces after builds
finish and without concurrent probes for controlled before/after measurements.

Autosave and file I/O are secondary source-confirmed stalls: note opening reads
synchronously; each change spawns a 500 ms timer, and the surviving timer flattens
and writes the document on the UI executor. Use one cancellable timer per note and
ordered background saves of immutable snapshots. Preserve pending saves across
note switches and prevent an older save from overwriting a newer one. This path
was excluded from the synthetic timings and is not the explanation for their
multi-second input cost.

## Evidence index

Raw logs, source hashes, PNG generator, probe source, and Metal traces:
`/Users/peihongyu/Projects/voidui/target/perf-audit/`.

Key implementation anchors:

- `/Users/peihongyu/Projects/voidnote/src/markdown/outline.rs:324`
- `/Users/peihongyu/Projects/voidui/src/editing/projection.rs:222`
- `/Users/peihongyu/Projects/voidui/src/editing/highlights.rs:39`
- `/Users/peihongyu/Projects/voidnote/src/markdown/mod.rs:56`
- `/Users/peihongyu/Projects/voidui/src/widgets/input/view.rs:17`
- `/Users/peihongyu/Projects/voidui/src/editing/layout/mod.rs:348`
- `/Users/peihongyu/Projects/voidui/src/editing/layout/engine.rs:97`
- `/Users/peihongyu/Projects/voidnote/src/markdown/views.rs:35`
- `/Users/peihongyu/Projects/voidui/src/media/image.rs:290`
- `/Users/peihongyu/Projects/voidnote/src/markdown/table/mod.rs:60`
- `/Users/peihongyu/Projects/voidui/src/editing/layout/cells.rs:18`
- `/Users/peihongyu/Projects/voidnote/src/markdown/math.rs:33`
- `/Users/peihongyu/Projects/voidui/src/widgets/input/client.rs:197`
- `/Users/peihongyu/Projects/voidui/src/core/window.rs:818`
- `/Users/peihongyu/Projects/voidui/crates/voidui_gpui_wgpu/src/wgpu_renderer.rs:1174`
- `/Users/peihongyu/Projects/voidui/crates/voidui_gpui_wgpu/src/wgpu_renderer.rs:1540`
- `/Users/peihongyu/Projects/voidui/crates/voidui_gpui_wgpu/src/wgpu_renderer.rs:1842`
- `/Users/peihongyu/Projects/voidui/crates/voidui_gpui_wgpu/src/wgpu_atlas.rs:78`
- `/Users/peihongyu/Projects/voidui/crates/voidui_gpui_wgpu/src/text_system.rs:206`
- `/Users/peihongyu/Projects/voidnote/src/components/filetree.rs:205`
- `/Users/peihongyu/Projects/voidnote/src/components/editor.rs:54`

## Implementation follow-up

The implementation now spans VoidUI and Voidnote. See
[Performance and resource ownership](performance.md) for the public APIs, budgets,
correctness boundaries, and remaining full-parse/global-fit fallbacks.

A fresh optimized replay with 12 interaction samples produced:

| Workload | Baseline median input | Implemented median input |
| --- | ---: | ---: |
| 1,000 formatted units, 62,890 bytes | 1,707.18 ms | 9.72 ms |
| 1,000 table body rows x 4 columns | 37.56 ms | 3.57 ms |
| 512 unique formulas | 5.66 ms | 0.70 ms |
| 513 unique formulas | 22.78 ms | 0.70 ms |

The formatted-document initial layout fell from 1,789.86 ms to 19.62 ms and
selection updates from 4.014 ms to 0.039 ms. The table's initial requested heap
was 4,827,388 bytes versus 17,734,467 bytes in the baseline. Table geometry remains
proportional to the document; only nearby cells retain full text layouts.

Ten offscreen-capable 2048 x 2048 PNG descriptions now open in about 9.04 ms and
leave about 0.54 MiB of requested heap after disposal in the **headless metadata
probe**. This probe no longer decodes images, so it is not a measurement of ten
fully displayed images. Separate regression tests verify worker decoding, visible
pixel publication, variant sharing, and the configured retention budget.

During an earlier successful native run, focused caret blinking held scene builds
at two while presentations increased from 8 to 14. A subsequent offscreen native
snapshot verified the cropped vector-path renderer. The final Mac session became
locked; surface acquisition repeatedly returned `Occluded`, so a final native
Metal utilization/memory comparison was not completed. No GPU percentage reduction
is claimed. Shader validation and GPU glyph-lifetime tests run independently of
native surface presentation.

Commands, complete logs, source backup, and derived results are retained under
`/Users/peihongyu/Projects/voidui/target/perf-implementation/`. In particular,
`last-replay.txt` records the final replay (the 512-formula control is in `verified-replay.txt`). The original audit logs remain under
`target/perf-audit/results/`.
