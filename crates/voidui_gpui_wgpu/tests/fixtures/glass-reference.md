# Glass arithmetic and image fixtures

These are numerical test inputs/outputs, not Apple shader binaries or sources.
Reference host: macOS 26.5 (25F71), Apple M3. The installed QuartzCore metallib
SHA-256 is `88f8a82780d57dcabc005b62668451e3ded245d959c1afda0405ddbe602db94a`.

- `glass-math.bin`: 1024 records, each 36 little-endian float32 values.
  Header vec4 (operation ID followed by padding), seven input vec4s and one
  expected vec4. IDs 0/1/2 use 256 cases each from extracted FP32 AIR arithmetic
  with explicit host intrinsic shims (union, supercircle, key/fill highlight).
  ID 3 uses 256 actual native GPU refraction samples. Seed: 2605.
- `glass-tone.json`: 64 deterministic input/expected records from direct native
  calls to `ColorMatrix::set_ycc_composite`. Seed: 2605. Expected rows contain
  three RGB coefficients and an offset, before half conversion.
- `glass-blur-source.rgba16f`: 128 x 96, four little-endian half channels.
- `glass-blur-native.rgba16f`: 64 x 48 output from Apple's installed
  `variable_blur_downsample_compute`, using 16 x 16 imageblock tiles. Input
  red/green/blue are deterministic periodic ramps and alpha is one.

Reproduction tools live in `scripts/diagnostics/` at the workspace root:
`extract-metal-ir.py`, `glass-math-fixtures.py`, `check-native-refraction.py`,
`native-color-matrix.m`, and `native-glass-blur.m`. A larger locally generated
arithmetic corpus can be supplied with `VOIDUI_GLASS_FIXTURES`.

The tests distinguish host-recompiled arithmetic from native GPU results.
Their tolerances are not a full-frame or cross-platform fidelity guarantee.

`glass-background/` contains 16 controlled complete FP32 background shader cases.
`manifest.json` records every native uniform. `source-N.bin` and `sdf.bin` are
little-endian float32 RGBA inputs; `*-expected.bin` is Apple's actual GPU output;
`*-portable.bin` contains 21 aligned float32 vec4 uniform records. Recreate them
with `scripts/diagnostics/background-fixtures.py`. Transparent target clear and
matching quad interpolation are part of the reference protocol.
