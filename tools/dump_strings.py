#!/usr/bin/env python3
"""Extract a capture's string objects from the real object into strings.txt.

Usage: dump_strings.py <real.o> [objdump]  -> lines `@NNN <hexbytes>` (NUL trimmed)

Lists LOCAL `@N` OBJECT symbols in `.sdata`/`.data` whose contents end in NUL
(string literals). The jump table (also a `.data` @N object) is excluded by its
4-alignment + pointer-sized entries heuristic: strings carry odd sizes/text.
"""
import subprocess, sys, re
obj = sys.argv[1]
objdump = sys.argv[2] if len(sys.argv) > 3 else "/Users/zcanann/Documents/projects/FFCC-Decomp/build/binutils/powerpc-eabi-objdump"
syms = subprocess.run([objdump, "-t", obj], capture_output=True, text=True).stdout
sections = {}
def section_bytes(name):
    if name in sections:
        return sections[name]
    dump = subprocess.run([objdump, "-s", "-j", name, obj], capture_output=True, text=True).stdout
    data = bytearray()
    for line in dump.splitlines():
        m = re.match(r"^ ([0-9a-f]{4}) ((?:[0-9a-f ]{8,9}){1,4})", line)
        if m:
            offset = int(m.group(1), 16)
            hexes = m.group(2).replace(" ", "")
            while len(data) < offset:
                data.append(0)
            data.extend(bytes.fromhex(hexes))
    sections[name] = data
    return data
for line in syms.splitlines():
    m = re.match(r"^([0-9a-f]{8}) l\s+O\s+(\.s?data)\s+([0-9a-f]{8})\s+(@\d+)$", line)
    if not m:
        continue
    offset, section, size, name = int(m.group(1), 16), m.group(2), int(m.group(3), 16), m.group(4)
    data = section_bytes(section)
    blob = bytes(data[offset:offset+size])
    if not blob.endswith(b"\x00"):
        continue
    body = blob[:-1]
    # exclude the jump table (all-zero relocated words, size multiple of 4, > 0x40)
    if size > 0x40 and size % 4 == 0 and all(b == 0 for b in blob):
        continue
    print(name, body.hex())
