#!/usr/bin/env python3

from __future__ import annotations

import argparse
import unittest

from parity_dashboard import failure_reason, snapshot
from parity_frontier import build_frontier
from parity_identity import configuration_id
from reference_parity import stable_sample


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
    }
    value.update(overrides)
    value["configuration_id"] = configuration_id(value)
    return value


class IdentityTests(unittest.TestCase):
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


class FrontierTests(unittest.TestCase):
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
