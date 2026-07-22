#!/usr/bin/env python3
"""Run resumable, exact-flag MWCC A/B tests over the reference inventory."""

from __future__ import annotations

import argparse
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any, Dict, Iterable, List, Optional, Sequence, Tuple

from parity_identity import configuration_id, files_fingerprint, observation_id


STATUSES = (
    "BYTE",
    "DIFF",
    "DEFER",
    "HARNESS",
    "MISSING_DEPENDENCY",
    "INVALID_CONFIGURATION",
    "UNSUPPORTED_BUILD",
)


ACTIVE_ROW_PROCESSES: set[int] = set()
ACTIVE_ROW_PROCESSES_LOCK = threading.Lock()

HARNESS_INPUTS = (
    "refctx.sh",
    "refctx_pch.py",
    "refctx_pragmas.py",
    "reference_parity.py",
    "parity_identity.py",
    "decompctx_runner.py",
    "object_code_metrics.py",
)


def register_active_row_process(process_group: int) -> None:
    with ACTIVE_ROW_PROCESSES_LOCK:
        ACTIVE_ROW_PROCESSES.add(process_group)


def unregister_active_row_process(process_group: int) -> None:
    with ACTIVE_ROW_PROCESSES_LOCK:
        ACTIVE_ROW_PROCESSES.discard(process_group)


def terminate_active_row_processes() -> None:
    """Kill every in-flight refctx process group owned by this runner."""

    with ACTIVE_ROW_PROCESSES_LOCK:
        process_groups = tuple(ACTIVE_ROW_PROCESSES)
    for process_group in process_groups:
        try:
            os.killpg(process_group, signal.SIGKILL)
        except ProcessLookupError:
            pass


def install_termination_handlers() -> None:
    """Make an interrupted batch stop its row subprocesses before unwinding."""

    def terminate(signum, _frame):
        terminate_active_row_processes()
        if signum == signal.SIGINT:
            raise KeyboardInterrupt
        raise SystemExit(128 + signum)

    signal.signal(signal.SIGINT, terminate)
    signal.signal(signal.SIGTERM, terminate)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def result_cache_name(compiler_hash: str, harness_hash: str) -> str:
    """Name a result cache from both independently changing tool inputs."""

    return f"{compiler_hash[:16]}-{harness_hash[:16]}.jsonl"


def immutable_compiler_snapshot(source: Path) -> Tuple[tempfile.TemporaryDirectory, Path, str]:
    """Copy one stable compiler image and fingerprint exactly that image.

    A long parity batch must not observe a later `cargo build` replacing the
    executable between rows. Hash-before/hash-after plus the copied hash also
    detects replacement during the snapshot itself.
    """

    directory = tempfile.TemporaryDirectory(prefix="mwcc-parity-compiler-")
    snapshot = Path(directory.name) / source.name
    for _ in range(3):
        before = sha256_file(source)
        shutil.copy2(source, snapshot)
        after = sha256_file(source)
        copied = sha256_file(snapshot)
        if before == after == copied:
            return directory, snapshot, copied
    directory.cleanup()
    raise RuntimeError(f"compiler changed repeatedly while snapshotting: {source}")


def harness_fingerprint(script_dir: Path) -> str:
    """Hash every executable input that can change a row classification."""

    return files_fingerprint(script_dir / name for name in HARNESS_INPUTS)


def immutable_harness_snapshot(
    script_dir: Path,
) -> Tuple[tempfile.TemporaryDirectory, Path, str]:
    """Copy one stable, self-contained harness image for a complete batch.

    `refctx.sh` resolves its helpers relative to `tools/`, so the snapshot
    preserves that tiny repository layout. Hash-before/hash-after plus the copy
    hash prevents a concurrent edit from producing a mixed harness image.
    """

    directory = tempfile.TemporaryDirectory(prefix="mwcc-parity-harness-")
    snapshot_tools = Path(directory.name) / "tools"
    snapshot_tools.mkdir()
    for _ in range(3):
        before = harness_fingerprint(script_dir)
        for name in HARNESS_INPUTS:
            shutil.copy2(script_dir / name, snapshot_tools / name)
        after = harness_fingerprint(script_dir)
        copied = harness_fingerprint(snapshot_tools)
        if before == after == copied:
            return directory, snapshot_tools / "refctx.sh", copied
    directory.cleanup()
    raise RuntimeError("harness changed repeatedly while snapshotting")


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


def row_configuration_id(row: Dict[str, Any]) -> str:
    return row.get("configuration_id") or configuration_id(row)


def load_selection(path: Path) -> set[str]:
    text = path.read_text(encoding="utf-8")
    try:
        document = json.loads(text)
    except json.JSONDecodeError:
        return {line.strip() for line in text.splitlines() if line.strip()}
    if isinstance(document, dict):
        document = document.get("configuration_ids", [])
    if not isinstance(document, list) or not all(isinstance(item, str) for item in document):
        raise ValueError("selection must be a JSON list/object of configuration IDs")
    return set(document)


def selection_is_probability_sample(path: Path) -> bool:
    """Identify audit manifests whose membership must not be post-filtered."""

    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return False
    return (
        isinstance(document, dict)
        and document.get("kind") == "simple_random_sample_without_replacement"
        and isinstance(document.get("sample_configuration_ids"), list)
    )


def stable_sample(rows: List[Dict[str, Any]], size: int, seed: str) -> List[Dict[str, Any]]:
    if size <= 0 or size >= len(rows):
        return rows

    def rank(row: Dict[str, Any]) -> bytes:
        identity = row_configuration_id(row)
        return hashlib.sha256(f"{seed}\0{identity}".encode()).digest()

    return sorted(rows, key=rank)[:size]


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
    if args.selection is not None:
        selection = load_selection(args.selection)
        selected = [row for row in selected if row_configuration_id(row) in selection]
    if args.sample_size:
        selected = stable_sample(selected, args.sample_size, args.sample_seed)
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
    return next(
        (line for line in output.splitlines() if not line.startswith("PARITY_META ")),
        "",
    )


def parity_metadata(output: str) -> Dict[str, str]:
    """Extract machine-readable evidence provenance from refctx output."""

    metadata: Dict[str, str] = {}
    for line in output.splitlines():
        if not line.startswith("PARITY_META "):
            continue
        for field in line.removeprefix("PARITY_META ").split():
            key, separator, value = field.partition("=")
            if separator and key and value:
                metadata[key] = value
    return metadata


def code_verdict(output: str, object_status: str) -> Optional[str]:
    """Return the independently measured code+text-relocation result.

    Whole-object equality implies code equality. Other object outcomes count
    only when refctx emitted an explicit same-flags code projection; parser,
    debug, and harness failures otherwise remain unmeasured.
    """

    for line in output.splitlines():
        for result in ("BYTE", "DIFF", "DEFER", "EMPTY"):
            if line.startswith(f"CODE {result}"):
                return result
    return "BYTE" if object_status == "BYTE" else None


def classify(output: str, returncode: int) -> str:
    first = verdict_line(output)
    for status in ("BYTE", "DIFF", "DEFER", "MISSING_DEPENDENCY", "INVALID_CONFIGURATION"):
        if first.startswith(status):
            return status
    return "HARNESS" if returncode != 0 or first else "HARNESS"


def run_row(
    row: Dict[str, Any],
    reference_root: Path,
    refctx: Path,
    compiler: Path,
    timeout: int,
    code_projection: bool,
    configured_only: bool = False,
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
    environment["REFCTX_CODE_PROJECTION"] = "1" if code_projection else "0"
    environment["REFCTX_CONFIGURED_ONLY"] = "1" if configured_only else "0"
    process = subprocess.Popen(
        command,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
        start_new_session=True,
    )
    register_active_row_process(process.pid)
    try:
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            # refctx is a shell pipeline. Killing only that shell leaves the actual
            # compiler running and every timed-out audit row continues consuming a
            # core. Give each row its own session and terminate the entire process
            # group before returning the timeout observation.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.communicate()
            return "HARNESS", f"timed out after {timeout}s"
    finally:
        unregister_active_row_process(process.pid)
    output = "\n".join(part.strip() for part in (stdout, stderr) if part.strip())
    return classify(output, process.returncode), output


def bounded_completion_order(rows, executor, observe, jobs):
    """Yield finished observations while keeping at most ``jobs`` in flight.

    A census can contain tens of thousands of rows. Submitting all of them at
    once wastes memory, and waiting for inventory order can strand already
    completed evidence behind one giant TU. Completion-order cache writes make
    interruption genuinely resumable without changing row classification.
    """

    indexed = iter(enumerate(rows, 1))
    pending = {}

    def submit_one() -> bool:
        try:
            source_index, row = next(indexed)
        except StopIteration:
            return False
        pending[executor.submit(observe, row)] = (source_index, row)
        return True

    for _ in range(jobs):
        if not submit_one():
            break
    while pending:
        finished, _ = wait(pending, return_when=FIRST_COMPLETED)
        for future in finished:
            source_index, row = pending.pop(future)
            yield source_index, row, future.result()
            submit_one()


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
    parser.add_argument(
        "--code-projection",
        action="store_true",
        help="retry debug-deferred rows with -sym off for diagnostic code metrics",
    )
    parser.add_argument(
        "--configured-only",
        action="store_true",
        help="stop after the authoritative configured-source A/B result when available",
    )
    parser.add_argument(
        "--jobs",
        type=int,
        default=1,
        help="compile this many independent configurations concurrently (default: 1)",
    )
    parser.add_argument("--selection", type=Path, help="run stable configuration IDs from a frontier manifest")
    parser.add_argument("--write-selection", type=Path, help="write the selected stable configuration IDs")
    parser.add_argument("--sample-size", type=int, default=0, help="select a deterministic hash-ranked sample")
    parser.add_argument("--sample-seed", default="mwcc-parity-v1", help="seed for deterministic sampling")
    parser.add_argument("--rerun", action="store_true", help="ignore cached results")
    parser.add_argument("--list", action="store_true", help="list selected rows without compiling")
    return parser.parse_args(argv)


def main() -> int:
    args = parse_args()
    install_termination_handlers()
    if args.shard_count < 1 or not 0 <= args.shard_index < args.shard_count:
        print("invalid shard index/count", file=sys.stderr)
        return 2
    if args.jobs < 1:
        print("--jobs must be positive", file=sys.stderr)
        return 2
    if args.selection is not None and args.sample_size:
        print("--selection and --sample-size are mutually exclusive", file=sys.stderr)
        return 2
    if (
        args.selection is not None
        and args.matching_only
        and selection_is_probability_sample(args.selection)
    ):
        print(
            "--matching-only cannot post-filter a probability-sample selection; "
            "run the fixed audit without it",
            file=sys.stderr,
        )
        return 2
    script_dir = Path(__file__).resolve().parent
    root = script_dir.parent
    compiler = args.compiler if args.compiler.is_absolute() else root / args.compiler
    if not compiler.is_file():
        print(f"compiler not found: {compiler} (build it with cargo build --release -p mwcc)", file=sys.stderr)
        return 2

    try:
        inventory = load_inventory(args, script_dir)
    except (OSError, RuntimeError, json.JSONDecodeError) as error:
        print(f"inventory failed: {error}", file=sys.stderr)
        return 2
    rows = selected_rows(inventory["translation_units"], args)
    if args.write_selection is not None:
        args.write_selection.parent.mkdir(parents=True, exist_ok=True)
        selection = {
            "schema_version": 1,
            "sample_seed": args.sample_seed if args.sample_size else None,
            "configuration_ids": [row_configuration_id(row) for row in rows],
        }
        args.write_selection.write_text(
            json.dumps(selection, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    if args.list:
        for row in rows:
            print(
                f'{row["project"]}\t{row["variant"]}\t{row["mw_version"]}\t'
                f'{row["language"]}\t{row["source"]}\t{row_configuration_id(row)}'
            )
        print(f"== {len(rows)} selected translation-unit configurations ==")
        return 0

    try:
        compiler_snapshot, compiler, compiler_hash = immutable_compiler_snapshot(compiler)
        harness_snapshot, refctx, harness_hash = immutable_harness_snapshot(script_dir)
    except (OSError, RuntimeError) as error:
        print(f"tool snapshot failed: {error}", file=sys.stderr)
        return 2
    fingerprint = compiler_hash + ":" + harness_hash
    cache = args.cache
    if cache is None:
        cache = root / "target" / "reference-parity" / result_cache_name(
            compiler_hash, harness_hash
        )
    cache.parent.mkdir(parents=True, exist_ok=True)
    lock_path = cache.with_suffix(cache.suffix + ".lock")
    cache_lock = lock_path.open("a+", encoding="utf-8")
    try:
        fcntl.flock(cache_lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        print(f"waiting for active cache writer: {cache}", file=sys.stderr)
        fcntl.flock(cache_lock.fileno(), fcntl.LOCK_EX)
    cached = {} if args.rerun else load_cache(cache)
    build_support: Dict[str, Tuple[bool, str]] = {
        build: build_supported(compiler, build)
        for build in dict.fromkeys(
            row["mw_version"]
            for row in rows
            if observation_id(row_configuration_id(row), fingerprint) not in cached
        )
    }
    counts = {status: 0 for status in STATUSES}
    code_counts = {status: 0 for status in ("BYTE", "DIFF", "DEFER", "EMPTY")}
    code_unmeasured = 0
    reused = 0

    def observe(row: Dict[str, Any]) -> Tuple[Dict[str, Any], bool]:
        config_identity = row_configuration_id(row)
        identity = observation_id(config_identity, fingerprint)
        if identity in cached:
            return cached[identity], True

        observed_at = datetime.now(timezone.utc).isoformat()
        row_started = time.monotonic()
        build = row["mw_version"]
        supported, support_detail = build_support[build]
        if supported:
            status, detail = run_row(
                row,
                Path(inventory["reference_root"]),
                refctx,
                compiler,
                args.timeout,
                args.code_projection,
                args.configured_only,
            )
        else:
            status, detail = "UNSUPPORTED_BUILD", support_detail
        return (
            {
                "id": identity,
                "configuration_id": config_identity,
                "tool_fingerprint": fingerprint,
                "compiler_sha256": compiler_hash,
                "harness_sha256": harness_hash,
                "observed_at": observed_at,
                "elapsed_seconds": round(time.monotonic() - row_started, 6),
                "status": status,
                "project": row["project"],
                "variant": row["variant"],
                "source": row["source"],
                "mw_version": row["mw_version"],
                "language": row["language"],
                "matching": row["matching"],
                "source_sha256": row.get("source_sha256"),
                "output": detail,
                "evidence": parity_metadata(detail),
            },
            False,
        )

    with cache.open("a", encoding="utf-8") as cache_output, ThreadPoolExecutor(
        max_workers=args.jobs
    ) as executor:
        # Results are appended only by this main thread, so JSONL records cannot
        # interleave even though expensive rows complete out of inventory order.
        for completed, (source_index, row, (record, was_cached)) in enumerate(
            bounded_completion_order(rows, executor, observe, args.jobs), 1
        ):
            status = record["status"]
            detail = record.get("output", "")
            if was_cached:
                reused += 1
            else:
                cache_output.write(json.dumps(record, sort_keys=True) + "\n")
                cache_output.flush()
            counts[status] = counts.get(status, 0) + 1
            code_status = code_verdict(detail, status)
            if code_status is None:
                code_unmeasured += 1
            else:
                code_counts[code_status] += 1
            first_detail = verdict_line(detail)
            print(
                f'[{completed}/{len(rows)}; row {source_index}] {status:<17} {row["project"]} '
                f'{row["variant"]} {row["mw_version"]} {row["source"]} — {first_detail}'
            )

    summary = " / ".join(f"{status} {counts.get(status, 0)}" for status in STATUSES)
    print(f"== {len(rows)} configurations: {summary} / cached {reused} ==")
    code_measured = code_counts["BYTE"] + code_counts["DIFF"]
    print(
        f"layers: whole-object exact {counts['BYTE']}/{len(rows)} configured; "
        f"code exact {code_counts['BYTE']}/{code_measured} measured, "
        f"wrong {code_counts['DIFF']}/{code_measured}, "
        f"projection-deferred {code_counts['DEFER']}, empty {code_counts['EMPTY']}, "
        f"unmeasured {code_unmeasured}"
    )
    print(f"cache: {cache}")
    fcntl.flock(cache_lock.fileno(), fcntl.LOCK_UN)
    cache_lock.close()
    return 1 if any(
        counts[status]
        for status in (
            "DIFF",
            "HARNESS",
            "MISSING_DEPENDENCY",
            "INVALID_CONFIGURATION",
            "UNSUPPORTED_BUILD",
        )
    ) else 0


if __name__ == "__main__":
    raise SystemExit(main())
