#!/usr/bin/env python3
"""Run one failure-focused reference-project parity iteration and report it."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import sys
from typing import List, Optional, Sequence

from parity_identity import files_fingerprint


INVENTORY_SCHEMA_VERSION = 3


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def result_arguments(paths: List[Path]) -> List[str]:
    output: List[str] = []
    for path in paths:
        output.extend(("--result", str(path)))
    return output


def newest_other_tool(paths: List[Path], current: str) -> Optional[str]:
    newest: Optional[tuple[str, str]] = None
    for path in paths:
        with path.open(encoding="utf-8") as source:
            for line in source:
                try:
                    record = json.loads(line)
                    tool = record["tool_fingerprint"]
                    observed_at = record.get("observed_at", "")
                except (json.JSONDecodeError, KeyError):
                    continue
                if tool == current:
                    continue
                candidate = (observed_at, tool)
                if newest is None or candidate > newest:
                    newest = candidate
    return newest[1] if newest else None


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=Path("target/release/mwcc"))
    parser.add_argument("--reference-root", type=Path)
    parser.add_argument("--state-dir", type=Path, default=Path("target/reference-parity/frontier"))
    parser.add_argument("--size", type=int, default=256)
    parser.add_argument("--byte-audit", type=int, default=16)
    parser.add_argument("--seed", default="mwcc-frontier-v1")
    parser.add_argument("--epoch", default="0", help="change to rotate equally ranked work")
    parser.add_argument("--refresh-inventory", action="store_true")
    parser.add_argument("--rerun", action="store_true")
    parser.add_argument("--frontier-only", action="store_true")
    parser.add_argument("--version", action="append", help="limit to a compiler build (repeatable)")
    return parser.parse_args(argv)


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(argv)
    root = Path(__file__).resolve().parent.parent
    tools = root / "tools"
    compiler = args.compiler if args.compiler.is_absolute() else root / args.compiler
    state = args.state_dir if args.state_dir.is_absolute() else root / args.state_dir
    inventory = state / "inventory.json"
    frontier = state / "frontier.json"
    runs = state / "runs"
    snapshots = state / "snapshots"
    state.mkdir(parents=True, exist_ok=True)
    runs.mkdir(exist_ok=True)
    snapshots.mkdir(exist_ok=True)

    if not compiler.is_file():
        print(f"compiler not found: {compiler}", file=sys.stderr)
        return 2

    inventory_stale = True
    if inventory.is_file():
        try:
            inventory_stale = json.loads(inventory.read_text(encoding="utf-8")).get(
                "schema_version"
            ) != INVENTORY_SCHEMA_VERSION
        except (OSError, json.JSONDecodeError):
            inventory_stale = True
    if args.refresh_inventory or args.reference_root is not None or inventory_stale:
        command = [sys.executable, str(tools / "reference_inventory.py")]
        if args.reference_root is not None:
            command.append(str(args.reference_root))
        generated = subprocess.run(command, text=True, capture_output=True)
        if not generated.stdout:
            print(generated.stderr.strip() or "inventory generation failed", file=sys.stderr)
            return 2
        try:
            document = json.loads(generated.stdout)
        except json.JSONDecodeError as error:
            print(f"inventory generation returned invalid JSON: {error}", file=sys.stderr)
            return 2
        inventory.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        if generated.returncode:
            print("warning: inventory contains project capture errors", file=sys.stderr)

    compiler_hash = sha256_file(compiler)
    harness_hash = files_fingerprint(
        (tools / "refctx.sh", tools / "reference_parity.py", tools / "parity_identity.py")
    )
    fingerprint = f"{compiler_hash}:{harness_hash}"
    result = runs / f"{compiler_hash[:16]}-{harness_hash[:16]}.jsonl"
    previous_results = sorted(runs.glob("*.jsonl"))

    filters: List[str] = []
    for version in args.version or []:
        filters.extend(("--version", version))
    frontier_command = [
        sys.executable,
        str(tools / "parity_frontier.py"),
        "--inventory",
        str(inventory),
        "--output",
        str(frontier),
        "--size",
        str(args.size),
        "--byte-audit",
        str(args.byte_audit),
        "--seed",
        args.seed,
        "--epoch",
        args.epoch,
        *filters,
        *result_arguments(previous_results),
    ]
    if subprocess.run(frontier_command).returncode:
        return 2
    if args.frontier_only:
        return 0

    run_command = [
        sys.executable,
        str(tools / "reference_parity.py"),
        "--inventory",
        str(inventory),
        "--compiler",
        str(compiler),
        "--selection",
        str(frontier),
        "--cache",
        str(result),
        *filters,
    ]
    if args.rerun:
        run_command.append("--rerun")
    run_status = subprocess.run(run_command).returncode
    if run_status not in (0, 1):
        return run_status

    all_results = sorted(runs.glob("*.jsonl"))
    dashboard_command = [
        sys.executable,
        str(tools / "parity_dashboard.py"),
        "--inventory",
        str(inventory),
        "--tool-fingerprint",
        fingerprint,
        *filters,
        *result_arguments(all_results),
    ]
    baseline = newest_other_tool(all_results, fingerprint)
    if baseline is not None:
        dashboard_command.extend(("--baseline-tool-fingerprint", baseline))
        for path in all_results:
            dashboard_command.extend(("--baseline-result", str(path)))
    if subprocess.run(dashboard_command).returncode:
        return 2

    json_command = [*dashboard_command, "--json"]
    captured = subprocess.run(json_command, text=True, capture_output=True)
    if captured.returncode:
        print(captured.stdout + captured.stderr, file=sys.stderr)
        return 2
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    snapshot = snapshots / f"{stamp}-{compiler_hash[:12]}-{harness_hash[:12]}.json"
    snapshot.write_text(captured.stdout, encoding="utf-8")
    print(f"snapshot: {snapshot}")
    print("nonpassing results are expected; only harness/tool failures make this command fail")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
