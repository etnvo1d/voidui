# Portable liquid glass

This experimental renderer uses WGPU on every platform. It does not embed
NSGlassEffectView, call private Apple APIs, or capture the desktop. The background
is the content already painted inside the same VoidUI window.

Continuous corners, gradient ovalization, smooth union, inner refraction,
key/fill rim arithmetic, YCbCr face transfer, the complete FP32 background
compositor and a blur-pyramid kernel have been
reconstructed from the installed macOS 26.5 implementation. See
[the research and parity record](liquid-glass-research.md) for native references,
measured numerical differences and features that remain unmatched. **Complete
AppKit appearance parity has not been established.**

## Enable the feature

Liquid glass is opt-in and independent of the default `editing` feature. Enable
`liquid-glass` on the `voidui` dependency (or on `voidui_gpui_wgpu` when using the
renderer directly):

```toml
[dependencies]
voidui = { path = "path/to/voidui", features = ["liquid-glass"] }
```

Without this feature, glass APIs, CSS property, scene primitives, shader sources,
pipelines and scratch textures are not compiled. Other rendering remains available.
`-voidui-liquid-glass` is an unsupported CSS property in that configuration.

## A single surface

```rust
use voidui::{div, GlassMaterial};

let panel = div()
    .width(280.)
    .height(180.)
    .border_radius(28.)
    .liquid_glass(Some(GlassMaterial::regular()))
    .child("Foreground content stays sharp");
```

Use `GlassMaterial::clear()` for a lower blur radius and different face transfer,
or `regular_dark()` for the observed dark face endpoints. Materials are explicit;
system theme or activation changes do not silently rewrite application styles.
Set `.liquid_glass(None)` to remove the material. The CSS equivalent is
`-voidui-liquid-glass: regular | clear | none`; CSS-wide values and ordinary
cascade precedence are supported. The property is not implicitly inherited.
An opaque CSS background painted over the glass will cover its optical effect.

Parameters include blur radius, refraction magnitude/height, gradient
ovalization, continuous/circular corners, key/fill lights, face tone, authored
tint and overall coverage. `GlassMaterial.background` enables the recovered
optical compositor with typed `GlassBackground`, `GlassBlurRamp`, `GlassBleed`,
`GlassOpticalShadow` and `GlassHoldingTone` controls. They expose outer refraction,
distance-dependent blur, bleed, background-aware shadows, holding/clamp behavior
and encoded EDR scale. `working_gamma`, `headroom`, `max_headroom` and
`sdr_shadow_opacity` expose the recovered host transfer controls. The presets are captured material inputs, not a dynamic
implementation of AppKit's geometry/activation/accessibility policy.

Native composition makes the face opaque after sampling and unpremultiplying its
background. For custom transparent lenses use `background: None`; that mode
preserves source alpha and does not apply the additional native background
branches. Finite-value and coefficient checks reject unusable GPU inputs.

## Liquid adhesion between controls

```rust
use voidui::{div, glass_group, GlassMaterial};

let controls = glass_group(GlassMaterial::clear(), 32.)
    .flex()
    .gap(6.)
    .child(div().width(100.).height(80.).border_radius(30.).child("First"))
    .child(div().width(100.).height(80.).border_radius(30.).child("Second"));
```

The direct children's border boxes form one distance field. `spacing` controls
its smoothing width, in logical pixels; it is not an exact gap threshold. As the
boxes approach, their gradient-aware union can become a connecting neck. At zero
spacing the group uses the hard-union limit. Their contents paint afterwards.

The container contributes the glass, so its direct children do not also need
`.liquid_glass(...)`. Child backgrounds should stay transparent. Hidden and
display-none children and children promoted to the top layer do not contribute.
A liquid bridge does not create a new interactive control; normal widget hit
regions still apply.

Per-child translations are supported. Apply rotations and scales to the whole
container; independent child rotation/scale currently returns a descriptive draw
error instead of painting incorrect geometry. The group follows its container's
scrolling, clipping and rounded overflow. These restrictions are not present in
Apple's complete implementation and remain portability work.

The shape field is recalculated on the GPU when geometry changes. This does not
require a timer while idle. For a translation transition, author an initial
`transform: translateX(0px)` so the element already has a transform containing
block; changing from `none` can legitimately require layout.

## Standalone painting

`Painter::paint_glass` paints one lens. Use `paint_liquid_glass_group` with an
array of `(Bounds<Pixels>, Corners<Pixels>)`, a material, and a spacing value to
paint one merged field. Shapes share the current coordinate space and clip.
Paint labels after this call.

`Painter::paint_glass_group` remains a distinct operation: it shares one backdrop
capture between independent lenses without merging their fields. It is useful
for disjoint surfaces. Independent overlapping calls to `paint_glass` capture
previously composed lenses, preserving their layer order.

## Scheduling and memory

- Unchanged windows retain the existing demand-driven scheduling and frame cache.
- A capture copies only a padded optical region; pyramid passes use scissor
  regions. Full-size working/capture textures and a quarter-resolution RGBA16F
  mip chain are reused across revisions.
- Materials retain their own uniform buffers/bind groups. Unchanged parameters
  are not uploaded again. Ordinary styles store glass parameters behind an
  optional shared allocation, keeping large optics records out of every widget.
- `WindowOptions.render_cache.glass_bytes` caps scratch texture payload. The
  default is 128 MiB. Set it to zero for a non-refractive face-color fallback,
  including an application-controlled reduced-transparency mode.
- `RenderStats` exposes glass captures, filtered pixel count, scratch bytes and
  fallback count. These are workload counters, not GPU timestamp measurements.

Merged groups currently evaluate their shape list per shaded pixel. Large groups
of distant lenses should use independent shared-backdrop painting; a spatially
tiled field and automatic grouping have not been implemented. The memory budget
bounds textures, not scene records or all driver allocation overhead.

## Run and verify

```sh
cargo run --release --features liquid-glass --example liquid_glass
cargo run --release --features liquid-glass --example liquid_glass -- --smoke target/liquid-glass/smoke
cargo run --release -p voidui_gpui_wgpu --features liquid-glass --example glass_verify
cargo test -p voidui_gpui_wgpu --features liquid-glass --test glass_math --test glass_background -- --nocapture
cargo test --workspace --all-features --locked
```

Hover over A/B in the demo to separate and reunite the lenses. The smoke run saves
merged, separated and rejoined images, verifies animation produced multiple
frames, and checks no additional layout passes occurred during translation.

GPU arithmetic fixtures compare the portable functions with extracted native
arithmetic and actual native GPU outputs. They distinguish those reference types
explicitly. macOS/M3 has been exercised locally; Linux/Windows cross-compilation passes; runtime behavior
and cross-vendor floating-point/filtering differences still require validation.
