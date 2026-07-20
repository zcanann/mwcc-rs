#!/usr/bin/env python3

from __future__ import annotations

import argparse
import contextlib
import io
import unittest

from parity_audit import build_audit
from parity_dashboard import (
    code_component_result,
    code_result,
    failure_reason,
    representative_audit,
    runtime_summary,
    snapshot,
    wilson_interval,
    work_frontier,
)
from parity_frontier import build_frontier
from parity_identity import configuration_id
from parity_loop import parse_args as parse_loop_args
from reference_parity import result_cache_name, stable_sample


def row(**overrides):
    value = {
        "project": "project",
        "variant": "v1",
        "source": "src/test.c",
        "language": "c",
        "mw_version": "GC/2.6",
        "cflags": ["-O4,p"],
        "extra_cflags": [],
        "shift_jis": False,
        "extab_padding": None,
        "matching": True,
        "source_exists": True,
        "source_has_non_whitespace": True,
    }
    value.update(overrides)
    value["configuration_id"] = configuration_id(value)
    return value


class IdentityTests(unittest.TestCase):
    def test_parity_loop_separates_fast_work_from_periodic_audit(self):
        work = parse_loop_args(["--work-only"])
        self.assertTrue(work.work_only)
        self.assertEqual(work.size, 32)
        self.assertEqual(str(work.compiler), "target/debug/mwcc")
        self.assertTrue(parse_loop_args(["--audit-only"]).audit_only)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            parse_loop_args(["--work-only", "--audit-only"])

    def test_result_cache_name_changes_with_either_tool_input(self):
        baseline = result_cache_name("a" * 64, "b" * 64)
        self.assertNotEqual(baseline, result_cache_name("c" * 64, "b" * 64))
        self.assertNotEqual(baseline, result_cache_name("a" * 64, "d" * 64))

    def test_variant_and_progress_do_not_change_compiler_input_identity(self):
        first = row(variant="a", matching=True)
        second = row(variant="b", matching=False)
        self.assertEqual(first["configuration_id"], second["configuration_id"])

    def test_flags_change_identity(self):
        self.assertNotEqual(row()["configuration_id"], row(cflags=["-O1"])["configuration_id"])

    def test_source_content_changes_identity(self):
        self.assertNotEqual(
            row(source_sha256="a")["configuration_id"],
            row(source_sha256="b")["configuration_id"],
        )

    def test_stable_sample_is_order_independent(self):
        rows = [row(source=f"src/{index}.c") for index in range(20)]
        first = [item["configuration_id"] for item in stable_sample(rows, 5, "seed")]
        second = [item["configuration_id"] for item in stable_sample(list(reversed(rows)), 5, "seed")]
        self.assertEqual(first, second)


class DashboardTests(unittest.TestCase):
    def test_runtime_summary_reports_cost_distribution_and_missing_count(self):
        report = runtime_summary(
            [
                {"elapsed_seconds": 4.0},
                {"elapsed_seconds": 1.0},
                {},
                {"elapsed_seconds": 3.0},
                {"elapsed_seconds": 2.0},
            ]
        )
        self.assertEqual(report["measured"], 4)
        self.assertEqual(report["total_seconds"], 10.0)
        self.assertEqual(report["median_seconds"], 2.5)
        self.assertEqual(report["p95_seconds"], 4.0)
        self.assertEqual(report["max_seconds"], 4.0)

    def test_code_result_distinguishes_full_exact_projection_and_unknown(self):
        self.assertEqual(code_result({"status": "BYTE"}), "BYTE")
        self.assertEqual(
            code_result(
                {
                    "status": "DEFER",
                    "output": "DEFER  x.c — debug unsupported\nCODE BYTE — projection matches",
                }
            ),
            "BYTE",
        )
        self.assertEqual(
            code_result({"status": "DIFF", "output": "DIFF x.c\nCODE DIFF — mismatch"}),
            "DIFF",
        )
        self.assertEqual(
            code_result({"status": "BYTE", "output": "BYTE x.c\nCODE EMPTY — no code"}),
            "EMPTY",
        )
        self.assertIsNone(code_result({"status": "DEFER", "output": "DEFER x.c"}))

    def test_code_components_do_not_conflate_bytes_and_symbol_ordinals(self):
        record = {
            "status": "DEFER",
            "output": (
                "DEFER x.cpp — debug\n"
                "CODE DIFF — component mismatch\n"
                "TEXT_BYTES BYTE — raw bytes match\n"
                "TEXT_RELOC_SHAPE BYTE — sites match\n"
                "TEXT_RELOC_TARGETS DIFF — targets differ\n"
                "ANON_ORDINALS DIFF — only anonymous numbers differ"
            ),
        }
        self.assertEqual(code_component_result(record, "TEXT_BYTES"), "BYTE")
        self.assertEqual(code_component_result(record, "TEXT_RELOC_SHAPE"), "BYTE")
        self.assertEqual(code_component_result(record, "TEXT_RELOC_TARGETS"), "DIFF")
        self.assertEqual(code_component_result(record, "ANON_ORDINALS"), "DIFF")

    def test_snapshot_keeps_untested_in_the_denominator(self):
        rows = [row(source="src/a.c"), row(source="src/b.c"), row(source="src/missing.c", source_exists=False)]
        observations = {
            rows[0]["configuration_id"]: {"status": "BYTE"},
        }
        inventory = {
            "projects": [
                {
                    "name": "project",
                    "source_count": 3,
                    "mapped_source_count": 2,
                    "unmapped_sources": ["src/unmapped.c"],
                }
            ]
        }
        report = snapshot(inventory, rows, observations, "tool")
        self.assertEqual(report["configured"], 3)
        self.assertEqual(report["existing"], 2)
        self.assertEqual(report["missing_source"], 1)
        self.assertEqual(report["statuses"]["BYTE"], 1)
        self.assertEqual(report["statuses"]["UNTESTED"], 1)

    def test_one_unsupported_build_probe_classifies_the_whole_version(self):
        rows = [row(source=f"src/{index}.c", mw_version="Wii/1.0") for index in range(3)]
        observations = {
            rows[0]["configuration_id"]: {
                "status": "UNSUPPORTED_BUILD",
                "mw_version": "Wii/1.0",
                "output": "unsupported",
            }
        }
        report = snapshot({"projects": []}, rows, observations, "tool", {"Wii/1.0"})
        self.assertEqual(report["statuses"]["UNSUPPORTED_BUILD"], 3)
        self.assertEqual(report["observed"], 1)
        self.assertEqual(report["classified"], 3)
        self.assertEqual(report["build_coverage"]["unsupported_builds"], ["Wii/1.0"])
        self.assertEqual(
            report["build_coverage"]["configuration_counts"]["unsupported"], 3
        )

    def test_build_coverage_exposes_unprobed_identities(self):
        rows = [
            row(source="src/a.c", mw_version="GC/2.6"),
            row(source="src/b.c", mw_version="ProDG/3.5"),
        ]
        observations = {rows[0]["configuration_id"]: {"status": "BYTE"}}
        report = snapshot({"projects": []}, rows, observations, "tool")
        coverage = report["build_coverage"]
        self.assertEqual(coverage["supported_builds"], ["GC/2.6"])
        self.assertEqual(coverage["unsupported_builds"], [])
        self.assertEqual(coverage["unprobed_builds"], ["ProDG/3.5"])
        self.assertEqual(coverage["configuration_counts"]["unprobed"], 1)

    def test_failure_reason_extracts_reference_compiler_diagnostic(self):
        record = {
            "status": "HARNESS",
            "output": "### mwcceppc.exe Compiler:\n# Error: ^\n# illegal initialization",
        }
        self.assertEqual(failure_reason(record), "reference compiler: illegal initialization")

    def test_failure_reason_normalizes_defer_specific_names(self):
        record = {
            "status": "DEFER",
            "output": "DEFER  test.cpp — expected a type, found Identifier(\"Thing\")",
        }
        self.assertEqual(failure_reason(record), "expected a type, found Identifier(…)")

    def test_representative_audit_requires_the_complete_fixed_sample(self):
        rows = [row(source="src/a.c"), row(source="src/b.c")]
        observations = {rows[0]["configuration_id"]: {"status": "BYTE"}}
        report = representative_audit(
            rows, observations, {item["configuration_id"] for item in rows}
        )
        self.assertFalse(report["complete"])
        self.assertIsNone(report["estimate"])

    def test_representative_audit_reports_byte_successes_and_interval(self):
        rows = [row(source=f"src/{index}.c") for index in range(4)]
        observations = {
            item["configuration_id"]: {"status": "BYTE" if index == 0 else "DEFER"}
            for index, item in enumerate(rows)
        }
        report = representative_audit(
            rows, observations, {item["configuration_id"] for item in rows}
        )
        self.assertTrue(report["complete"])
        self.assertEqual(report["estimate"]["confirmed_proportion"], 0.25)
        self.assertEqual(report["estimate"]["identification_interval_low"], 0.25)
        self.assertEqual(report["estimate"]["identification_interval_high"], 0.25)
        low, high = wilson_interval(1, 4)
        self.assertLess(low, 0.25)
        self.assertGreater(high, 0.25)

    def test_harness_results_widen_identification_bounds(self):
        rows = [row(source=f"src/{index}.c") for index in range(4)]
        statuses = ("BYTE", "DEFER", "HARNESS", "MISSING_DEPENDENCY")
        observations = {
            item["configuration_id"]: {"status": statuses[index]}
            for index, item in enumerate(rows)
        }
        report = representative_audit(
            rows, observations, {item["configuration_id"] for item in rows}
        )
        self.assertEqual(report["estimate"]["identification_interval_low"], 0.25)
        self.assertEqual(report["estimate"]["identification_interval_high"], 0.75)
        self.assertEqual(report["estimate"]["resolved_proportion"], 0.5)
        self.assertEqual(report["estimate"]["supported_runnable_outcomes"], 2)
        self.assertEqual(report["estimate"]["supported_runnable_proportion"], 0.5)

    def test_invalid_configuration_is_measurement_unknown_not_compiler_failure(self):
        rows = [row(source=f"src/{index}.c") for index in range(3)]
        statuses = ("BYTE", "DEFER", "INVALID_CONFIGURATION")
        observations = {
            item["configuration_id"]: {"status": statuses[index]}
            for index, item in enumerate(rows)
        }
        report = representative_audit(
            rows, observations, {item["configuration_id"] for item in rows}
        )
        self.assertEqual(report["estimate"]["known_nonparity"], 1)
        self.assertEqual(report["estimate"]["measurement_unknown"], 1)
        self.assertEqual(report["estimate"]["resolved_outcomes"], 2)

    def test_supported_runnable_and_emitted_safety_have_explicit_denominators(self):
        rows = [row(source=f"src/{index}.c") for index in range(5)]
        statuses = ("BYTE", "DIFF", "DEFER", "UNSUPPORTED_BUILD", "MISSING_DEPENDENCY")
        observations = {
            item["configuration_id"]: {"status": statuses[index]}
            for index, item in enumerate(rows)
        }
        report = representative_audit(
            rows, observations, {item["configuration_id"] for item in rows}
        )
        estimate = report["estimate"]
        self.assertEqual(estimate["supported_runnable_outcomes"], 3)
        self.assertEqual(estimate["supported_runnable_proportion"], 1 / 3)
        self.assertEqual(estimate["emitted_objects"], 2)
        self.assertEqual(estimate["emitted_exact"], 1)
        self.assertEqual(estimate["emitted_wrong"], 1)
        self.assertEqual(estimate["emitted_wrong_proportion"], 0.5)

    def test_code_diagnostic_has_its_own_measured_denominator(self):
        rows = [row(source=f"src/{index}.c") for index in range(4)]
        observations = {
            rows[0]["configuration_id"]: {"status": "BYTE"},
            rows[1]["configuration_id"]: {
                "status": "DEFER",
                "output": "DEFER x.c — debug\nCODE BYTE — projected",
            },
            rows[2]["configuration_id"]: {
                "status": "DIFF",
                "output": "DIFF x.c\nCODE DIFF — mismatch",
            },
            rows[3]["configuration_id"]: {"status": "DEFER", "output": "DEFER x.c — parser"},
        }
        report = representative_audit(
            rows, observations, {item["configuration_id"] for item in rows}
        )
        estimate = report["estimate"]
        self.assertEqual(estimate["code_measured"], 3)
        self.assertEqual(estimate["code_exact"], 2)
        self.assertEqual(estimate["code_wrong"], 1)
        self.assertEqual(estimate["code_exact_proportion"], 2 / 3)

    def test_layered_code_diagnostics_have_independent_denominators(self):
        rows = [row(source=f"src/{index}.cpp") for index in range(3)]
        observations = {
            rows[0]["configuration_id"]: {"status": "BYTE"},
            rows[1]["configuration_id"]: {
                "status": "DEFER",
                "output": (
                    "DEFER x.cpp — debug\n"
                    "CODE DIFF — components\n"
                    "TEXT_BYTES BYTE — exact\n"
                    "TEXT_RELOC_SHAPE BYTE — exact\n"
                    "TEXT_RELOC_TARGETS DIFF — ordinals\n"
                    "ANON_ORDINALS DIFF — only ordinals"
                ),
            },
            rows[2]["configuration_id"]: {"status": "DEFER", "output": "DEFER parser"},
        }
        estimate = representative_audit(
            rows, observations, {item["configuration_id"] for item in rows}
        )["estimate"]
        self.assertEqual(
            estimate["code_components"]["text_bytes"],
            {"measured": 2, "exact": 2, "wrong": 0, "empty": 0},
        )
        self.assertEqual(
            estimate["code_components"]["text_reloc_targets"],
            {"measured": 2, "exact": 1, "wrong": 1, "empty": 0},
        )
        self.assertEqual(estimate["anonymous_ordinal_only_mismatches"], 1)

    def test_substantive_source_diagnostic_excludes_trivial_exact_objects(self):
        rows = [
            row(source="src/empty.c", source_has_non_whitespace=False),
            row(source="src/code.c"),
            row(source="src/deferred.c"),
        ]
        observations = {
            rows[0]["configuration_id"]: {"status": "BYTE"},
            rows[1]["configuration_id"]: {"status": "BYTE"},
            rows[2]["configuration_id"]: {"status": "DEFER"},
        }
        report = representative_audit(
            rows, observations, {item["configuration_id"] for item in rows}
        )
        estimate = report["estimate"]
        self.assertEqual(estimate["successes"], 2)
        self.assertEqual(estimate["trivial_source_total"], 1)
        self.assertEqual(estimate["substantive_source_total"], 2)
        self.assertEqual(estimate["substantive_source_successes"], 1)
        self.assertEqual(estimate["substantive_source_resolved_proportion"], 0.5)

    def test_audit_suppresses_estimate_after_inventory_drift(self):
        rows = [row(source="src/a.c"), row(source="src/b.c")]
        selection = {item["configuration_id"] for item in rows}
        observations = {identity: {"status": "BYTE"} for identity in selection}
        report = representative_audit(
            rows,
            observations,
            selection,
            {
                "kind": "simple_random_sample_without_replacement",
                "population_size": 3,
                "configuration_ids": sorted(selection),
                "seed": "fixed",
                "epoch": "0",
            },
        )
        self.assertFalse(report["design_valid"])
        self.assertIsNone(report["estimate"])

    def test_frontier_is_explicitly_not_a_parity_estimate(self):
        rows = [row(source="src/a.c"), row(source="src/b.c")]
        selection = {item["configuration_id"] for item in rows}
        observations = {
            rows[0]["configuration_id"]: {"status": "BYTE"},
            rows[1]["configuration_id"]: {"status": "DEFER"},
        }
        report = work_frontier(
            rows,
            observations,
            {"configuration_ids": sorted(selection), "universe_size": 2},
        )
        self.assertFalse(report["is_parity_estimate"])
        self.assertEqual(report["statuses"]["BYTE"], 1)
        self.assertEqual(report["statuses"]["DEFER"], 1)


class AuditSelectionTests(unittest.TestCase):
    def test_fixed_audit_is_deterministic_and_order_independent(self):
        rows = [row(source=f"src/{index}.c") for index in range(20)]
        first = build_audit(rows, 7, "seed", "0")
        second = build_audit(list(reversed(rows)), 7, "seed", "0")
        self.assertEqual(first["configuration_ids"], second["configuration_ids"])
        self.assertEqual(first["sample_configuration_ids"], second["sample_configuration_ids"])
        self.assertEqual(len(first["sample_configuration_ids"]), 7)

    def test_fixed_audit_adds_rare_version_sentinel_outside_sample(self):
        rows = [row(source=f"src/{index}.c") for index in range(20)]
        rare = row(source="src/rare.c", mw_version="GC/1.1")
        rows.append(rare)
        audit = build_audit(rows, 1, "seed", "0")
        if rare["configuration_id"] not in audit["sample_configuration_ids"]:
            self.assertIn(rare["configuration_id"], audit["configuration_ids"])
            self.assertIn(
                rare["configuration_id"], audit["version_sentinel_configuration_ids"]
            )
        self.assertEqual(set(audit["version_coverage"]), {"GC/1.1", "GC/2.6"})

    def test_version_sentinel_prefers_small_matching_source(self):
        common = [row(source=f"src/common-{index}.c") for index in range(20)]
        large = row(
            source="src/large.c", mw_version="GC/1.1", source_size_bytes=10000
        )
        small = row(
            source="src/small.c", mw_version="GC/1.1", source_size_bytes=100
        )
        nonmatching = row(
            source="src/nonmatching.c",
            mw_version="GC/1.1",
            source_size_bytes=1,
            matching=False,
        )
        audit = build_audit(common + [large, small, nonmatching], 1, "seed", "0")
        if not any(
            item["configuration_id"] in audit["sample_configuration_ids"]
            for item in (large, small, nonmatching)
        ):
            self.assertEqual(audit["version_coverage"]["GC/1.1"], small["configuration_id"])


class FrontierTests(unittest.TestCase):
    def test_zero_size_frontier_supports_audit_only_runs(self):
        rows = [row(source="src/a.c")]
        args = argparse.Namespace(size=0, byte_audit=0, seed="seed", epoch="0")
        frontier = build_frontier(rows, {}, args)
        self.assertEqual(frontier["configuration_ids"], [])

    def test_nonpassing_results_stay_ahead_of_untested_and_byte_audit(self):
        rows = [row(source=f"src/{index}.c") for index in range(8)]
        observations = {
            rows[0]["configuration_id"]: {"status": "DIFF"},
            rows[1]["configuration_id"]: {"status": "DEFER"},
            rows[2]["configuration_id"]: {"status": "HARNESS"},
            rows[3]["configuration_id"]: {"status": "BYTE"},
            rows[4]["configuration_id"]: {"status": "BYTE"},
        }
        args = argparse.Namespace(size=5, byte_audit=1, seed="seed", epoch="0")
        frontier = build_frontier(rows, observations, args)
        chosen = set(frontier["configuration_ids"])
        self.assertIn(rows[0]["configuration_id"], chosen)
        self.assertIn(rows[1]["configuration_id"], chosen)
        self.assertIn(rows[2]["configuration_id"], chosen)
        self.assertEqual(frontier["previous_status_counts"]["BYTE"], 1)

    def test_frontier_reserves_a_probe_for_an_unobserved_version(self):
        rows = [row(source=f"src/a{index}.c") for index in range(5)]
        rows.append(row(source="src/wii.c", mw_version="Wii/1.0"))
        observations = {
            item["configuration_id"]: {"status": "DIFF"}
            for item in rows[:-1]
        }
        args = argparse.Namespace(size=3, byte_audit=0, seed="seed", epoch="0")
        frontier = build_frontier(rows, observations, args)
        self.assertIn(rows[-1]["configuration_id"], frontier["configuration_ids"])
        self.assertEqual(frontier["probed_versions"], ["Wii/1.0"])

    def test_new_tool_reprobes_versions_seen_only_by_an_old_tool(self):
        rows = [row(source="src/gc.c"), row(source="src/wii.c", mw_version="Wii/1.0")]
        old_observations = {
            item["configuration_id"]: {"status": "BYTE"}
            for item in rows
        }
        args = argparse.Namespace(size=2, byte_audit=0, seed="seed", epoch="0")
        frontier = build_frontier(rows, old_observations, args, {})
        self.assertEqual(set(frontier["probed_versions"]), {"GC/2.6", "Wii/1.0"})


if __name__ == "__main__":
    unittest.main()
