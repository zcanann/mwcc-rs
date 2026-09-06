import unittest

from float_snapshot_graph import UnsupportedGraph, compare_graphs, parse_graph


def assembly(*instructions):
    return "\n".join(f"{index * 4:x}: 00 00 00 00 {text}" for index, text in enumerate(instructions))


class SnapshotGraphTests(unittest.TestCase):
    def test_schedule_and_registers_are_independent_of_operation_identity(self):
        reference = parse_graph(assembly("lfs f3,0(r4)", "lfs f4,4(r4)",
                                        "fmuls f0,f3,f1", "fmuls f1,f4,f2",
                                        "stfs f0,0(r3)", "stfs f1,4(r3)", "blr"))
        candidate = parse_graph(assembly("lfs f5,4(r4)", "fmuls f6,f5,f2",
                                        "lfs f7,0(r4)", "fmuls f8,f7,f1",
                                        "stfs f8,0(r3)", "stfs f6,4(r3)", "blr"))
        result = compare_graphs(reference, candidate)
        self.assertTrue(result["graph_equal"])
        self.assertEqual(result["reference_order"], [2, 0, 3, 1, 4, 5])

    def test_read_reordering_across_a_store_changes_memory_identity(self):
        before = parse_graph(assembly("lfs f0,0(r4)", "lfs f1,4(r4)",
                                     "stfs f0,0(r3)", "stfs f1,4(r3)", "blr"))
        after = parse_graph(assembly("lfs f0,0(r4)", "stfs f0,0(r3)",
                                    "lfs f1,4(r4)", "stfs f1,4(r3)", "blr"))
        self.assertFalse(compare_graphs(before, after)["graph_equal"])

    def test_repeated_reads_remain_distinct_and_arithmetic_keeps_operand_order(self):
        graph = parse_graph(assembly("lfs f0,0(r4)", "lfs f1,0(r4)",
                                     "fsubs f2,f0,f1", "stfs f2,0(r3)", "blr"))
        reverse = parse_graph(assembly("lfs f0,0(r4)", "lfs f1,0(r4)",
                                       "fsubs f2,f1,f0", "stfs f2,0(r3)", "blr"))
        self.assertNotEqual(graph[0]["identity"], graph[1]["identity"])
        self.assertFalse(compare_graphs(graph, reverse)["graph_equal"])
        self.assertEqual(graph[3]["extra_deps"], [0, 1])

    def test_unsupported_instructions_are_not_silently_discarded(self):
        for instruction in ["lwz r4,0(r4)", "b 0", "fadd f0,f1,f2", ".long 0xffffffff"]:
            with self.assertRaises(UnsupportedGraph):
                parse_graph(assembly("lfs f0,0(r4)", instruction, "blr"))
        with self.assertRaises(UnsupportedGraph):
            parse_graph(assembly("lfs f0,0(r4)", "blr", "lfs f1,0(r4)"))
        with self.assertRaises(UnsupportedGraph):
            parse_graph(assembly("lfs f0,0(r4)") + "\n4: ff .byte 0xff\n" + assembly("blr"))


if __name__ == "__main__":
    unittest.main()
