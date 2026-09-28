#!/usr/bin/env python3
"""Section-level comparison of two MWCC ELF objects (reference vs candidate)."""
import struct, sys

def sections(data):
    shoff, = struct.unpack_from(">I", data, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from(">HHH", data, 0x2E)
    raw = [struct.unpack_from(">IIIIIIIIII", data, shoff + i * shentsize) for i in range(shnum)]
    names = raw[shstrndx]
    out = []
    for s in raw:
        n = data[names[4] + s[0]:]
        n = n[: n.index(b"\0")].decode()
        body = data[s[4]:s[4] + s[5]] if s[1] != 8 else b""
        out.append((n, s, body))
    return out

def main():
    a, b = open(sys.argv[1], "rb").read(), open(sys.argv[2], "rb").read()
    sa, sb = sections(a), sections(b)
    print(f"sizes {len(a)} vs {len(b)}; sections {len(sa)} vs {len(sb)}")
    for i in range(max(len(sa), len(sb))):
        x = sa[i] if i < len(sa) else None
        y = sb[i] if i < len(sb) else None
        if x and y and x[0] == y[0] and x[1][1:] [:3] == y[1][1:][:3] and x[1][5:] == y[1][5:] and x[2] == y[2]:
            continue
        def d(s):
            if not s: return "-"
            n, h, body = s
            return f"{n} type={h[1]} flags={h[2]:x} size={h[5]} link={h[6]} info={h[7]} align={h[8]} entsize={h[9]}"
        print(f"[{i}] ref: {d(x)}\n     our: {d(y)}")
        if x and y and x[2] != y[2]:
            first = next((k for k in range(min(len(x[2]), len(y[2]))) if x[2][k] != y[2][k]), min(len(x[2]), len(y[2])))
            print(f"     first byte diff at +0x{first:x}: ref {x[2][first:first+16].hex()} our {y[2][first:first+16].hex()}")

main()
