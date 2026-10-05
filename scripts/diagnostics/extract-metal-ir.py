#!/usr/bin/env python3
"""Extract selected AIR modules from a local metallib for reproducible analysis.

Keep extracted Apple binaries/IR outside the source tree. This tool does not
reconstruct Metal source, and it does not prove which shader a live frame uses.
Clang is used only as a bitcode reader with optimization passes disabled; it
changes the printed target triple, so the original bitcode remains authoritative.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess


def air_slice(data):
    if data[:4] == b"MTLB":
        return data, 0
    if data[:4] != b"\xca\xfe\xba\xbe":
        raise ValueError("expected thin MTLB or 32-bit FAT container")
    count = struct.unpack_from(">I", data, 4)[0]
    for index in range(count):
        cpu, _, offset, size, _ = struct.unpack_from(">5I", data, 8 + index * 20)
        if cpu == 0x1000017:
            return data[offset:offset + size], offset
    raise ValueError("no AIR slice in this container")


def functions(data):
    table = struct.unpack_from("<Q", data, 24)[0]
    bitcode = struct.unpack_from("<Q", data, 72)[0]
    count = struct.unpack_from("<I", data, table)[0]
    cursor = table + 4
    for _ in range(count):
        end = cursor + struct.unpack_from("<I", data, cursor)[0]
        position = cursor + 4
        tags = {}
        while position < end:
            tag = data[position:position + 4].decode("ascii")
            position += 4
            if tag == "ENDT":
                break
            size = struct.unpack_from("<H", data, position)[0]
            position += 2
            tags[tag] = data[position:position + size]
            position += size
        name = tags["NAME"].rstrip(b"\0").decode()
        offset = bitcode + struct.unpack("<3Q", tags["OFFT"])[2]
        magic, _, start, length, _ = struct.unpack_from("<5I", data, offset)
        if magic != 0x0B17C0DE:
            raise ValueError(f"invalid bitcode wrapper for {name}")
        module = data[offset + start:offset + start + length]
        if len(module) != length or module[:4] != b"BC\xc0\xde":
            raise ValueError(f"invalid bitcode payload for {name}")
        yield name, offset, module
        cursor = end


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("library", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--select", default="glass|sdf|fixed_frag|brim|rim")
    args = parser.parse_args()
    original = args.library.read_bytes()
    data, slice_offset = air_slice(original)
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = {"source": str(args.library.resolve()), "sha256": hashlib.sha256(original).hexdigest(),
                "air_slice_offset": slice_offset, "functions": []}
    for name, offset, module in functions(data):
        row = {"name": name, "wrapper_offset_in_file": slice_offset + offset,
               "bitcode_bytes": len(module), "sha256": hashlib.sha256(module).hexdigest()}
        manifest["functions"].append(row)
        if not re.search(args.select, name):
            continue
        if not re.fullmatch(r"[A-Za-z0-9_.$-]+", name):
            raise ValueError(f"unsafe function filename: {name!r}")
        path = args.output / f"{name}.bc"
        path.write_bytes(module)
        result = subprocess.run(["xcrun", "clang", "-S", "-emit-llvm", "-Xclang", "-disable-llvm-passes",
                                 "-x", "ir", str(path), "-o", str(path.with_suffix(".ll"))], capture_output=True, text=True)
        row["ir_decoded"] = result.returncode == 0
        row["diagnostic"] = result.stderr.strip()
        print(f"{name}: {len(module)} bytes; IR {'decoded' if row['ir_decoded'] else 'FAILED'}")
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
