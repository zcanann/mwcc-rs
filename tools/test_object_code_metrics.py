#!/usr/bin/env python3

import unittest
from unittest.mock import patch
import subprocess
from pathlib import Path

from object_code_metrics import (
    FunctionMatch,
    TextFunction,
    TextRelocation,
    describe_function_delta,
    describe_function_parity,
    function_parity,
    parse_section_bytes,
    parse_text_functions,
    parse_text_relocations,
    run_objdump,
    statuses,
)


class ObjectCodeMetricsTests(unittest.TestCase):
    @patch("object_code_metrics.subprocess.run")
    def test_missing_text_section_is_empty_not_a_metric_failure(self, run):
        run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=1,
            stdout="data.o: file format elf32-powerpc\n",
            stderr="objdump: section '.text' mentioned in a -j option, but not found\n",
        )
        self.assertEqual(run_objdump(Path("objdump"), "-s", "-j", ".text", "data.o"), run.return_value.stdout)

    def test_parses_text_without_ascii_column(self):
        output = """
Contents of section .text:
 0000 38600000 4e800020                    8`..N.. 
"""
        self.assertEqual(parse_section_bytes(output), bytes.fromhex("386000004e800020"))

    def test_classifies_anonymous_ordinal_separately(self):
        reference = [
            TextRelocation(0x24, "R_PPC_EMB_SDA21", "@79"),
            TextRelocation(0x64, "R_PPC_REL24", "callee__Ff"),
        ]
        candidate = [
            TextRelocation(0x24, "R_PPC_EMB_SDA21", "@22"),
            TextRelocation(0x64, "R_PPC_REL24", "callee__Ff"),
        ]
        result = statuses(b"same", b"same", reference, candidate)
        self.assertIn("TEXT_BYTES BYTE — raw .text bytes match", result)
        self.assertIn("TEXT_RELOC_SHAPE BYTE — relocation offsets and types match", result)
        self.assertIn(
            "TEXT_RELOC_TARGETS DIFF — first difference at relocation 0: reference @79, candidate @22",
            result,
        )
        self.assertIn(
            "ANON_ORDINALS DIFF — anonymous symbol numbers are the only relocation-target difference",
            result,
        )
        self.assertTrue(result[0].startswith("CODE DIFF"))

    def test_parses_relocation_shape_and_target(self):
        output = """
RELOCATION RECORDS FOR [.text]:
OFFSET   TYPE              VALUE
00000024 R_PPC_EMB_SDA21   @79
00000064 R_PPC_REL24       cBgW_CheckBGround__Ff
"""
        self.assertEqual(
            parse_text_relocations(output),
            [
                TextRelocation(0x24, "R_PPC_EMB_SDA21", "@79"),
                TextRelocation(0x64, "R_PPC_REL24", "cBgW_CheckBGround__Ff"),
            ],
        )

    def test_parses_defined_text_function_symbols(self):
        output = """
00000000 g     F .text  00000008 public__Fv
00000008  w    F .text  00000004 weak__Fv
00000000         *UND*  00000000 external__Fv
00000000 g     O .data  00000004 datum
"""
        self.assertEqual(
            parse_text_functions(output),
            [
                TextFunction(0, 8, "public__Fv"),
                TextFunction(8, 4, "weak__Fv"),
            ],
        )

    def test_function_parity_is_position_independent_and_relocation_aware(self):
        reference_bytes = bytes.fromhex("01020304 aabbccdd 05060708")
        candidate_bytes = bytes.fromhex("aabbccdd 01020304 05060709")
        reference_functions = [
            TextFunction(0, 4, "first"),
            TextFunction(4, 4, "second"),
            TextFunction(8, 4, "missing"),
        ]
        candidate_functions = [
            TextFunction(4, 4, "first"),
            TextFunction(0, 4, "second"),
            TextFunction(8, 4, "extra"),
        ]
        parity = function_parity(
            reference_bytes,
            candidate_bytes,
            [TextRelocation(0, "R_PPC_REL24", "callee_a")],
            [TextRelocation(4, "R_PPC_REL24", "callee_b")],
            reference_functions,
            candidate_functions,
        )
        self.assertEqual(parity.text_exact_functions, 2)
        self.assertEqual(parity.code_exact_functions, 1)
        self.assertEqual(parity.text_exact_reference_bytes, 8)
        self.assertEqual(parity.code_exact_reference_bytes, 4)
        self.assertEqual(parity.missing_functions, 1)
        self.assertEqual(parity.candidate_only_functions, 1)
        self.assertIn("2/3 functions exact", describe_function_parity(parity, False))
        self.assertIn(
            "1/3 relocation-aware functions exact",
            describe_function_parity(parity, True),
        )

    def test_function_parity_ignores_object_local_anonymous_ordinals(self):
        parity = function_parity(
            bytes.fromhex("c0200000"),
            bytes.fromhex("c0200000"),
            [TextRelocation(0, "R_PPC_EMB_SDA21", "@277")],
            [TextRelocation(0, "R_PPC_EMB_SDA21", "@243")],
            [TextFunction(0, 4, "load_literal")],
            [TextFunction(0, 4, "load_literal")],
        )

        self.assertEqual(parity.text_exact_functions, 1)
        self.assertEqual(parity.code_exact_functions, 1)

    def test_describes_paired_function_gains_and_regressions(self):
        baseline = {
            "still_exact": FunctionMatch(4, True, True, True),
            "new_coverage": FunctionMatch(12, False, False, False),
            "new_text": FunctionMatch(8, True, False, False),
            "regressed": FunctionMatch(16, True, True, True),
        }
        candidate = {
            "still_exact": FunctionMatch(4, True, True, True),
            "new_coverage": FunctionMatch(12, True, False, False),
            "new_text": FunctionMatch(8, True, True, False),
            "regressed": FunctionMatch(16, True, False, False),
        }

        self.assertEqual(
            describe_function_delta(baseline, candidate),
            [
                "FUNCTION_COVERAGE_DELTA +1 functions, +12 reference bytes; "
                "gained: new_coverage; regressed: none",
                "FUNCTION_TEXT_DELTA +0 functions, -8 reference bytes; "
                "gained: new_text; regressed: regressed",
                "FUNCTION_CODE_DELTA -1 functions, -16 reference bytes; "
                "gained: none; regressed: regressed",
            ],
        )


if __name__ == "__main__":
    unittest.main()
