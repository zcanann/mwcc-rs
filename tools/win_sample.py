#!/usr/bin/env python3
"""Poor-man's sampling profiler for Windows hosts without ETW privileges.

Launches a command, then repeatedly attaches cdb non-invasively to capture the
main thread's stack, and prints inclusive/exclusive frame counts. Build with
``cargo build --profile profiling -p mwcc`` so frames carry symbols.

Usage: python tools/win_sample.py [--interval S] [--samples N] -- cmd args...
"""

import argparse
import collections
import os
import re
import subprocess
import sys
import time

CDB = r"C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\cdb.exe"
SYMPATH = os.environ.get("MWCC_SYMPATH", os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "profiling"))
FRAME = re.compile(r"^[0-9a-f]+ [0-9a-f`]+ [0-9a-f`]+ (\S+)", re.I)


def sample(pid: int, depth: int) -> list[str]:
    out = subprocess.run(
        [CDB, "-pv", "-p", str(pid), "-y", SYMPATH, "-c",
         f".reload /f; ~0 kc {depth}; qd"],
        capture_output=True, text=True, errors="replace",
    ).stdout
    frames = []
    started = False
    for line in out.splitlines():
        if re.match(r"^\s*#?\s*Call Site", line):
            started = True
            continue
        if started:
            line = line.strip()
            if not line or line.startswith("quit"):
                break
            frames.append(re.sub(r"^[0-9a-f]+\s+", "", line))
    return frames


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--interval", type=float, default=0.5)
    ap.add_argument("--samples", type=int, default=60)
    ap.add_argument("--depth", type=int, default=60)
    ap.add_argument("--cwd")
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    args = ap.parse_args()
    cmd = args.cmd[1:] if args.cmd and args.cmd[0] == "--" else args.cmd
    proc = subprocess.Popen(cmd, cwd=args.cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.5)
    inclusive, exclusive = collections.Counter(), collections.Counter()
    taken = 0
    for _ in range(args.samples):
        if proc.poll() is not None:
            break
        frames = sample(proc.pid, args.depth)
        if not frames:
            continue
        taken += 1
        exclusive[frames[0]] += 1
        for f in set(frames):
            inclusive[f] += 1
        time.sleep(args.interval)
    proc.kill()
    print(f"{taken} samples")
    print("== exclusive ==")
    for f, n in exclusive.most_common(15):
        print(f"{n:4} {f[:160]}")
    print("== inclusive ==")
    for f, n in inclusive.most_common(45):
        print(f"{n:4} {f[:160]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
