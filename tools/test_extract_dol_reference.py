import struct
import unittest

from extract_dol_reference import function_range, read_dol_range


class DolReferenceTests(unittest.TestCase):
    def image(self):
        data = bytearray(0x120)
        struct.pack_into(">I", data, 0, 0x100)
        struct.pack_into(">I", data, 0x48, 0x80004000)
        struct.pack_into(">I", data, 0x90, 0x20)
        data[0x100:] = bytes(range(32))
        return data

    def test_virtual_range_uses_section_offset(self):
        self.assertEqual(read_dol_range(self.image(), 0x80004008, 8), bytes(range(8, 16)))

    def test_range_cannot_cross_section_or_wrap(self):
        for address, size in [(0x80004018, 12), (0x80003FFC, 8), (0xFFFFFFFF, 4), (0, 0)]:
            with self.subTest(address=address, size=size), self.assertRaises(ValueError):
                read_dol_range(self.image(), address, size)

    def test_truncated_data_and_ambiguous_sections_are_rejected(self):
        with self.assertRaises(ValueError):
            read_dol_range(self.image()[:-1], 0x80004000, 4)
        with self.assertRaises(ValueError):
            read_dol_range(b"", 0x80004000, 4)
        data = self.image()
        for table in [0, 0x48, 0x90]:
            data[table + 4:table + 8] = data[table:table + 4]
        with self.assertRaises(ValueError):
            read_dol_range(data, 0x80004000, 4)

    def test_symbol_must_be_unique_sized_function(self):
        row = "f__Fv = .text:0x80004008; // type:function size:0x8 scope:global\n"
        self.assertEqual(function_range(row, "f__Fv"), (0x80004008, 8))
        for invalid in [row + row, row.replace("type:function", "type:object"), row.replace("size:0x8", "")]:
            with self.subTest(symbols=invalid), self.assertRaises(ValueError):
                function_range(invalid, "f__Fv")


if __name__ == "__main__":
    unittest.main()
