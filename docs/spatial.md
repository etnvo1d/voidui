# Sticky positioning and 2D transforms

```rust
use voidui::{div, Transform, TransformOrigin};

let header = div().sticky().top(0).z_index(1);
let card = div()
    .size(160, 80)
    .transform("translate(10%, 12px) rotate(-8deg) scale(1.1)".parse::<Transform>().unwrap())
    .transform_origin("center center".parse::<TransformOrigin>().unwrap());
```

```css
header { position: sticky; top: 0; z-index: 1; }
.card {
    transform: translate(10%, 12px) rotate(-8deg);
    transform-origin: 50% 50%;
    transition: transform 180ms ease;
}
.card:hover { transform: translate(10%, 12px) rotate(0deg); }
```

Run `cargo run --example spatial` for an interactive example, or
`cargo run --example spatial -- --snapshot /tmp/spatial.png` for a native GPU
pixel check and screenshot.

## Sticky

Sticky boxes keep their normal-flow slot. Physical insets (`top`, `right`,
`bottom`, `left`, and `inset`) constrain their border box against the nearest
scrollport. An axis with two auto insets stays in normal flow. Percentages use
the scrollport's corresponding dimension, including `calc()`, `min()`, `max()`,
and `clamp()` expressions supported by layout.

The containing block limits travel. Margins participate in the position box;
oversized sticky boxes reduce the logical end inset, which may become negative.
Horizontal direction follows the existing LTR/RTL layout support. Sticky always
establishes a stacking context, including with `z-index:auto`.

`overflow:hidden` establishes a scrollport even when it is not scrollable by the
user. `overflow:clip` does not. Classic scrollbar gutters reduce the available
scrollport. A sticky box without a scrolling ancestor uses the viewport.

## Transform

The CSS parser accepts `none`, `matrix`, `translate`, `translateX`, `translateY`,
`scale`, `scaleX`, `scaleY`, `rotate`, `skew`, `skewX`, and `skewY`. Arguments follow
CSS function syntax, including commas where required. Translation lengths use
the library's existing `px`, `%`, and CSS length arithmetic support. Angles use
`deg`, `rad`, `grad`, or `turn`; unitless zero is accepted. Invalid or unsupported
functions invalidate the declaration instead of partially applying it.

`transform-origin` accepts horizontal/vertical keywords and length-percentages;
it defaults to the border-box center. An optional third coordinate must be zero.
Functions multiply in CSS order. Nested elements compose their matrices, while
normal layout sizes remain unchanged. A non-none transform establishes both a
stacking context and the containing block for absolute/fixed descendants, even
when its matrix is identity. Viewport-fixed and top-layer elements escape
unrelated ancestor transforms and clips.

Scrollable overflow includes the union of the normal and transformed extents:
scaling down never removes normal-flow overflow. Transforms and transform origins
participate in transitions. Matching primitive functions interpolate directly;
unmatched suffixes use CSS 2D matrix decomposition. Singular matrices have no
painted or interactive area. Transform updates do not invalidate text layout.

`WidgetTree::bounds` and `content_bounds` retain pre-transform coordinates.
`visual_bounds` returns a window-space axis-aligned bounding rectangle, analogous
to `getBoundingClientRect`. `window_to_layout` and `window_to_local` expose inverse
mapping for custom adapters. Mouse and drag `local_position`, editing pointers,
text selection, scrollbars, and native titlebar hit regions use inverse mapping.
IME candidate rectangles and selection geometry are returned in window space.

## Performance

Untransformed nodes retain only a null visual-state pointer and use the existing
rectangle path. Descendants with no additional transform or clip share their
ancestor's coordinates. Clip ancestry is immutable and shared, including by
native hit-test snapshots. Warm hit testing allocates nothing.

Transforms execute in the existing GPU primitive pipelines, including text,
images, paths, gradients, borders, and shadows. They do not allocate an offscreen
surface per element. GPU spatial records and clipping ancestors are deduplicated
per scene; upload buffers retain capacity. Identity primitives avoid spatial
buffer reads. Primitive records add two 32-bit fields for the spatial index and
explicit alignment padding; ordinary scenes need no spatial records.

Scrolling updates only the affected layout subtree, with no Taffy pass or text
reshaping. Transform changes refresh geometry and overflow; only changes to
containing-block ownership or classic scrollbar gutters require layout. Static
windows continue to sleep between changes. Retained paint order is reused while
stacking membership is unchanged.

Use `cargo run --release --example spatial_bench -- 1000 1000` to measure requested
heap bytes, scroll/hit time, and allocation counts without a native window or GPU.
Reported memory excludes allocator overhead and RSS. The benchmark asserts zero
warm hit allocations and no scrolling-induced reflow or paint-order rebuild.

## Scope and verification

This is CSS 2D support for the library's block, flex, and grid widgets. It does not
add HTML parsing, inline/table formatting, vertical writing modes, 3D transforms,
perspective, individual `translate`/`rotate`/`scale` properties, `transform-box`,
CSS relative units beyond the existing layout parser, or rounded overflow clips.
SVG's internal `transform` attribute remains separate from the widget's CSS
transform. Raster images and glyphs retain their atlas resolution while transformed.

`tests/spatial.rs` covers layout, clipping, containing blocks, overflow, input,
and transition behavior. Renderer tests validate all three WGSL transports and
spatial scene reuse. `tests/fixtures/spatial-reference.html` contains standalone
browser comparison scenarios; open it manually to inspect live browser values.
Browser comparison is separate from the Rust tests.

References: [CSS Positioned Layout](https://www.w3.org/TR/css-position-3/#sticky-pos)
and [CSS Transforms Level 1](https://www.w3.org/TR/css-transforms-1/).
