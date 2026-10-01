#!/usr/bin/env python3
"""Windows-native reference-project parity runner.

The POSIX harness (``parity_loop.py``/``refctx.sh``) drives the real compiler
through wibo. On a Windows host ``mwcceppc.exe`` runs natively, so this runner
compiles each configured translation unit from the inventory produced by
``reference_inventory.py`` with both compilers, using the project's exact flag
vector from the project root, and compares the whole objects.

Verdicts:
  BYTE     whole objects byte-identical (the credited parity measure)
  DIFF     both compiled, objects differ (function-level match counts reported)
  DEFER    reference compiled, mwcc-rs declined
  REFFAIL  the reference compiler rejected the configuration (not a mwcc-rs fault)
  NOREF    reference compiler binary unavailable

Results are appended to a JSON-lines cache keyed by configuration id and the
mwcc binary's hash, so reruns after an unchanged compiler are free.

Usage:
  python tools/win_parity.py --sample 200 --seed 1
  python tools/win_parity.py --project marioparty4 --version GC/2.6 --limit 50
  python tools/win_parity.py --verdict DEFER --rerun      # retry cached DEFERs
"""

from __future__ import annotations

import argparse
import collections
import concurrent.futures as cf
import hashlib
import json
import os
from pathlib import Path
import random
import re
import shlex
import shutil
import struct
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent.parent
DEFAULT_INVENTORY = HERE / "target/reference-parity/inventory.json"
DEFAULT_CACHE = HERE / "target/reference-parity/win-results.jsonl"
FFCC = Path(os.environ.get("FFCC", HERE.parent / "FFCC-Decomp"))
COMPILER_ROOTS = [
    Path(p) for p in os.environ.get("REFCTX_COMPILER_ROOTS", "").split(os.pathsep) if p
] + [FFCC / "build/compilers", HERE.parent / "Metrowerks/misc/compilers_latest"] + sorted(
    # Sibling decomp checkouts carry point builds FFCC lacks (e.g. GC/3.0a3p1).
    p for p in HERE.parent.glob("*/build/compilers") if p.is_dir())
SJISWRAP = FFCC / "build/tools/sjiswrap.exe"
EXCLUDED = {"GC/1.3.2r", "ProDG/3.5"}


def reference_compiler(version: str) -> Path | None:
    for root in COMPILER_ROOTS:
        candidate = root / version / "mwcceppc.exe"
        if candidate.is_file():
            return candidate
    return None


def split_flags(flags: list[str]) -> list[str]:
    out: list[str] = []
    for flag in flags:
        out.extend(shlex.split(flag, posix=True))
    return out


def file_hash(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


# --- minimal ELF32 BE reader for function-level diagnostics -----------------

def elf_functions(data: bytes) -> dict[str, bytes]:
    """Map function symbol name -> raw bytes (relocation fields as emitted)."""
    if data[:4] != b"\x7fELF":
        return {}
    shoff, = struct.unpack_from(">I", data, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from(">HHH", data, 0x2E)
    sections = []
    for i in range(shnum):
        off = shoff + i * shentsize
        sections.append(struct.unpack_from(">IIIIIIIIII", data, off))
    functions: dict[str, bytes] = {}
    for sec in sections:
        if sec[1] != 2:  # SHT_SYMTAB
            continue
        strtab = sections[sec[6]]
        for j in range(sec[5] // 16):
            name_off, value, size, info, _other, shndx = struct.unpack_from(
                ">IIIBBH", data, sec[4] + j * 16
            )
            if info & 0xF != 2 or shndx == 0 or shndx >= len(sections):
                continue
            s = data[strtab[4] + name_off:]
            name = s[: s.index(b"\0")].decode("latin-1")
            text = sections[shndx]
            start = text[4] + value
            functions[name] = data[start:start + size]
    return functions


READ_ONLY_DATA = (".rodata", ".sdata2", ".sbss2")


def elf_relocations(data: bytes) -> tuple[dict[str, list], list[dict]]:
    """Relocations of each function and the object's data layout.

    Returns ``(relocs, layout)``:

    * ``relocs`` maps function symbol name -> list of
      ``[offset_in_function, elf_reloc_type, symbol_name, addend]`` taken from
      the SHT_RELA sections that apply to the function's section;
    * ``layout`` lists every non-function, non-file symbol defined in a
      section as ``{"name", "section", "value", "size", "type", "bind"}``
      (section symbols are named after their section; local labels such as
      ``...bss.0`` and ``@123`` are included). Objects in initialized
      read-only sections and ``@N`` objects also carry ``"data"`` (hex of up
      to 256 bytes) so their contents can be modeled.
    """
    if data[:4] != b"\x7fELF":
        return {}, []
    shoff, = struct.unpack_from(">I", data, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from(">HHH", data, 0x2E)
    sections = [struct.unpack_from(">IIIIIIIIII", data, shoff + i * shentsize) for i in range(shnum)]

    def cstr(offset: int) -> str:
        end = data.index(b"\0", offset)
        return data[offset:end].decode("latin-1")

    shstr = sections[shstrndx][4] if shstrndx < len(sections) else 0
    section_names = [cstr(shstr + sec[0]) if shstrndx else "" for sec in sections]
    symbols: list[tuple[str, int, int, int, int, int]] = []  # name, value, size, type, bind, shndx
    symtab_index = None
    for index, sec in enumerate(sections):
        if sec[1] != 2:  # SHT_SYMTAB
            continue
        symtab_index = index
        strtab = sections[sec[6]]
        for j in range(sec[5] // 16):
            name_off, value, size, info, _other, shndx = struct.unpack_from(">IIIBBH", data, sec[4] + j * 16)
            name = cstr(strtab[4] + name_off)
            if info & 0xF == 3 and 0 < shndx < len(sections):  # STT_SECTION
                name = section_names[shndx]
            symbols.append((name, value, size, info & 0xF, info >> 4, shndx))
        break
    relocs: dict[str, list] = {}
    if symtab_index is None:
        return relocs, []
    functions = [(s[0], s[5], s[1], s[2]) for s in symbols if s[3] == 2 and 0 < s[5] < len(sections)]
    for sec in sections:
        if sec[1] != 4 or sec[6] != symtab_index:  # SHT_RELA against our symtab
            continue
        target = sec[7]
        entries = []
        for j in range(sec[5] // 12):
            r_offset, r_info, r_addend = struct.unpack_from(">IIi", data, sec[4] + j * 12)
            entries.append((r_offset, r_info & 0xFF, r_info >> 8, r_addend))
        for name, shndx, value, size in functions:
            if shndx != target:
                continue
            rows = relocs.setdefault(name, [])
            for r_offset, r_type, r_sym, r_addend in entries:
                if value <= r_offset < value + size:
                    symbol = symbols[r_sym][0] if r_sym < len(symbols) else ""
                    rows.append([r_offset - value, r_type, symbol, r_addend])
    layout = []
    for name, value, size, kind, bind, shndx in symbols:
        if kind in (2, 4) or not 0 < shndx < len(sections):  # FUNC, FILE, UNDEF, ABS
            continue
        section = section_names[shndx]
        entry = {"name": name, "section": section, "value": value, "size": size, "type": kind, "bind": bind}
        sec = sections[shndx]
        if sec[1] == 1 and size and (section in READ_ONLY_DATA or name.startswith("@")):  # PROGBITS
            entry["data"] = data[sec[4] + value: sec[4] + value + min(size, 256)].hex()
        layout.append(entry)
    return relocs, layout


def function_layout(relocs: list, layout: list[dict]) -> list[dict]:
    """The part of ``layout`` a function's relocations can reach: every
    symbol in a section that one of its relocation symbols is defined in."""
    names = {r[2] for r in relocs}
    sections = {e["section"] for e in layout if e["name"] in names}
    return [e for e in layout if e["section"] in sections]


def run(cmd: list[str], cwd: Path, timeout: int) -> tuple[int, str]:
    try:
        proc = subprocess.run(
            cmd, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            timeout=timeout,
        )
        return proc.returncode, proc.stdout.decode("latin-1", "replace")
    except subprocess.TimeoutExpired:
        return -999, "TIMEOUT"


def last_diag(log: str) -> str:
    lines = [l.strip() for l in log.splitlines() if l.strip()]
    for line in reversed(lines):
        if line.startswith("mwcc:"):
            return line[5:].strip()[:300]
    return (lines[-1] if lines else "")[:300]


PCH_CACHE = HERE / "target/reference-parity/pch-cache"
MISSING_MCH = re.compile(r"the file '([^']+\.mch)' cannot be opened")


def generate_pch(ref_prefix: list[str], flags: list[str], project: Path, mch: str,
                 timeout: int) -> Path | None:
    """Precompile a missing generated ``.mch`` from its textual ``.pch``.

    Clean decomp checkouts omit these build products. The result depends on the
    compiler and flag vector, so it is cached under a key of both; many
    translation units share one header. Returns the access-path root to prepend.
    """
    pch_source = mch[:-4] + ".pch"
    matches = [m for m in project.rglob(Path(pch_source).name)
               if m.as_posix().endswith(pch_source) and "/build/" not in m.as_posix()]
    if not matches:
        return None
    key = hashlib.sha256(json.dumps([ref_prefix, flags, str(project), mch]).encode()).hexdigest()[:20]
    root = PCH_CACHE / key
    target = root / mch
    if target.is_file():
        return root
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix="pch.", dir=root))
    cmd = [*ref_prefix, *flags, "-lang=c++", "-c", str(matches[0]), "-o", str(tmp),
           "-precompile", Path(mch).name]
    rc, _log = run(cmd, project, timeout)
    built = tmp / Path(mch).name
    if rc == 0 and built.is_file():
        try:
            os.replace(built, target)
        except OSError:
            pass
    shutil.rmtree(tmp, ignore_errors=True)
    return root if target.is_file() else None


def reference_diag(log: str) -> str:
    lines = [l.strip("# \t\r") for l in log.splitlines()]
    for i, line in enumerate(lines):
        if line.startswith("Error:") and i + 1 < len(lines):
            return lines[i + 1][:300]
    return last_diag(log)


def evaluate(tu: dict, mwcc: Path, root: Path, timeout: int, keep: Path | None,
             project_functions: bool = True) -> dict:
    result = {"id": tu["configuration_id"], "project": tu["project"],
              "source": tu["source"], "version": tu["mw_version"],
              "language": tu["language"]}
    compiler = reference_compiler(tu["mw_version"])
    if compiler is None:
        result["verdict"] = "NOREF"
        return result
    project = root / tu["project"]
    flags = split_flags(tu["cflags"])
    scratch = Path(tempfile.mkdtemp(prefix="winpar."))
    try:
        ref_o, our_o = scratch / "ref.o", scratch / "our.o"
        ref_prefix = [str(compiler)]
        if tu.get("shift_jis") and SJISWRAP.is_file():
            ref_prefix = [str(SJISWRAP), *ref_prefix]
        t0 = time.time()
        rc, log = run([*ref_prefix, *flags, "-c", tu["source"], "-o", str(ref_o)], project, timeout)
        pch_roots: list[str] = []
        for _attempt in range(4):
            missing = MISSING_MCH.search(log)
            if rc == 0 or not missing:
                break
            root = generate_pch(ref_prefix, flags, project, missing.group(1).replace("\\", "/"), timeout)
            if root is None or str(root) in pch_roots:
                break
            pch_roots.append(str(root))
            access = [a for r in pch_roots for a in ("-i", r)]
            rc, log = run([*ref_prefix, *access, *flags, "-c", tu["source"], "-o", str(ref_o)],
                          project, timeout)
        result["ref_seconds"] = round(time.time() - t0, 2)
        if rc != 0 or not ref_o.is_file():
            result["verdict"] = "REFFAIL"
            result["detail"] = reference_diag(log)
            return result
        if pch_roots:
            result["pch"] = True
            flags = [a for r in pch_roots for a in ("-i", r)] + flags
        our_cmd = [str(mwcc), "--build", tu["mw_version"], *flags, "-c",
                   tu["source"], "-o", str(our_o)]
        t0 = time.time()
        rc, log = run(our_cmd, project, timeout)
        result["our_seconds"] = round(time.time() - t0, 2)
        if rc != 0 or not our_o.is_file():
            result["verdict"] = "DEFER"
            result["detail"] = "TIMEOUT" if rc == -999 else last_diag(log)
            if rc != -999 and project_functions:
                # Function-level projection: both compilers without debug
                # info, ours continuing past unsupported functions.
                ref_p, our_p = scratch / "ref.p.o", scratch / "our.p.o"
                prc, _ = run([*ref_prefix, *flags, "-sym", "off", "-c", tu["source"], "-o", str(ref_p)],
                             project, timeout)
                orc, olog = run([str(mwcc), "--build", tu["mw_version"], "--parity-keep-going", *flags,
                                 "-sym", "off", "-c", tu["source"], "-o", str(our_p)], project, timeout)
                result["skipped"] = [
                    re.sub(r"^mwcc: parity skipped function '[^']*': ", "", line.strip())[:160]
                    for line in olog.splitlines() if "parity skipped function" in line][:200]
                if orc != 0:
                    result["projection_failure"] = last_diag(olog)
                if prc == 0 and ref_p.is_file():
                    rf = elf_functions(ref_p.read_bytes())
                    of = elf_functions(our_p.read_bytes()) if orc == 0 and our_p.is_file() else {}
                    result["functions"] = len(rf)
                    result["functions_exact"] = sum(1 for n, b in rf.items() if of.get(n) == b)
                    result["functions_missing"] = sum(1 for n in rf if n not in of)
            return result
        ref_bytes, our_bytes = ref_o.read_bytes(), our_o.read_bytes()
        if ref_bytes == our_bytes:
            result["verdict"] = "BYTE"
            result["functions"] = len(elf_functions(ref_bytes))
            return result
        result["verdict"] = "DIFF"
        rf, of = elf_functions(ref_bytes), elf_functions(our_bytes)
        exact = sum(1 for n, b in rf.items() if of.get(n) == b)
        result["functions"] = len(rf)
        result["functions_exact"] = exact
        result["functions_missing"] = sum(1 for n in rf if n not in of)
        result["mismatched"] = sorted(n for n, b in rf.items() if n in of and of[n] != b)[:40]
        if keep is not None:
            dest = keep / tu["configuration_id"][:16]
            dest.mkdir(parents=True, exist_ok=True)
            shutil.copy(ref_o, dest / "ref.o")
            shutil.copy(our_o, dest / "our.o")
            (dest / "tu.json").write_text(json.dumps(tu, indent=1))
        return result
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def load_cache(path: Path, fingerprint: str) -> dict[str, dict]:
    cache: dict[str, dict] = {}
    if path.is_file():
        for line in path.read_text().splitlines():
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if row.get("fingerprint") == fingerprint:
                cache[row["id"]] = row
    return cache


def previous_rows(path: Path) -> dict[str, dict]:
    """Latest observation per configuration from any compiler fingerprint."""
    rows: dict[str, dict] = {}
    if path.is_file():
        for line in path.read_text().splitlines():
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            rows[row["id"]] = row
    return rows


def main() -> int:
    os.environ.setdefault("MWCC_EXPERIMENTAL_BUILDS", "1")
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--inventory", type=Path, default=DEFAULT_INVENTORY)
    ap.add_argument("--root", type=Path, default=HERE / "reference_projects")
    ap.add_argument("--cache", type=Path, default=DEFAULT_CACHE)
    ap.add_argument("--mwcc", type=Path, default=HERE / "target/release/mwcc.exe")
    ap.add_argument("--project", action="append")
    ap.add_argument("--version", action="append")
    ap.add_argument("--language")
    ap.add_argument("--source-regex")
    ap.add_argument("--verdict", action="append",
                    help="restrict to configurations whose latest cached verdict is this")
    ap.add_argument("--detail-regex", help="restrict to configurations whose latest cached detail matches")
    ap.add_argument("--sample", type=int, help="random sample size")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--limit", type=int)
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) - 2))
    ap.add_argument("--timeout", type=int, default=120)
    ap.add_argument("--rerun", action="store_true", help="ignore same-fingerprint cache")
    ap.add_argument("--keep", type=Path, help="keep DIFF objects under this directory")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    inventory = json.loads(args.inventory.read_text())
    tus = [t for t in inventory["translation_units"]
           if t["mw_version"] not in EXCLUDED and t.get("source_exists", True)]
    if args.project:
        tus = [t for t in tus if t["project"] in args.project]
    if args.version:
        tus = [t for t in tus if t["mw_version"] in args.version]
    if args.language:
        tus = [t for t in tus if t["language"] == args.language]
    if args.source_regex:
        rx = re.compile(args.source_regex)
        tus = [t for t in tus if rx.search(t["source"])]
    if args.verdict:
        prev = previous_rows(args.cache)
        tus = [t for t in tus if prev.get(t["configuration_id"], {}).get("verdict") in args.verdict]
    if args.detail_regex:
        prev = previous_rows(args.cache)
        rx = re.compile(args.detail_regex)
        tus = [t for t in tus if rx.search(prev.get(t["configuration_id"], {}).get("detail", ""))]
    if args.sample is not None and args.sample < len(tus):
        tus = random.Random(args.seed).sample(tus, args.sample)
    if args.limit is not None:
        tus = tus[: args.limit]

    fingerprint = file_hash(args.mwcc)[:16]
    cache = {} if args.rerun else load_cache(args.cache, fingerprint)
    prev = previous_rows(args.cache)
    todo = [t for t in tus if t["configuration_id"] not in cache]
    print(f"fingerprint {fingerprint}: {len(tus)} selected, {len(tus) - len(todo)} cached, {len(todo)} to run",
          file=sys.stderr)

    # Copy the compiler so a concurrent rebuild cannot swap binaries mid-run.
    frozen = Path(tempfile.gettempdir()) / f"mwcc-{fingerprint}.exe"
    if not frozen.is_file():
        shutil.copy(args.mwcc, frozen)

    results: dict[str, dict] = {t["configuration_id"]: cache[t["configuration_id"]]
                                for t in tus if t["configuration_id"] in cache}
    args.cache.parent.mkdir(parents=True, exist_ok=True)
    with args.cache.open("a") as out, cf.ThreadPoolExecutor(args.jobs) as pool:
        futures = {pool.submit(evaluate, t, frozen, args.root, args.timeout, args.keep): t for t in todo}
        for n, fut in enumerate(cf.as_completed(futures), 1):
            row = fut.result()
            row["fingerprint"] = fingerprint
            results[row["id"]] = row
            out.write(json.dumps(row) + "\n")
            out.flush()
            if not args.quiet:
                extra = row.get("detail", "")
                if row["verdict"] == "DIFF":
                    extra = f"{row['functions_exact']}/{row['functions']} fn"
                print(f"[{n}/{len(todo)}] {row['verdict']:7} {row['project']} {row['version']} {row['source']}  {extra}",
                      file=sys.stderr)

    summarize(list(results.values()), prev)
    return 0


def summarize(rows: list[dict], prev: dict[str, dict]) -> None:
    counts = collections.Counter(r["verdict"] for r in rows)
    comparable = [r for r in rows if r["verdict"] in ("BYTE", "DIFF", "DEFER")]
    print("\n== verdicts ==")
    for verdict in ("BYTE", "DIFF", "DEFER", "REFFAIL", "NOREF"):
        print(f"  {verdict:8} {counts.get(verdict, 0)}")
    if comparable:
        byte = counts.get("BYTE", 0)
        print(f"  BYTE rate over reference-compilable: {byte}/{len(comparable)} = {100 * byte / len(comparable):.1f}%")
    for label, subset in (("DIFF", [r for r in rows if r["verdict"] == "DIFF"]),
                          ("DEFER (keep-going projection)", [r for r in rows if r["verdict"] == "DEFER"]),
                          ("ALL comparable", comparable)):
        fn_total = sum(r.get("functions", 0) for r in subset)
        fn_exact = sum(r.get("functions_exact", 0) for r in subset)
        if label == "ALL comparable":
            # BYTE rows count every function as exact.
            fn_total += sum(r.get("functions", 0) for r in subset if r["verdict"] == "BYTE")
            fn_exact += sum(r.get("functions", 0) for r in subset if r["verdict"] == "BYTE")
        if fn_total:
            print(f"  {label} function bytes exact: {fn_exact}/{fn_total} = {100 * fn_exact / fn_total:.1f}%")
    moved = collections.Counter()
    for r in rows:
        before = prev.get(r["id"])
        if before and before.get("fingerprint") != r.get("fingerprint") and before["verdict"] != r["verdict"]:
            moved[f"{before['verdict']}->{r['verdict']}"] += 1
    if moved:
        print("== movement vs previous fingerprint ==")
        for k, v in moved.most_common():
            print(f"  {k:16} {v}")
    by_version = collections.defaultdict(collections.Counter)
    for r in comparable:
        by_version[r["version"]][r["verdict"]] += 1
    print("== by version (BYTE/DIFF/DEFER) ==")
    for v in sorted(by_version):
        c = by_version[v]
        print(f"  {v:12} {c['BYTE']:5} {c['DIFF']:5} {c['DEFER']:5}")
    skipped = collections.Counter()
    for r in rows:
        for reason in r.get("skipped", []):
            reason = re.sub(r" \(in (structured|function|guard)[^)]*\)", "", reason)
            skipped[re.sub(r"\d+", "N", reason)[:120]] += 1
    if skipped:
        print("== top per-function skip reasons (keep-going projection) ==")
        for reason, n in skipped.most_common(40):
            print(f"  {n:5}  {reason}")
    defer = collections.Counter(
        re.sub(r"\d+", "N", r.get("detail", ""))[:110] for r in rows if r["verdict"] == "DEFER")
    print("== top DEFER reasons ==")
    for reason, n in defer.most_common(25):
        print(f"  {n:4}  {reason}")


if __name__ == "__main__":
    sys.exit(main())
