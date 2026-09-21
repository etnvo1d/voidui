# Parley 0.11.1 line-height diagnostics

## Reproduce without the adapter

```sh
cargo test -p voidui_gpui_wgpu --test parley_line_height -- --nocapture --test-threads=1
```

The test registers only the bundled IBM Plex Sans regular font and uses native
Parley builders directly. It shapes `a\nb\nc` with spans `0..2`, `2..4`, `4..5`,
no wrap width and `quantize=false`. The diagnostics print source ranges, actual
font sizes, native run heights and font synthesis. They pin the dependency's
observed behavior; application correctness is tested separately in `rich_text`.

Both `style_run_builder` and `ranged_builder` produce these results:

| Inputs | Extra split | Actual line heights |
| --- | --- | --- |
| Sizes 16/16/16; heights 24/48/20 | None | 20/20/20 |
| Sizes 16/16/16; heights 24/48/20 | Middle `liga=0` | 48/20/20 |
| Sizes 16/16/16; heights 24/48/20 | Middle synthetic bold | 20/20/20 |
| Sizes 16/16/16; heights 24/48/20 | Zero-size out-of-flow boxes | 48/20/20 |
| Sizes 16/32/12; heights 24/48/20 | Any of the above | 48/20/20 |
| Sizes 16/32/12; uniform absolute 24 | None | 24/24/24 |
| Sizes 16/32/12; uniform relative 1.5 | None | 24/48/18 |
| Sizes 16/32/12; relative 1.5/1.5/(20/12) | None | 24/53.333332/20 |

## Exact dependency source path

All line numbers below refer to the unmodified `parley-0.11.1` package source:

- `src/context.rs:111-118`: the indexed builder skips range splitting, not shaping.
- `src/builder.rs:62-77` and `163-177`: both builders call `build_into_layout`.
- `src/builder.rs:306-350`: assign styles to character metadata, then call the
  same `shape_text` implementation.
- `src/shape/mod.rs:127-138`: assign the new `item.style_index` before deciding
  whether to flush. The split predicate includes font size, locale, variations,
  features and spacing, but omits line height.
- `src/shape/mod.rs:166-189`: flush the previous text range with that item; update
  its font size and other shaping properties only after the flush.
- `src/shape/mod.rs:480-494`: pass `item.size` and `item.style_index` to `push_run`.
- `src/layout/data.rs:434-442`: read line height from `styles[style_index]`, using
  the stale range's font size for relative heights.

This explains the scope: glyph sizes are retained correctly, while the metric
reads the next (or last coalesced) style. Uniform absolute heights hide the wrong
index because every style carries the same height. A uniform relative multiplier
also hides it because it is multiplied by the retained correct font size.
Changing features forces more flushes but does not repair the premature index
assignment. Synthetic bold changes actual synthesis, not this height lookup.

## Adapter boundary

The backend uses common native absolute heights plus local target row metrics.
No arbitrary feature tags, spacing perturbations, invisible source characters,
dependency patches or private-data mutation are used. Native wrapping and glyphs
are retained. The native layout's vertical coordinates can differ from display;
Paragraph's geometry and painting APIs are the authority.

The complete adapter regression suite is:

```sh
cargo test -p voidui_gpui_wgpu
```

Tests cover exact source-derived heights, independent uniform-row scene
comparisons, tight leading, row hit testing, selection gestures, clamping,
CRLF and Unicode separators, trailing empty rows, resizing without reshaping,
same-width zero allocations and row-capacity reuse.
