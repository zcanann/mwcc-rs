#!/usr/bin/env python3

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import contextlib
import io
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import unittest

from parity_audit import build_audit
from parity_dashboard import (
    authoritative_result,
    blocker_family_breakdown,
    code_component_result,
    code_result,
    compiler_blocker_family,
    delta,
    failure_reason,
    normalize_reason,
    print_brief,
    representative_audit,
    runtime_summary,
    snapshot,
    wilson_interval,
    work_frontier,
)
from parity_frontier import build_frontier
from parity_identity import configuration_id
from parity_loop import (
    acquire_state_lock,
    most_comparable_other_tool,
    parse_args as parse_loop_args,
    persistent_compiler_image,
)
from reference_parity import (
    bounded_completion_order,
    code_verdict,
    harness_fingerprint,
    immutable_compiler_snapshot,
    parity_metadata,
    parse_args as parse_reference_args,
    result_cache_name,
    run_row,
    register_active_row_process,
    selection_is_probability_sample,
    stable_sample,
    terminate_active_row_processes,
    unregister_active_row_process,
    verdict_line,
)
from refctx_pch import generated_pch_paths


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
    def test_reason_normalization_accepts_preserved_source_basenames(self):
        self.assertEqual(
            normalize_reason(
                "failed at /tmp/refctx.A1b2C3/ours/__ppc_eabi_init.cpp:17"
            ),
            "failed at <context>:17",
        )

    def test_refctx_metadata_is_machine_readable_without_hiding_verdict(self):
        output = (
            "PARITY_META oracle_direct=RUNNABLE\n"
            "PARITY_META comparison_input=DIRECT\n"
            "BYTE src/test.c — exact"
        )
        self.assertEqual(
            parity_metadata(output),
            {"oracle_direct": "RUNNABLE", "comparison_input": "DIRECT"},
        )
        self.assertEqual(verdict_line(output), "BYTE src/test.c — exact")

    def test_refctx_metadata_keeps_the_last_updated_value(self):
        output = (
            "PARITY_META oracle_direct=REJECTED\n"
            "PARITY_META oracle_direct=RUNNABLE\n"
            "DEFER src/test.c — parser gap"
        )
        self.assertEqual(parity_metadata(output), {"oracle_direct": "RUNNABLE"})

    def test_runner_code_layer_requires_explicit_projection_or_exact_object(self):
        self.assertEqual(code_verdict("BYTE src/test.c — exact", "BYTE"), "BYTE")
        self.assertEqual(
            code_verdict("DEFER test.c — debug\nCODE BYTE — projected", "DEFER"),
            "BYTE",
        )
        self.assertEqual(
            code_verdict("DIFF test.c\nCODE DIFF — mismatch", "DIFF"),
            "DIFF",
        )
        self.assertIsNone(code_verdict("DEFER test.c — parser", "DEFER"))

    def test_harness_fingerprint_covers_every_row_classification_input(self):
        names = (
            "refctx.sh",
            "refctx_pch.py",
            "reference_parity.py",
            "parity_identity.py",
            "decompctx_runner.py",
            "object_code_metrics.py",
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in names:
                (root / name).write_text(name, encoding="utf-8")
            baseline = harness_fingerprint(root)
            for name in names:
                path = root / name
                original = path.read_text(encoding="utf-8")
                path.write_text(f"{original} changed", encoding="utf-8")
                self.assertNotEqual(harness_fingerprint(root), baseline, name)
                path.write_text(original, encoding="utf-8")

    def test_generated_pch_paths_are_normalized_and_deduplicated(self):
        context = (
            '/* "include/d/dolzel_rel.h" line 4 "d/dolzel_rel.mch" */\n'
            '/* "include/d/dolzel_rel.h" line 4 "d\\dolzel_rel.mch" */\n'
            '/* "include/d/dolzel_rel.h" line 5 "not-a-pch.h" */\n'
            'int declaration;\n'
        )
        self.assertEqual(generated_pch_paths(context.splitlines()), ["d/dolzel_rel.mch"])

    def test_parity_loop_separates_fast_work_from_periodic_audit(self):
        default = parse_loop_args([])
        self.assertFalse(default.audit_only)
        self.assertFalse(default.with_audit)
        work = parse_loop_args(["--work-only"])
        self.assertTrue(work.work_only)
        self.assertEqual(work.size, 32)
        self.assertEqual(work.jobs, 4)
        self.assertEqual(str(work.compiler), "target/debug/mwcc")
        self.assertFalse(work.no_build)
        self.assertTrue(parse_loop_args(["--no-build"]).no_build)
        self.assertEqual(parse_loop_args(["--jobs", "2"]).jobs, 2)
        self.assertTrue(parse_loop_args(["--audit-only"]).audit_only)
        self.assertTrue(parse_loop_args(["--with-audit"]).with_audit)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            parse_loop_args(["--work-only", "--audit-only"])

    def test_parity_loop_rejects_duplicate_state_owners(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)
            first = acquire_state_lock(state)
            try:
                with self.assertRaisesRegex(RuntimeError, "already active"):
                    acquire_state_lock(state)
            finally:
                first.close()

            replacement = acquire_state_lock(state)
            replacement.close()

    def test_reference_runner_parallelism_is_explicit_and_bounded(self):
        defaults = parse_reference_args([])
        self.assertEqual(defaults.jobs, 1)
        self.assertFalse(defaults.code_projection)
        self.assertEqual(parse_reference_args(["--jobs", "4"]).jobs, 4)
        self.assertTrue(
            parse_reference_args(["--code-projection"]).code_projection
        )

        release_slow = threading.Event()

        def observe(value):
            if value == "slow":
                release_slow.wait()
            return value.upper()

        with ThreadPoolExecutor(max_workers=2) as executor:
            observations = bounded_completion_order(
                ["slow", "fast", "later"], executor, observe, 2
            )
            first = next(observations)
            release_slow.set()
            completed = [first, *observations]
        self.assertEqual(first, (2, "fast", "FAST"))
        self.assertCountEqual(
            completed,
            [
                (1, "slow", "SLOW"),
                (2, "fast", "FAST"),
                (3, "later", "LATER"),
            ],
        )

    def test_reference_timeout_kills_the_entire_refctx_process_group(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            child_pid = root / "child.pid"
            refctx = root / "refctx.sh"
            refctx.write_text(
                "#!/bin/sh\n"
                "sleep 60 &\n"
                'echo "$!" > "$PARITY_TEST_CHILD_PID"\n'
                "wait\n",
                encoding="utf-8",
            )
            refctx.chmod(0o755)
            previous = os.environ.get("PARITY_TEST_CHILD_PID")
            os.environ["PARITY_TEST_CHILD_PID"] = str(child_pid)
            try:
                status, detail = run_row(
                    row(), root, refctx, root / "compiler", 1, False
                )
            finally:
                if previous is None:
                    os.environ.pop("PARITY_TEST_CHILD_PID", None)
                else:
                    os.environ["PARITY_TEST_CHILD_PID"] = previous
            self.assertEqual((status, detail), ("HARNESS", "timed out after 1s"))
            pid = int(child_pid.read_text(encoding="utf-8"))
            for _ in range(20):
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.05)
            else:
                self.fail(f"timed-out refctx child {pid} is still running")

    def test_runner_termination_kills_every_active_row_process_group(self):
        process = subprocess.Popen(["sleep", "60"], start_new_session=True)
        register_active_row_process(process.pid)
        try:
            terminate_active_row_processes()
            process.wait(timeout=2)
            self.assertIsNotNone(process.returncode)
        finally:
            unregister_active_row_process(process.pid)
            if process.poll() is None:
                os.killpg(process.pid, 9)
                process.wait(timeout=2)

    def test_result_cache_name_changes_with_either_tool_input(self):
        baseline = result_cache_name("a" * 64, "b" * 64)
        self.assertNotEqual(baseline, result_cache_name("c" * 64, "b" * 64))
        self.assertNotEqual(baseline, result_cache_name("a" * 64, "d" * 64))

    def test_compiler_snapshot_is_immutable_across_later_rebuilds(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "mwcc"
            source.write_bytes(b"first compiler image")
            snapshot_directory, snapshot, fingerprint = immutable_compiler_snapshot(source)
            try:
                source.write_bytes(b"replacement compiler image")
                self.assertEqual(snapshot.read_bytes(), b"first compiler image")
                self.assertEqual(len(fingerprint), 64)
            finally:
                snapshot_directory.cleanup()

    def test_parity_loop_persists_content_addressed_compiler_image(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "mwcc"
            store = root / "images"
            source.write_bytes(b"first compiler image")
            first, first_hash = persistent_compiler_image(source, store)

            source.write_bytes(b"replacement compiler image")
            second, second_hash = persistent_compiler_image(source, store)

            self.assertEqual(first.read_bytes(), b"first compiler image")
            self.assertEqual(second.read_bytes(), b"replacement compiler image")
            self.assertNotEqual(first_hash, second_hash)
            self.assertEqual(first.parent.name, first_hash)
            self.assertEqual(second.parent.name, second_hash)

    def test_parity_baseline_prefers_overlap_over_newer_focused_probe(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            complete = root / "complete.jsonl"
            focused = root / "focused.jsonl"
            complete.write_text(
                "".join(
                    f'{{"tool_fingerprint":"complete","configuration_id":"{identity}",'
                    '"observed_at":"2026-01-01T00:00:00Z"}\n'
                    for identity in ("a", "b", "c")
                ),
                encoding="utf-8",
            )
            focused.write_text(
                '{"tool_fingerprint":"focused","configuration_id":"a",'
                '"observed_at":"2026-12-01T00:00:00Z"}\n',
                encoding="utf-8",
            )
            self.assertEqual(
                most_comparable_other_tool(
                    [complete, focused], "current", {"a", "b", "c"}
                ),
                "complete",
            )

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

    def test_probability_sample_manifest_is_distinguished_from_work_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            audit = Path(directory) / "audit.json"
            audit.write_text(
                '{"kind":"simple_random_sample_without_replacement",'
                '"sample_configuration_ids":[]}',
                encoding="utf-8",
            )
            work = Path(directory) / "work.json"
            work.write_text('{"configuration_ids":[]}', encoding="utf-8")
            self.assertTrue(selection_is_probability_sample(audit))
            self.assertFalse(selection_is_probability_sample(work))


class DashboardTests(unittest.TestCase):
    def test_delta_separates_authoritative_and_configured_source_movement(self):
        def observation(status, configured=None, direct=True):
            evidence = {}
            if direct:
                evidence.update(
                    oracle_direct="RUNNABLE",
                    comparison_input="DIRECT",
                    reference_object="DIRECT",
                )
            if configured is not None:
                evidence["configured_source"] = configured
            return {"status": status, "evidence": evidence}

        baseline = {
            "gain": observation("DIFF", "DEFER"),
            "loss": observation("BYTE", "BYTE"),
            "newly_measured": observation("HARNESS"),
        }
        current = {
            "gain": observation("BYTE", "BYTE"),
            "loss": observation("DIFF", "DIFF"),
            "newly_measured": observation("BYTE", "BYTE"),
        }

        movement = delta(current, baseline, set(current))

        self.assertEqual(movement["common_observations"], 3)
        self.assertEqual(movement["authoritative"]["common_observations"], 2)
        self.assertEqual(movement["authoritative"]["byte_gained"], 1)
        self.assertEqual(movement["authoritative"]["byte_lost"], 1)
        self.assertEqual(movement["configured_source"]["common_observations"], 2)
        self.assertEqual(movement["configured_source"]["byte_gained"], 1)
        self.assertEqual(movement["configured_source"]["byte_lost"], 1)

    def test_brief_status_never_presents_the_work_queue_as_parity(self):
        rows = [row(source="src/a.c"), row(source="src/b.c")]
        observations = {
            rows[0]["configuration_id"]: {"status": "BYTE"},
            rows[1]["configuration_id"]: {"status": "DEFER"},
        }
        report = snapshot({"projects": []}, rows, observations, "fingerprint")
        report["work_frontier"] = work_frontier(
            rows,
            observations,
            {
                "configuration_ids": [item["configuration_id"] for item in rows],
                "universe_size": 2,
            },
        )
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            print_brief(report, None)
        rendered = output.getvalue()
        self.assertIn("0/2 configured TUs", rendered)
        self.assertIn("NOT RUN", rendered)
        self.assertIn("FAILURE-BIASED, NOT A PARITY ESTIMATE", rendered)

    def test_brief_status_exposes_audit_quality_unknowns_and_cost(self):
        rows = [
            row(source="src/empty.c", source_has_non_whitespace=False),
            row(source="src/exact.c"),
            row(source="src/deferred.c"),
            row(source="src/harness.c"),
        ]
        observations = {
            rows[0]["configuration_id"]: {
                "status": "BYTE",
                "elapsed_seconds": 1.0,
            },
            rows[1]["configuration_id"]: {
                "status": "BYTE",
                "elapsed_seconds": 2.0,
            },
            rows[2]["configuration_id"]: {
                "status": "DEFER",
                "elapsed_seconds": 3.0,
            },
            rows[3]["configuration_id"]: {
                "status": "HARNESS",
                "elapsed_seconds": 4.0,
            },
        }
        report = snapshot({"projects": []}, rows, observations, "fingerprint")
        report["representative_audit"] = representative_audit(
            rows,
            observations,
            {item["configuration_id"] for item in rows},
        )
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            print_brief(report, None)
        rendered = output.getvalue()
        self.assertIn("substantive-source audit", rendered)
        self.assertIn("whitespace-only rows excluded 1", rendered)
        self.assertIn("measurement-unknown attribution", rendered)
        self.assertIn("resolved exact 2/3", rendered)
        self.assertIn("emitted-object quality (conditional, not feature coverage)", rendered)
        self.assertIn("sampled compiler blocker families", rendered)
        self.assertIn("1x ", rendered)
        self.assertIn("audit execution cost", rendered)
        self.assertIn("summed 10.0s", rendered)

    def test_representative_audit_keeps_sample_diagnostics_separate(self):
        rows = [
            row(project="alpha", source="src/exact.c"),
            row(project="alpha", source="src/deferred.c"),
            row(project="beta", source="src/missing.c"),
        ]
        observations = {
            rows[0]["configuration_id"]: {"status": "BYTE"},
            rows[1]["configuration_id"]: {
                "status": "DEFER",
                "output": "DEFER deferred.c — loop codegen is not implemented yet",
            },
            rows[2]["configuration_id"]: {
                "status": "MISSING_DEPENDENCY",
                "output": "MISSING_DEPENDENCY missing.c — generated.inc",
            },
        }
        selected = {rows[0]["configuration_id"], rows[1]["configuration_id"]}
        report = representative_audit(rows, observations, selected)
        self.assertEqual([item["name"] for item in report["by_project"]], ["alpha"])
        self.assertEqual(report["by_project"][0]["total"], 2)
        self.assertEqual(report["blockers"][0]["status"], "DEFER")
        self.assertNotIn("generated.inc", report["blockers"][0]["reason"])

    def test_compiler_blocker_families_are_stable_architecture_categories(self):
        self.assertEqual(
            compiler_blocker_family(
                "DEFER", "a value live across a call needs the callee-saved register allocator"
            ),
            "backend lowering / registers / scheduling",
        )
        self.assertEqual(
            compiler_blocker_family(
                "DEFER", "expected ParenClose, found Comma at token 42"
            ),
            "front end / parsing and resolution",
        )
        self.assertEqual(
            compiler_blocker_family("DIFF", "object bytes differ"),
            "emitted object mismatch",
        )

    def test_blocker_family_breakdown_sums_diagnostics_without_measurement_failures(self):
        families = blocker_family_breakdown(
            [
                {
                    "status": "DEFER",
                    "reason": "loop codegen is not implemented yet",
                    "count": 3,
                    "examples": ["a/loop.c"],
                },
                {
                    "status": "DEFER",
                    "reason": "switch dispatch codegen is not implemented yet",
                    "count": 2,
                    "examples": ["b/switch.c"],
                },
                {
                    "status": "HARNESS",
                    "reason": "timed out after 30 seconds",
                    "count": 9,
                    "examples": ["c/slow.c"],
                },
            ]
        )
        self.assertEqual(len(families), 1)
        self.assertEqual(families[0]["family"], "control flow")
        self.assertEqual(families[0]["count"], 5)

    def test_exact_output_against_original_object_earns_credit_with_synthetic_input(self):
        observation = {
            "status": "BYTE",
            "evidence": {
                "oracle_direct": "RUNNABLE",
                "comparison_input": "SYNTHETIC",
                "reference_object": "DIRECT",
            },
        }
        self.assertEqual(authoritative_result(observation), "BYTE")

        observation["status"] = "DEFER"
        self.assertEqual(authoritative_result(observation), "UNKNOWN")

    def test_runtime_summary_reports_cost_distribution_and_missing_count(self):
        report = runtime_summary(
            [
                {"elapsed_seconds": 4.0, "observed_at": "2026-01-01T00:00:00+00:00"},
                {"elapsed_seconds": 1.0, "observed_at": "2026-01-01T00:00:01+00:00"},
                {},
                {"elapsed_seconds": 3.0, "observed_at": "2026-01-01T00:00:05+00:00"},
                {"elapsed_seconds": 2.0, "observed_at": "2026-01-01T00:00:06+00:00"},
            ]
        )
        self.assertEqual(report["measured"], 4)
        self.assertEqual(report["total_seconds"], 10.0)
        self.assertEqual(report["active_wall_seconds"], 7.0)
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
        self.assertEqual(report["authoritative_byte"], 0)

    def test_snapshot_full_corpus_lower_bound_requires_direct_evidence(self):
        rows = [row(source="src/direct.c"), row(source="src/synthetic.c")]
        observations = {
            rows[0]["configuration_id"]: {
                "status": "BYTE",
                "evidence": {"oracle_direct": "RUNNABLE", "comparison_input": "DIRECT"},
            },
            rows[1]["configuration_id"]: {
                "status": "BYTE",
                "evidence": {
                    "oracle_direct": "RUNNABLE",
                    "comparison_input": "SYNTHETIC",
                },
            },
        }
        report = snapshot({"projects": []}, rows, observations, "tool")
        self.assertEqual(report["statuses"]["BYTE"], 2)
        self.assertEqual(report["authoritative_byte"], 1)
        self.assertEqual(report["rates"]["byte_of_existing"], 0.5)
        self.assertEqual(report["goal_completion"]["authoritative_exact"], 1)
        self.assertEqual(report["goal_completion"]["configured_source_exact"], 0)
        self.assertEqual(report["goal_completion"]["projects_proven_complete"], 0)

    def test_goal_completion_does_not_credit_a_bridge_only_match(self):
        source = row(source="src/bridge-only.c")
        observation = {
            "status": "BYTE",
            "evidence": {
                "oracle_direct": "RUNNABLE",
                "comparison_input": "DIRECT",
                "reference_object": "DIRECT",
                "configured_source": "DEFER",
            },
        }
        goal = snapshot(
            {"projects": []},
            [source],
            {source["configuration_id"]: observation},
            "tool",
        )["goal_completion"]

        self.assertEqual(goal["authoritative_exact"], 1)
        self.assertEqual(goal["configured_source_exact"], 0)
        self.assertEqual(goal["by_project"][0]["remaining"], 1)
        self.assertEqual(goal["projects_proven_complete"], 0)

    def test_snapshot_exposes_projects_outside_the_mwcc_denominator(self):
        configured = row(project="configured")
        inventory = {
            "projects": [
                {"name": "configured", "status": "ok"},
                {"name": "n64_ido", "status": "no_mwcc_configure"},
                {"name": "broken", "status": "capture_error"},
            ],
            "translation_units": [configured],
        }

        report = snapshot(
            inventory,
            [configured],
            {},
            "tool",
            source_projects={"configured", "n64_ido", "broken"},
        )

        self.assertEqual(report["project_scope"]["discovered"], 3)
        self.assertEqual(report["project_scope"]["mwcc_configured"], 1)
        self.assertEqual(
            report["project_scope"]["without_mwcc_configure"], ["n64_ido"]
        )
        self.assertEqual(report["project_scope"]["capture_errors"], ["broken"])

    def test_goal_completion_requires_every_project_configuration(self):
        rows = [
            row(project="complete", source="src/a.c"),
            row(project="partial", source="src/a.c"),
            row(project="partial", source="src/b.c"),
        ]
        observations = {
            item["configuration_id"]: {
                "status": "BYTE",
                "evidence": {
                    "oracle_direct": "RUNNABLE",
                    "comparison_input": "DIRECT",
                    "configured_source": "BYTE",
                },
            }
            for item in rows[:2]
        }
        goal = snapshot({"projects": []}, rows, observations, "tool")[
            "goal_completion"
        ]
        self.assertEqual(goal["authoritative_exact"], 2)
        self.assertEqual(goal["configured_source_exact"], 2)
        self.assertEqual(goal["configurations"], 3)
        self.assertEqual(goal["projects_proven_complete"], 1)
        self.assertEqual(goal["projects"], 2)

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
        self.assertEqual(
            report["sample_configuration_ids"],
            sorted(item["configuration_id"] for item in rows),
        )
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

    def test_only_direct_original_tu_comparisons_earn_parity_credit(self):
        rows = [row(source=f"src/{index}.c") for index in range(5)]
        observations = {
            rows[0]["configuration_id"]: {
                "status": "BYTE",
                "evidence": {"oracle_direct": "RUNNABLE", "comparison_input": "DIRECT"},
            },
            rows[1]["configuration_id"]: {
                "status": "BYTE",
                "evidence": {
                    "oracle_direct": "REJECTED",
                    "comparison_input": "SYNTHETIC",
                },
            },
            rows[2]["configuration_id"]: {
                "status": "DEFER",
                "evidence": {"oracle_direct": "RUNNABLE", "comparison_input": "DIRECT"},
            },
            rows[3]["configuration_id"]: {
                "status": "DEFER",
                "evidence": {
                    "oracle_direct": "RUNNABLE",
                    "comparison_input": "SYNTHETIC",
                },
            },
            rows[4]["configuration_id"]: {
                "status": "MISSING_DEPENDENCY",
                "evidence": {"oracle_direct": "REJECTED"},
            },
        }
        estimate = representative_audit(
            rows, observations, {item["configuration_id"] for item in rows}
        )["estimate"]
        self.assertTrue(estimate["authoritative_provenance"])
        self.assertEqual(estimate["successes"], 1)
        self.assertEqual(estimate["known_nonparity"], 1)
        self.assertEqual(estimate["measurement_unknown"], 3)
        self.assertEqual(estimate["non_authoritative_unknown"], 2)
        self.assertEqual(estimate["oracle_runnable"], 3)
        self.assertEqual(estimate["oracle_runnable_proportion"], 3 / 5)
        self.assertEqual(estimate["oracle_runnable_known_nonparity"], 1)
        self.assertEqual(estimate["oracle_runnable_unknown"], 1)
        self.assertEqual(estimate["oracle_runnable_confirmed_proportion"], 1 / 3)
        self.assertEqual(estimate["oracle_runnable_identification_high"], 2 / 3)
        self.assertEqual(estimate["emitted_objects"], 1)

    def test_configured_source_probe_is_a_separate_drop_in_measure(self):
        rows = [row(source=f"src/{index}.c") for index in range(4)]
        observations = {
            rows[0]["configuration_id"]: {
                "status": "BYTE",
                "evidence": {
                    "oracle_direct": "RUNNABLE",
                    "comparison_input": "DIRECT",
                    "configured_source": "BYTE",
                },
            },
            rows[1]["configuration_id"]: {
                "status": "DEFER",
                "evidence": {
                    "oracle_direct": "RUNNABLE",
                    "comparison_input": "SYNTHETIC",
                    "configured_source": "DEFER",
                },
            },
            rows[2]["configuration_id"]: {
                "status": "DIFF",
                "evidence": {
                    "oracle_direct": "RUNNABLE",
                    "comparison_input": "SYNTHETIC",
                    "configured_source": "DIFF",
                },
            },
            rows[3]["configuration_id"]: {
                "status": "MISSING_DEPENDENCY",
                "evidence": {"oracle_direct": "REJECTED"},
            },
        }
        estimate = representative_audit(
            rows, observations, {item["configuration_id"] for item in rows}
        )["estimate"]
        self.assertTrue(estimate["configured_source_provenance"])
        self.assertEqual(estimate["configured_source_exact"], 1)
        self.assertEqual(estimate["configured_source_known_nonparity"], 2)
        self.assertEqual(estimate["configured_source_unknown"], 1)

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

    def test_fixed_audit_covers_every_project_version_language_cell(self):
        rows = [row(source=f"src/common-{index}.c") for index in range(20)]
        rare = row(
            project="rare-project",
            source="src/rare.cpp",
            language="c++",
            mw_version="GC/1.1",
        )
        rows.append(rare)
        audit = build_audit(rows, 1, "seed", "0")
        cells = {
            (cell["project"], cell["mw_version"], cell["language"]): cell[
                "configuration_id"
            ]
            for cell in audit["coverage_cells"]
        }
        self.assertEqual(
            set(cells),
            {("project", "GC/2.6", "c"), ("rare-project", "GC/1.1", "c++")},
        )
        self.assertIn(rare["configuration_id"], audit["configuration_ids"])
        self.assertEqual(
            len(audit["configuration_ids"]), len(set(audit["configuration_ids"]))
        )


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

    def test_core_byte_with_configured_source_failure_reenters_work_pool(self):
        rows = [row(source=f"src/{index}.c") for index in range(4)]
        observations = {
            rows[0]["configuration_id"]: {
                "status": "BYTE",
                "evidence": {"configured_source": "DEFER"},
            },
            rows[1]["configuration_id"]: {
                "status": "BYTE",
                "evidence": {"configured_source": "BYTE"},
            },
        }
        args = argparse.Namespace(size=1, byte_audit=0, seed="seed", epoch="0")
        frontier = build_frontier(rows, observations, args)
        self.assertEqual(
            frontier["configuration_ids"], [rows[0]["configuration_id"]]
        )
        self.assertEqual(
            frontier["previous_status_counts"], {"DEFER": 1}
        )

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
