#!/usr/bin/env python3
"""Classify kept DIFF objects from win_parity.py by which sections differ."""
import collections, json, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
from elf_sections_diff import sections  # noqa: E402

root = Path(sys.argv[1] if len(sys.argv) > 1 else "target/reference-parity/diffs")
fingerprint = sys.argv[2] if len(sys.argv) > 2 else None
rows = {}
for line in open("target/reference-parity/win-results.jsonl"):
    r = json.loads(line)
    if fingerprint is None or r.get("fingerprint") == fingerprint:
        rows[r["id"][:16]] = r
combo = collections.Counter()
per_section = collections.Counter()
examples = collections.defaultdict(list)
for d in sorted(root.iterdir()):
    r = rows.get(d.name)
    if not r or r["verdict"] != "DIFF":
        continue
    a = {n: (h, b) for n, h, b in sections((d / "ref.o").read_bytes())}
    b = {n: (h, bb) for n, h, bb in sections((d / "our.o").read_bytes())}
    differing = sorted(n for n in set(a) | set(b)
                       if n not in a or n not in b or a[n][1] != b[n][1] or a[n][0][1:3] + a[n][0][5:] != b[n][0][1:3] + b[n][0][5:])
    key = tuple(differing)
    combo[key] += 1
    examples[key].append(f"{r['version']} {r['project']} {r['source']} {r.get('functions_exact')}/{r.get('functions')}")
    for n in differing:
        per_section[n] += 1
print("== per section ==")
for n, c in per_section.most_common():
    print(f"{c:4} {n}")
print("== combinations ==")
for k, c in combo.most_common(20):
    print(f"{c:4} {' '.join(k)}")
    for e in examples[k][:3]:
        print("        ", e)
