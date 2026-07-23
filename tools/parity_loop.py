#!/usr/bin/env python3
"""Run one failure-focused reference-project parity iteration and report it."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from typing import List, Optional, Sequence, TextIO

from reference_parity import (
    harness_fingerprint,
    immutable_compiler_snapshot,
    result_cache_name,
)


INVENTORY_SCHEMA_VERSION = 5


def acquire_state_lock(state: Path) -> TextIO:
    """Exclusively own one parity state directory for this invocation.

    The frontier and fixed audit share manifests and append-only result caches.
    Two loops targeting the same state directory would duplicate expensive
    compilations and can interleave cache writes, so fail fast instead of
    pretending that concurrency is useful here. ``flock`` releases the lock
    automatically if the process exits or is interrupted.
    """

    path = state / ".parity-loop.lock"
    handle = path.open("a+", encoding="utf-8")
    try:
        fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError as error:
        handle.seek(0)
        owner = handle.read().strip() or "owner metadata unavailable"
        handle.close()
        raise RuntimeError(f"parity state is already active ({owner})") from error

    handle.seek(0)
    handle.truncate()
    json.dump(
        {
            "pid": os.getpid(),
            "started_at": datetime.now(timezone.utc).isoformat(),
        },
        handle,
        sort_keys=True,
    )
    handle.write("\n")
    handle.flush()
    return handle


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def persistent_compiler_image(source: Path, store: Path) -> tuple[Path, str]:
    """Persist and return one content-addressed compiler image.

    The parity runner also makes a private copy for each process, but pinning
    the input here ensures every runner launched by one loop invocation starts
    from the same bytes even if a concurrent Cargo build replaces ``source``.
    Keeping the image in the state directory also makes an interrupted audit
    explicitly resumable with ``--compiler <image> --no-build``.
    """

    temporary_directory, temporary_image, compiler_hash = immutable_compiler_snapshot(
        source
    )
    try:
        destination_directory = store / compiler_hash
        destination_directory.mkdir(parents=True, exist_ok=True)
        destination = destination_directory / source.name
        if destination.is_file():
            if sha256_file(destination) != compiler_hash:
                raise RuntimeError(f"corrupt compiler image: {destination}")
            return destination, compiler_hash

        with tempfile.NamedTemporaryFile(
            prefix=f".{source.name}.", dir=destination_directory, delete=False
        ) as output:
            staging = Path(output.name)
        try:
            shutil.copy2(temporary_image, staging)
            if sha256_file(staging) != compiler_hash:
                raise RuntimeError(f"compiler image changed while persisting: {source}")
            staging.replace(destination)
        finally:
            staging.unlink(missing_ok=True)
        return destination, compiler_hash
    finally:
        temporary_directory.cleanup()


def result_arguments(paths: List[Path]) -> List[str]:
    output: List[str] = []
    for path in paths:
        output.extend(("--result", str(path)))
    return output


def most_comparable_other_tool(
    paths: List[Path], current: str, comparison_ids: set[str]
) -> Optional[str]:
    """Choose the prior fingerprint with the most comparable observations.

    A newer focused probe must not displace a complete fixed audit as the
    baseline. Maximize distinct overlap with this invocation's selections,
    then use recency only to break equal-coverage ties.
    """

    observations: dict[str, dict[str, str]] = {}
    for path in paths:
        with path.open(encoding="utf-8") as source:
            for line in source:
                try:
                    record = json.loads(line)
                    tool = record["tool_fingerprint"]
                    identity = record["configuration_id"]
                    observed_at = record.get("observed_at", "")
                except (json.JSONDecodeError, KeyError):
                    continue
                if tool == current or (comparison_ids and identity not in comparison_ids):
                    continue
                by_identity = observations.setdefault(tool, {})
                by_identity[identity] = max(observed_at, by_identity.get(identity, ""))
    if not observations:
        return None
    return max(
        observations,
        key=lambda tool: (
            len(observations[tool]),
            max(observations[tool].values(), default=""),
            tool,
        ),
    )


def manifest_configuration_ids(path: Path) -> set[str]:
    document = json.loads(path.read_text(encoding="utf-8"))
    return set(document.get("configuration_ids", []))


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=Path("target/debug/mwcc"))
    parser.add_argument(
        "--no-build",
        action="store_true",
        help="use the selected compiler binary as-is instead of rebuilding the default debug compiler",
    )
    parser.add_argument(
        "--jobs",
        type=int,
        default=4,
        help="run independent reference configurations concurrently (default: 4)",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        help="set both work and audit timeouts (compatibility override)",
    )
    parser.add_argument(
        "--work-timeout",
        type=int,
        default=60,
        help="maximum seconds per work-queue configuration (default: 60)",
    )
    parser.add_argument(
        "--audit-timeout",
        type=int,
        default=300,
        help="maximum seconds per representative-audit configuration (default: 300)",
    )
    parser.add_argument("--reference-root", type=Path)
    parser.add_argument("--state-dir", type=Path, default=Path("target/reference-parity/frontier"))
    parser.add_argument(
        "--size",
        type=int,
        default=32,
        help="rotating nonpassing work budget (default: 32 configurations)",
    )
    parser.add_argument(
        "--byte-audit",
        type=int,
        default=0,
        help="old BYTE rows to spend work-frontier slots on (the fixed audit is the default regression sample)",
    )
    parser.add_argument("--audit-size", type=int, default=384)
    parser.add_argument("--audit-seed", default="mwcc-representative-audit-v1")
    parser.add_argument("--audit-epoch", default="0")
    parser.add_argument(
        "--audit-purpose",
        choices=("paired-panel", "fresh-holdout"),
        default="paired-panel",
        help="label the audit's statistical role (default: paired-panel)",
    )
    parser.add_argument("--seed", default="mwcc-frontier-v1")
    parser.add_argument("--epoch", default="0", help="change to rotate equally ranked work")
    parser.add_argument("--refresh-inventory", action="store_true")
    parser.add_argument("--rerun", action="store_true")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--work-only",
        action="store_true",
        help="explicit spelling of the default rotating failure-prioritized edit loop",
    )
    mode.add_argument(
        "--audit-only",
        action="store_true",
        help="run only the fixed representative audit (periodic measurement)",
    )
    mode.add_argument(
        "--with-audit",
        action="store_true",
        help="run both the rotating work queue and periodic fixed representative audit",
    )
    parser.add_argument(
        "--frontier-only",
        action="store_true",
        help="prepare selection manifests without compiling (legacy name)",
    )
    parser.add_argument("--version", action="append", help="limit to a compiler build (repeatable)")
    return parser.parse_args(argv)


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(argv)
    if args.jobs < 1:
        print("--jobs must be positive", file=sys.stderr)
        return 2
    work_timeout = args.timeout if args.timeout is not None else args.work_timeout
    audit_timeout = args.timeout if args.timeout is not None else args.audit_timeout
    if work_timeout < 1 or audit_timeout < 1:
        print("timeouts must be positive", file=sys.stderr)
        return 2
    run_audit = args.audit_only or args.with_audit
    run_frontier = not args.audit_only
    root = Path(__file__).resolve().parent.parent
    tools = root / "tools"
    compiler_source = (
        args.compiler if args.compiler.is_absolute() else root / args.compiler
    )
    default_compiler = root / "target/debug/mwcc"
    state = args.state_dir if args.state_dir.is_absolute() else root / args.state_dir
    inventory = state / "inventory.json"
    frontier = state / "frontier.json"
    audit = state / "audit.json"
    runs = state / "runs"
    snapshots = state / "snapshots"
    compiler_images = state / "compiler-images"
    state.mkdir(parents=True, exist_ok=True)
    try:
        state_lock = acquire_state_lock(state)
    except (OSError, RuntimeError) as error:
        print(error, file=sys.stderr)
        return 2
    runs.mkdir(exist_ok=True)
    snapshots.mkdir(exist_ok=True)
    compiler_images.mkdir(exist_ok=True)

    if not args.no_build and compiler_source.resolve() == default_compiler.resolve():
        build = subprocess.run(["cargo", "build", "-q", "-p", "mwcc"], cwd=root)
        if build.returncode:
            print("failed to build the default parity compiler", file=sys.stderr)
            return 2

    if not compiler_source.is_file():
        print(f"compiler not found: {compiler_source}", file=sys.stderr)
        return 2

    try:
        compiler, compiler_hash = persistent_compiler_image(
            compiler_source, compiler_images
        )
    except (OSError, RuntimeError) as error:
        print(f"compiler image failed: {error}", file=sys.stderr)
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

    harness_hash = harness_fingerprint(tools)
    fingerprint = f"{compiler_hash}:{harness_hash}"
    result = runs / result_cache_name(compiler_hash, harness_hash)
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
        "--tool-fingerprint",
        fingerprint,
        *filters,
        *result_arguments(previous_results),
    ]
    if run_frontier and subprocess.run(frontier_command).returncode:
        return 2
    audit_command = [
        sys.executable,
        str(tools / "parity_audit.py"),
        "--inventory",
        str(inventory),
        "--output",
        str(audit),
        "--size",
        str(args.audit_size),
        "--seed",
        args.audit_seed,
        "--epoch",
        args.audit_epoch,
        "--purpose",
        args.audit_purpose,
        *filters,
    ]
    if run_audit and subprocess.run(audit_command).returncode:
        return 2
    if args.frontier_only:
        return 0

    if run_audit:
        audit_run_command = [
            sys.executable,
            str(tools / "reference_parity.py"),
            "--inventory",
            str(inventory),
            "--compiler",
            str(compiler),
            "--selection",
            str(audit),
            "--cache",
            str(result),
            "--jobs",
            str(args.jobs),
            "--timeout",
            str(audit_timeout),
            "--configured-only",
            *filters,
        ]
        if args.rerun:
            audit_run_command.append("--rerun")
        run_status = subprocess.run(audit_run_command).returncode
        if run_status not in (0, 1):
            return run_status

    if run_frontier:
        frontier_run_command = [
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
            "--jobs",
            str(args.jobs),
            "--timeout",
            str(work_timeout),
            *filters,
        ]
        if args.rerun:
            frontier_run_command.append("--rerun")
        run_status = subprocess.run(frontier_run_command).returncode
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
        "--brief",
        *filters,
        *result_arguments(all_results),
    ]
    if run_audit:
        dashboard_command.extend(("--audit-selection", str(audit)))
    if run_frontier:
        dashboard_command.extend(("--frontier-selection", str(frontier)))
    comparison_ids: set[str] = set()
    if run_audit:
        comparison_ids.update(manifest_configuration_ids(audit))
    if run_frontier:
        comparison_ids.update(manifest_configuration_ids(frontier))
    baseline = most_comparable_other_tool(all_results, fingerprint, comparison_ids)
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
    state_lock.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
