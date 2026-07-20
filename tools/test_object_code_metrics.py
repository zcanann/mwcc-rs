#!/usr/bin/env python3

import unittest
from unittest.mock import patch
import subprocess
from pathlib import Path

from object_code_metrics import (
    TextRelocation,
    parse_section_bytes,
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


if __name__ == "__main__":
    unittest.main()
