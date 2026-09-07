"""Narrow Gekko paired-single support for instruction-level Unicorn probes.

Only non-updating psq_l/psq_st and an explicitly zero GQR are supported.
W=0 transfers both lanes; W=1 transfers lane zero and fills load lane one with 1.
Finite binary32 values (including signed zero and subnormals) round-trip through
Unicorn's ordinary FPR lane; the second lane is tracked separately. Arithmetic,
quantization, and non-finite operands are deliberately outside this model.
"""

import math
import struct


class UnquantizedPairedSingle:
    def __init__(self, uc, *, gqr_values, second_lanes=None, on_store=None):
        from unicorn import UC_HOOK_CODE
        from unicorn.ppc_const import UC_PPC_REG_0, UC_PPC_REG_FPR0, UC_PPC_REG_PC
        self.uc = uc
        self.gqr_values = dict(gqr_values)
        self.on_store = on_store
        self.second = dict(second_lanes or {})
        self.instructions = 0
        self.gpr0 = UC_PPC_REG_0
        self.fpr0 = UC_PPC_REG_FPR0
        self.pc = UC_PPC_REG_PC
        self.hook = uc.hook_add(UC_HOOK_CODE, self._step)

    def _step(self, uc, address, size, _data):
        word = int.from_bytes(uc.mem_read(address, 4), 'big')
        opcode = word >> 26
        if opcode not in (56, 60):
            return
        register, base = (word >> 21) & 31, (word >> 16) & 31
        w, gqr = (word >> 15) & 1, (word >> 12) & 7
        if self.gqr_values.get(gqr) != 0:
            raise ValueError(f'unsupported paired-single format at {address:#x}: {word:#x}')
        displacement = word & 4095
        if displacement & 2048:
            displacement -= 4096
        effective = ((uc.reg_read(self.gpr0 + base) if base else 0) + displacement) & 0xffffffff
        if opcode == 56:
            if w:
                first = struct.unpack('>f', uc.mem_read(effective, 4))[0]
                second = 1.0
            else:
                first, second = struct.unpack('>2f', uc.mem_read(effective, 8))
            if not math.isfinite(first) or not math.isfinite(second):
                raise ValueError('non-finite paired-single loads are not modeled')
            uc.reg_write(self.fpr0 + register, int.from_bytes(struct.pack('>d', first), 'big'))
            self.second[register] = second
        else:
            first = struct.unpack('>d', uc.reg_read(self.fpr0 + register).to_bytes(8, 'big'))[0]
            if not math.isfinite(first) or (not w and (register not in self.second
                    or not math.isfinite(self.second[register]))):
                raise ValueError('paired-single store needs initialized finite lanes')
            raw = struct.pack('>f', first) if w else struct.pack('>2f', first, self.second[register])
            uc.mem_write(effective, raw)
            if self.on_store is not None:
                self.on_store(effective, raw)
        self.instructions += 1
        uc.reg_write(self.pc, address + 4)
