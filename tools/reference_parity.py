#!/usr/bin/env python3
"""Run resumable, exact-flag MWCC A/B tests over the reference inventory."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
from typing import Any, Dict, Iterable, List, Optional, Sequence, Tuple


STATUSES = ("BYTE", "DIFF", "DEFER", "HARNESS", "UNSUPPORTED_BUILD")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_inventory(args: argparse.Namespace, script_dir: Path) -> Dict[str, Any]:
    if args.inventory is not None:
        with args.inventory.open(encoding="utf-8") as source:
            return json.load(source)
    command = [sys.executable, str(script_dir / "reference_inventory.py")]
    if args.root is not None:
        command.append(str(args.root))
    result = subprocess.run(command, text=True, capture_output=True)
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "reference inventory failed")
    inventory = json.loads(result.stdout)
    if args.write_inventory is not None:
        args.write_inventory.parent.mkdir(parents=True, exist_ok=True)
        with args.write_inventory.open("w", encoding="utf-8") as output:
            json.dump(inventory, output, indent=2, sort_keys=True)
            output.write("\n")
    return inventory


def flatten_flags(row: Dict[str, Any]) -> List[str]:
    flags: List[str] = []
    for entry in [*row["cflags"], *row["extra_cflags"]]:
        flags.extend(shlex.split(entry))
    return flags


def selected_rows(rows: Iterable[Dict[str, Any]], args: argparse.Namespace) -> List[Dict[str, Any]]:
    source_pattern = re.compile(args.source) if args.source else None
    selected = []
    for row in rows:
        if not row["source_exists"]:
            continue
        if args.project and row["project"] not in args.project:
            continue
        if args.variant and row["variant"] not in args.variant:
            continue
        if args.version and row["mw_version"] not in args.version:
            continue
        if args.language and row["language"] not in args.language:
            continue
        if args.matching_only and not row["matching"]:
            continue
        if source_pattern and source_pattern.search(row["source"]) is None:
            continue
        selected.append(row)
    if args.shard_count > 1:
        selected = [
            row
            for index, row in enumerate(selected)
            if index % args.shard_count == args.shard_index
        ]
    if args.limit:
        selected = selected[: args.limit]
    return selected


def build_supported(compiler: Path, build: str) -> Tuple[bool, str]:
    environment = os.environ.copy()
    environment["MWCC_EXPERIMENTAL_BUILDS"] = "1"
    with tempfile.TemporaryDirectory(prefix="mwcc-build-probe.") as scratch:
        source = Path(scratch) / "probe.c"
        output = Path(scratch) / "probe.o"
        source.write_text("int mwcc_reference_probe;\n", encoding="utf-8")
        result = subprocess.run(
            [str(compiler), "--build", build, "-c", str(source), "-o", str(output)],
            text=True,
            capture_output=True,
            env=environment,
        )
    detail = (result.stderr or result.stdout).strip().splitlines()
    return result.returncode == 0, detail[0] if detail else "unsupported compiler build"


def row_id(row: Dict[str, Any], tool_fingerprint: str) -> str:
    identity = {
        "tool": tool_fingerprint,
        "project": row["project"],
        "source": row["source"],
        "build": row["mw_version"],
        "cflags": row["cflags"],
        "extra_cflags": row["extra_cflags"],
        "shift_jis": row["shift_jis"],
    }
    encoded = json.dumps(identity, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def load_cache(path: Path) -> Dict[str, Dict[str, Any]]:
    cached: Dict[str, Dict[str, Any]] = {}
    if not path.is_file():
        return cached
    with path.open(encoding="utf-8") as source:
        for line in source:
            try:
                record = json.loads(line)
                cached[record["id"]] = record
            except (json.JSONDecodeError, KeyError):
                continue
    return cached


def verdict_line(output: str) -> str:
    for line in output.splitlines():
        if line.startswith(("BYTE", "DIFF", "DEFER")):
            return line
    return output.splitlines()[0] if output.splitlines() else ""


def classify(output: str, returncode: int) -> str:
    first = verdict_line(output)
    for status in ("BYTE", "DIFF", "DEFER"):
        if first.startswith(status):
            return status
    return "HARNESS" if returncode != 0 or first else "HARNESS"


def run_row(
    row: Dict[str, Any],
    reference_root: Path,
    refctx: Path,
    compiler: Path,
    timeout: int,
) -> Tuple[str, str]:
    project = reference_root / row["project"]
    command = [
        str(refctx),
        str(project),
        row["source"],
        row["mw_version"],
        *flatten_flags(row),
    ]
    environment = os.environ.copy()
    environment["REFCTX_EMPTY_BASE"] = "1"
    environment["MWCC_BIN"] = str(compiler)
    environment["MWCC_EXPERIMENTAL_BUILDS"] = "1"
    try:
        result = subprocess.run(
            command,
            text=True,
            capture_output=True,
            env=environment,
            timeout=timeout,
        )
        output = "\n".join(part.strip() for part in (result.stdout, result.stderr) if part.strip())
        return classify(output, result.returncode), output
    except subprocess.TimeoutExpired:
        return "HARNESS", f"timed out after {timeout}s"


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, help="reuse a generated inventory JSON")
    parser.add_argument("--write-inventory", type=Path, help="save a newly generated inventory")
    parser.add_argument("--root", type=Path, help="reference_projects root")
    parser.add_argument("--compiler", type=Path, default=Path("target/release/mwcc"))
    parser.add_argument("--cache", type=Path, help="JSONL result cache (default: target/reference-parity)")
    parser.add_argument("--project", action="append", help="project name (repeatable)")
    parser.add_argument("--variant", action="append", help="configured project variant (repeatable)")
    parser.add_argument("--version", action="append", help="full build label, e.g. GC/1.3.2")
    parser.add_argument("--language", choices=("c", "c++"), action="append")
    parser.add_argument("--source", help="source-path regular expression")
    parser.add_argument("--matching-only", action="store_true")
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--shard-count", type=int, default=1)
    parser.add_argument("--shard-index", type=int, default=0)
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--rerun", action="store_true", help="ignore cached results")
    parser.add_argument("--list", action="store_true", help="list selected rows without compiling")
    return parser.parse_args(argv)


def main() -> int:
    args = parse_args()
    if args.shard_count < 1 or not 0 <= args.shard_index < args.shard_count:
        print("invalid shard index/count", file=sys.stderr)
        return 2
    script_dir = Path(__file__).resolve().parent
    root = script_dir.parent
    compiler = args.compiler if args.compiler.is_absolute() else root / args.compiler
    refctx = script_dir / "refctx.sh"
    if not compiler.is_file():
        print(f"compiler not found: {compiler} (build it with cargo build --release -p mwcc)", file=sys.stderr)
        return 2

    try:
        inventory = load_inventory(args, script_dir)
    except (OSError, RuntimeError, json.JSONDecodeError) as error:
        print(f"inventory failed: {error}", file=sys.stderr)
        return 2
    rows = selected_rows(inventory["translation_units"], args)
    if args.list:
        for row in rows:
            print(
                f'{row["project"]}\t{row["variant"]}\t{row["mw_version"]}\t'
                f'{row["language"]}\t{row["source"]}'
            )
        print(f"== {len(rows)} selected translation-unit configurations ==")
        return 0

    compiler_hash = sha256_file(compiler)
    fingerprint = compiler_hash + ":" + sha256_file(refctx)
    cache = args.cache
    if cache is None:
        cache = root / "target" / "reference-parity" / f"{fingerprint[:20]}.jsonl"
    cache.parent.mkdir(parents=True, exist_ok=True)
    cached = {} if args.rerun else load_cache(cache)
    build_support: Dict[str, Tuple[bool, str]] = {}
    counts = {status: 0 for status in STATUSES}
    reused = 0

    with cache.open("a", encoding="utf-8") as cache_output:
        for index, row in enumerate(rows, 1):
            identity = row_id(row, fingerprint)
            if identity in cached:
                record = cached[identity]
                status = record["status"]
                detail = record.get("output", "")
                reused += 1
            else:
                build = row["mw_version"]
                if build not in build_support:
                    build_support[build] = build_supported(compiler, build)
                supported, support_detail = build_support[build]
                if supported:
                    status, detail = run_row(
                        row,
                        Path(inventory["reference_root"]),
                        refctx,
                        compiler,
                        args.timeout,
                    )
                else:
                    status, detail = "UNSUPPORTED_BUILD", support_detail
                record = {
                    "id": identity,
                    "status": status,
                    "project": row["project"],
                    "variant": row["variant"],
                    "source": row["source"],
                    "mw_version": row["mw_version"],
                    "language": row["language"],
                    "output": detail,
                }
                cache_output.write(json.dumps(record, sort_keys=True) + "\n")
                cache_output.flush()
            counts[status] = counts.get(status, 0) + 1
            first_detail = verdict_line(detail)
            print(
                f'[{index}/{len(rows)}] {status:<17} {row["project"]} '
                f'{row["variant"]} {row["mw_version"]} {row["source"]} — {first_detail}'
            )

    summary = " / ".join(f"{status} {counts.get(status, 0)}" for status in STATUSES)
    print(f"== {len(rows)} configurations: {summary} / cached {reused} ==")
    print(f"cache: {cache}")
    return 1 if counts["DIFF"] or counts["HARNESS"] or counts["UNSUPPORTED_BUILD"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
