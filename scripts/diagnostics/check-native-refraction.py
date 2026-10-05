#!/usr/bin/env python3
"""Compare a recovered refraction equation with Apple's installed GPU shader.

Usage: python3 scripts/diagnostics/check-native-refraction.py target/glass-reference
This checks one controlled FP32/no-bleed branch, not the complete glass effect.
Generated AIR, uniforms, binaries and pixels remain in the requested directory.
"""
import hashlib
import json
import math
from pathlib import Path
import re
import struct
import subprocess
import sys

from importlib.util import module_from_spec, spec_from_file_location


def main():
    output = Path(sys.argv[1]).resolve()
    output.mkdir(parents=True, exist_ok=True)
    scripts = Path(__file__).resolve().parent
    library = Path('/System/Library/Frameworks/QuartzCore.framework/Versions/A/Resources/default.metallib')
    spec = spec_from_file_location('extractor', scripts / 'extract-metal-ir.py')
    extractor = module_from_spec(spec)
    spec.loader.exec_module(extractor)
    source = library.read_bytes()
    air, slice_offset = extractor.air_slice(source)
    function = 'glass_background_no_bleed_lpf'
    name, offset, bitcode = next(row for row in extractor.functions(air) if row[0] == function)
    bc = output / f'{name}.bc'
    bc.write_bytes(bitcode)
    ir_path = bc.with_suffix('.ll')
    subprocess.run(['xcrun', 'clang', '-S', '-emit-llvm', '-Xclang', '-disable-llvm-passes',
                    '-x', 'ir', str(bc), '-o', str(ir_path)], check=True)
    ir = ir_path.read_text()
    metadata = re.search(r'!\d+ = !\{i32 0, i32 16, i32 0, !"float4", !"displacement_mat"[^\n]+', ir).group()
    fields = {name: (int(offset), typ) for offset, size, typ, name in re.findall(
        r'i32 (\d+), i32 (\d+), i32 0, !"([^"]+)", !"([^"]+)"', metadata)}
    size = int(re.search(r'!"air.buffer_size", i32 (\d+)', ir).group(1))
    # Controlled input, not a claim about AppKit's complete live uniform buffer.
    values = dict(displacement_mat=[1/256, 0, 0, 1/64], inner_refraction_amount=-60,
                  inner_refraction_inv_height=1/20, outer_refraction_inv_height=1/20,
                  refraction_threshold0=-1, refraction_threshold1=-.5, blur_alpha0=1,
                  blur_dist0=0, blur_dist1=1, blur_dist2=2, blur_dist3=3,
                  complex_refraction=1, clamp_limit=1, sdr_white_value=1)
    uniforms = bytearray(size)
    for key, value in values.items():
        start, typ = fields[key]
        numbers = value if isinstance(value, list) else [value]
        struct.pack_into('<' + ('e' if typ.startswith('half') else 'f') * len(numbers), uniforms, start, *numbers)
    uniform_path = output / 'uniforms.bin'
    uniform_path.write_bytes(uniforms)
    executable = output / 'native-glass-shader'
    subprocess.run(['xcrun', 'clang', '-fobjc-arc', '-framework', 'Foundation', '-framework', 'Metal',
                    str(scripts / 'native-glass-shader.m'), '-o', str(executable)], check=True)
    pixels = output / 'reference.rgba32f'
    run = subprocess.run([str(executable), str(library), function, str(uniform_path), str(pixels)],
                         check=True, text=True, capture_output=True)
    raw = pixels.read_bytes()
    error = 0.0
    for y in range(64):
        distance = -(y + .5) / 64 * 40
        t = max(0, min(1, -distance / 20))
        displacement = -60 * (1 - max(0, min(1, math.sqrt(t * (2 - t)))))
        for x in range(64, 192):
            actual = struct.unpack_from('<4f', raw, (y * 256 + x) * 16)
            expected = [(x + .5 + displacement) / 256, (y + .5) / 64, .25, 1.]
            if not all(math.isfinite(v) for v in actual):
                raise AssertionError('non-finite native output')
            error = max(error, *(abs(a-b) for a,b in zip(actual, expected)))
    # Native bilinear sampling quantizes coordinates. This bound is a measured
    # comparison tolerance, not a promise of bit-identical arithmetic across GPUs.
    if error > 1e-5:
        raise AssertionError(f'native refraction mismatch: {error}')
    report = dict(source=str(library), source_sha256=hashlib.sha256(source).hexdigest(),
                  function=function, bitcode_sha256=hashlib.sha256(bitcode).hexdigest(),
                  wrapper_offset_in_file=slice_offset+offset, uniforms_size=size, fields=fields,
                  authored_uniforms=values, pixels_checked=8192, maximum_channel_error=error,
                  tolerance=1e-5, gpu_output=run.stdout.strip(),
                  scope='FP32/no-bleed inner refraction on a linear source and supplied SDF; not full AppKit equivalence')
    (output / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(run.stdout, end='')
    print(f'PASS {report["pixels_checked"]} pixels; maximum channel error {error:.9g}')


if __name__ == '__main__':
    main()
