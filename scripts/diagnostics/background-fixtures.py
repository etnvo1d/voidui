#!/usr/bin/env python3
"""Run the installed FP32 background shader against controlled multi-level inputs.
Outputs include native and portable uniform layouts and actual native GPU pixels.
Usage: background-fixtures.py extracted/glass_background_lpf.ll output-directory
"""
import hashlib
import json
import math
from pathlib import Path
import re
import struct
import subprocess
import sys


def half(x):
    return struct.unpack('<e', struct.pack('<e',x))[0]


def main():
    ir=Path(sys.argv[1]).read_text();half_mode=Path(sys.argv[1]).stem.endswith('lph');out=Path(sys.argv[2]);out.mkdir(parents=True,exist_ok=True)
    meta=re.search(r'!\d+ = !\{i32 0, i32 16, i32 0, !"float4", !"displacement_mat"[^\n]+',ir).group()
    fields={name:(int(offset),typ) for offset,_,typ,name in re.findall(r'i32 (\d+), i32 (\d+), i32 0, !"([^"]+)", !"([^"]+)"',meta)}
    size=int(re.search(r'!"air.buffer_size", i32 (\d+)',ir)[1]);width=64;height=48;levels=5;edr=1.25
    (out/'input.json').write_text(json.dumps(dict(width=width,height=height,mip_count=levels,edr=edr)))
    for level in range(levels):
        w=max(1,width>>level);h=max(1,height>>level);data=[]
        for y in range(h):
            for x in range(w):
                alpha=.3+.7*(x+.5)/w
                data.extend([alpha*(.05+(x+.5)/w+level*.25),alpha*(.1+(y+.5)/h+level*.1),alpha*(.05+level*.13),alpha])
        (out/f'source-{level}.bin').write_bytes(struct.pack('<'+'f'*len(data),*data))
    data=[]
    for y in range(height):
        for x in range(width):
            angle=(x+.5)/width*2*math.pi
            data.extend([((y+.5)/height-.5)*32, .7*math.cos(angle),.7*math.sin(angle),[0.,.35,.8,1.][(x//4)%4]])
    (out/'sdf.bin').write_bytes(struct.pack('<'+'f'*len(data),*data))
    identity=[[1,0,0,0],[0,1,0,0],[0,0,1,0]]
    base=dict(displacement_mat=[1/width,0,0,1/height],inner_refraction_amount=-12,inner_refraction_inv_height=1/8,
        outer_refraction_amount=9,outer_refraction_inv_height=1/11,refraction_threshold0=-3,refraction_threshold1=1,refraction_opacity=.4,
        blur_radius=3,blur_alpha0=1,blur_alpha1=.5,blur_alpha2=-.2,blur_alpha3=.4,blur_dist0=-20,blur_dist1=-5,blur_dist2=2,blur_dist3=12,
        face_opacity=.8,edge_bleed_amount=14,edge_bleed_inv_height=1/10,edge_bleed_blur_radius=6,edge_bleed_dist0=3,edge_bleed_dist1=-2,
        edge_bleed_opacity=.6,bleed_darken=[1,0],shadow_offset=[0,3/height],shadow_amount=6,shadow_inv_height=1/12,
        shadow_opacity=.5,shadow_inv_radius=1/6,shadow_dist_offset=-.5,shadow_blur_radius=2,shadow_contribution=.4,shadow_face_opacity=.2,
        holding_tone_opacity=.7,sdr_white_value=.85,sdr_shadow_dist0=-2,sdr_shadow_dist1=1,clamp_limit=1.1,preserve_hue=0,complex_refraction=1)
    for prefix,matrix in [('face',[[.8,.1,0,.15],[0,.9,.1,.1],[.1,0,.7,.2]]),('bleed',[[.7,.2,0,.1],[.1,.6,.1,.2],[.1,0,.8,.1]]),('shadow',[[.6,0,0,0],[0,.6,0,0],[0,0,.6,0]])]:
        for i,row in enumerate(matrix):base[f'{prefix}_cm{i}']=list(map(half,row))
    cases=[('all',{}),('coincident_blur_stops',dict(blur_alpha0=1,blur_alpha1=.5,blur_alpha2=0,blur_alpha3=-.5,blur_dist0=-115,blur_dist1=-1,blur_dist2=0,blur_dist3=0)),('coincident_negative',dict(blur_dist2=-1,blur_dist3=-1)),('coincident_positive',dict(blur_dist2=2,blur_dist3=2)),('no_bleed',{}),('simple',dict(complex_refraction=0)),('no_outer',dict(refraction_opacity=0)),
        ('no_face',dict(face_opacity=0)),('no_vibrancy',dict(shadow_contribution=0)),('preserve_hue',dict(preserve_hue=1,clamp_limit=.7)),
        ('no_holding',dict(holding_tone_opacity=0)),('no_clamp',dict(clamp_limit=0)),('light_bleed',dict(bleed_darken=[-1,1])),
        ('no_shadow',dict(shadow_opacity=0)),('tiny_shadow',dict(shadow_opacity=1e-8)),('zero_blur',dict(blur_radius=0,edge_bleed_blur_radius=0,shadow_blur_radius=0))]
    executable=out/'native-glass-shader'
    subprocess.run(['xcrun','clang','-fobjc-arc','-framework','Foundation','-framework','Metal',str(Path(__file__).with_name('native-glass-shader.m')),'-o',str(executable)],check=True)
    library='/System/Library/Frameworks/QuartzCore.framework/Versions/A/Resources/default.metallib'
    manifest=dict(source=library,source_sha256=hashlib.sha256(Path(library).read_bytes()).hexdigest(),width=width,height=height,mip_count=levels,precision='half' if half_mode else 'float',cases=[])
    for name,changes in cases:
        p=base|changes;native=bytearray(size)
        for key,value in p.items():
            offset,typ=fields[key];values=value if isinstance(value,list) else [value]
            struct.pack_into('<'+('e' if typ.startswith('half') else 'f')*len(values),native,offset,*values)
        # Portable layout is 21 aligned vec4 records, exactly as in the WGSL struct.
        rows=[p['displacement_mat'],[p['inner_refraction_amount'],p['inner_refraction_inv_height'],p['outer_refraction_amount'],p['outer_refraction_inv_height']],
            [p['refraction_threshold0'],p['refraction_threshold1'],p['refraction_opacity'],p['complex_refraction']],
            [p['blur_radius'],p['edge_bleed_blur_radius'],p['shadow_blur_radius'],p['face_opacity']],
            [p[f'blur_alpha{i}'] for i in range(4)],[p[f'blur_dist{i}'] for i in range(4)],
            *[p[f'{prefix}_cm{i}'] for prefix in ['face','bleed','shadow'] for i in range(3)],
            [p['edge_bleed_amount'],p['edge_bleed_inv_height'],p['edge_bleed_dist0'],p['edge_bleed_dist1']],
            [p['edge_bleed_opacity'],*p['bleed_darken'],int(name!='no_bleed')],
            [p['shadow_amount'],p['shadow_inv_height'],p['shadow_opacity'],p['shadow_contribution']],
            [0,3,p['shadow_inv_radius'],p['shadow_dist_offset']],
            [p['holding_tone_opacity'],p['sdr_white_value'],p['sdr_shadow_dist0'],p['sdr_shadow_dist1']],
            [p['clamp_limit'],p['preserve_hue'],edr,p['shadow_face_opacity']]]
        assert len(rows)==21
        (out/f'{name}-native.bin').write_bytes(native)
        (out/f'{name}-portable.bin').write_bytes(struct.pack('<84f',*(v for row in rows for v in row)))
        function=('glass_background_no_bleed_' if name=='no_bleed' else 'glass_background_')+('lph' if half_mode else 'lpf')
        subprocess.run([str(executable),library,function,str(out/f'{name}-native.bin'),str(out/f'{name}-expected.bin'),str(out)],check=True,stdout=subprocess.DEVNULL)
        manifest['cases'].append(dict(name=name,function=function,parameters=p))
        print(name,'native GPU reference saved')
    (out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')


if __name__=='__main__':main()
