import hashlib
import struct
import unittest

from compare_linked_dol import compare, relocate


class LinkedDolTests(unittest.TestCase):
    def fixture(self, kind=109, literal=False, duplicate=False):
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
        if duplicate:
            entries += sym.pack(5, 0, 4, 0x11, 0, 6)
        blobs = [b'', struct.pack('>II', 0x80600000, 0x4E800020), entries,
                 b'\0f\0g\0j\0', names, struct.pack('>IIi', 2, (2 << 8) | kind, 0), bytes.fromhex('44800000')]
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

    def bss_alias_fixture(self):
        data, dol, source, layout = self.fixture(literal=True)
        data = bytearray(data.replace(b'.sdata2\0', b'.sbss\0\0\0').replace(b'\0f\0g\0', b'\0f\0h\0'))
        shoff = struct.unpack_from('>I', data, 32)[0]
        struct.pack_into('>I', data, shoff + 6 * 40 + 4, 8)
        dol = bytearray(dol)
        struct.pack_into('>II', dol, 0xD8, 0x80004010, 4)
        source = source.replace('.sdata2:', '.sbss:')
        layout.update(dol_sha256=hashlib.sha256(dol).hexdigest(),
                      symbols_sha256=hashlib.sha256(source.encode()).hexdigest(),
                      literals=[], data_symbols={'g': 0x80004010},
                      bss_symbol_aliases={'h': 'g'})
        return bytes(data), bytes(dol), source, layout

    def test_explicit_bss_alias_checks_section_size_and_original_range(self):
        data, dol, source, layout = self.bss_alias_fixture()
        self.assertTrue(compare(data, dol, source, layout)['functions'][0]['exact'])
        bad = bytearray(dol)
        struct.pack_into('>I', bad, 0xDC, 0)
        layout['dol_sha256'] = hashlib.sha256(bad).hexdigest()
        with self.assertRaises(ValueError):
            compare(data, bytes(bad), source, layout)
        layout['dol_sha256'] = hashlib.sha256(dol).hexdigest()
        bad = bytearray(data)
        shoff = struct.unpack_from('>I', bad, 32)[0]
        struct.pack_into('>I', bad, shoff + 6 * 40 + 4, 1)
        with self.assertRaises(ValueError):
            compare(bytes(bad), dol, source, layout)
        bad = bytearray(data)
        symoff = struct.unpack_from('>I', bad, shoff + 2 * 40 + 16)[0]
        struct.pack_into('>I', bad, symoff + 2 * 16 + 8, 8)
        with self.assertRaises(ValueError):
            compare(bytes(bad), dol, source, layout)

    def test_bss_alias_rejects_missing_and_conflicting_symbols(self):
        data, dol, source, layout = self.bss_alias_fixture()
        for aliases in [{'missing': 'g'}, {'g': 'g'}, {'h': 'unverified'}]:
            layout['bss_symbol_aliases'] = aliases
            with self.assertRaises(ValueError):
                compare(data, dol, source, layout)

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

    def test_external_call_is_verified_without_becoming_a_candidate_function(self):
        data, dol, source, layout = self.fixture(kind=10)
        data, dol = bytearray(data), bytearray(dol)
        struct.pack_into('>I', data, 52, 0x48000001)
        struct.pack_into('>I', dol, 0x100, 0x48000011)
        source = source.replace('g = .sdata2:', 'g = .text:').replace('type:object', 'type:function')
        layout.update(dol_sha256=hashlib.sha256(dol).hexdigest(),
                      symbols_sha256=hashlib.sha256(source.encode()).hexdigest(),
                      data_symbols={}, external_functions=['g'])
        results = compare(bytes(data), bytes(dol), source, layout)['functions']
        self.assertEqual(len(results), 1)
        self.assertTrue(results[0]['exact'])
        layout['external_functions'] = ['missing_helper']
        with self.assertRaises(ValueError):
            compare(bytes(data), bytes(dol), source, layout)

    def test_initialized_data_ignores_local_ordinals_but_verifies_range_and_bytes(self):
        data, dol, source, layout = self.fixture(literal=True)
        data = data.replace(b'.sdata2\0', b'.data\0\0\0').replace(b'\0f\0g\0', b'\0f\0h\0')
        source = source.replace('.sdata2:', '.data:')
        layout['symbols_sha256'] = hashlib.sha256(source.encode()).hexdigest()
        layout['data_images'] = [dict(reference_symbol='g', **layout['literals'][0])]
        layout['literals'] = []
        self.assertTrue(compare(data, dol, source, layout)['functions'][0]['exact'])
        layout['data_images'][0]['address'] += 4
        with self.assertRaises(ValueError):
            compare(data, dol, source, layout)
        layout['data_images'][0]['address'] -= 4
        layout['data_images'][0]['hex'] = '00000000'
        with self.assertRaises(ValueError):
            compare(data, dol, source, layout)

    def test_address_halves_preserve_opcode_and_apply_signed_low_carry(self):
        self.assertEqual(relocate(0x3C600000, 5, 0x80008010, 0, {}), 0x3C608000)
        self.assertEqual(relocate(0x3C600000, 6, 0x80008010, 0, {}), 0x3C608001)
        self.assertEqual(relocate(0x38630000, 4, 0x80008010, 0, {}), 0x38638010)
        self.assertEqual(relocate(0x3C600000, 6, 0x80007FFF, 0, {}), 0x3C608000)

    def test_equal_initialized_objects_do_not_alias(self):
        data, dol, source, layout = self.fixture(literal=True, duplicate=True)
        data = data.replace(b'.sdata2\0', b'.data\0\0\0')
        source = source.replace('.sdata2:', '.data:')
        layout['symbols_sha256'] = hashlib.sha256(source.encode()).hexdigest()
        layout['data_images'] = [dict(reference_symbol='g', **layout['literals'][0])]
        layout['literals'] = []
        result = compare(data, dol, source, layout)['functions'][0]
        self.assertFalse(result['exact'])
        self.assertIn('ambiguous', result['unresolved_relocations'][0]['reason'])

    def test_sda_requires_one_valid_base(self):
        self.assertEqual(relocate(0x80600000, 109, 0x100010, 0, {'13': 0x100020}), 0x806DFFF0)
        for bases in [{}, {'2': 0}, {'2': 0x100000, '13': 0x100020}, {'3': 0x100000}]:
            with self.assertRaises(ValueError):
                relocate(0x80600000, 109, 0x100010, 0, bases)

    def jump_table_fixture(self, fixups=((0, 1, 1, 4),)):
        data, dol, source, layout = self.fixture(literal=True)
        data = data.replace(b'.sdata2\0', b'.data\0\0\0')
        source = source.replace('.sdata2:', '.data:')
        shoff = struct.unpack_from('>I', data, 32)[0]
        headers = [list(struct.unpack_from('>10I', data, shoff + i * 40)) for i in range(7)]
        blobs = [data[h[4]:h[4]+h[5]] for h in headers]
        name_offset = len(blobs[4])
        blobs[4] += b'.rela.data\0'
        blobs[6] = bytes(4)
        blobs.append(b''.join(struct.pack('>IIi', at, (symbol << 8) | kind, addend)
                              for at, kind, symbol, addend in fixups))
        headers.append([name_offset, 4, 0, 0, 0, 0, 2, 6, 4, 12])
        linked = bytearray(data[:52])
        for header, blob in zip(headers, blobs):
            header[4:6] = [len(linked), len(blob)]
            linked.extend(blob)
        struct.pack_into('>I', linked, 32, len(linked))
        struct.pack_into('>H', linked, 48, len(headers))
        for header in headers:
            linked.extend(struct.pack('>10I', *header))
        dol = bytearray(dol)
        struct.pack_into('>I', dol, 0x110, 0x80004004)
        layout.update(dol_sha256=hashlib.sha256(dol).hexdigest(),
                      symbols_sha256=hashlib.sha256(source.encode()).hexdigest(), literals=[],
                      data_images=[dict(reference_symbol='g', address=0x80004010, hex='80004004')])
        return bytes(linked), bytes(dol), source, layout

    def test_jump_table_identity_uses_verified_relocated_function_targets(self):
        self.assertTrue(compare(*self.jump_table_fixture())['functions'][0]['exact'])
        data, dol, source, layout = self.jump_table_fixture(((0, 1, 1, 0),))
        result = compare(data, dol, source, layout)['functions'][0]
        self.assertFalse(result['exact'])
        self.assertIn('missing', result['unresolved_relocations'][0]['reason'])

    def test_invalid_or_overlapping_jump_table_relocations_cannot_match(self):
        for fixups in [((0, 4, 1, 4),), ((2, 1, 1, 4),), ((0, 1, 1, 8),),
                       ((0, 1, 1, -4),), ((0, 1, 1, 2),), ((0, 1, 2, 0),),
                       ((0, 1, 1, 4), (0, 1, 1, 4))]:
            with self.subTest(fixups=fixups):
                result = compare(*self.jump_table_fixture(fixups))['functions'][0]
                self.assertFalse(result['exact'])
                self.assertTrue(result['unresolved_relocations'])

    def test_named_small_data_section_selects_its_abi_base(self):
        data, dol, source, layout = self.fixture()
        layout['sda_bases']['13'] = layout['sda_bases']['2'] + 16
        self.assertTrue(compare(data, dol, source, layout)['functions'][0]['exact'])
        source = source.replace('.sdata2:', '.sdata:')
        layout['symbols_sha256'] = hashlib.sha256(source.encode()).hexdigest()
        self.assertFalse(compare(data, dol, source, layout)['functions'][0]['exact'])


if __name__ == '__main__':
    unittest.main()
