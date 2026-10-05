#!/usr/bin/env python3
"""Check the recovered SDF union formula against extracted Apple IR on the CPU.

Usage: python3 scripts/diagnostics/check-sdf-union.py path/to/fixed_frag_lpf_cpf.ll output
The arithmetic body is extracted unchanged. Its linkage/name are changed and
AIR dot/mix/saturate intrinsics are supplied by explicit host implementations.
This checks the algebra; it is NOT a measurement of Apple's GPU fast intrinsics.
"""
import ctypes
import hashlib
import json
import math
from pathlib import Path
import random
import re
import subprocess
import sys


def main():
    source = Path(sys.argv[1])
    output = Path(sys.argv[2]).resolve()
    output.mkdir(parents=True, exist_ok=True)
    ir = source.read_text()
    match = re.search(r'^define internal fastcc <4 x float> @[^\n]*9sdf_union[^\n]*\{\n.*?^}', ir, re.M | re.S)
    if not match:
        raise ValueError('FP32 SDF union function not found')
    original = match.group()
    body = re.sub(r'@[^ (]+\(', '@reference_sdf_union(', original, count=1)
    body = body.replace('define internal fastcc', 'define')
    body = re.sub(r' #\d+', '', body)
    declarations = '''
declare float @air.dot.v2f32(<2 x float>, <2 x float>)
declare float @air.fast_saturate.f32(float)
declare float @air.mix.f32(float, float, float)
declare <2 x float> @air.mix.v2f32(<2 x float>, <2 x float>, <2 x float>)
'''
    path = output / 'union-reference.ll'
    path.write_text(body + declarations)
    shim = output / 'intrinsics.c'
    shim.write_text('''
#include <math.h>
typedef float V2 __attribute__((ext_vector_type(2)));
typedef float V4 __attribute__((ext_vector_type(4)));
float dot(V2 a,V2 b) __asm__("_air.dot.v2f32");
float dot(V2 a,V2 b) { return a.x*b.x+a.y*b.y; }
float sat(float x) __asm__("_air.fast_saturate.f32");
float sat(float x) { return fminf(1.f,fmaxf(0.f,x)); }
float mix1(float a,float b,float h) __asm__("_air.mix.f32");
float mix1(float a,float b,float h) { return a+(b-a)*h; }
V2 mix2(V2 a,V2 b,V2 h) __asm__("_air.mix.v2f32");
V2 mix2(V2 a,V2 b,V2 h) { return a+(b-a)*h; }
extern V4 reference_sdf_union(V4,V4,float);
void evaluate(const float *a,const float *b,float k,float *out) {
    V4 r=reference_sdf_union((V4){a[0],a[1],a[2],a[3]},(V4){b[0],b[1],b[2],b[3]},k);
    for(int i=0;i<4;i++) out[i]=r[i];
}
''')
    libpath = output / 'union-reference.dylib'
    subprocess.run(['xcrun', 'clang', '-O0', '-ffp-contract=off', '-shared', str(path), str(shim), '-o', str(libpath)], check=True)
    library = ctypes.CDLL(str(libpath))
    array = ctypes.c_float * 4
    library.evaluate.argtypes = [ctypes.POINTER(ctypes.c_float), ctypes.POINTER(ctypes.c_float), ctypes.c_float, ctypes.POINTER(ctypes.c_float)]
    f32 = lambda value: ctypes.c_float(value).value
    clamp = lambda value: max(0., min(1., value))
    rng = random.Random(2605)
    error = 0.
    checked = 0
    for _ in range(10000):
        angles = [rng.uniform(-math.pi, math.pi) for _ in range(2)]
        magnitudes = [rng.uniform(.1, 1.) for _ in range(2)]
        a = array(rng.uniform(-100, 100), magnitudes[0]*math.cos(angles[0]), magnitudes[0]*math.sin(angles[0]), 1.)
        b = array(rng.uniform(-100, 100), magnitudes[1]*math.cos(angles[1]), magnitudes[1]*math.sin(angles[1]), 1.)
        smoothing = f32(rng.uniform(.1, 100))
        k = smoothing * clamp(.5 - .5 * (a[1]*b[1] + a[2]*b[2]))
        if k < 1e-5:
            continue  # The zero-width path is a separate host/dispatch question.
        h = clamp(.5 + .5 * (b[0] - a[0]) / k)
        expected = [b[0]+(a[0]-b[0])*h-k*h*(1-h),
                    b[1]+(a[1]-b[1])*h, b[2]+(a[2]-b[2])*h, 1.]
        result = array()
        library.evaluate(a, b, smoothing, result)
        if not all(math.isfinite(v) for v in result):
            raise AssertionError('non-finite reference result')
        error = max(error, *(abs(actual-target) for actual,target in zip(result, expected)))
        checked += 1
    if error > 5e-5:
        raise AssertionError(f'union formula mismatch: {error}')
    report = dict(source_ir=str(source), extracted_function_sha256=hashlib.sha256(original.encode()).hexdigest(),
                  random_seed=2605, cases_checked=checked, maximum_channel_error=error, tolerance=5e-5,
                  scope='FP32 algebra checked against extracted IR with host AIR intrinsic shims; positive effective smoothing only; not GPU equivalence')
    (output / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f'PASS {checked} union cases; maximum channel error {error:.9g}')


if __name__ == '__main__':
    main()
