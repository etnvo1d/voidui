# Liquid Glass: native implementation research

## Status

The portable implementation now contains recovered continuous-corner,
gradient-ovalization, union, inner-refraction, key/fill-highlight and YCbCr
face-matrix calculations. `glass_group` paints a unified field with liquid
adhesion before descendant content. The original cubic warp/power-law rim and
fixed separable blur were removed. Background filtering now uses a reduced
RGBA16F pyramid and the recovered 13-tap variable-blur kernel.

The complete recovered **FP32 background composition function is now integrated**:
inner/outer refraction, piecewise blur, face/bleed/shadow transforms, vibrancy,
holding tone, hue-preserving clamps and EDR scaling. It is isolated in
`glass_background.wgsl` and invoked by the UI's optical pipeline. A separate
alpha-preserving mode remains available with `GlassMaterial.background = None`.

**Full Apple-equivalent window output is still unverified.** Remaining differences
include FP16 specialization throughout the background function, capture/base-mip
construction, native adaptive parameters, foreground vibrancy and display color
management. The renderer's SDR target cannot display extended headroom. Individual
shader parity must not be presented as complete AppKit pixel parity.

### Current differential results on M3

- 16 actual native GPU background fixtures, each 64 x 48 RGBA32F: 49,152 pixels
  and 196,608 channel comparisons. Maximum channel error `2.3841858e-7` on M3.
  The test uses the same quad interpolants, controlled mip textures, varying
  source alpha, non-unit gradients, fractional/zero SDF validity, shadow regions,
  coincident blur stops and EDR scale. It executes the production WGSL function.
- 4,500 extracted-AIR arithmetic cases (host intrinsic shims): maximum absolute
  errors of about `7.63e-6` for union, `0.001141` for supercircle distance, and
  `5.96e-7` for highlights. The larger shape outlier crosses an explicitly
  half-rounded intermediate; the 768-case checked-in subset measures below
  `3.58e-7` for shapes. Tests report this difference rather than claiming exact
  floating-point identity.
- 256 checked-in samples from Apple's actual GPU background shader: portable
  inner-refraction maximum channel error `7.526e-6`.
- 64 direct calls to native `ColorMatrix::set_ycc_composite`: the Rust matrix
  construction differs by at most `1.193e-7` before half coefficient packing.
- Apple's actual `variable_blur_downsample_compute` executed on a 128 x 96
  RGBA16F fixture, compared with the portable 64 x 48 output: maximum difference
  2 half ULPs. This validates that kernel fixture, not the entire pyramid chain.
- End-to-end scene checks verify a connecting neck at gap 6 and separation at
  gap 40 for smoothing 32, overlapping captures, foreground ordering, alpha,
  rounded clipping, transforms, retained presentation and budget fallback.
- A native-window separation/reunion smoke run produced 87 frames with zero
  additional layout passes after the baseline. This is not an isolated GPU
  timing or a frame-by-frame comparison with AppKit.

See [the usage guide](liquid-glass.md) for public APIs and current constraints.

## Local source and reproducibility

Observed on macOS 26.5, build 25F71, Apple M3, on 2026-10-05.
No online descriptions were used as evidence for the formulas below.

- Native shader library:
  `/System/Library/Frameworks/QuartzCore.framework/Versions/A/Resources/default.metallib`
- Library SHA-256:
  `88f8a82780d57dcabc005b62668451e3ded245d959c1afda0405ddbe602db94a`
- QuartzCore dyld image UUID: `C57B87A6-283C-3588-87EF-145BBEE8E257`.
- The universal library has 19 slices. The AIR slice starts at byte 392 and
  contains 180 function entries, including FP16 and FP32 variants.
- Shader compiler metadata: Apple Metal 32023.884, AIR 2.8.0, SDK 26.5.

`extract-metal-ir.py` reads the function records and bitcode wrappers and
preserves each extracted bitcode module separately. `clang -emit-llvm` can print
these modules without the optional Metal Toolchain. It changes the printed host
target triple; original bitcode and its hash remain authoritative. Extraction
alone does not establish which specialized variant WindowServer uses in a frame.

```sh
python3 scripts/diagnostics/extract-metal-ir.py \
  /System/Library/Frameworks/QuartzCore.framework/Versions/A/Resources/default.metallib \
  target/liquid-glass/reverse/quartzcore

xcrun dyld_info -disassemble \
  /System/Library/Frameworks/QuartzCore.framework/Versions/A/QuartzCore \
  > target/liquid-glass/reverse/quartzcore-disassembly.txt
```

Apple binaries and derived IR stay under ignored `target/`; they are not
vendored into the portable renderer. Checked-in evidence contains observations,
formula descriptions and test summaries.

## What the running native container exposes

The probe creates its own window and reads only that window's objects.
The active window uses this path:

```text
NSGlassEffectView / NSGlassEffectContainerView
  AppKit hosting view -> SwiftUI.SDFLayer
    CABackdropLayer + CAFilter("glassBackground")
      CASDFLayer + CASDFOutputEffect
        CASDFElementLayer(s), operation = union
    CASDFLayer + CASDFKeyFillHighlightEffect
```

For regular, light glass, a container with spacing 20 reports:

- `CASDFLayer.smoothness = 20`;
- `gaussianRadius = 0`;
- both element operations are `union`;
- both `gradientOvalization` values are `0.5`;
- the highlight layer also has smoothness 20.

The gap was independently changed to 0, 10, 20 and 40. The corresponding raw
layer observations are in `evidence/liquid-glass/container-spacing20-gap*.txt`.
The similarly named `mergeElements` property remained zero; its value must not
be mistaken for proof that no SDF union occurs.

The active regular fixture reports backdrop scale 0.25, inner amount -60,
inner height 20 and refraction opacity 0.3. Clear reports scale 0.5 and a different
parameter set. In the inactive regular baseline refraction opacity is zero and
the key/fill highlight colors have zero alpha. The inactive probe intentionally
uses a local run loop without full application launch; this is a controlled
inactive baseline, not a captured user-driven deactivation transition.

```sh
xcrun clang -fobjc-arc -framework AppKit -framework QuartzCore \
  scripts/diagnostics/native-glass-probe.m -o target/liquid-glass/native-glass-probe

target/liquid-glass/native-glass-probe regular light active 20 10
```

## Recovered SDF union arithmetic

The installed `fixed_frag_lpf_cpf` module contains
`CA::OGL::Metal::ShaderUtils_<float>::sdf_union` and calls to it from its fragment
program. Its bitcode SHA-256 is
`63e61630a0a9e67126c8456f189c31b12abb9ba1b59d3bd2848638c365fe7d03`.

For two valid fields `A = (da, ga.x, ga.y, a)` and
`B = (db, gb.x, gb.y, b)`, positive effective smoothing is:

```text
k = smoothing * saturate(0.5 - 0.5 * dot(ga, gb))
h = saturate(0.5 + 0.5 * (db - da) / k)
d = mix(db, da, h) - k * h * (1 - h)
g = mix(gb, ga, h)
output = (d, g.x, g.y, 1)
```

The second field has a special invalid-alpha path: when its alpha is zero,
its distance becomes 10000 and its gradient becomes zero. The function does
not normalize the mixed gradient. In particular, smoothing depends on the
incoming gradient directions/magnitudes; replacing it with a constant-width
smooth minimum loses part of the native behavior. The inputs include upstream
gradient ovalization, which still needs to be followed end to end.

Generated FP32 IR locations: `fixed_frag_lpf_cpf.ll:5669-5700`; fragment call
sites at lines 3320 and 4761. Line numbers refer to this compiler's extracted
output and are supplementary to the module/function hashes.

Validation: the extracted arithmetic body was compiled for the host CPU with
explicit dot/mix/saturate intrinsic shims and compared with the equation above
on 10,000 deterministic inputs. Maximum channel error was approximately
`7.9933e-6`. This checks recovered algebra, not Apple GPU intrinsic accuracy,
zero effective smoothing, FP16 behavior, or complete native container pixels.

```sh
python3 scripts/diagnostics/check-sdf-union.py \
  target/liquid-glass/reverse/quartzcore/fixed_frag_lpf_cpf.ll \
  target/liquid-glass/union-reference
```

## Recovered inner refraction and direct native GPU check

The background shader's `complex_refraction` branch uses this radial profile:

```text
t = saturate(-distance * inner_refraction_inv_height)
shift = inner_refraction_amount * (1 - saturate(sqrt(t * (2 - t))))
uv = source_uv + displacement_matrix * sdf_gradient * shift
```

This is the algebra of the recovered branch, not its complete composition:
outer refraction, piecewise blur, face/bleed/shadow color transforms, opacity,
EDR and later clipping still affect the result. FP16 variants introduce explicit
rounding stages that the equation does not show.

To verify that this equation matches actual native shader behavior, the
reference harness loads Apple's installed library through Metal and executes
`glass_background_no_bleed_lpf`, unmodified. It supplies a known linear source
texture, known SDF/gradient texture and a controlled uniform buffer. The helper
vertex shader merely supplies full-screen coordinates.

- Actual fragment bindings: source texture 3, SDF texture 4, uniforms buffer 1,
  EDR buffer 6.
- FP32 `GlassBackgroundUniforms` buffer size: 272 bytes. The FP16 background
  variant declares 224 bytes; the layouts cannot be interchanged.
- 8,192 interior pixels compared on M3.
- Maximum channel error: `7.51980732e-6`, below the `1e-5` test threshold.
- This is a real Apple GPU shader comparison for one isolated branch. It is
  **not** a comparison between VoidUI and a complete AppKit glass view.

```sh
python3 scripts/diagnostics/check-native-refraction.py \
  target/liquid-glass/native-reference
```

The native shader also selects explicit texture LOD from the effective blur
radius. In the inspected branch:

```text
lod = max(0, log2(radius < 2 ? 1 + radius / 2 : radius))
```

A faithful implementation therefore needs the corresponding source pyramid
construction and sampling behavior. The portable pyramid uses a recovered native downsample kernel, but its
base-level capture and complete host scheduling are not yet equivalent.

## Still required before claiming parity

1. Trace the live uniform packing and specialization choices, including the
   relationship between container spacing, SDF union, gradient ovalization,
   supercircle shape parameters and device scale.
2. Complete source-pyramid host behavior and FP16 specialization. FP32
   bleed/shadow, distance-dependent blur and holding/clamp/EDR arithmetic are now
   integrated and tested. Native per-geometry adaptive inputs remain open.
3. Execute native reference fixtures across light/dark, regular/clear,
   active/inactive, small/large shapes and approaching/separating pairs.
4. Compare complete native scenes, foreground vibrancy and color management
   before claiming appearance parity.
5. Validate the portable backend on Windows/Linux GPUs. Matching formulas does
   not, by itself, establish bit-identical results across FP modes, fast-math
   implementations, texture filtering and display color pipelines.

The existing approximate renderer cannot satisfy these requirements merely by
adjusting its presets or adding a visually plausible adhesion curve.

## Additional reference harnesses

`native-glass-blur.m` loads and executes the installed compute function with
imageblock-backed tiles. The checked-in source/output fixtures are numerical
textures generated by this harness, not redistributed Apple shader programs.

```sh
xcrun clang -fobjc-arc -framework Foundation -framework Metal \
  scripts/diagnostics/native-glass-blur.m -o target/liquid-glass/native-glass-blur
target/liquid-glass/native-glass-blur target/liquid-glass/blur-reference

python3 scripts/diagnostics/glass-math-fixtures.py \
  target/liquid-glass/reverse/quartzcore/fixed_frag_lpf_cpf.ll \
  target/liquid-glass/math-reference
VOIDUI_GLASS_FIXTURES="$PWD/target/liquid-glass/math-reference/fixtures.bin" \
  cargo test -p voidui_gpui_wgpu --features liquid-glass --test glass_math -- --nocapture
```

`native-color-matrix.m` calls the identified routine in its own process. Its
address must be obtained from the current image's disassembly; it is not a
portable application API and is not invoked by VoidUI. The native routine first
transforms RGB to YCbCr, adjusts luma endpoints and chroma saturation, transforms
back with its rounded inverse coefficients, then applies premultiplied fill.
The conversion constants were read from the two tables referenced by that
routine. The resulting small off-diagonal residuals are intentional.

Host disassembly also confirms highlight directions `(sin(angle), -cos(angle))`,
spread thresholds `cos(spread)`, and contrast parameters `1 / amount - 2`.
The portable API exposes these authored values rather than fitting rim curves.

Directly cloning AppKit/SwiftUI layer subclasses initially failed to produce a
valid reference frame. Rebuilding their geometry using generic CALayer plus
CASDF/CABackdrop/CAPortal classes succeeded. Text backing contents are omitted;
this local CARenderer trace is not a WindowServer frame capture.

The trace recorded `glass_background_lph` and its actual 224-byte uniforms.
The decoded buffer is checked in as
`evidence/liquid-glass/traced-regular-uniforms.json`. For the 140 x 140 pair it
contains outer amount 28, inner amount -60, inner inverse height 0.05,
blur radius about 1.409524, bleed blur radius 19.6, shadow face opacity about
0.331429, holding opacity 1, and half-rounded clamp 1.03125. These are effective
GPU inputs; copying CAFilter property values directly does not reproduce them.

```sh
xcrun clang -fobjc-arc -DGLASS_TRACE -framework AppKit -framework QuartzCore \
  -framework Metal scripts/diagnostics/native-glass-probe.m \
  scripts/diagnostics/native-glass-trace.m -o target/liquid-glass/native-glass-trace
GLASS_TRACE_OUTPUT=target/liquid-glass/traced-regular \
  target/liquid-glass/native-glass-trace regular light active 20 10
python3 scripts/diagnostics/decode-glass-uniforms.py \
  target/liquid-glass/reverse/quartzcore/glass_background_lph.ll \
  target/liquid-glass/traced-regular/0004-glass_background_lph-slot1.bin \
  target/liquid-glass/traced-regular/uniforms.json
```

Trace file sequence numbers can change when Core Animation changes its pass
schedule; choose the captured `glass_background_lph` buffer bound at slot 1.
Only the probe process is instrumented. No system process is attached or patched.

An explicit-half arithmetic experiment was also compared with 16 actual `lph`
outputs. It did not meet the requested fidelity (errors reached about 0.00366),
so it was **not connected to production or accepted by widening the test bound**.
Its source/results remain under `target/liquid-glass/reverse` for further analysis.
The production pipeline still uses the validated FP32 compositor. A broad 0.005
exploratory bound was diagnostic only, not an acceptance criterion.

## Complete FP32 background differential test

```sh
python3 scripts/diagnostics/background-fixtures.py \
  target/liquid-glass/reverse/quartzcore/glass_background_lpf.ll \
  target/liquid-glass/background-reference
VOIDUI_GLASS_BACKGROUND_FIXTURES="$PWD/target/liquid-glass/background-reference" \
  cargo test -p voidui_gpui_wgpu --features liquid-glass --test glass_background -- --nocapture
```

The native oracle loads the installed `glass_background_lpf` or
`glass_background_no_bleed_lpf` function without modifying it. Per-level source
textures deliberately have different colors so an incorrect LOD cannot pass
merely because the source is a linear ramp. Source alpha varies across the image;
the field includes invalid and fractional-validity regions and gradients of
length 0.7. Fixtures cover disabled branches independently, positive/negative/
zero coincident blur stops, hue-preserving clamps and a non-unit EDR factor.

Two test discrepancies were resolved from evidence rather than widening tolerance:

- Metal render-pass clear color defaults to opaque alpha. Explicit transparent
  clearing was required to compare discarded fragments with WGPU.
- A fullscreen triangle with fragment-side UV division differs numerically from
  the native quad's interpolated UVs. Matching primitive/interpolation reduced
  the fractional-coverage fixture error from `2.384e-6` to `2.384e-7`.

The runtime also now uses a dedicated premultiplied source-over pipeline for the
native optical layer. Replacing pixels from the shared snapshot would let a
sibling's transparent shadow padding erase an earlier lens. A regression fixture
uses overlapping padding and negligible shadow opacity to verify both faces
survive. The alpha-preserving custom-material path retains replacement blending.

Native host disassembly confirms blur packing stores opacity differences:
`[o0, o0-o1, o1-o2, o2-o3]`, while shader ramps use four distance stops. Shadow
sampling negates the visible shadow offset. Clip/capture extents include optical
shadows and the largest blur/displacement from active background branches.

A live, bundled native probe was visually inspected through the native-app tool.
Its regular-light pair on a blue background visibly formed a narrow connection
at spacing 20 / gap 10. This is visual confirmation only, not a registered
pixel-difference test of the two complete applications.

### Headroom and host transfer correction

A debugger breakpoint on `GlassBackgroundFilter::render` in the probe process
confirmed the working transfer exponent at context offset `0x258` is 2.2 on this
build. The disassembly applies `pow(clamp, 1 / exponent)` before shader packing;
1.06961 becomes approximately 1.031061, then half rounding gives the captured
1.03125. The portable background exposes `working_gamma` explicitly.

Host packing also computes a headroom fraction, increases shadow fill opacity by
`(1-headroomFraction)*sdrShadowOpacity`, and suppresses holding tone by the same
headroom fraction. Premultiplied shadow fill RGB remains unchanged when alpha
increases. This policy is now implemented in Rust. A unit test reconstructs the
traced shadow matrix from authored properties and matches all 12 half matrix
coefficients plus shadow face opacity, holding opacity and clamp.

`working_gamma` is only this host parameter transform; it is not a complete
replacement for color-space conversion or a display HDR swapchain.

### Cross-platform compilation

The workspace and all targets were cross-checked locally for
`x86_64-pc-windows-gnu` and `x86_64-unknown-linux-gnu`, in addition to macOS.
These checks compile the shared renderer but do not execute D3D/Vulkan shaders
or establish cross-vendor pixel parity. Current runtime GPU validation is M3.
