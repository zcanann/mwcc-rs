import hashlib
import struct
import unittest

from compare_linked_dol import compare, relocate


class LinkedDolTests(unittest.TestCase):
    def fixture(self, kind=109, literal=False):
        address = 0x80004000
        dol = bytearray(0x120)
        for table, value in [(0, 0x100), (0x48, address), (0x90, 0x20)]:
            struct.pack_into('>I', dol, table, value)
        struct.pack_into('>II', dol, 0x100, 0x8062FFF0, 0x4E800020)
        dol[0x110:0x114] = bytes.fromhex('44800000')
        source = 'f = .text:0x80004000; // type:function size:0x8\ng = .sdata2:0x80004010; // type:object size:0x4\n'
        names = b'\0.text\0.symtab\0.strtab\0.shstrtab\0.rela.text\0.sdata2\0'
        sym = struct.Struct('>IIIBBH')
        entries = bytes(16) + sym.pack(1, 0, 8, 0x12, 0, 1)
        entries += sym.pack(3, 0, 4 if literal else 0, 0x11, 0, 6 if literal else 0)
        blobs = [b'', struct.pack('>II', 0x80600000, 0x4E800020), entries,
                 b'\0f\0g\0', names, struct.pack('>IIi', 2, (2 << 8) | kind, 0), bytes.fromhex('44800000')]
        section_names = ['', '.text', '.symtab', '.strtab', '.shstrtab', '.rela.text', '.sdata2']
        data = bytearray(52)
        headers = []
        for index, blob in enumerate(blobs):
            headers.append((0 if not index else names.index(section_names[index].encode()),
                            [0, 1, 2, 3, 3, 4, 1][index], 0, 0, len(data), len(blob),
                            3 if index == 2 else 2 if index == 5 else 0,
                            1 if index == 5 else 0, 4, 16 if index == 2 else 12 if index == 5 else 0))
            data.extend(blob)
        shoff = len(data)
        for header in headers:
            data.extend(struct.pack('>10I', *header))
        struct.pack_into('>16sHHIIIIIHHHHHH', data, 0, b'\x7fELF\x01\x02\x01'+bytes(9),
                         1, 20, 1, 0, 0, shoff, 0, 52, 0, 0, 40, len(headers), 4)
        layout = dict(dol_sha256=hashlib.sha256(dol).hexdigest(),
                      symbols_sha256=hashlib.sha256(source.encode()).hexdigest(), functions=['f'],
                      data_symbols={} if literal else {'g': address+16}, sda_bases={'2': address+32},
                      literals=[dict(hex='44800000', address=address+16)] if literal else [])
        return bytes(data), bytes(dol), source, layout

    def test_named_data_and_verified_literal_link_to_original(self):
        for literal in [False, True]:
            with self.subTest(literal=literal):
                result = compare(*self.fixture(literal=literal))['functions'][0]
                self.assertTrue(result['exact'])
                self.assertEqual(result['candidate_linked_sha256'], result['reference_sha256'])

    def test_unknown_relocation_and_changed_instruction_are_not_exact(self):
        result = compare(*self.fixture(kind=42))['functions'][0]
        self.assertFalse(result['exact'])
        self.assertEqual(result['unresolved_relocations'][0]['kind'], 42)
        data, *rest = self.fixture()
        data = bytearray(data)
        data[59] ^= 1  # Change the return instruction, outside the relocation.
        self.assertFalse(compare(bytes(data), *rest)['functions'][0]['exact'])

    def test_reference_pins_and_literal_contents_are_enforced(self):
        data, dol, source, layout = self.fixture(literal=True)
        with self.assertRaises(ValueError):
            compare(data, dol, source+'\n', layout)
        layout['literals'][0]['hex'] = '00000000'
        with self.assertRaises(ValueError):
            compare(data, dol, source, layout)

    def test_missing_function_and_wrong_data_address_cannot_match(self):
        data, dol, source, layout = self.fixture()
        layout['data_symbols']['g'] += 4
        with self.assertRaises(ValueError):
            compare(data, dol, source, layout)
        data, dol, source, layout = self.fixture()
        data = data.replace(b'\0f\0g\0', b'\0x\0g\0')
        self.assertFalse(compare(data, dol, source, layout)['functions'][0]['exact'])

    def test_branch_preserves_link_bit_and_checks_range(self):
        self.assertEqual(relocate(0x48000001, 10, 0x80004000, 0x80004010, {}), 0x4BFFFFF1)
        for target in [0x80004001, 0x82004000]:
            with self.assertRaises(ValueError):
                relocate(0x48000001, 10, target, 0x80004000, {})

    def test_sda_requires_one_valid_base(self):
        self.assertEqual(relocate(0x80600000, 109, 0x100010, 0, {'13': 0x100020}), 0x806DFFF0)
        for bases in [{}, {'2': 0}, {'2': 0x100000, '13': 0x100020}, {'3': 0x100000}]:
            with self.assertRaises(ValueError):
                relocate(0x80600000, 109, 0x100010, 0, bases)


if __name__ == '__main__':
    unittest.main()
