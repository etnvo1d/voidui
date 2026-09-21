# Editor framework verification

## Delivered paths

- `buffer.rs` isolates byte-addressed rope storage, bounded range reads and snapshots.
- `projection.rs` defines hiding, replacement, anchored content, block styles and mappings.
- `extensions.rs` defines ordered projection/command extensions and stable source anchors.
- `layout/` owns viewport indexing, bounded native-flow caching, geometry and painting.
- `block_layout.rs` supplies source-backed custom arrangements and grid layout.
- `views.rs` mounts existing widget subtrees and routes embedded control input/IME.
- `flow.rs` is the shared native paragraph adapter, with no document partitioning policy.

The existing input/textarea/rich-editor controls use this pipeline. Read-only text
labels retain their established shared paragraph path. Highlighting remains outside
history. Document text and style edits continue through the existing atomic validator
and bounded undo stack.

## Automated verification

The workspace/all-target run passed 567 tests, with two intentional/existing ignored
tests. Root doctests passed 61 tests. The no-default-feature library check, example
check and formatting check passed. Logs are under `target/editor-refactor/`.

New real-font/CPU-scene coverage includes source/display mappings, hidden Markdown
markers, cross-paragraph folds, custom object baselines, real inline images, embedded
input/IME/candidate coordinates, extension commands/lifecycle, table cell editing,
viewport eviction/remounting, virtual compound blocks with remote caret queries,
incremental index equivalence and source snapshots without flattening.

```sh
cargo test --workspace --all-targets
cargo test --doc -p voidui
cargo check --no-default-features --lib
cargo fmt --all --check
cargo run --example editor_extensions
```

This round verifies headless scene and geometry behavior; the native window example
was compiled but not visually reverified. It does not claim new native accessibility
or platform-specific input adapters.

## Performance sample

A single release run in this macOS/aarch64 workspace, using bundled IBM Plex Sans,
1,000 paragraphs, 42,890 UTF-8 bytes and 1,000 bold spans. History is disabled for
timing. The updated benchmark uses a 400 × 600 viewport and default overscan rather
than forcing whole-document intrinsic layout.

```sh
cargo run --release --example rich_text_bench -- 1000 500
```

| Measurement | Sample |
| --- | ---: |
| Empty sessions, average over 10,000 handles | 384 requested bytes/session |
| Source and retained spans | 238,712 requested bytes |
| Initial viewport preparation | 2.65 ms |
| Retained viewport layout and font resources | 403,352 requested bytes |
| 500 local text edits | 203.90 µs/edit; 500 shaped paragraphs |
| 500 local format edits | 179.57 µs/edit; 500 shaped paragraphs |
| 20 width changes | 1.19 ms; no new shaping |

These are requested Rust allocation bytes and instrumented CPU timings, not RSS,
GPU memory or a universal latency guarantee. The older rich-text benchmark measured
all paragraphs; its memory and resize work are not directly equivalent. The new
framework keeps more per-session bookkeeping but much less offscreen glyph data.
The structural test still limits EditorState itself to 320 bytes.

The deterministic 20,000-paragraph test checks fewer than 100 initial shaping calls,
a 64-block cache target across distant scrolls, and no document materialization.
A separate 10,000-row compound block emits only viewport children and can materialize
an offscreen caret through `required_position`.

## Explicit boundaries

The default text flow still shapes a complete hard paragraph, preserving its Unicode
context. Arbitrarily long unbroken paragraphs are not automatically split. Custom
block providers can supply segmented/paged layout through the viewport-dependent
arrangement interface. Source-block metadata remains vector-backed; edits remap
prefix/suffix metadata with linear index cost, while changed-line reads and glyph
layout remain localized. Cache targets retain the visible working set even when
it exceeds the requested block count.

The source-backed table provider is layout infrastructure, not a bundled general
HTML/Markdown schema. Serialization, merge/split commands, schema validation and
rectangular-selection policy are extension responsibilities.
