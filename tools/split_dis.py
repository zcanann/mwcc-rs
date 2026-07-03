#!/usr/bin/env python3
"""Split a multi-function objdump dis into per-function rebased files.

Usage: split_dis.py <real.dis> <out_dir>
Each <out_dir>/<fn>.dis has addresses AND branch-target operands rebased to 0
so dis2rust's function-relative labels work.
"""
import re, sys, os
src, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
lines = open(src).read().splitlines()
funcs = []  # (name, base, [lines])
current = None
for ln in lines:
    m = re.match(r'^([0-9a-f]+) <(\S+)>:$', ln)
    if m:
        current = (m.group(2), int(m.group(1), 16), [])
        funcs.append(current)
        continue
    if current is not None:
        current[2].append(ln)
for name, base, body in funcs:
    fixed = [f"00000000 <{name}>:"]
    for ln in body:
        m = re.match(r'^(\s*)([0-9a-f]+)(:\s+(?:[0-9a-f]{2} ){4}\s*\S+\s*)(.*)$', ln)
        if m:
            addr = int(m.group(2), 16) - base
            rest = m.group(4)
            # rebase branch-target operands: a bare hex operand followed by <...>
            def rb(mm):
                return format(int(mm.group(1), 16) - base, 'x') + ' <'
            rest = re.sub(r'\b([0-9a-f]+) <', rb, rest)
            fixed.append(f"{m.group(1)}{format(addr, 'x')}{m.group(3)}{rest}")
        else:
            fixed.append(ln)
    open(os.path.join(out, name + ".dis"), 'w').write("\n".join(fixed) + "\n")
    print(f"{name}: {len(body)} lines, base {base:#x}")
