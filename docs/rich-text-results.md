# Rich text implementation notes and verification

## Starting constraints and changes

The original text widget measured one `SharedString` as a leaf; sibling strings
therefore entered Taffy independently. Its PreparedText cache key contained one
font/size/height. EditorState stored only String edits, so formatting could not
participate in revision checks, undo, constraints, rich insertion, or preedit.
EditorLayout split hard paragraphs and synthesized exactly one TextRun per block.
Paragraph height and caret y assumed a uniform row height. The backend already
accepted colors/fonts/decorations on runs but not run-specific size/line height.

The shared inline model now feeds both text widgets and EditorLayout. Sparse
canonical spans sit next to document bytes; text/style deltas share one history.
EditorLayout compares paragraph-local visual runs, retaining unchanged paragraphs
and avoiding per-paragraph temporary run allocation. Text labels use rich-aware
weak keys to share identical layouts and rejoin after intrinsic width probes.

Mixed height tests exposed an additional Parley 0.11.1 issue: the shaping item
updates its style index before flushing a previous item, and line-height-only
changes do not force a new item. A compact, optional row adapter now supplies
consistent display geometry through public Paragraph APIs. It uses public Parley
layout/breaker APIs and does not modify the dependency. See
[Rich text foundations](rich-text.md#mixed-height-geometry) for the raw-layout
coordinate contract and remaining functionality boundaries.

## Reproducible performance sample

A release run on this macOS/aarch64 workspace, using the bundled IBM Plex Sans
font and no window/GPU. This is one sample, not a baseline comparison or a claim
about every workload. The allocator reports requested live Rust bytes; it excludes
allocator metadata, native allocations, GPU resources and RSS.

```sh
cargo run --release --example rich_text_bench -- 1000 500
```

The workload contains 1,000 paragraphs, 42,890 UTF-8 bytes, and one bold span per
paragraph. History is disabled for timing local edits. Native font/layout resources
are included in the layout memory delta. The allocation counter itself adds cost.

| Measurement | Result |
| --- | ---: |
| Empty sessions, average over 10,000 handles | 328 requested bytes/session |
| Document bytes plus 1,000 retained spans | 234,890 requested bytes |
| Initial paragraph layout | 10.60 ms |
| Retained layout and font resources | 5,511,936 requested bytes |
| 500 local text replacements | 241.86 µs/edit, 500 shaped paragraphs |
| 500 local italic changes | 265.34 µs/edit, 500 shaped paragraphs |
| 20 width changes across all paragraphs | 5.03 ms, zero new shapes |

Separate tests verify that two distant changes in 1,000 rich paragraphs shape
exactly two paragraphs, including retention of the native buffers between them.
One hundred identical rich labels share one shaping call across layout and resize.
Uniform paragraphs allocate no mixed-height adapter. Actual timing varies with
hardware and runtime load; shaping counts are the deterministic regression checks.

## Verification boundaries

Verification completed: the workspace/all-target suite passed 540 tests (two ignored:
one existing layout case and the opt-in performance report). Default-feature
doctests passed 58 tests; no-default-feature doctests passed 54 tests. The
no-default-feature library check and formatting check also passed.

Integration coverage includes nested style inheritance, UTF-8 validation, CRLF and
Unicode separators, rich fragment insertion, metadata retention, no-op and stale
transactions, independent randomized edit/reference-model histories, multi-caret
commands, bounded delta history, preedit cancellation/rejection/commit, pointer
selection, visual navigation, exact mixed-height row geometry, glyph paint colors,
cache isolation, unchanged paragraph retention, and failed-layout atomicity.

The native example supports formatting, preedit and undo smoke snapshots:

```sh
cargo run --example rich_text -- --smoke target/rich-text-smoke
```

Native smoke was attempted but could not render while the macOS session was locked.
It was stopped without snapshots. CPU real-font geometry/scene tests remain the
verified rendering path for this change; native GUI verification is still pending.

Run the current full suite and compile checks with:

```sh
cargo test --workspace --all-targets
cargo test --doc -p voidui
cargo check --no-default-features --lib
cargo fmt --all --check
```

The manual performance-report test is intentionally ignored by normal test runs;
run it with `cargo test --release --test rich_editing -- --ignored --nocapture`.
