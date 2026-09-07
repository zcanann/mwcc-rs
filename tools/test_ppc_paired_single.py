import struct
import unittest

try:
    from unicorn import Uc, UC_ARCH_PPC, UC_MODE_32, UC_MODE_BIG_ENDIAN
    from unicorn.ppc_const import UC_PPC_REG_0, UC_PPC_REG_FPR0, UC_PPC_REG_MSR, UC_PPC_REG_LR
    from ppc_paired_single import UnquantizedPairedSingle
except ImportError:
    Uc = None


@unittest.skipIf(Uc is None, 'Unicorn is required for paired-single execution tests')
class PairedSingleTests(unittest.TestCase):
    def machine(self, code, values, gqr_values=None):
        uc = Uc(UC_ARCH_PPC, UC_MODE_32 | UC_MODE_BIG_ENDIAN)
        uc.mem_map(0x10000, 0x10000)
        uc.mem_map(0x100000, 0x10000)
        uc.mem_write(0x10000, b''.join(word.to_bytes(4, 'big') for word in code) + bytes.fromhex('4e800020'))
        uc.mem_write(0x100010, values)
        uc.reg_write(UC_PPC_REG_0 + 3, 0x100018)
        uc.reg_write(UC_PPC_REG_0 + 4, 0x100100)
        uc.reg_write(UC_PPC_REG_MSR, 0x2000)
        uc.reg_write(UC_PPC_REG_LR, 0x1f000)
        writes = []
        model = UnquantizedPairedSingle(uc, gqr_values={0: 0} if gqr_values is None else gqr_values,
                                      on_store=lambda address, raw: writes.append((address, raw)))
        return uc, model, writes

    def test_round_trips_negative_displacement_and_finite_edge_values(self):
        # psq_l f2,-8(r3),0,0; psq_st f2,0(r4),0,0
        for raw in [struct.pack('>2f', 1.25, -2.5), bytes.fromhex('8000000000000000'),
                    bytes.fromhex('00000001007fffff'), bytes.fromhex('7f7fffffff7fffff')]:
            with self.subTest(raw=raw.hex()):
                uc, model, writes = self.machine([0xe0430ff8, 0xf0440000], raw)
                uc.emu_start(0x10000, 0x1f000, count=20)
                self.assertEqual(bytes(uc.mem_read(0x100100, 8)), raw)
                self.assertEqual(writes, [(0x100100, raw)])
                self.assertEqual(model.instructions, 2)

    def test_store_observes_a_native_first_lane_update(self):
        # fneg f2,f2 between the paired load and store changes only lane zero.
        uc, _, _ = self.machine([0xe0430ff8, 0xfc401050, 0xf0440000], struct.pack('>2f', 1.25, 2.5))
        uc.emu_start(0x10000, 0x1f000, count=20)
        self.assertEqual(bytes(uc.mem_read(0x100100, 8)), struct.pack('>2f', -1.25, 2.5))

    def test_single_element_transfer_fills_second_load_lane(self):
        uc, model, writes = self.machine([0xe0438ff8, 0xf0448000], struct.pack('>2f', -1.25, 7.5))
        uc.emu_start(0x10000, 0x1f000, count=20)
        self.assertEqual(model.second[2], 1.0)
        self.assertEqual(writes, [(0x100100, struct.pack('>f', -1.25))])
        self.assertEqual(bytes(uc.mem_read(0x100104, 4)), bytes(4))

    def test_rejects_unmodeled_formats_and_nonfinite_data(self):
        for word, gqr, raw in [(0xe0431ff8, {0: 0}, bytes(8)),
                               (0xe0430ff8, {0: 4}, bytes(8)),
                               (0xe0430ff8, {}, bytes(8)),
                               (0xe0430ff8, {0: 0}, bytes.fromhex('7f80000000000000'))]:
            with self.subTest(word=word, gqr=gqr, raw=raw.hex()):
                uc, _, _ = self.machine([word], raw, gqr)
                with self.assertRaises(ValueError):
                    uc.emu_start(0x10000, 0x1f000, count=20)

    def test_caller_second_lane_can_be_saved_before_a_load(self):
        uc, model, _ = self.machine([0xf0440000], bytes(8))
        model.second[2] = 2.5
        uc.reg_write(UC_PPC_REG_FPR0 + 2, int.from_bytes(struct.pack('>d', 1.25), 'big'))
        uc.emu_start(0x10000, 0x1f000, count=20)
        self.assertEqual(bytes(uc.mem_read(0x100100, 8)), struct.pack('>2f', 1.25, 2.5))

    def test_rejects_store_without_a_second_lane(self):
        uc, _, _ = self.machine([0xf0440000], bytes(8))
        with self.assertRaises(ValueError):
            uc.emu_start(0x10000, 0x1f000, count=20)


if __name__ == '__main__':
    unittest.main()
