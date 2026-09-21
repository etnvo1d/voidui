# CSS transitions, colors, gradients and box shadows

Run `cargo run --example effects` and hover the lower card. The example stylesheet
is `examples/styles/effects.css`. Effects use the normal stylesheet cascade and
runtime hot reload; they do not require a Cargo feature.

```css
.card {
    color: oklch(90% .08 155);
    background: linear-gradient(125deg in oklch longer hue,
        oklch(70% .18 20), color(display-p3 .1 .7 .5) 60%, #345ac9);
    border-radius: 12px;
    box-shadow: 0 6px 16px rgb(0 0 0 / .3), inset 0 1px 1px #ffffff30;
    transition: color 200ms ease, border-radius 200ms ease, box-shadow 200ms ease;
}
.card:hover {
    color: white;
    border-radius: 24px;
    box-shadow: 0 10px 24px rgb(0 0 0 / .4), inset 0 0 10px #55ddaa40;
}
```

The fluent API sets the same longhands:

```rust
use std::time::Duration;
use voidui::div;
use voidui::style::{
    color::Color,
    gradient::Gradient,
    shadow::BoxShadow,
    transition::{Transition, Easing},
};

let gradient: Gradient = "linear-gradient(to right in oklab, #e95, #58c)".parse()?;
let black: Color = "rgb(0 0 0 / .3)".parse()?;
let card = div()
    .background_image(gradient)
    .box_shadow(BoxShadow::new(0.0, 6.0, 16.0, 0.0, black))
    .transition(Transition::new("box-shadow", Duration::from_millis(200))
        .easing(Easing::EASE_OUT));
# Ok::<(), String>(())
```

`background_image` and `box_shadow` also accept arrays, vectors, and explicit
`CssValue::Inherit`, `Initial`, or `Unset`. `transition` accepts one transition or
an array/list. Independent `transition_property`, `transition_duration`,
`transition_delay`, and `transition_timing_function` setters are available;
duration/delay longhand lists use seconds. An empty `transition(...)` list disables
transitions. `background(color)` remains an alias for setting background-color;
CSS's `background` shorthand resets both background-color and background-image.

## Transitions and invalidation

The four transition longhands and the `transition` shorthand support comma-separated
lists, list repetition, the last matching property, `all`, `none`, CSS-wide
inherit/initial/unset, positive and negative delay, and zero-duration delayed
changes. Timing functions include linear, ease/ease-in/ease-out/ease-in-out,
cubic-bezier(), steps() with all jump positions, and piecewise `linear()`.

A transition starts from an existing displayed style, not the initial construction
of an element or its first appearance after display:none. Interrupted transitions
start at their sampled current value. Reversing uses the CSS shortening factor.
Changing only timing parameters does not restart an existing transition. Removing
a property from transition-property, hiding the element or an ancestor, and
replacing the tree cancel affected transitions.

Currently interpolated properties:

- color, background-color, border-color, uniform border-radius, box-shadow;
- integer z-index and visibility (see [Stacking and overlays](overlays.md));
- font-size, font-weight, line-height;
- width/height/min/max, flex-basis, margins, padding, border widths, gaps and insets
  when both endpoints use the same supported unit (px or percentage);
- flex-grow and flex-shrink.

Shorthands such as margin, padding, border-width and gap match their longhands.
Unknown transition-property names remain in the list but do not animate anything.
Discrete values (including background-image, auto/intrinsic dimensions and inset
mismatches in shadow lists) change immediately. Mixed px/percentage interpolation
requires a calc-capable layout representation and is not implemented. Group opacity,
3D transforms, transition-behavior:allow-discrete, @starting-style, @keyframes, and
DOM TransitionEvent dispatch are not implemented. This is a native widget subset,
not a claim of complete browser CSS conformance.

`WidgetTree::update_styles(Instant)` samples computed values and returns separate
layout/paint invalidation flags. `layout_computed` then reflows those exact sampled
values without reading the clock again. The native window runtime
uses these flags to reflow only metric changes. Text widgets use current paint
color and alignment with their cached glyph positions, so foreground and inherited
color transitions do not reshape text. Unaffected branches are skipped during
animation and selectors are not rematched on each frame.

Only active nodes allocate transition state. Playing transitions request the next
presentation-paced frame; positive delays install a single deadline. Steps easing
waits until its next jump rather than drawing unchanged intermediate frames. Completion
removes the animation deadline. Hidden, minimized, zero-sized and suspended windows
remain parked. GPU failures use the existing bounded retry policy rather than an
animation-driven busy loop.

## Color spaces and output

CSS colors use the `color` crate's floating-point representation and conversions:
named/hex, rgb()/rgba(), hsl()/hsla(), hwb(), lab(), lch(), oklab(), oklch(), and
color() in sRGB, linear sRGB, Display P3, A98 RGB, ProPhoto RGB, Rec.2020, XYZ D50
and XYZ D65. Missing components (`none`) are preserved for interpolation.
`currentColor` resolves against the receiving element's current foreground.

Interpolation uses premultiplied alpha and supports shorter/longer/increasing/
decreasing hue paths in polar gradient spaces. Styles do not quantize floating
values to RGBA8. `Color::new(ColorSpace, [c0, c1, c2, alpha])` constructs a typed
color; `Color::interpolate` also works outside a widget tree.

The presentation target remains SDR sRGB. Out-of-gamut colors use CSS's Oklch
binary-search gamut mapping with local MINDE when painted; the authored values
remain in their original space. Display P3 input/interpolation is supported, but
native P3/HDR surface output and ICC profiles are not. Relative-color syntax and
color-mix() are not implemented. Rendering to UNORM targets preserves encoded
sRGB bytes; sRGB framebuffer formats receive linear shader output.

## Gradients

Supported functions: linear-gradient(), radial-gradient(), conic-gradient(), and
the three repeating variants. They support multiple stops, omitted stop positions,
two-position stops, negative/out-of-range positions, coincident hard stops, color
hints, and `in <color-space> [<hue-direction> hue]`. The default interpolation space
is Oklab. Color-stop fixup and CSS angle conventions apply, including rectangular
corner directions.

Radial gradients support circle/ellipse, closest/farthest side/corner, and explicit
radii. Centers accept one or two px/percentage/keyword coordinates. Conic gradients
support `from <angle>` and `at <position>`, with angle/percentage stops. Stops and
centers do not yet accept calc(), font-relative units or three/four-value edge
positions. The initial background behavior is implemented: gradient images fill
the padding-box and repeat behind the border, with border-box clipping. Additional
background-position/size/repeat/origin/clip longhands, image URLs and blend modes
are not implemented.

Each gradient paints one rounded GPU quad regardless of stop count. A variable-size
color ramp is stored in a storage buffer (or uint texture on WebGL); fragments find
stops with binary search. Color-space conversion and nonlinear ramp subdivision
are cached per element, keyed by immutable image identity, painting-box dimensions,
and currentColor when used. Moving an element or changing unrelated colors does
not rebuild its ramp. Scene replay remaps ramp indices and scene clearing retains
capacity. There is no viewport-sized CPU gradient bitmap or fixed two-color limit.

Ramps adaptively approximate the requested interpolation, targeting half an 8-bit
output step, and preserve hard-stop positions exactly. Coincident repeating stops
use their average color; repeating periods smaller than a physical pixel are
averaged. This is an SDR rendering approximation, not unlimited color precision.

## Box shadows

Lists support none, inset, two to four px lengths (x/y offset, nonnegative blur,
signed spread) and any supported color. Omitted color means currentColor. The first
shadow appears on top. Outer shadows paint before backgrounds and exclude the
border-box even when it is transparent. Inner shadows paint above backgrounds and
below borders, clipped to the padding-box. Shadows neither change layout size nor
clip themselves to their element's overflow setting; ancestor clips still apply.

The blur shader uses a Gaussian approximation with sigma = CSS blur / 2. Gaussian
tails participate in culling and draw ordering. Positive spread adjusts small
corner radii. Shadow interpolation pads the shorter list with transparent zero
shadows and rejects mismatched inset flags. Nonuniform border widths currently use
a circular inner-corner approximation; elliptical corner radii are not supported.

## Verification and standards

```sh
cargo test --workspace
cargo run --example effects -- --smoke target/effects-smoke
cargo run --example effects -- --pixels
cargo bench --bench css -- 1000 1000
cargo bench --bench effects -- 1000 600
```

The smoke test captures native frames, checks an authored color against GPU pixels,
verifies paint-only transitions do not increment layout counts, and verifies a
two-second idle interval has no unsolicited frames. The pixel fixture checks outer
shadow knockout, inset clipping, shadow order, linear hard stops, radial geometry
and premultiplied alpha. Deterministic clock tests cover delays, reversal, retargeting,
list matching, inheritance, cancellation and easing. WGSL validation covers storage,
WebGL and subpixel variants. Windows/Linux compile checks do not replace runtime
validation on those operating systems.

References: [CSS Transitions 1](https://www.w3.org/TR/css-transitions-1/),
[CSS Easing](https://www.w3.org/TR/css-easing-1/),
[CSS Color 4](https://www.w3.org/TR/css-color-4/),
[CSS Images 3](https://www.w3.org/TR/css-images-3/),
[CSS Images 4](https://www.w3.org/TR/css-images-4/),
[CSS Backgrounds and Borders](https://www.w3.org/TR/css-backgrounds-3/#box-shadow).

See [Sticky positioning and 2D transforms](spatial.md) for post-layout
coordinates, transformed clipping, and containing blocks.
