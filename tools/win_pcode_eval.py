#!/usr/bin/env python3
"""Measure the staged PCode backend function by function.

Compiles each canary with the real mwcceppc and with mwcc-rs under
``MWCC_PCODE=only --parity-keep-going`` (functions the new pipeline cannot
lower are skipped, not handed to the legacy owners), then reports:

* claimed: functions the new pipeline produced,
* exact:   claimed functions byte-identical to the reference,
* legacy_exact: of the claimed functions, how many the legacy backend
  already matched (so a switch-over's net effect is visible).

It also histograms the first rejection reason of unclaimed functions,
which is the new pipeline's work queue.

Usage: python tools/win_pcode_eval.py [--build GC/2.6] [--filter SUBSTR] [--show-diffs N]
"""

from __future__ import annotations

import argparse
import collections
import concurrent.futures as cf
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE / "tools"))
import win_parity  # noqa: E402

BASELINE = [
    ["-nodefaults"], ["-proc", "gekko"], ["-align", "powerpc"], ["-enum", "int"],
    ["-fp", "hardware"], ["-O4,p"], ["-inline", "auto"], ["-maxerrors", "1"],
    ["-nosyspath"], ["-RTTI", "off"], ["-fp_contract", "on"], ["-str", "reuse"],
]


def directive(text: str, key: str) -> list[str] | None:
    for line in text.splitlines():
        stripped = line.lstrip()
        for prefix in (f"// {key}:", f"//{key}:"):
            if stripped.startswith(prefix):
                return shlex.split(stripped[len(prefix):])
    return None


def family(option: str) -> str:
    return "-O" if option.startswith("-O") or option == "-opt" else option


def flags_for(text: str) -> list[str]:
    extra = directive(text, "flags") or []
    families = {family(a) for a in extra if a.startswith("-")}
    base = [a for group in BASELINE if family(group[0]) not in families for a in group]
    return extra + base


CACHE = HERE / "target/pcode-eval-cache"


def compile_one(source: Path, build: str, mwcc: Path, compiler: Path, legacy_too: bool = True) -> dict:
    text = source.read_text(errors="replace")
    builds = directive(text, "builds")
    if builds and build not in builds and build.removeprefix("GC/") not in builds:
        return {"source": source.name, "skipped": True}
    flags = flags_for(text)
    if source.suffix == ".c":
        flags = flags + ["-lang=c"]
    with tempfile.TemporaryDirectory() as scratch:
        ref, ours, legacy = (Path(scratch) / name for name in ("ref.o", "our.o", "legacy.o"))
        cached = CACHE / build.replace("/", "_") / (source.name + ".o")
        missing_marker = cached.with_suffix(".missing")
        if missing_marker.is_file():
            return {"source": source.name, "skipped": True}
        if cached.is_file() and cached.stat().st_mtime >= source.stat().st_mtime:
            ref.write_bytes(cached.read_bytes())
        else:
            subprocess.run([str(compiler), *flags, "-c", str(source), "-o", str(ref)],
                           capture_output=True)
            cached.parent.mkdir(parents=True, exist_ok=True)
            if not ref.is_file():
                missing_marker.write_text("")
                return {"source": source.name, "skipped": True}
            cached.write_bytes(ref.read_bytes())
        env = dict(os.environ, MWCC_EXPERIMENTAL_BUILDS="1", MWCC_PCODE="only")
        run = subprocess.run([str(mwcc), "--build", build, "--parity-keep-going", *flags,
                              "-c", str(source), "-o", str(ours)],
                             capture_output=True, text=True, errors="replace", env=env)
        if legacy_too:
            legacy_env = dict(os.environ, MWCC_EXPERIMENTAL_BUILDS="1")
            legacy_env.pop("MWCC_PCODE", None)
            subprocess.run([str(mwcc), "--build", build, "--parity-keep-going", *flags,
                            "-c", str(source), "-o", str(legacy)], capture_output=True, env=legacy_env)
        reference = win_parity.elf_functions(ref.read_bytes())
        produced = win_parity.elf_functions(ours.read_bytes()) if ours.is_file() else {}
        ref_relocs, ref_layout = win_parity.elf_relocations(ref.read_bytes())
        our_relocs, our_layout = win_parity.elf_relocations(ours.read_bytes()) if ours.is_file() else ({}, [])
        old = win_parity.elf_functions(legacy.read_bytes()) if legacy.is_file() else {}
        skipped = {}
        for line in run.stderr.splitlines():
            match = re.match(r"mwcc: parity skipped function '([^']*)': (.*)", line)
            if match:
                skipped[match.group(1)] = match.group(2)
        functions = []
        for name, code in reference.items():
            claimed = name in produced and name not in skipped
            kind = ""
            if claimed and produced[name] != code:
                kind = classify(code, produced[name])
            functions.append({
                "kind": kind,
                "name": name,
                "claimed": claimed,
                "exact": claimed and produced[name] == code,
                "legacy_exact": old.get(name) == code,
                "reason": skipped.get(name, "" if claimed else run.stderr.strip()[-160:]),
                # Instruction words, for offline mismatch analysis.
                "reference_words": code.hex() if kind else "",
                "produced_words": produced[name].hex() if kind else "",
            })
            if kind:
                # Relocations and the data they reach, for tools/ppc_semantic_check.py.
                functions[-1].update(relocation_fields(ref_relocs.get(name, []), ref_layout,
                                                       our_relocs.get(name, []), our_layout))
        return {"source": source.name, "functions": functions}


def relocation_fields(ref_relocs: list, ref_layout: list, our_relocs: list, our_layout: list) -> dict:
    return {
        "reference_relocs": ref_relocs,
        "produced_relocs": our_relocs,
        "reference_layout": win_parity.function_layout(ref_relocs, ref_layout),
        "produced_layout": win_parity.function_layout(our_relocs, our_layout),
    }


def opcode_shape(word: int) -> int:
    """Primary+extended opcode with register fields masked."""
    primary = word >> 26
    if primary in (31, 63, 59, 4, 19):
        return (primary << 16) | ((word >> 1) & 0x3FF)
    return primary << 16


def classify(reference: bytes, produced: bytes) -> str:
    ref = [int.from_bytes(reference[i:i + 4], "big") for i in range(0, len(reference), 4)]
    ours = [int.from_bytes(produced[i:i + 4], "big") for i in range(0, len(produced), 4)]
    if sorted(ref) == sorted(ours):
        return "order"
    if len(ref) == len(ours) and [opcode_shape(w) for w in ref] == [opcode_shape(w) for w in ours]:
        return "registers"
    if sorted(opcode_shape(w) for w in ref) == sorted(opcode_shape(w) for w in ours):
        return "order+registers"
    return "selection"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--build", default="GC/2.6")
    parser.add_argument("--filter")
    parser.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) - 2))
    parser.add_argument("--show-diffs", type=int, default=0)
    parser.add_argument("--no-legacy", action="store_true", help="skip the legacy comparison compile")
    parser.add_argument("--json-out", help="write per-function results here")
    args = parser.parse_args()
    compiler = win_parity.reference_compiler(args.build)
    mwcc = HERE / "target/release/mwcc.exe"
    sources = sorted(p for p in (HERE / "canaries").iterdir() if p.suffix in (".c", ".cpp"))
    if args.filter:
        sources = [p for p in sources if args.filter in p.name]
    with cf.ThreadPoolExecutor(args.jobs) as pool:
        results = list(pool.map(lambda s: compile_one(s, args.build, mwcc, compiler, not args.no_legacy), sources))
    functions = [f | {"source": r["source"]} for r in results if not r.get("skipped")
                 for f in r["functions"]]
    if args.json_out:
        import json
        Path(args.json_out).write_text(json.dumps(functions))
    claimed = [f for f in functions if f["claimed"]]
    exact = [f for f in claimed if f["exact"]]
    print(f"{args.build}: {len(functions)} reference functions, {len(claimed)} claimed "
          f"by PCode, {len(exact)} exact")
    print(f"  of claimed: legacy exact {sum(f['legacy_exact'] for f in claimed)}; "
          f"PCode-only wins {sum(f['exact'] and not f['legacy_exact'] for f in claimed)}; "
          f"PCode losses {sum(f['legacy_exact'] and not f['exact'] for f in claimed)}")
    kinds = collections.Counter(f["kind"] for f in claimed if not f["exact"])
    print("  mismatch kinds:", dict(kinds.most_common()))
    reasons = collections.Counter(
        re.sub(r"\d+", "N", f["reason"])[:110] for f in functions if not f["claimed"])
    print("== top rejection reasons ==")
    for reason, count in reasons.most_common(25):
        print(f"  {count:5}  {reason}")
    wrong = [f for f in claimed if not f["exact"]]
    for f in wrong[: args.show_diffs]:
        print(f"  DIFF[{f['kind']}] {f['source']} {f['name']} (legacy {'exact' if f['legacy_exact'] else 'diff'})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
