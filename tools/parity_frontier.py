#!/usr/bin/env python3
"""Build a persistent parity work frontier from stable configuration results."""

from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence

from parity_dashboard import latest_observations, load_inventory, load_results
from parity_identity import configuration_id


PRIORITY = ("DIFF", "DEFER", "HARNESS")


def latest_any_tool(records: List[Dict[str, Any]]) -> Dict[str, Dict[str, Any]]:
    latest: Dict[str, Dict[str, Any]] = {}
    for record in records:
        identity = record["configuration_id"]
        if identity not in latest or record.get("observed_at", "") >= latest[identity].get("observed_at", ""):
            latest[identity] = record
    return latest


def rank(identity: str, seed: str, epoch: str, bucket: str) -> bytes:
    return hashlib.sha256(f"{seed}\0{epoch}\0{bucket}\0{identity}".encode()).digest()


def choose(
    identities: List[str], count: int, seed: str, epoch: str, bucket: str
) -> List[str]:
    return sorted(identities, key=lambda identity: rank(identity, seed, epoch, bucket))[:count]


def candidate_rows(inventory: Dict[str, Any], args: argparse.Namespace) -> List[Dict[str, Any]]:
    rows = []
    for row in inventory["translation_units"]:
        if not row["source_exists"]:
            continue
        if args.project and row["project"] not in args.project:
            continue
        if args.version and row["mw_version"] not in args.version:
            continue
        if args.language and row["language"] not in args.language:
            continue
        if args.matching_only and not row["matching"]:
            continue
        row["configuration_id"] = row.get("configuration_id") or configuration_id(row)
        rows.append(row)
    return rows


def build_frontier(
    rows: List[Dict[str, Any]],
    observations: Dict[str, Dict[str, Any]],
    args: argparse.Namespace,
    build_observations: Optional[Dict[str, Dict[str, Any]]] = None,
) -> Dict[str, Any]:
    build_observations = build_observations if build_observations is not None else observations
    universe = {row["configuration_id"] for row in rows}
    by_status: Dict[str, List[str]] = {}
    for identity in universe:
        observation = observations.get(identity)
        status = observation["status"] if observation else "UNTESTED"
        by_status.setdefault(status, []).append(identity)

    selected: List[str] = []
    selected_set: set[str] = set()
    audit_target = min(args.byte_audit, args.size, len(by_status.get("BYTE", [])))
    work_target = args.size - audit_target

    # One configuration is enough to probe whether a compiler build exists.
    # Reserve those probes so a large failure backlog cannot hide a version.
    probed_versions: List[str] = []
    for version in sorted({row["mw_version"] for row in rows}):
        version_rows = [row for row in rows if row["mw_version"] == version]
        if any(row["configuration_id"] in build_observations for row in version_rows):
            continue
        candidates = [row["configuration_id"] for row in version_rows]
        picked = choose(candidates, 1, args.seed, args.epoch, f"VERSION:{version}")
        if len(selected) + len(picked) > work_target:
            break
        selected.extend(picked)
        selected_set.update(picked)
        probed_versions.append(version)

    for status in PRIORITY:
        remaining = work_target - len(selected)
        if remaining <= 0:
            break
        candidates = [identity for identity in by_status.get(status, []) if identity not in selected_set]
        picked = choose(candidates, remaining, args.seed, args.epoch, status)
        selected.extend(picked)
        selected_set.update(picked)

    remaining = work_target - len(selected)
    if remaining > 0:
        candidates = [identity for identity in by_status.get("UNTESTED", []) if identity not in selected_set]
        picked = choose(candidates, remaining, args.seed, args.epoch, "UNTESTED")
        selected.extend(picked)
        selected_set.update(picked)

    byte_candidates = [identity for identity in by_status.get("BYTE", []) if identity not in selected_set]
    picked = choose(byte_candidates, audit_target, args.seed, args.epoch, "BYTE_AUDIT")
    selected.extend(picked)
    selected_set.update(picked)

    if len(selected) < args.size:
        remainder = [identity for identity in universe if identity not in selected_set]
        selected.extend(choose(remainder, args.size - len(selected), args.seed, args.epoch, "REMAINDER"))

    previous = Counter(
        observations[identity]["status"] if identity in observations else "UNTESTED"
        for identity in selected
    )
    return {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "seed": args.seed,
        "epoch": args.epoch,
        "universe_size": len(universe),
        "configuration_ids": selected,
        "probed_versions": probed_versions,
        "previous_status_counts": dict(sorted(previous.items())),
    }


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--result", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--size", type=int, default=256)
    parser.add_argument(
        "--byte-audit",
        type=int,
        default=0,
        help="old BYTE rows to retain in this failure-biased queue",
    )
    parser.add_argument("--seed", default="mwcc-frontier-v1")
    parser.add_argument("--epoch", default="0", help="change to rotate the frontier and BYTE audit")
    parser.add_argument(
        "--tool-fingerprint",
        help="reserve build probes not yet observed by this compiler/harness revision",
    )
    parser.add_argument("--project", action="append")
    parser.add_argument("--version", action="append")
    parser.add_argument("--language", choices=("c", "c++"), action="append")
    parser.add_argument("--matching-only", action="store_true")
    return parser.parse_args(argv)


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(argv)
    if args.size < 1 or args.byte_audit < 0:
        print("parity frontier: size must be positive and byte audit non-negative")
        return 2
    try:
        inventory = load_inventory(args.inventory)
        records = load_results(args.result)
        observations = latest_any_tool(records)
        build_observations = (
            latest_observations(records, args.tool_fingerprint)
            if args.tool_fingerprint is not None
            else observations
        )
        frontier = build_frontier(
            candidate_rows(inventory, args), observations, args, build_observations
        )
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(frontier, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"parity frontier: {error}")
        return 2
    print(
        f"frontier: {len(frontier['configuration_ids'])}/{frontier['universe_size']} configurations -> {args.output}"
    )
    print("previous statuses:", json.dumps(frontier["previous_status_counts"], sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
