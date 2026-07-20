#!/usr/bin/env python3
"""Select a fixed, representative audit sample from the parity universe."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence

from parity_dashboard import load_inventory
from parity_identity import configuration_id


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


def audit_rank(identity: str, seed: str, epoch: str) -> bytes:
    return hashlib.sha256(f"{seed}\0{epoch}\0{identity}".encode()).digest()


def build_audit(rows: List[Dict[str, Any]], size: int, seed: str, epoch: str) -> Dict[str, Any]:
    identities = sorted({row["configuration_id"] for row in rows})
    sample = sorted(identities, key=lambda identity: audit_rank(identity, seed, epoch))[
        : min(size, len(identities))
    ]
    rows_by_version: Dict[str, List[str]] = {}
    row_by_identity = {row["configuration_id"]: row for row in rows}
    for row in rows:
        rows_by_version.setdefault(row["mw_version"], []).append(row["configuration_id"])
    sample_set = set(sample)
    version_coverage: Dict[str, str] = {}
    sentinels: List[str] = []
    for version, version_identities in sorted(rows_by_version.items()):
        represented = sorted(sample_set & set(version_identities))
        if represented:
            version_coverage[version] = represented[0]
            continue
        # A sentinel is a build-coverage canary, not part of the estimator.
        # Prefer an authored matching source and then the smallest context so
        # a giant, harness-fragile TU cannot obscure whether the compiler build
        # itself works. Hash ranking only breaks equal-cost ties deterministically.
        sentinel = min(
            set(version_identities),
            key=lambda identity: (
                not row_by_identity[identity].get("matching", False),
                not row_by_identity[identity].get("source_has_non_whitespace", True),
                row_by_identity[identity].get("source_size_bytes", 1 << 62),
                audit_rank(identity, f"{seed}\0VERSION\0{version}", epoch),
            ),
        )
        version_coverage[version] = sentinel
        sentinels.append(sentinel)
    return {
        "schema_version": 2,
        "kind": "simple_random_sample_without_replacement",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "seed": seed,
        "epoch": epoch,
        "population_size": len(identities),
        # Execution is the statistically representative sample plus only the
        # sentinels needed to exercise compiler identities missed by chance.
        "configuration_ids": sample + sentinels,
        "sample_configuration_ids": sample,
        "version_coverage": version_coverage,
        "version_sentinel_configuration_ids": sentinels,
    }


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--size", type=int, default=384)
    parser.add_argument("--seed", default="mwcc-representative-audit-v1")
    parser.add_argument("--epoch", default="0", help="change deliberately to rotate the fixed audit")
    parser.add_argument("--project", action="append")
    parser.add_argument("--version", action="append")
    parser.add_argument("--language", choices=("c", "c++"), action="append")
    parser.add_argument("--matching-only", action="store_true")
    return parser.parse_args(argv)


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(argv)
    if args.size < 1:
        print("parity audit: size must be positive")
        return 2
    try:
        inventory = load_inventory(args.inventory)
        audit = build_audit(candidate_rows(inventory, args), args.size, args.seed, args.epoch)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(audit, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"parity audit: {error}")
        return 2
    print(
        f"representative audit: {len(audit['sample_configuration_ids'])}/"
        f"{audit['population_size']} sample configurations + "
        f"{len(audit['version_sentinel_configuration_ids'])} version sentinels -> {args.output}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
