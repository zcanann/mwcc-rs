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
    selected = sorted(identities, key=lambda identity: audit_rank(identity, seed, epoch))[
        : min(size, len(identities))
    ]
    return {
        "schema_version": 1,
        "kind": "simple_random_sample_without_replacement",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "seed": seed,
        "epoch": epoch,
        "population_size": len(identities),
        "configuration_ids": selected,
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
        f"representative audit: {len(audit['configuration_ids'])}/{audit['population_size']} "
        f"configurations -> {args.output}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
