#!/usr/bin/env python3
"""Side-by-side reference vs PCode-pipeline disassembly of canary functions.

Uses the same flags and reference cache as tools/win_pcode_eval.py.

Usage: python tools/win_pcode_show.py <canary file> <function> [<function>...] [--build GC/2.6]
"""

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE / "tools"))
import win_parity  # noqa: E402
import win_pcode_eval as evaluation  # noqa: E402

OBJDUMP = Path(os.environ.get(
    "REFCTX_OBJDUMP", "C:/Projects/FFCC-Decomp/build/binutils/powerpc-eabi-objdump.exe"))


def disassemble(path: Path, function: str) -> list[str]:
    text = subprocess.run([str(OBJDUMP), "-dr", "--no-show-raw-insn", str(path)],
                          capture_output=True, text=True).stdout
    lines, inside = [], False
    for line in text.splitlines():
        if re.match(r"^[0-9a-f]+ <", line):
            inside = line.rstrip().endswith(f"<{function}>:")
            continue
        if inside and line.strip():
            line = re.sub(r"^\s*[0-9a-f]+:\s*", "", line).replace("\t", " ")
            lines.append(re.sub(r" +", " ", line.strip()))
    return lines


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source")
    parser.add_argument("functions", nargs="+")
    parser.add_argument("--build", default="GC/2.6")
    parser.add_argument("--legacy", action="store_true", help="show the legacy backend instead")
    args = parser.parse_args()
    source = Path(args.source)
    text = source.read_text(errors="replace")
    flags = evaluation.flags_for(text) + (["-lang=c"] if source.suffix == ".c" else [])
    compiler = win_parity.reference_compiler(args.build)
    with tempfile.TemporaryDirectory() as scratch:
        ref, ours = Path(scratch) / "r.o", Path(scratch) / "o.o"
        subprocess.run([str(compiler), *flags, "-c", str(source), "-o", str(ref)], capture_output=True)
        env = dict(os.environ, MWCC_EXPERIMENTAL_BUILDS="1")
        if not args.legacy:
            env["MWCC_PCODE"] = "only"
        run = subprocess.run([str(HERE / "target/release/mwcc.exe"), "--build", args.build,
                              "--parity-keep-going", *flags, "-c", str(source), "-o", str(ours)],
                             capture_output=True, text=True, env=env)
        for function in args.functions:
            print(f"==== {function}")
            for line in run.stderr.splitlines():
                if f"'{function}'" in line:
                    print("  ", line)
            left = disassemble(ref, function) if ref.is_file() else ["<no reference object>"]
            right = disassemble(ours, function) if ours.is_file() else []
            for index in range(max(len(left), len(right))):
                a = left[index] if index < len(left) else ""
                b = right[index] if index < len(right) else ""
                marker = " " if a == b else "*"
                print(f"{marker} {a:<40} | {b}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
