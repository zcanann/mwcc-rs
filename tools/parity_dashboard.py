#!/usr/bin/env python3
"""Report reference parity with explicit denominators and untested coverage."""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import re
from typing import Any, Dict, Iterable, List, Optional, Sequence

from parity_identity import configuration_id


STATUSES = ("BYTE", "DIFF", "DEFER", "HARNESS", "UNSUPPORTED_BUILD", "UNTESTED")


def normalize_reason(reason: str) -> str:
    reason = re.sub(r"/[^ ]+/refctx\.[^/ ]+/(?:ours/)?ctx(?:\.cpp|\.c)?", "<context>", reason)
    reason = re.sub(r'Identifier\("[^"]+"\)', "Identifier(…)", reason)
    reason = re.sub(r"member '[^']+' on a non-struct-pointer base", "member access on a non-struct-pointer base", reason)
    reason = re.sub(r"struct '[^']+' is not declared", "struct is not declared", reason)
    reason = re.sub(r"0x[0-9a-fA-F]+", "<candidate>", reason)
    return reason


def failure_reason(record: Dict[str, Any]) -> str:
    status = record["status"]
    output = record.get("output", "")
    lines = output.splitlines()
    if status == "DIFF":
        return "object bytes differ"
    if status == "UNSUPPORTED_BUILD":
        return f"compiler build is unsupported: {record.get('mw_version', '<unknown>')}"
    if status == "DEFER":
        first = next((line for line in lines if line.startswith("DEFER")), lines[0] if lines else "deferred")
        return normalize_reason(first.rpartition(" — ")[2])
    if status == "HARNESS":
        if output.startswith("decompctx failed"):
            return "decompctx failed"
        if output.startswith("timed out"):
            return output
        unknown = next((line.strip("# ") for line in lines if "Unknown option" in line), None)
        if unknown:
            return f"reference compiler: {unknown}"
        for index, line in enumerate(lines):
            if "Error:" not in line:
                continue
            for detail in lines[index + 1 :]:
                detail = detail.strip("# ")
                if not detail or set(detail) <= set("^~_-"):
                    continue
                if detail.startswith("Too many errors"):
                    break
                if detail.startswith("undefined identifier"):
                    return "reference compiler: undefined identifier"
                return normalize_reason(f"reference compiler: {detail}")
        return normalize_reason(lines[0] if lines else "harness failed")
    return status.lower()


def blocker_breakdown(
    rows: List[Dict[str, Any]],
    observations: Dict[str, Dict[str, Any]],
    unsupported_versions: set[str],
) -> List[Dict[str, Any]]:
    grouped: Dict[tuple[str, str], Dict[str, Any]] = {}
    for row in rows:
        observation = observations.get(row["configuration_id"])
        if observation is None and row["mw_version"] in unsupported_versions:
            observation = {
                "status": "UNSUPPORTED_BUILD",
                "mw_version": row["mw_version"],
                "output": f"compiler build is unsupported: {row['mw_version']}",
            }
        if observation is None or observation["status"] == "BYTE":
            continue
        key = (observation["status"], failure_reason(observation))
        entry = grouped.setdefault(
            key,
            {"status": key[0], "reason": key[1], "count": 0, "examples": []},
        )
        entry["count"] += 1
        if len(entry["examples"]) < 3:
            entry["examples"].append(f'{row["project"]}/{row["source"]}')
    return sorted(grouped.values(), key=lambda entry: (-entry["count"], entry["status"], entry["reason"]))


def load_inventory(path: Path) -> Dict[str, Any]:
    with path.open(encoding="utf-8") as source:
        return json.load(source)


def load_results(paths: Iterable[Path]) -> List[Dict[str, Any]]:
    records: List[Dict[str, Any]] = []
    for path in paths:
        with path.open(encoding="utf-8") as source:
            for line_number, line in enumerate(source, 1):
                try:
                    record = json.loads(line)
                except json.JSONDecodeError as error:
                    raise ValueError(f"{path}:{line_number}: {error}") from error
                if record.get("configuration_id") and record.get("tool_fingerprint"):
                    records.append(record)
    return records


def select_tool(records: List[Dict[str, Any]], requested: Optional[str]) -> Optional[str]:
    if requested:
        matches = sorted(
            {record["tool_fingerprint"] for record in records if record["tool_fingerprint"].startswith(requested)}
        )
        if len(matches) != 1:
            raise ValueError(f"tool fingerprint prefix matched {len(matches)} runs")
        return matches[0]
    if not records:
        return None
    newest = max(records, key=lambda record: record.get("observed_at", ""))
    return newest["tool_fingerprint"]


def latest_observations(
    records: List[Dict[str, Any]], tool_fingerprint: Optional[str]
) -> Dict[str, Dict[str, Any]]:
    latest: Dict[str, Dict[str, Any]] = {}
    for record in records:
        if tool_fingerprint is not None and record["tool_fingerprint"] != tool_fingerprint:
            continue
        identity = record["configuration_id"]
        if identity not in latest or record.get("observed_at", "") >= latest[identity].get("observed_at", ""):
            latest[identity] = record
    return latest


def load_selection(path: Optional[Path]) -> Optional[set[str]]:
    if path is None:
        return None
    document = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(document, dict):
        document = document.get("configuration_ids", [])
    if not isinstance(document, list):
        raise ValueError("selection must contain configuration_ids")
    return set(document)


def filtered_rows(
    inventory: Dict[str, Any], args: argparse.Namespace
) -> List[Dict[str, Any]]:
    selection = load_selection(args.selection)
    rows = []
    for row in inventory["translation_units"]:
        identity = row.get("configuration_id") or configuration_id(row)
        row["configuration_id"] = identity
        if args.project and row["project"] not in args.project:
            continue
        if args.version and row["mw_version"] not in args.version:
            continue
        if args.language and row["language"] not in args.language:
            continue
        if args.matching_only and not row["matching"]:
            continue
        if selection is not None and identity not in selection:
            continue
        rows.append(row)
    return rows


def status_counts(
    rows: List[Dict[str, Any]],
    observations: Dict[str, Dict[str, Any]],
    unsupported_versions: set[str],
) -> Counter[str]:
    counts: Counter[str] = Counter()
    for row in rows:
        if not row["source_exists"]:
            continue
        observation = observations.get(row["configuration_id"])
        if observation is not None:
            status = observation["status"]
        elif row["mw_version"] in unsupported_versions:
            status = "UNSUPPORTED_BUILD"
        else:
            status = "UNTESTED"
        counts[status] += 1
    return counts


def breakdown(
    rows: List[Dict[str, Any]],
    observations: Dict[str, Dict[str, Any]],
    unsupported_versions: set[str],
    key: str,
) -> List[Dict[str, Any]]:
    groups: Dict[str, List[Dict[str, Any]]] = defaultdict(list)
    for row in rows:
        if row["source_exists"]:
            groups[str(row[key])].append(row)
    output = []
    for name in sorted(groups):
        counts = status_counts(groups[name], observations, unsupported_versions)
        output.append({"name": name, "total": len(groups[name]), **{status: counts[status] for status in STATUSES}})
    return output


def snapshot(
    inventory: Dict[str, Any],
    rows: List[Dict[str, Any]],
    observations: Dict[str, Dict[str, Any]],
    tool: Optional[str],
    unsupported_versions: Optional[set[str]] = None,
    source_projects: Optional[set[str]] = None,
) -> Dict[str, Any]:
    unsupported_versions = unsupported_versions or set()
    existing = [row for row in rows if row["source_exists"]]
    missing = len(rows) - len(existing)
    counts = status_counts(rows, observations, unsupported_versions)
    observed = sum(row["configuration_id"] in observations for row in existing)
    classified = len(existing) - counts["UNTESTED"]
    evaluable = counts["BYTE"] + counts["DIFF"] + counts["DEFER"]
    projects = source_projects or {row["project"] for row in rows}
    project_entries = [project for project in inventory["projects"] if project["name"] in projects]
    unmapped = sum(len(project.get("unmapped_sources", [])) for project in project_entries)
    discovered = sum(project.get("source_count", 0) for project in project_entries)
    mapped = sum(project.get("mapped_source_count", 0) for project in project_entries)
    return {
        "tool_fingerprint": tool,
        "configured": len(rows),
        "existing": len(existing),
        "missing_source": missing,
        "observed": observed,
        "classified": classified,
        "evaluable": evaluable,
        "source_inventory": {"discovered": discovered, "mapped": mapped, "unmapped": unmapped},
        "statuses": {status: counts[status] for status in STATUSES},
        "rates": {
            "byte_of_existing": counts["BYTE"] / len(existing) if existing else 0.0,
            "byte_of_evaluable": counts["BYTE"] / evaluable if evaluable else 0.0,
            "observed_of_existing": observed / len(existing) if existing else 0.0,
            "classified_of_existing": classified / len(existing) if existing else 0.0,
        },
        "by_language": breakdown(rows, observations, unsupported_versions, "language"),
        "by_version": breakdown(rows, observations, unsupported_versions, "mw_version"),
        "by_project": breakdown(rows, observations, unsupported_versions, "project"),
        "blockers": blocker_breakdown(existing, observations, unsupported_versions),
    }


def delta(
    current: Dict[str, Dict[str, Any]], baseline: Dict[str, Dict[str, Any]], universe: set[str]
) -> Dict[str, Any]:
    transitions: Counter[str] = Counter()
    for identity in universe & current.keys() & baseline.keys():
        before = baseline[identity]["status"]
        after = current[identity]["status"]
        if before != after:
            transitions[f"{before}->{after}"] += 1
    return {
        "common_observations": len(universe & current.keys() & baseline.keys()),
        "byte_gained": sum(count for transition, count in transitions.items() if transition.endswith("->BYTE")),
        "byte_lost": sum(count for transition, count in transitions.items() if transition.startswith("BYTE->")),
        "transitions": dict(sorted(transitions.items())),
    }


def print_breakdown(title: str, rows: List[Dict[str, Any]]) -> None:
    print(f"\n{title}")
    print(f"{'name':28} {'total':>7} {'BYTE':>7} {'DIFF':>7} {'DEFER':>7} {'HARNESS':>8} {'UNSUP':>7} {'UNTEST':>8}")
    for row in rows:
        print(
            f"{row['name'][:28]:28} {row['total']:7d} {row['BYTE']:7d} {row['DIFF']:7d} "
            f"{row['DEFER']:7d} {row['HARNESS']:8d} {row['UNSUPPORTED_BUILD']:7d} {row['UNTESTED']:8d}"
        )


def print_snapshot(report: Dict[str, Any], delta_report: Optional[Dict[str, Any]]) -> None:
    tool = report["tool_fingerprint"]
    print("== reference parity snapshot ==")
    print(f"tool: {(tool or '<no observations>')[:24]}")
    print(
        f"sources: {report['source_inventory']['mapped']}/{report['source_inventory']['discovered']} mapped "
        f"({report['source_inventory']['unmapped']} unmapped)"
    )
    print(
        f"configurations: {report['existing']} existing / {report['configured']} configured "
        f"({report['missing_source']} missing source)"
    )
    print(
        f"classified: {report['classified']}/{report['existing']} "
        f"({report['rates']['classified_of_existing']:.1%}); "
        f"direct configuration compilations: {report['observed']}"
    )
    print()
    print(f"{'status':18} {'count':>8} {'% existing':>12}")
    for status in STATUSES:
        count = report["statuses"][status]
        rate = count / report["existing"] if report["existing"] else 0.0
        print(f"{status:18} {count:8d} {rate:11.1%}")
    print(
        f"\nproven exact parity (lower bound): {report['statuses']['BYTE']}/{report['existing']} existing "
        f"({report['rates']['byte_of_existing']:.1%}); "
        f"sample outcome: {report['statuses']['BYTE']}/{report['evaluable']} directly evaluable "
        f"({report['rates']['byte_of_evaluable']:.1%})"
    )
    if delta_report is not None:
        print(
            f"delta: +{delta_report['byte_gained']} BYTE / -{delta_report['byte_lost']} BYTE "
            f"across {delta_report['common_observations']} common observations"
        )
        for transition, count in delta_report["transitions"].items():
            print(f"  {transition}: {count}")
    print_breakdown("by language", report["by_language"])
    print_breakdown("by version", report["by_version"])
    print_breakdown("by project", report["by_project"])
    if report["blockers"]:
        print("\ntop blockers")
        for blocker in report["blockers"][:20]:
            examples = ", ".join(blocker["examples"])
            print(f"{blocker['count']:5d} {blocker['status']:<8} {blocker['reason']} [{examples}]")


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--result", type=Path, action="append", default=[])
    parser.add_argument("--baseline-result", type=Path, action="append", default=[])
    parser.add_argument("--tool-fingerprint", help="current tool fingerprint prefix")
    parser.add_argument("--baseline-tool-fingerprint", help="baseline tool fingerprint prefix")
    parser.add_argument("--selection", type=Path)
    parser.add_argument("--project", action="append")
    parser.add_argument("--version", action="append")
    parser.add_argument("--language", choices=("c", "c++"), action="append")
    parser.add_argument("--matching-only", action="store_true")
    parser.add_argument("--json", action="store_true")
    return parser.parse_args(argv)


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(argv)
    try:
        inventory = load_inventory(args.inventory)
        rows = filtered_rows(inventory, args)
        records = load_results(args.result)
        tool = select_tool(records, args.tool_fingerprint)
        observations = latest_observations(records, tool)
        unsupported_versions = {
            record["mw_version"]
            for record in observations.values()
            if record["status"] == "UNSUPPORTED_BUILD"
        }
        source_projects = set(args.project) if args.project else {
            project["name"] for project in inventory["projects"]
        }
        report = snapshot(
            inventory, rows, observations, tool, unsupported_versions, source_projects
        )
        delta_report = None
        if args.baseline_result:
            baseline_records = load_results(args.baseline_result)
            baseline_tool = select_tool(baseline_records, args.baseline_tool_fingerprint)
            baseline = latest_observations(baseline_records, baseline_tool)
            universe = {row["configuration_id"] for row in rows if row["source_exists"]}
            delta_report = delta(observations, baseline, universe)
            report["delta"] = delta_report
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"parity dashboard: {error}")
        return 2
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print_snapshot(report, delta_report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
