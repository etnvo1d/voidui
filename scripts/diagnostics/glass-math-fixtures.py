#!/usr/bin/env python3
"""Generate arithmetic fixtures from extracted native IR, with explicit host AIR shims.
The fixtures validate WGSL translation, not native GPU fast-intrinsic equivalence.
Usage: glass-math-fixtures.py fixed_frag_lpf_cpf.ll output-directory
"""
import ctypes
import hashlib
import json
import math
from pathlib import Path
import random
import re
import struct
import subprocess
import sys


def main():
    source=Path(sys.argv[1]);out=Path(sys.argv[2]);out.mkdir(parents=True,exist_ok=True)
    text=source.read_text();bodies=[]
    for needle,name in [('9sdf_union','reference_union'),('15supercircle_sdf','reference_shape'),('22sdf_key_fill_highlight','reference_highlight')]:
        match=next(m for m in re.finditer(r'^define [^\n]+\{\n.*?^}',text,re.M|re.S) if needle in m.group().splitlines()[0])
        body=match.group();body=re.sub(r'@[^ (]+\(',f'@{name}(',body,count=1)
        body=body.replace('define internal fastcc','define');body=re.sub(r' #\d+','',body);bodies.append(body)
    calls=set(re.findall(r'@(air\.[^(]+)\(', '\n'.join(bodies)))
    declarations=[]
    for line in text.splitlines():
        if line.startswith('declare ') and any('@'+name+'(' in line for name in calls):
            declarations.append(re.sub(r' #\d+','',line))
    path=out/'native-math.ll';path.write_text('\n'.join(bodies+declarations))
    shim=out/'shim.c';shim.write_text('''
#include <math.h>
typedef float V2 __attribute__((ext_vector_type(2)));
typedef float V3 __attribute__((ext_vector_type(3)));
typedef float V4 __attribute__((ext_vector_type(4)));
float derivative;
#define FN(ret,name,air,args,...) ret name args __asm__("_air." air); ret name args __VA_ARGS__
FN(float,dot,"dot.v2f32",(V2 a,V2 b),{return a.x*b.x+a.y*b.y;})
FN(float,sat,"fast_saturate.f32",(float x),{return fminf(1,fmaxf(0,x));})
FN(float,mix1,"mix.f32",(float a,float b,float t),{return a+(b-a)*t;})
FN(V2,mix2,"mix.v2f32",(V2 a,V2 b,V2 t),{return a+(b-a)*t;})
FN(float,root,"fast_sqrt.f32",(float x),{return sqrtf(x);})
FN(float,iroot,"fast_rsqrt.f32",(float x),{return 1.f/sqrtf(x);})
FN(float,absolute,"fast_fabs.f32",(float x),{return fabsf(x);})
FN(V2,absolute2,"fast_fabs.v2f32",(V2 x),{return (V2){fabsf(x.x),fabsf(x.y)};})
FN(float,maximum,"fast_fmax.f32",(float a,float b),{return fmaxf(a,b);})
FN(float,minimum,"fast_fmin.f32",(float a,float b),{return fminf(a,b);})
FN(V2,maximum2,"fast_fmax.v2f32",(V2 a,V2 b),{return (V2){fmaxf(a.x,b.x),fmaxf(a.y,b.y)};})
FN(float,convert,"convert.f.f32.s.i32",(int x),{return x;})
FN(float,fw,"fwidth.f32",(float x),{return derivative;})
FN(void,discard,"discard_fragment",(void),{})
extern V4 reference_union(V4,V4,float);
extern V3 reference_shape(V2,V2,float,V2);
extern V4 reference_highlight(V4,V4,V4,V4,V4,V4);
void evaluate(int op,const float *data,float *out) {
    const V4 *p=(const V4*)data;V4 r;
    if(op==0) r=reference_union(p[0],p[1],p[2].x);
    else if(op==1) {V3 v=reference_shape(p[0].xy,p[0].zw,p[1].x,p[1].yz);r=(V4){v.x,v.y,v.z,1};}
    else {derivative=p[6].x;r=reference_highlight(p[0],p[1],p[2],p[3],p[4],p[5]);}
    for(int i=0;i<4;i++)out[i]=r[i];
}
''')
    dylib=out/'native-math.dylib';subprocess.run(['xcrun','clang','-O0','-ffp-contract=off','-shared',str(path),str(shim),'-o',str(dylib)],check=True)
    lib=ctypes.CDLL(str(dylib.resolve()));lib.evaluate.argtypes=[ctypes.c_int,ctypes.POINTER(ctypes.c_float),ctypes.POINTER(ctypes.c_float)]
    rng=random.Random(2605);records=[];counts=[0]*3
    for op in range(3):
        for _ in range(1500):
            data=[0.]*28
            if op==0:
                for base in [0,4]:
                    angle=rng.uniform(-math.pi,math.pi);mag=rng.uniform(.1,1.)
                    data[base:base+4]=[rng.uniform(-80,80),math.cos(angle)*mag,math.sin(angle)*mag,1.]
                data[8]=rng.uniform(.1,80)
            elif op==1:
                hw,hh=rng.uniform(20,160),rng.uniform(20,160);radius=rng.uniform(1,min(hw,hh))
                blend=[max(0,min(1,2.8915570351735034*(1-v/(1.5286649465560913*radius)))) for v in [hw,hh]]
                data[:7]=[rng.uniform(0,hw+5),rng.uniform(0,hh+5),hw,hh,radius,*blend]
            else:
                angle=rng.uniform(-math.pi,math.pi);mag=rng.uniform(.1,1.)
                data[:4]=[rng.uniform(-3,1),math.cos(angle)*mag,math.sin(angle)*mag,1.]
                data[4:8]=[rng.uniform(.5,4),rng.uniform(-1,.9),rng.uniform(-.5,3),-math.sqrt(.5)]
                data[8:12]=[-math.sqrt(.5),rng.uniform(.5,4),rng.uniform(-1,.9),rng.uniform(-.5,3)]
                data[12:16]=[math.sqrt(.5),math.sqrt(.5),rng.random(),0]
                data[16:20]=[1,.9,.8,1];data[20:24]=[.7,.8,1,1];data[24]=rng.uniform(.2,2)
            inputs=(ctypes.c_float*28)(*data);result=(ctypes.c_float*4)();lib.evaluate(op,inputs,result)
            if not all(math.isfinite(v) for v in result): continue
            records.append(struct.pack('<36f',op,0,0,0,*inputs,*result));counts[op]+=1
    (out/'fixtures.bin').write_bytes(b''.join(records))
    manifest=dict(seed=2605,counts=counts,source_ir_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        scope='Extracted FP32 AIR arithmetic with host intrinsic shims; shape retains explicit half rounding; derivatives supplied.')
    (out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');print(manifest)

if __name__=='__main__':main()
