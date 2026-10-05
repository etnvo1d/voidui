#!/usr/bin/env python3
"""Decode captured background uniforms using the corresponding AIR metadata.
Usage: decode-glass-uniforms.py function.ll captured-buffer.bin output.json
The shader variant determines offsets/types; never mix lpf and lph layouts.
"""
import hashlib
import json
from pathlib import Path
import re
import struct
import sys

ir=Path(sys.argv[1]).read_text();buffer=Path(sys.argv[2]).read_bytes()
metadata=re.search(r'!\d+ = !\{i32 0, i32 16, i32 0, !"float4", !"displacement_mat"[^\n]+',ir).group()
values={};layout={}
for offset,size,typ,name in re.findall(r'i32 (\d+), i32 (\d+), i32 0, !"([^"]+)", !"([^"]+)"',metadata):
    count=int(typ[-1]) if typ[-1].isdigit() else 1
    numbers=struct.unpack_from('<'+('e' if typ.startswith('half') else 'f')*count,buffer,int(offset))
    values[name]=list(numbers) if count>1 else numbers[0]
    layout[name]=dict(offset=int(offset),size=int(size),type=typ)
size=int(re.search(r'!"air.buffer_size", i32 (\d+)',ir)[1])
report=dict(shader=Path(sys.argv[1]).stem,buffer_bytes=size,
    payload_sha256=hashlib.sha256(buffer[:size]).hexdigest(),layout=layout,values=values)
Path(sys.argv[3]).write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(values,indent=2))
