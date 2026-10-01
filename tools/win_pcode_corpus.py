#!/usr/bin/env python3
"""PCode-vs-legacy measurement on reference_projects translation units.

For each sampled translation unit (from the win_parity inventory) this
compiles, all with debug info off (`-sym off`, the function-level projection):

  * the reference compiler,
  * mwcc-rs legacy (`--parity-keep-going`, MWCC_PCODE unset),
  * mwcc-rs PCode only (`--parity-keep-going`, MWCC_PCODE=only), whose skipped
    functions are the ones the PCode pipeline does not claim,

and records per function: claimed by PCode, PCode exact, legacy exact. The
JSON output has the same shape as win_pcode_eval.py's, so
tools/ppc_semantic_check.py and the tally scripts run on it unchanged.

Usage:
  python tools/win_pcode_corpus.py --sample 150 --seed 1 --json-out target/corpus-pcode.json
  python tools/win_pcode_corpus.py --project marioparty4 --version GC/2.6 --limit 40
"""

from __future__ import annotations

import argparse
import collections
import concurrent.futures as cf
import json
import os
from pathlib import Path
import random
import re
import shutil
import sys
import tempfile

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE / "tools"))
import win_parity as parity  # noqa: E402
from win_pcode_eval import classify, relocation_fields  # noqa: E402

# Builds whose selects keep branches (refused by PCode) or that mwcc-rs does
# not model are not worth sampling for a PCode comparison.
PCODE_BUILDS = {"GC/1.1", "GC/1.1p1", "GC/1.2.5", "GC/1.2.5n", "GC/1.3", "GC/1.3.2", "GC/2.0", "GC/2.0p1",
                "GC/2.6", "GC/2.7", "GC/3.0a3", "GC/3.0a3p1", "Wii/1.0"}
SKIPPED = re.compile(r"mwcc: parity skipped function '([^']*)': (.*)")


def compile_ours(mwcc: Path, tu: dict, flags: list[str], project: Path, output: Path,
                 timeout: int, pcode: bool) -> tuple[dict[str, bytes], dict[str, str], str, tuple]:
    environment = dict(os.environ)
    if pcode:
        environment["MWCC_PCODE"] = "only"
    else:
        environment.pop("MWCC_PCODE", None)
    import subprocess
    try:
        process = subprocess.run(
            [str(mwcc), "--build", tu["mw_version"], "--parity-keep-going", *flags, "-sym", "off",
             "-c", tu["source"], "-o", str(output)],
            cwd=project, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=timeout, env=environment)
        log = process.stdout.decode("latin-1", "replace")
        code = process.returncode
    except subprocess.TimeoutExpired:
        return {}, {}, "TIMEOUT", ({}, [])
    skipped = {}
    for line in log.splitlines():
        match = SKIPPED.match(line.strip())
        if match:
            skipped[match.group(1)] = match.group(2)[:160]
    ok = code == 0 and output.is_file()
    functions = parity.elf_functions(output.read_bytes()) if ok else {}
    relocations = parity.elf_relocations(output.read_bytes()) if ok else ({}, [])
    return functions, skipped, "" if code == 0 else parity.last_diag(log), relocations


def evaluate(tu: dict, mwcc: Path, root: Path, timeout: int) -> dict:
    result = {"id": tu["configuration_id"], "project": tu["project"], "source": tu["source"],
              "version": tu["mw_version"], "functions": []}
    compiler = parity.reference_compiler(tu["mw_version"])
    if compiler is None:
        result["status"] = "NOREF"
        return result
    project = root / tu["project"]
    flags = parity.split_flags(tu["cflags"])
    scratch = Path(tempfile.mkdtemp(prefix="pcorp."))
    try:
        ref_prefix = [str(compiler)]
        if tu.get("shift_jis") and parity.SJISWRAP.is_file():
            ref_prefix = [str(parity.SJISWRAP), *ref_prefix]
        ref_o = scratch / "ref.o"
        command = lambda extra: [*ref_prefix, *extra, *flags, "-sym", "off", "-c", tu["source"], "-o", str(ref_o)]
        access: list[str] = []
        rc, log = parity.run(command(access), project, timeout)
        pch_roots: list[str] = []
        for _attempt in range(4):
            missing = parity.MISSING_MCH.search(log)
            if rc == 0 or not missing:
                break
            pch_root = parity.generate_pch(ref_prefix, flags, project, missing.group(1).replace("\\", "/"), timeout)
            if pch_root is None or str(pch_root) in pch_roots:
                break
            pch_roots.append(str(pch_root))
            access = [a for r in pch_roots for a in ("-i", r)]
            rc, log = parity.run(command(access), project, timeout)
        if rc != 0 or not ref_o.is_file():
            result["status"] = "REFFAIL"
            return result
        flags = access + flags
        reference = parity.elf_functions(ref_o.read_bytes())
        ref_relocs, ref_layout = parity.elf_relocations(ref_o.read_bytes())
        # (PCode is the only code generator: one compile serves both.)
        pcode, pcode_skipped, pcode_failure, (our_relocs, our_layout) = compile_ours(
            mwcc, tu, flags, project, scratch / "pcode.o", timeout, pcode=True)
        legacy, legacy_skipped, legacy_failure = pcode, pcode_skipped, pcode_failure
        result["status"] = "OK" if not (legacy_failure or pcode_failure) else "PARTIAL"
        result["legacy_failure"] = legacy_failure
        result["pcode_failure"] = pcode_failure
        for name, code in reference.items():
            claimed = name in pcode and name not in pcode_skipped
            produced = pcode.get(name) if claimed else None
            kind = classify(code, produced) if claimed and produced != code else ""
            result["functions"].append({
                "source": f"{tu['project']}:{tu['source']}",
                "version": tu["mw_version"],
                "name": name,
                "claimed": claimed,
                "exact": claimed and produced == code,
                "legacy_exact": legacy.get(name) == code,
                "kind": kind,
                "reason": "" if claimed else pcode_skipped.get(name, pcode_failure)[:160],
                "reference_words": code.hex() if kind else "",
                "produced_words": produced.hex() if kind else "",
            })
            if kind:
                result["functions"][-1].update(relocation_fields(
                    ref_relocs.get(name, []), ref_layout, our_relocs.get(name, []), our_layout))
        return result
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def main() -> int:
    os.environ.setdefault("MWCC_EXPERIMENTAL_BUILDS", "1")
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--inventory", type=Path, default=parity.DEFAULT_INVENTORY)
    parser.add_argument("--root", type=Path, default=HERE / "reference_projects")
    parser.add_argument("--mwcc", type=Path, default=HERE / "target/release/mwcc.exe")
    parser.add_argument("--project", action="append")
    parser.add_argument("--version", action="append")
    parser.add_argument("--sample", type=int)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) - 2))
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--json-out", type=Path)
    args = parser.parse_args()

    inventory = json.loads(args.inventory.read_text())
    tus = [t for t in inventory["translation_units"]
           if t["mw_version"] in PCODE_BUILDS and t.get("source_exists", True) and t["language"] == "c"]
    if args.project:
        tus = [t for t in tus if t["project"] in args.project]
    if args.version:
        tus = [t for t in tus if t["mw_version"] in args.version]
    # Units the parity runner saw compile on the reference side with functions
    # (skips data-only files and ones needing generated assets).
    previous = parity.previous_rows(parity.DEFAULT_CACHE)
    tus = [t for t in tus
           if previous.get(t["configuration_id"], {}).get("verdict") in ("BYTE", "DIFF", "DEFER")
           and previous[t["configuration_id"]].get("functions", 1) > 0]
    # One configuration per source file: variants of a file mostly repeat it.
    unique: dict[tuple[str, str], dict] = {}
    for tu in tus:
        unique.setdefault((tu["project"], tu["source"]), tu)
    tus = sorted(unique.values(), key=lambda t: t["configuration_id"])
    if args.sample is not None and args.sample < len(tus):
        tus = random.Random(args.seed).sample(tus, args.sample)
    if args.limit is not None:
        tus = tus[: args.limit]

    fingerprint = parity.file_hash(args.mwcc)[:16]
    frozen = Path(tempfile.gettempdir()) / f"mwcc-{fingerprint}.exe"
    if not frozen.is_file():
        shutil.copy(args.mwcc, frozen)
    print(f"{len(tus)} translation units", file=sys.stderr)
    results = []
    with cf.ThreadPoolExecutor(args.jobs) as pool:
        for done, result in enumerate(pool.map(lambda tu: evaluate(tu, frozen, args.root, args.timeout), tus), 1):
            results.append(result)
            if done % 25 == 0:
                print(f"  {done}/{len(tus)}", file=sys.stderr)

    functions = [f for r in results for f in r["functions"]]
    statuses = collections.Counter(r["status"] for r in results)
    claimed = [f for f in functions if f["claimed"]]
    print(f"units {len(results)} {dict(statuses)}; functions {len(functions)}")
    print(f"legacy exact overall: {sum(f['legacy_exact'] for f in functions)}")
    print(f"PCode claimed {len(claimed)}: PCode exact {sum(f['exact'] for f in claimed)}, "
          f"legacy exact {sum(f['legacy_exact'] for f in claimed)}; "
          f"PCode-only wins {sum(f['exact'] and not f['legacy_exact'] for f in claimed)}, "
          f"PCode losses {sum(f['legacy_exact'] and not f['exact'] for f in claimed)}")
    kinds = collections.Counter(f["kind"] for f in claimed if not f["exact"])
    print(f"mismatch kinds: {dict(kinds)}")
    by_version = collections.defaultdict(lambda: [0, 0, 0])
    for result in results:
        for f in result["functions"]:
            if f["claimed"]:
                row = by_version[result["version"]]
                row[0] += 1
                row[1] += f["exact"]
                row[2] += f["legacy_exact"]
    for version, (count, exact, legacy) in sorted(by_version.items()):
        print(f"  {version}: claimed {count}, PCode exact {exact}, legacy exact {legacy}")
    reasons = collections.Counter(re.sub(r"\d+", "N", f["reason"])[:100] for f in functions if not f["claimed"])
    print("== top PCode refusal reasons ==")
    for reason, count in reasons.most_common(20):
        print(f"  {count:6d}  {reason}")
    if args.json_out:
        args.json_out.write_text(json.dumps(functions))
    return 0


if __name__ == "__main__":
    sys.exit(main())
