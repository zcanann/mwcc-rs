#!/usr/bin/env python3
"""Report reference parity with explicit denominators and untested coverage."""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import json
import math
from pathlib import Path
import re
from typing import Any, Dict, Iterable, List, Optional, Sequence

from parity_identity import configuration_id


STATUSES = (
    "BYTE",
    "DIFF",
    "DEFER",
    "HARNESS",
    "MISSING_DEPENDENCY",
    "INVALID_CONFIGURATION",
    "UNSUPPORTED_BUILD",
    "UNTESTED",
)


def normalize_reason(reason: str) -> str:
    reason = re.sub(
        r"/(?:[^/\s]+/)*refctx\.[^/\s]+/(?:ours/)?[^/\s:'\")]+",
        "<context>",
        reason,
    )
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
    if status == "MISSING_DEPENDENCY":
        return output.rpartition(" — ")[2] or "source dependency is absent"
    if status == "INVALID_CONFIGURATION":
        return output.rpartition(" — ")[2] or "reference compiler rejects configured flags"
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


def code_result(record: Dict[str, Any]) -> Optional[str]:
    """Return exact code+text-relocation status when the harness measured it.

    A whole-object BYTE necessarily has exact code. DIFF and debug-info DEFER
    records carry an explicit component result from newer harnesses; older
    records remain unknown rather than being retroactively guessed.
    """

    for line in record.get("output", "").splitlines():
        if line.startswith("CODE BYTE"):
            return "BYTE"
        if line.startswith("CODE DIFF"):
            return "DIFF"
        if line.startswith("CODE DEFER"):
            return "DEFER"
        if line.startswith("CODE EMPTY"):
            return "EMPTY"
    # Legacy whole-object results predate explicit component reporting. Exact
    # objects still imply equal code, though they cannot distinguish an empty
    # code section until rerun with the current harness.
    if record["status"] == "BYTE":
        return "BYTE"
    return None


def code_component_result(record: Dict[str, Any], component: str) -> Optional[str]:
    """Return one independently measured code component status.

    New harness records expose these dimensions directly.  A legacy whole-object
    BYTE safely implies equality for every component; no claim is inferred from
    legacy failures or deferrals.
    """

    prefix = f"{component} "
    for line in record.get("output", "").splitlines():
        if line.startswith(prefix):
            return line[len(prefix) :].split(maxsplit=1)[0]
    if record["status"] == "BYTE":
        return "BYTE"
    return None


def component_summary(
    observations: Iterable[Dict[str, Any]], component: str
) -> Dict[str, int]:
    results = Counter(
        status
        for observation in observations
        if (status := code_component_result(observation, component)) is not None
    )
    return {
        "measured": results["BYTE"] + results["DIFF"],
        "exact": results["BYTE"],
        "wrong": results["DIFF"],
        "empty": results["EMPTY"],
    }


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


def load_selection_manifest(path: Path) -> Dict[str, Any]:
    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict) or not isinstance(document.get("configuration_ids"), list):
        raise ValueError("selection manifest must contain configuration_ids")
    return document


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


def build_coverage(
    rows: List[Dict[str, Any]],
    observations: Dict[str, Dict[str, Any]],
    unsupported_versions: set[str],
) -> Dict[str, Any]:
    """Report compiler-identity coverage independently of row sampling."""

    configuration_counts: Counter[str] = Counter()
    observed_versions: set[str] = set()
    for row in rows:
        if not row["source_exists"]:
            continue
        version = row["mw_version"]
        configuration_counts[version] += 1
        if row["configuration_id"] in observations:
            observed_versions.add(version)

    versions = set(configuration_counts)
    unsupported = sorted(versions & unsupported_versions)
    supported = sorted(observed_versions - unsupported_versions)
    unprobed = sorted(versions - observed_versions - unsupported_versions)
    return {
        "total_builds": len(versions),
        "supported_builds": supported,
        "unsupported_builds": unsupported,
        "unprobed_builds": unprobed,
        "configuration_counts": {
            "supported": sum(configuration_counts[version] for version in supported),
            "unsupported": sum(configuration_counts[version] for version in unsupported),
            "unprobed": sum(configuration_counts[version] for version in unprobed),
        },
    }


def goal_completion(
    rows: List[Dict[str, Any]], observations: Dict[str, Dict[str, Any]]
) -> Dict[str, Any]:
    """Report literal proof against the all-configurations success criterion."""

    by_project: Dict[str, List[Dict[str, Any]]] = defaultdict(list)
    for row in rows:
        if row["source_exists"]:
            by_project[row["project"]].append(row)

    projects = []
    for name, project_rows in sorted(by_project.items()):
        exact = sum(
            authoritative_result(observations[row["configuration_id"]]) == "BYTE"
            for row in project_rows
            if row["configuration_id"] in observations
        )
        projects.append(
            {
                "name": name,
                "configurations": len(project_rows),
                "authoritative_exact": exact,
                "remaining": len(project_rows) - exact,
                "proven_complete": exact == len(project_rows),
            }
        )
    return {
        "criterion": "every configured translation unit is whole-object byte-identical",
        "configurations": sum(item["configurations"] for item in projects),
        "authoritative_exact": sum(item["authoritative_exact"] for item in projects),
        "projects": len(projects),
        "projects_proven_complete": sum(item["proven_complete"] for item in projects),
        "by_project": projects,
    }


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
    authoritative_byte = sum(
        authoritative_result(observations[row["configuration_id"]]) == "BYTE"
        for row in existing
        if row["configuration_id"] in observations
    )
    classified = len(existing) - counts["UNTESTED"]
    evaluable = counts["BYTE"] + counts["DIFF"] + counts["DEFER"]
    projects = source_projects or {row["project"] for row in rows}
    project_entries = [project for project in inventory["projects"] if project["name"] in projects]
    unmapped = sum(len(project.get("unmapped_sources", [])) for project in project_entries)
    discovered = sum(project.get("source_count", 0) for project in project_entries)
    mapped = sum(project.get("mapped_source_count", 0) for project in project_entries)
    substantive_existing = sum(
        bool(row.get("source_has_non_whitespace", True)) for row in existing
    )
    return {
        "tool_fingerprint": tool,
        "configured": len(rows),
        "existing": len(existing),
        "missing_source": missing,
        "observed": observed,
        "classified": classified,
        "evaluable": evaluable,
        "authoritative_byte": authoritative_byte,
        "source_inventory": {"discovered": discovered, "mapped": mapped, "unmapped": unmapped},
        "source_content": {
            "substantive_existing": substantive_existing,
            "trivial_existing": len(existing) - substantive_existing,
        },
        "goal_completion": goal_completion(rows, observations),
        "build_coverage": build_coverage(rows, observations, unsupported_versions),
        "statuses": {status: counts[status] for status in STATUSES},
        "rates": {
            "byte_of_existing": (
                authoritative_byte / len(existing) if existing else 0.0
            ),
            "raw_byte_of_existing": counts["BYTE"] / len(existing) if existing else 0.0,
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


def wilson_interval(successes: int, total: int, z: float = 1.959963984540054) -> tuple[float, float]:
    if total <= 0:
        return (0.0, 1.0)
    proportion = successes / total
    denominator = 1.0 + z * z / total
    center = (proportion + z * z / (2.0 * total)) / denominator
    radius = (
        z
        * math.sqrt(proportion * (1.0 - proportion) / total + z * z / (4.0 * total * total))
        / denominator
    )
    return (max(0.0, center - radius), min(1.0, center + radius))


def runtime_summary(observations: Iterable[Dict[str, Any]]) -> Dict[str, Any]:
    """Summarize measured per-configuration wall time without guessing missing values."""

    elapsed = sorted(
        float(observation["elapsed_seconds"])
        for observation in observations
        if isinstance(observation.get("elapsed_seconds"), (int, float))
        and observation["elapsed_seconds"] >= 0
    )
    if not elapsed:
        return {
            "measured": 0,
            "total_seconds": None,
            "median_seconds": None,
            "p95_seconds": None,
            "max_seconds": None,
        }
    count = len(elapsed)
    middle = count // 2
    median = (
        elapsed[middle]
        if count % 2
        else (elapsed[middle - 1] + elapsed[middle]) / 2.0
    )
    p95_index = math.ceil(0.95 * count) - 1
    return {
        "measured": count,
        "total_seconds": sum(elapsed),
        "median_seconds": median,
        "p95_seconds": elapsed[p95_index],
        "max_seconds": elapsed[-1],
    }


def substantive_source_estimate(
    rows: List[Dict[str, Any]],
    selected: set[str],
    observations: Dict[str, Dict[str, Any]],
    authoritative: bool = False,
) -> Dict[str, Any]:
    """Measure the fixed sample without whitespace-only source placeholders."""

    row_by_identity = {row["configuration_id"]: row for row in rows}
    substantive = {
        identity
        for identity in selected
        if row_by_identity[identity].get("source_has_non_whitespace", True)
    }
    counts = Counter(
        authoritative_result(observations[identity])
        if authoritative
        else observations[identity]["status"]
        for identity in substantive
    )
    unknown = len(substantive) - (
        counts["BYTE"] + counts["DIFF"] + counts["DEFER"] + counts["UNSUPPORTED_BUILD"]
    )
    resolved = len(substantive) - unknown
    low, high = wilson_interval(counts["BYTE"], resolved)
    return {
        "substantive_source_total": len(substantive),
        "trivial_source_total": len(selected) - len(substantive),
        "substantive_source_successes": counts["BYTE"],
        "substantive_source_confirmed_proportion": (
            counts["BYTE"] / len(substantive) if substantive else None
        ),
        "substantive_source_known_nonparity": (
            counts["DIFF"] + counts["DEFER"] + counts["UNSUPPORTED_BUILD"]
        ),
        "substantive_source_measurement_unknown": unknown,
        "substantive_source_resolved_outcomes": resolved,
        "substantive_source_resolved_proportion": (
            counts["BYTE"] / resolved if resolved else None
        ),
        "substantive_source_resolved_interval_low": low,
        "substantive_source_resolved_interval_high": high,
    }


def observation_evidence(observation: Dict[str, Any]) -> Dict[str, str]:
    evidence = observation.get("evidence")
    return evidence if isinstance(evidence, dict) else {}


def authoritative_result(observation: Dict[str, Any]) -> str:
    """Credit only comparisons against the original TU's real-MWCC object."""

    status = observation["status"]
    if status == "UNSUPPORTED_BUILD":
        return status
    evidence = observation_evidence(observation)
    reference_is_direct = evidence.get("reference_object") == "DIRECT" or (
        "reference_object" not in evidence
        and evidence.get("comparison_input") == "DIRECT"
    )
    if (
        evidence.get("oracle_direct") == "RUNNABLE"
        and reference_is_direct
        and status == "BYTE"
    ):
        return status
    if (
        evidence.get("oracle_direct") == "RUNNABLE"
        and evidence.get("comparison_input") == "DIRECT"
        and status in ("DIFF", "DEFER")
    ):
        return status
    return "UNKNOWN"


def representative_audit(
    rows: List[Dict[str, Any]],
    observations: Dict[str, Dict[str, Any]],
    selection: set[str],
    manifest: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    universe = {row["configuration_id"] for row in rows if row["source_exists"]}
    selected = universe & selection
    direct = {identity: observations[identity] for identity in selected if identity in observations}
    counts = Counter(observation["status"] for observation in direct.values())
    complete = len(direct) == len(selected)
    manifest = manifest or {}
    execution_requested = set(manifest.get("configuration_ids", selection))
    execution_selected = universe & execution_requested
    execution_direct = {
        identity: observations[identity]
        for identity in execution_selected
        if identity in observations
    }
    version_coverage = {
        version: observations[identity]["status"] if identity in observations else "UNTESTED"
        for version, identity in manifest.get("version_coverage", {}).items()
        if identity in universe
    }
    coverage_cells = [
        {
            **cell,
            "status": (
                observations[cell["configuration_id"]]["status"]
                if cell["configuration_id"] in observations
                else "UNTESTED"
            ),
        }
        for cell in manifest.get("coverage_cells", [])
        if cell.get("configuration_id") in universe
    ]
    declared_population = manifest.get("population_size")
    population_matches = declared_population is None or declared_population == len(universe)
    selection_members_present = len(selected) == len(selection)
    design_valid = population_matches and selection_members_present
    result: Dict[str, Any] = {
        "method": manifest.get("kind", "simple_random_sample_without_replacement"),
        "seed": manifest.get("seed"),
        "epoch": manifest.get("epoch"),
        "population_size": len(universe),
        "declared_population_size": declared_population,
        "design_valid": design_valid,
        "population_matches": population_matches,
        "selection_members_present": selection_members_present,
        "requested": len(selection),
        "selected": len(selected),
        "observed": len(direct),
        "complete": complete,
        "statuses": {status: counts[status] for status in STATUSES if status != "UNTESTED"},
        "execution_requested": len(execution_requested),
        "execution_selected": len(execution_selected),
        "execution_observed": len(execution_direct),
        "version_coverage": version_coverage,
        "version_sentinels": len(manifest.get("version_sentinel_configuration_ids", [])),
        "breadth_coverage_dimensions": manifest.get("coverage_dimensions", []),
        "breadth_coverage_cells": len(coverage_cells),
        "breadth_coverage_observed": sum(
            cell["status"] != "UNTESTED" for cell in coverage_cells
        ),
        "breadth_sentinels": len(
            manifest.get("coverage_sentinel_configuration_ids", [])
        ),
        "runtime": runtime_summary(execution_direct.values()),
        "estimate": None,
    }
    if complete and selected and design_valid:
        provenance_complete = all(
            observation["status"] == "UNSUPPORTED_BUILD"
            or "oracle_direct" in observation_evidence(observation)
            for observation in direct.values()
        )
        result["provenance_complete"] = provenance_complete
        credited_counts = (
            Counter(authoritative_result(observation) for observation in direct.values())
            if provenance_complete
            else counts
        )
        successes = credited_counts["BYTE"]
        known_nonparity = (
            credited_counts["DIFF"]
            + credited_counts["DEFER"]
            + credited_counts["UNSUPPORTED_BUILD"]
        )
        unknown = len(selected) - successes - known_nonparity
        resolved = len(selected) - unknown
        resolved_low, resolved_high = wilson_interval(successes, resolved)
        supported_runnable = (
            successes + credited_counts["DIFF"] + credited_counts["DEFER"]
        )
        supported_low, supported_high = wilson_interval(successes, supported_runnable)
        emitted = successes + credited_counts["DIFF"]
        emitted_exact_low, emitted_exact_high = wilson_interval(successes, emitted)
        confirmed_low, confirmed_high = wilson_interval(successes, len(selected))
        identification_upper_successes = successes + unknown
        identification_upper_low, identification_upper_high = wilson_interval(
            identification_upper_successes, len(selected)
        )
        oracle_runnable = sum(
            observation_evidence(observation).get("oracle_direct") == "RUNNABLE"
            for observation in direct.values()
        )
        oracle_runnable_unknown = sum(
            observation_evidence(observation).get("oracle_direct") == "RUNNABLE"
            and authoritative_result(observation) == "UNKNOWN"
            for observation in direct.values()
        )
        oracle_confirmed_low, oracle_confirmed_high = wilson_interval(
            successes, oracle_runnable
        )
        oracle_upper_low, oracle_upper_high = wilson_interval(
            successes + oracle_runnable_unknown, oracle_runnable
        )
        code_results = Counter(
            code_status
            for observation in direct.values()
            if (code_status := code_result(observation)) is not None
        )
        code_measured = code_results["BYTE"] + code_results["DIFF"]
        code_exact_low, code_exact_high = wilson_interval(code_results["BYTE"], code_measured)
        code_components = {
            "text_bytes": component_summary(direct.values(), "TEXT_BYTES"),
            "text_reloc_shape": component_summary(direct.values(), "TEXT_RELOC_SHAPE"),
            "text_reloc_targets": component_summary(direct.values(), "TEXT_RELOC_TARGETS"),
        }
        anonymous_ordinal_only_mismatches = sum(
            code_component_result(observation, "ANON_ORDINALS") == "DIFF"
            for observation in direct.values()
        )
        result["estimate"] = {
            "measure": "configured_byte_exact",
            "successes": successes,
            "total": len(selected),
            "known_nonparity": known_nonparity,
            "measurement_unknown": unknown,
            "harness_unknown": counts["HARNESS"],
            "missing_dependency_unknown": counts["MISSING_DEPENDENCY"],
            "invalid_configuration_unknown": counts["INVALID_CONFIGURATION"],
            "non_authoritative_unknown": max(
                0,
                unknown
                - counts["HARNESS"]
                - counts["MISSING_DEPENDENCY"]
                - counts["INVALID_CONFIGURATION"],
            ),
            "confirmed_proportion": successes / len(selected),
            "confirmed_confidence": 0.95,
            "confirmed_interval_low": confirmed_low,
            "confirmed_interval_high": confirmed_high,
            "identification_interval_low": successes / len(selected),
            "identification_interval_high": (successes + unknown) / len(selected),
            "identification_upper_interval_low": identification_upper_low,
            "identification_upper_interval_high": identification_upper_high,
            "authoritative_provenance": provenance_complete,
            "oracle_runnable": oracle_runnable if provenance_complete else None,
            "oracle_runnable_unknown": (
                oracle_runnable_unknown if provenance_complete else None
            ),
            "oracle_runnable_confirmed_proportion": (
                successes / oracle_runnable
                if provenance_complete and oracle_runnable
                else None
            ),
            "oracle_runnable_confirmed_interval_low": oracle_confirmed_low,
            "oracle_runnable_confirmed_interval_high": oracle_confirmed_high,
            "oracle_runnable_identification_high": (
                (successes + oracle_runnable_unknown) / oracle_runnable
                if provenance_complete and oracle_runnable
                else None
            ),
            "oracle_runnable_upper_interval_low": oracle_upper_low,
            "oracle_runnable_upper_interval_high": oracle_upper_high,
            "resolved_outcomes": resolved,
            "resolved_proportion": successes / resolved if resolved else None,
            "resolved_confidence": 0.95,
            "resolved_interval_low": resolved_low,
            "resolved_interval_high": resolved_high,
            # Conditional compiler-quality view: excludes unsupported builds and
            # rows the harness could not run. This complements (never replaces)
            # the configured-goal estimate above.
            "supported_runnable_outcomes": supported_runnable,
            "supported_runnable_proportion": (
                successes / supported_runnable if supported_runnable else None
            ),
            "supported_runnable_confidence": 0.95,
            "supported_runnable_interval_low": supported_low,
            "supported_runnable_interval_high": supported_high,
            # Safety view for byte-exact-or-defer: among objects actually emitted,
            # how many were exact versus silently wrong.
            "emitted_objects": emitted,
            "emitted_exact": successes,
            "emitted_wrong": credited_counts["DIFF"],
            "emitted_exact_proportion": successes / emitted if emitted else None,
            "emitted_wrong_proportion": (
                credited_counts["DIFF"] / emitted if emitted else None
            ),
            "emitted_exact_confidence": 0.95,
            "emitted_exact_interval_low": emitted_exact_low,
            "emitted_exact_interval_high": emitted_exact_high,
            # Component diagnostic only: this includes `-sym off` projections
            # for debug-info deferrals and therefore earns no whole-object
            # parity credit. It isolates backend/code convergence from debug
            # section coverage with an explicit measured denominator.
            "code_measured": code_measured,
            "code_exact": code_results["BYTE"],
            "code_wrong": code_results["DIFF"],
            "code_deferred": code_results["DEFER"],
            "code_empty": code_results["EMPTY"],
            "code_exact_proportion": (
                code_results["BYTE"] / code_measured if code_measured else None
            ),
            "code_exact_confidence": 0.95,
            "code_exact_interval_low": code_exact_low,
            "code_exact_interval_high": code_exact_high,
            "code_components": code_components,
            "anonymous_ordinal_only_mismatches": anonymous_ordinal_only_mismatches,
            # Whitespace-only source placeholders can produce exact trivial
            # objects. They remain in the goal metric, while this conditional
            # view makes their contribution explicit.
            **substantive_source_estimate(
                rows, selected, direct, authoritative=provenance_complete
            ),
        }
    return result


def work_frontier(
    rows: List[Dict[str, Any]],
    observations: Dict[str, Dict[str, Any]],
    manifest: Dict[str, Any],
) -> Dict[str, Any]:
    universe = {row["configuration_id"] for row in rows if row["source_exists"]}
    requested = set(manifest["configuration_ids"])
    selected = universe & requested
    direct = {identity: observations[identity] for identity in selected if identity in observations}
    counts = Counter(observation["status"] for observation in direct.values())
    return {
        "method": "failure_prioritized_work_queue",
        "is_parity_estimate": False,
        "seed": manifest.get("seed"),
        "epoch": manifest.get("epoch"),
        "universe_size": len(universe),
        "declared_universe_size": manifest.get("universe_size"),
        "selected": len(selected),
        "requested": len(requested),
        "observed": len(direct),
        "previous_statuses": manifest.get("previous_status_counts", {}),
        "statuses": {status: counts[status] for status in STATUSES if status != "UNTESTED"},
    }


def print_breakdown(title: str, rows: List[Dict[str, Any]]) -> None:
    print(f"\n{title}")
    print(
        f"{'name':28} {'total':>7} {'BYTE':>7} {'DIFF':>7} {'DEFER':>7} "
        f"{'HARNESS':>8} {'MISSDEP':>8} {'INVALID':>8} {'UNSUP':>7} {'UNTEST':>8}"
    )
    for row in rows:
        print(
            f"{row['name'][:28]:28} {row['total']:7d} {row['BYTE']:7d} {row['DIFF']:7d} "
            f"{row['DEFER']:7d} {row['HARNESS']:8d} {row['MISSING_DEPENDENCY']:8d} "
            f"{row['INVALID_CONFIGURATION']:8d} {row['UNSUPPORTED_BUILD']:7d} "
            f"{row['UNTESTED']:8d}"
        )


def print_brief(report: Dict[str, Any], delta_report: Optional[Dict[str, Any]]) -> None:
    """Print the few numbers that can legitimately answer "where are we?".

    Each line names its evidence layer and denominator. In particular, the
    failure-biased work queue is never allowed to masquerade as a parity rate.
    """

    tool = report["tool_fingerprint"]
    goal = report["goal_completion"]
    builds = report["build_coverage"]
    print("== parity status (denominator-first) ==")
    print(f"compiler+harness fingerprint: {(tool or '<no observations>')[:24]}")
    print(
        "formal completion — authoritative whole-object exact: "
        f"{goal['authoritative_exact']}/{goal['configurations']} configured TUs; "
        f"complete project matrices {goal['projects_proven_complete']}/{goal['projects']}"
    )
    print(
        f"measurement coverage — direct observations: {report['observed']}/{report['existing']} "
        f"existing configured TUs; compiler identities "
        f"{len(builds['supported_builds'])}/{builds['total_builds']} supported, "
        f"{len(builds['unsupported_builds'])} unsupported, {len(builds['unprobed_builds'])} unprobed"
    )

    audit = report.get("representative_audit")
    if audit is None:
        print("representative audit — NOT RUN for this fingerprint; no corpus parity estimate")
    elif not audit["complete"] or not audit["design_valid"] or audit["estimate"] is None:
        print(
            f"representative audit — INVALID/INCOMPLETE: {audit['observed']}/{audit['selected']} "
            "sample rows observed; no corpus parity estimate"
        )
    else:
        estimate = audit["estimate"]
        print(
            "representative whole-object audit — "
            f"exact {estimate['successes']}/{estimate['total']} = "
            f"{estimate['confirmed_proportion']:.1%}; known nonparity "
            f"{estimate['known_nonparity']}/{estimate['total']}; measurement unknown "
            f"{estimate['measurement_unknown']}/{estimate['total']}; "
            f"95% CI on confirmed share {estimate['confirmed_interval_low']:.1%}.."
            f"{estimate['confirmed_interval_high']:.1%}"
        )
        if estimate["oracle_runnable"]:
            print(
                "real-MWCC-runnable sample stratum — whole-object exact "
                f"{estimate['successes']}/{estimate['oracle_runnable']} = "
                f"{estimate['oracle_runnable_confirmed_proportion']:.1%}; "
                f"pipeline-unknown {estimate['oracle_runnable_unknown']}/"
                f"{estimate['oracle_runnable']}"
            )
        if estimate["code_measured"]:
            print(
                "code+text-relocation diagnostic — exact "
                f"{estimate['code_exact']}/{estimate['code_measured']} = "
                f"{estimate['code_exact_proportion']:.1%}; wrong "
                f"{estimate['code_wrong']}/{estimate['code_measured']}; "
                f"projection-deferred {estimate['code_deferred']}"
            )
        print(
            "audit coverage — compiler versions "
            f"{sum(status != 'UNTESTED' for status in audit['version_coverage'].values())}/"
            f"{len(audit['version_coverage'])}; project/version/language cells "
            f"{audit['breadth_coverage_observed']}/{audit['breadth_coverage_cells']}"
        )

    frontier = report.get("work_frontier")
    if frontier is not None:
        outcomes = ", ".join(
            f"{status} {frontier['statuses'][status]}"
            for status in STATUSES
            if status != "UNTESTED" and frontier["statuses"][status]
        )
        print(
            "iteration queue — FAILURE-BIASED, NOT A PARITY ESTIMATE: "
            f"{frontier['observed']}/{frontier['selected']} observed from "
            f"N={frontier['universe_size']}; {outcomes or 'no outcomes'}"
        )
    if delta_report is not None:
        print(
            "cached comparison delta — diagnostic only: "
            f"+{delta_report['byte_gained']} exact / -{delta_report['byte_lost']} exact "
            f"across {delta_report['common_observations']} common observations"
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
    goal = report["goal_completion"]
    print(
        "goal completion proof: whole-object exact "
        f"{goal['authoritative_exact']}/{goal['configurations']} configured TUs; "
        f"fully exact project matrices {goal['projects_proven_complete']}/{goal['projects']}"
    )
    print(
        f"classified: {report['classified']}/{report['existing']} "
        f"({report['rates']['classified_of_existing']:.1%}); "
        f"direct configuration compilations: {report['observed']}"
    )
    build_report = report["build_coverage"]
    build_counts = build_report["configuration_counts"]
    identity_coverage_rate = (
        build_counts["supported"] / report["existing"] if report["existing"] else 0.0
    )
    print(
        f"compiler identities: {len(build_report['supported_builds'])}/"
        f"{build_report['total_builds']} probed supported; "
        f"{len(build_report['unsupported_builds'])} unsupported; "
        f"{len(build_report['unprobed_builds'])} unprobed"
    )
    print(
        f"identity-covered configurations: {build_counts['supported']}/"
        f"{report['existing']} ({identity_coverage_rate:.3%}); "
        f"unsupported {build_counts['unsupported']}; unprobed {build_counts['unprobed']}"
    )
    if build_report["unsupported_builds"]:
        print(f"unsupported identities: {', '.join(build_report['unsupported_builds'])}")
    if build_report["unprobed_builds"]:
        print(f"unprobed identities: {', '.join(build_report['unprobed_builds'])}")
    print()
    print(f"{'raw status':18} {'count':>8} {'% existing':>12}")
    for status in STATUSES:
        count = report["statuses"][status]
        rate = count / report["existing"] if report["existing"] else 0.0
        print(f"{status:18} {count:8d} {rate:11.1%}")
    audit = report.get("representative_audit")
    if audit is not None:
        print(
            f"fixed parity audit (SRSWOR; seed={audit['seed']!r}, epoch={audit['epoch']!r}): "
            f"n={audit['selected']} from N={audit['population_size']}; "
            f"{audit['observed']}/{audit['selected']} observed "
            f"({'complete' if audit['complete'] else 'INCOMPLETE'}; "
            f"design {'valid' if audit['design_valid'] else 'INVALID'})"
        )
        runtime = audit["runtime"]
        if audit["version_coverage"]:
            covered = sum(status != "UNTESTED" for status in audit["version_coverage"].values())
            print(
                f"audit compiler-version coverage: {covered}/{len(audit['version_coverage'])} "
                f"observed ({audit['version_sentinels']} out-of-sample sentinels)"
            )
        if audit["breadth_coverage_cells"]:
            dimensions = " x ".join(audit["breadth_coverage_dimensions"])
            print(
                f"audit breadth coverage ({dimensions}): "
                f"{audit['breadth_coverage_observed']}/{audit['breadth_coverage_cells']} "
                f"cells observed ({audit['breadth_sentinels']} out-of-sample sentinels)"
            )
        if runtime["measured"]:
            print(
                f"audit execution cost: {runtime['total_seconds']:.1f}s aggregate for "
                f"{runtime['measured']} rows; median {runtime['median_seconds']:.3f}s; "
                f"p95 {runtime['p95_seconds']:.3f}s; max {runtime['max_seconds']:.3f}s"
            )
        if not audit["design_valid"]:
            print(
                "audit estimate suppressed: inventory population or fixed sample membership changed; "
                "regenerate the audit deliberately"
            )
        estimate = audit["estimate"]
        if estimate is not None:
            credit_label = (
                "authoritative direct-TU byte parity"
                if estimate["authoritative_provenance"]
                else "confirmed byte parity in representative audit (legacy provenance)"
            )
            print(
                f"{credit_label}: {estimate['successes']}/{estimate['total']} = "
                f"{estimate['confirmed_proportion']:.1%}; sample 95% CI "
                f"{estimate['confirmed_interval_low']:.1%}.."
                f"{estimate['confirmed_interval_high']:.1%}"
            )
            print(
                "audit measurement completeness: resolved parity outcomes "
                f"{estimate['resolved_outcomes']}/{estimate['total']} = "
                f"{estimate['resolved_outcomes'] / estimate['total']:.1%}; "
                f"unknown {estimate['measurement_unknown']}/{estimate['total']}"
            )
            if estimate["substantive_source_total"]:
                print(
                    "non-whitespace-source diagnostic: "
                    f"{estimate['substantive_source_successes']}/"
                    f"{estimate['substantive_source_total']} = "
                    f"{estimate['substantive_source_confirmed_proportion']:.1%} confirmed; "
                    f"{estimate['trivial_source_total']} whitespace-only rows excluded"
                )
                if estimate["substantive_source_resolved_outcomes"]:
                    print(
                        "non-whitespace resolved-outcome parity: "
                        f"{estimate['substantive_source_successes']}/"
                        f"{estimate['substantive_source_resolved_outcomes']} = "
                        f"{estimate['substantive_source_resolved_proportion']:.1%}; "
                        "conditional 95% CI "
                        f"{estimate['substantive_source_resolved_interval_low']:.1%}.."
                        f"{estimate['substantive_source_resolved_interval_high']:.1%}"
                    )
            print(
                f"known non-parity: {estimate['known_nonparity']}/{estimate['total']} "
                "(DIFF + DEFER + unsupported compiler build)"
            )
            print(
                f"measurement-unknown: {estimate['measurement_unknown']}/{estimate['total']} "
                f"(harness {estimate['harness_unknown']}, "
                f"missing dependency {estimate['missing_dependency_unknown']}, "
                f"invalid configuration {estimate['invalid_configuration_unknown']}, "
                f"non-authoritative comparison {estimate['non_authoritative_unknown']}); "
                f"sample parity bounds "
                f"{estimate['identification_interval_low']:.1%}.."
                f"{estimate['identification_interval_high']:.1%}; upper-endpoint "
                f"sample 95% CI {estimate['identification_upper_interval_low']:.1%}.."
                f"{estimate['identification_upper_interval_high']:.1%}"
            )
            if estimate["authoritative_provenance"] and estimate["oracle_runnable"]:
                print(
                    "real-MWCC-runnable stratum: "
                    f"{estimate['oracle_runnable']} sampled rows; confirmed parity "
                    f"{estimate['oracle_runnable_confirmed_proportion']:.1%}; "
                    "identification bounds "
                    f"{estimate['oracle_runnable_confirmed_proportion']:.1%}.."
                    f"{estimate['oracle_runnable_identification_high']:.1%} "
                    f"({estimate['oracle_runnable_unknown']} pipeline-unknown); "
                    "endpoint sample 95% CIs "
                    f"{estimate['oracle_runnable_confirmed_interval_low']:.1%}.."
                    f"{estimate['oracle_runnable_confirmed_interval_high']:.1%} / "
                    f"{estimate['oracle_runnable_upper_interval_low']:.1%}.."
                    f"{estimate['oracle_runnable_upper_interval_high']:.1%}"
                )
            elif estimate["resolved_outcomes"]:
                print(
                    f"resolved-outcome parity: "
                    f"{estimate['successes']}/{estimate['resolved_outcomes']} = "
                    f"{estimate['resolved_proportion']:.1%}; conditional 95% CI "
                    f"{estimate['resolved_interval_low']:.1%}.."
                    f"{estimate['resolved_interval_high']:.1%}"
                )
            if (
                not estimate["authoritative_provenance"]
                and estimate["supported_runnable_outcomes"]
            ):
                print(
                    f"supported+runnable parity: "
                    f"{estimate['successes']}/{estimate['supported_runnable_outcomes']} = "
                    f"{estimate['supported_runnable_proportion']:.1%}; conditional 95% CI "
                    f"{estimate['supported_runnable_interval_low']:.1%}.."
                    f"{estimate['supported_runnable_interval_high']:.1%}"
                )
            if estimate["emitted_objects"]:
                print(
                    f"emitted-object safety: exact {estimate['emitted_exact']}/"
                    f"{estimate['emitted_objects']} = "
                    f"{estimate['emitted_exact_proportion']:.1%}; "
                    f"wrong {estimate['emitted_wrong']}/{estimate['emitted_objects']} = "
                    f"{estimate['emitted_wrong_proportion']:.1%}; exact-share 95% CI "
                    f"{estimate['emitted_exact_interval_low']:.1%}.."
                    f"{estimate['emitted_exact_interval_high']:.1%}"
                )
            if estimate["code_measured"]:
                print(
                    "code+text-relocation diagnostic (includes non-credit debug-off projections): "
                    f"exact {estimate['code_exact']}/{estimate['code_measured']} = "
                    f"{estimate['code_exact_proportion']:.1%}; "
                    f"wrong {estimate['code_wrong']}/{estimate['code_measured']}; "
                    f"empty-code rows {estimate['code_empty']}; "
                    f"unmeasured-after-projection {estimate['code_deferred']}; "
                    f"exact-share 95% CI {estimate['code_exact_interval_low']:.1%}.."
                    f"{estimate['code_exact_interval_high']:.1%}"
                )
            for component, label in (
                ("text_bytes", "raw .text bytes"),
                ("text_reloc_shape", "text relocation offsets/types"),
                ("text_reloc_targets", "text relocation targets"),
            ):
                summary = estimate["code_components"][component]
                if summary["measured"]:
                    print(
                        f"{label}: exact {summary['exact']}/{summary['measured']}; "
                        f"wrong {summary['wrong']}/{summary['measured']}; "
                        f"empty {summary['empty']}"
                    )
            if estimate["anonymous_ordinal_only_mismatches"]:
                print(
                    "anonymous-ordinal-only relocation mismatches: "
                    f"{estimate['anonymous_ordinal_only_mismatches']}"
                )
        counts = " / ".join(
            f"{status} {audit['statuses'][status]}"
            for status in STATUSES
            if status != "UNTESTED"
        )
        print(f"audit outcomes: {counts}")
        audit_delta = audit.get("delta")
        if audit_delta is not None:
            print(
                f"fixed-audit delta: +{audit_delta['byte_gained']} BYTE / "
                f"-{audit_delta['byte_lost']} BYTE across "
                f"{audit_delta['common_observations']}/{audit['selected']} common sample rows"
            )
            for transition, count in audit_delta["transitions"].items():
                print(f"  {transition}: {count}")
    frontier = report.get("work_frontier")
    if frontier is not None:
        outcomes = " / ".join(
            f"{status} {frontier['statuses'][status]}"
            for status in STATUSES
            if status != "UNTESTED"
        )
        print(
            f"work frontier (failure-biased; NOT A PARITY ESTIMATE): "
            f"{frontier['observed']}/{frontier['selected']} observed from "
            f"N={frontier['universe_size']}"
        )
        print(f"frontier outcomes: {outcomes}")
    if delta_report is not None:
        print(
            f"all-cached delta (diagnostic, not an estimate): "
            f"+{delta_report['byte_gained']} BYTE / -{delta_report['byte_lost']} BYTE "
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
    parser.add_argument("--audit-selection", type=Path)
    parser.add_argument("--frontier-selection", type=Path)
    parser.add_argument("--project", action="append")
    parser.add_argument("--version", action="append")
    parser.add_argument("--language", choices=("c", "c++"), action="append")
    parser.add_argument("--matching-only", action="store_true")
    parser.add_argument(
        "--brief",
        action="store_true",
        help="print only denominator-qualified status layers",
    )
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
        audit_manifest = None
        if args.audit_selection is not None:
            audit_manifest = load_selection_manifest(args.audit_selection)
            sample_ids = audit_manifest.get(
                "sample_configuration_ids", audit_manifest["configuration_ids"]
            )
            report["representative_audit"] = representative_audit(
                rows,
                observations,
                set(sample_ids),
                audit_manifest,
            )
        if args.frontier_selection is not None:
            report["work_frontier"] = work_frontier(
                rows, observations, load_selection_manifest(args.frontier_selection)
            )
        delta_report = None
        if args.baseline_result:
            baseline_records = load_results(args.baseline_result)
            baseline_tool = select_tool(baseline_records, args.baseline_tool_fingerprint)
            baseline = latest_observations(baseline_records, baseline_tool)
            universe = {row["configuration_id"] for row in rows if row["source_exists"]}
            delta_report = delta(observations, baseline, universe)
            report["delta"] = delta_report
            if audit_manifest is not None:
                audit_ids = universe & set(
                    audit_manifest.get(
                        "sample_configuration_ids", audit_manifest["configuration_ids"]
                    )
                )
                report["representative_audit"]["delta"] = delta(
                    observations, baseline, audit_ids
                )
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"parity dashboard: {error}")
        return 2
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    elif args.brief:
        print_brief(report, delta_report)
    else:
        print_snapshot(report, delta_report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
