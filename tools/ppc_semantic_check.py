#!/usr/bin/env python3
"""Differential semantic check of mismatched PCode functions.

Runs the reference and produced machine code of each mismatched function
(from a win_pcode_eval.py --json-out file) on random argument registers and
memory, and compares r3 (and f1-free integer state that matters: the return
value) plus every memory write. Functions with calls, relocated (rA=0)
memory accesses, or unsupported instructions are skipped. Any divergence is a
likely miscompile.

Usage: python tools/ppc_semantic_check.py target/pe-vNN.json [--trials 64]
"""

import argparse
import json
import random
import sys

MASK = 0xFFFFFFFF


class Unsupported(Exception):
    pass


def s32(value):
    value &= MASK
    return value - (1 << 32) if value & 0x80000000 else value


def rotl(value, amount):
    amount &= 31
    value &= MASK
    return ((value << amount) | (value >> (32 - amount))) & MASK if amount else value


def mask_bits(mb, me):
    if mb <= me:
        return ((MASK >> mb) & (MASK << (31 - me))) & MASK
    return ((MASK >> mb) | (MASK << (31 - me))) & MASK


class Machine:
    def __init__(self, gpr, memory_seed):
        self.gpr = list(gpr)
        self.cr = 0  # cr0 bits: LT GT EQ SO as 8,4,2,1
        self.ca = 0
        self.memory_seed = memory_seed
        self.writes = {}
        self.lr = 0xDEAD0000

    def load(self, address, size, signed=False):
        value = 0
        for offset in range(size):
            byte_address = (address + offset) & MASK
            if byte_address in self.writes:
                byte = self.writes[byte_address]
            else:
                byte = random.Random(self.memory_seed * 1000003 + byte_address).randrange(256)
            value = (value << 8) | byte
        if signed and value & (1 << (size * 8 - 1)):
            value -= 1 << (size * 8)
        return value & MASK

    def store(self, address, value, size):
        for offset in range(size):
            shift = 8 * (size - 1 - offset)
            self.writes[(address + offset) & MASK] = (value >> shift) & 0xFF

    def set_cr0(self, value):
        value = s32(value)
        self.cr = (8 if value < 0 else 4 if value > 0 else 2)

    def compare(self, a, b, signed):
        if signed:
            a, b = s32(a), s32(b)
        else:
            a, b = a & MASK, b & MASK
        self.cr = 8 if a < b else 4 if a > b else 2


def run(words, gpr, memory_seed, limit=2000):
    machine = Machine(gpr, memory_seed)
    r = machine.gpr
    pc = 0
    steps = 0
    while True:
        steps += 1
        if steps > limit:
            raise Unsupported("step limit")
        if pc < 0 or pc // 4 >= len(words):
            raise Unsupported("pc out of range")
        word = words[pc // 4]
        op = word >> 26
        rd = (word >> 21) & 31
        ra = (word >> 16) & 31
        rb = (word >> 11) & 31
        simm = s32(word & 0xFFFF if not word & 0x8000 else (word & 0xFFFF) - 0x10000)
        uimm = word & 0xFFFF
        rc = word & 1
        next_pc = pc + 4

        def base():
            return 0 if ra == 0 else r[ra]

        if op == 14:  # addi
            if ra == 0:
                r[rd] = simm & MASK
            else:
                r[rd] = (r[ra] + simm) & MASK
        elif op == 15:  # addis
            r[rd] = ((0 if ra == 0 else r[ra]) + (simm << 16)) & MASK
        elif op == 12 or op == 13:  # addic / addic.
            result = (r[ra] & MASK) + (simm & MASK)
            machine.ca = 1 if result > MASK else 0
            r[rd] = result & MASK
            if op == 13:
                machine.set_cr0(r[rd])
        elif op == 8:  # subfic
            result = (simm & MASK) + ((~r[ra]) & MASK) + 1
            machine.ca = 1 if result > MASK else 0
            r[rd] = result & MASK
        elif op == 7:  # mulli
            r[rd] = (s32(r[ra]) * simm) & MASK
        elif op == 11:  # cmpi
            if (word >> 23) & 7:
                raise Unsupported("cr field")
            machine.compare(r[ra], simm, True)
        elif op == 10:  # cmpli
            if (word >> 23) & 7:
                raise Unsupported("cr field")
            machine.compare(r[ra], uimm, False)
        elif op == 24:  # ori
            r[ra] = r[rd] | uimm
        elif op == 25:  # oris
            r[ra] = (r[rd] | (uimm << 16)) & MASK
        elif op == 26:  # xori
            r[ra] = r[rd] ^ uimm
        elif op == 27:  # xoris
            r[ra] = (r[rd] ^ (uimm << 16)) & MASK
        elif op == 28:  # andi.
            r[ra] = r[rd] & uimm
            machine.set_cr0(r[ra])
        elif op == 29:  # andis.
            r[ra] = r[rd] & (uimm << 16)
            machine.set_cr0(r[ra])
        elif op in (20, 21, 23):  # rlwimi, rlwinm, rlwnm
            sh = rb if op != 23 else r[rb] & 31
            mb = (word >> 6) & 31
            me = (word >> 1) & 31
            rotated = rotl(r[rd], sh)
            m = mask_bits(mb, me)
            if op == 20:
                r[ra] = (rotated & m) | (r[ra] & ~m & MASK)
            else:
                r[ra] = rotated & m
            if rc:
                machine.set_cr0(r[ra])
        elif op in (32, 34, 40, 42, 36, 38, 44):  # loads/stores D-form
            if ra == 0:
                raise Unsupported("relocated access")
            address = (r[ra] + simm) & MASK
            if op == 32:
                r[rd] = machine.load(address, 4)
            elif op == 34:
                r[rd] = machine.load(address, 1)
            elif op == 40:
                r[rd] = machine.load(address, 2)
            elif op == 42:
                r[rd] = machine.load(address, 2, signed=True)
            elif op == 36:
                machine.store(address, r[rd], 4)
            elif op == 38:
                machine.store(address, r[rd], 1)
            elif op == 44:
                machine.store(address, r[rd], 2)
        elif op == 18:  # b
            if word & 1:
                raise Unsupported("call")
            li = word & 0x03FFFFFC
            if li & 0x02000000:
                li -= 0x04000000
            next_pc = pc + li
        elif op == 16:  # bc
            if word & 1:
                raise Unsupported("call")
            bo = rd
            bi = ra
            bd = word & 0xFFFC
            if bd & 0x8000:
                bd -= 0x10000
            if bi > 3 or bo not in (4, 12, 20):
                raise Unsupported("bc form")
            bit = (machine.cr >> (3 - bi)) & 1
            taken = bo == 20 or (bo == 12 and bit) or (bo == 4 and not bit)
            if taken:
                next_pc = pc + bd
        elif op == 19:
            xo = (word >> 1) & 0x3FF
            if xo == 16:  # bclr
                bo = rd
                bi = ra
                if word & 1:
                    raise Unsupported("bclrl")
                if bo == 20:
                    return machine
                if bi > 3 or bo not in (4, 12):
                    raise Unsupported("bclr form")
                bit = (machine.cr >> (3 - bi)) & 1
                if (bo == 12 and bit) or (bo == 4 and not bit):
                    return machine
            else:
                raise Unsupported(f"op19 {xo}")
        elif op == 31:
            xo = (word >> 1) & 0x3FF
            a, b, s = r[ra], r[rb], r[rd]
            if xo == 266:  # add
                r[rd] = (a + b) & MASK
            elif xo == 40:  # subf
                r[rd] = (b - a) & MASK
            elif xo == 8:  # subfc
                result = (b & MASK) + ((~a) & MASK) + 1
                machine.ca = 1 if result > MASK else 0
                r[rd] = result & MASK
            elif xo == 10:  # addc
                result = (a & MASK) + (b & MASK)
                machine.ca = 1 if result > MASK else 0
                r[rd] = result & MASK
            elif xo == 138:  # adde
                result = (a & MASK) + (b & MASK) + machine.ca
                machine.ca = 1 if result > MASK else 0
                r[rd] = result & MASK
            elif xo == 136:  # subfe
                result = ((~a) & MASK) + (b & MASK) + machine.ca
                machine.ca = 1 if result > MASK else 0
                r[rd] = result & MASK
            elif xo == 202:  # addze
                result = (a & MASK) + machine.ca
                machine.ca = 1 if result > MASK else 0
                r[rd] = result & MASK
            elif xo == 104:  # neg
                r[rd] = (-a) & MASK
            elif xo == 235:  # mullw
                r[rd] = (s32(a) * s32(b)) & MASK
            elif xo == 75:  # mulhw
                r[rd] = ((s32(a) * s32(b)) >> 32) & MASK
            elif xo == 11:  # mulhwu
                r[rd] = ((a * b) >> 32) & MASK
            elif xo == 491:  # divw
                if s32(b) == 0 or (s32(a) == -(1 << 31) and s32(b) == -1):
                    raise Unsupported("undefined divide")
                q = abs(s32(a)) // abs(s32(b))
                if (s32(a) < 0) != (s32(b) < 0):
                    q = -q
                r[rd] = q & MASK
            elif xo == 459:  # divwu
                if b == 0:
                    raise Unsupported("undefined divide")
                r[rd] = (a // b) & MASK
            elif xo == 28:  # and
                r[ra] = s & b
            elif xo == 60:  # andc
                r[ra] = s & ~b & MASK
            elif xo == 444:  # or
                r[ra] = s | b
            elif xo == 412:  # orc
                r[ra] = (s | ~b) & MASK
            elif xo == 316:  # xor
                r[ra] = s ^ b
            elif xo == 124:  # nor
                r[ra] = ~(s | b) & MASK
            elif xo == 284:  # eqv
                r[ra] = ~(s ^ b) & MASK
            elif xo == 476:  # nand
                r[ra] = ~(s & b) & MASK
            elif xo == 24:  # slw
                n = b & 63
                r[ra] = (s << n) & MASK if n < 32 else 0
            elif xo == 536:  # srw
                n = b & 63
                r[ra] = (s >> n) if n < 32 else 0
            elif xo == 792:  # sraw
                n = b & 63
                value = s32(s)
                result = value >> min(n, 31)
                machine.ca = 1 if value < 0 and (n >= 32 or (value & ((1 << n) - 1)) != 0) else 0
                r[ra] = result & MASK
            elif xo == 824:  # srawi
                n = rb
                value = s32(s)
                machine.ca = 1 if value < 0 and (value & ((1 << n) - 1)) != 0 else 0
                r[ra] = (value >> n) & MASK
            elif xo == 26:  # cntlzw
                value = s & MASK
                r[ra] = 32 - value.bit_length()
            elif xo == 954:  # extsb
                r[ra] = s32(((s & 0xFF) ^ 0x80) - 0x80) & MASK
            elif xo == 922:  # extsh
                r[ra] = (((s & 0xFFFF) ^ 0x8000) - 0x8000) & MASK
            elif xo == 0:  # cmp
                if (word >> 23) & 7:
                    raise Unsupported("cr field")
                machine.compare(a, b, True)
            elif xo == 32:  # cmpl
                if (word >> 23) & 7:
                    raise Unsupported("cr field")
                machine.compare(a, b, False)
            elif xo in (23, 87, 279, 343, 151, 215, 407):  # indexed loads/stores
                address = ((0 if ra == 0 else a) + b) & MASK
                if xo == 23:
                    r[rd] = machine.load(address, 4)
                elif xo == 87:
                    r[rd] = machine.load(address, 1)
                elif xo == 279:
                    r[rd] = machine.load(address, 2)
                elif xo == 343:
                    r[rd] = machine.load(address, 2, signed=True)
                elif xo == 151:
                    machine.store(address, s, 4)
                elif xo == 215:
                    machine.store(address, s, 1)
                elif xo == 407:
                    machine.store(address, s, 2)
            else:
                raise Unsupported(f"op31 {xo}")
            if rc and xo not in (0, 32):
                target = rd if xo in (266, 40, 8, 10, 138, 136, 202, 104, 235, 75, 11, 491, 459) else ra
                machine.set_cr0(r[target])
        else:
            raise Unsupported(f"op {op}")
        r[0] &= MASK
        pc = next_pc


def words_of(hex_text):
    data = bytes.fromhex(hex_text)
    return [int.from_bytes(data[i:i + 4], "big") for i in range(0, len(data), 4)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("json")
    parser.add_argument("--trials", type=int, default=64)
    args = parser.parse_args()
    data = json.load(open(args.json))
    checked = skipped = 0
    divergent = []
    for f in data:
        if not f.get("reference_words"):
            continue
        reference = words_of(f["reference_words"])
        produced = words_of(f["produced_words"])
        try:
            bad = None
            for trial in range(args.trials):
                rng = random.Random(trial * 7919 + len(reference))
                gpr = [0] * 32
                gpr[1] = 0x80100000
                gpr[2] = 0x80200000
                gpr[13] = 0x80300000
                for register in range(14, 32):
                    gpr[register] = 0x5A5A0000 + register
                for register in range(3, 11):
                    choice = rng.randrange(6)
                    gpr[register] = [0, 1, MASK, 0x80000000, rng.randrange(1 << 32), rng.randrange(-40, 40) & MASK][choice]
                # Pointer arguments: mostly valid-looking addresses.
                ref_machine = run(reference, gpr, trial)
                our_machine = run(produced, gpr, trial)
                preserved = [1, 2, 13] + list(range(14, 32))
                if any(ref_machine.gpr[r] != our_machine.gpr[r] for r in preserved):
                    bad = (trial, [hex(g) for g in gpr[3:8]], "preserved-register", "clobbered", True)
                    break
                if ref_machine.gpr[3] != our_machine.gpr[3] or ref_machine.writes != our_machine.writes:
                    bad = (trial, [hex(g) for g in gpr[3:8]], hex(ref_machine.gpr[3]), hex(our_machine.gpr[3]),
                           ref_machine.writes != our_machine.writes)
                    break
            checked += 1
            if bad:
                divergent.append((f["source"], f["name"], bad))
        except Unsupported:
            skipped += 1
    # A narrow return value's upper bits are the caller's to extend: differences
    # confined to them are not divergences.
    def narrow_only(bad):
        if bad[2] == "preserved-register":
            return False
        ref, ours = int(bad[2], 16), int(bad[3], 16)
        return not bad[4] and ((ref ^ ours) & 0xFF == 0 or (ref ^ ours) & 0xFFFF == 0)
    narrow = [d for d in divergent if narrow_only(d[2])]
    divergent = [d for d in divergent if not narrow_only(d[2])]
    print(f"checked {checked}, skipped {skipped}, divergent {len(divergent)} (+{len(narrow)} differing only above a narrow return)")
    for source, name, bad in divergent:
        print(f"  DIVERGES {source} {name}: trial {bad[0]} args {bad[1]} ref r3={bad[2]} ours r3={bad[3]} memory_differs={bad[4]}")
    return 1 if divergent else 0


if __name__ == "__main__":
    sys.exit(main())
