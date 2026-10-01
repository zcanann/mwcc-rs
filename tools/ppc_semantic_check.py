#!/usr/bin/env python3
"""Differential semantic check of mismatched PCode functions.

Runs the reference and produced machine code of each mismatched function
(from a win_pcode_eval.py / win_pcode_corpus.py --json-out file) on random
argument registers and memory, and compares the return value r3, the
callee-saved registers, LR, every memory write outside the stack frame, and
the sequence of calls made. Any divergence is a likely miscompile.

When the JSON carries relocations (``reference_relocs`` / ``produced_relocs``
and the matching ``*_layout``), relocated accesses and calls are modeled:

* every referenced data object gets a fake base address shared by both
  sides (by name; ``@N`` constants by content, ``name$N`` statics by base
  name when unique), and each (side, section) gets a region where objects
  sit at their real section offsets, so label-relative accesses
  (``...bss.0@l`` plus a displacement) resolve to the object at that offset.
  Memory is keyed on (object, offset) so both sides seed and compare the
  same bytes; read-only constants read their real contents;
* ``@sda21`` accesses use the fake address as their effective address;
* a call (``bl sym``, a tail ``b sym``, ``bctrl``, ``blrl``) is recorded with
  its arguments and the global memory written before it, then returns
  deterministic r3/r4 values, clobbers the volatile registers, and is assumed
  to have changed all global memory (later reads see fresh values) and to
  have filled stack buffers passed to it.

Floating point (lfs/lfd/stfs/stfd and their update/indexed forms, the
arithmetic, fused multiply-add, compare and convert instructions, Gekko
psq_l/psq_st with GQR0) is modeled too; f1 is compared like r3 and f14-f31
must be preserved. Calls to libm functions are modeled as pure functions of
their arguments. r3/f1 are not compared when they look like temporaries
(void functions). Writes into the function's own stack frame are not
compared. Extra report buckets: results that only differ when a call returns
a value outside 0..127 (narrow return extension), the same calls in another
order, and differing call argument registers.

Functions with unsupported instructions are skipped. Old JSON files without
relocation data are checked as before (calls and rA=0 accesses skip).

Usage: python tools/ppc_semantic_check.py target/pe-vNN.json [--trials 64] [--verbose]
"""

import argparse
import bisect
import collections
import hashlib
import json
import math
import random
import re
import struct
import sys
from fractions import Fraction

MASK = 0xFFFFFFFF
STACK_TOP = 0x80100000
STACK_LO, STACK_HI = STACK_TOP - 0x10000, STACK_TOP + 0x40
RETURN_LR = 0xDEAD0000
OBJECT_BASE = 0x90000000
SECTION_BASE = 0x53000000
SAVE_REST_GPR = re.compile(r"^_+(save|rest)gpr_(\d+)(_x)?$")
SAVE_REST_FPR = re.compile(r"^_+(save|rest)fpr_(\d+)(_x)?$")
# Side-effect-free library functions (errno aside): modeled as functions of
# their arguments, so their evaluation order is free.
PURE_CALLS = {name + suffix for name in (
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sqrt", "fabs", "floor", "ceil", "fmod", "pow",
    "exp", "log", "log10", "sinh", "cosh", "tanh", "ldexp", "abs", "labs")
    for suffix in ("", "f")} - {"absf", "labsf"}

R_PPC_ADDR32, R_PPC_ADDR16, R_PPC_ADDR16_LO, R_PPC_ADDR16_HI, R_PPC_ADDR16_HA = 1, 3, 4, 5, 6
R_PPC_REL24, R_PPC_EMB_SDA21 = 10, 109


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


def f_bits(value):
    return struct.unpack(">Q", struct.pack(">d", value))[0]


def f_from_bits(bits):
    return struct.unpack(">d", struct.pack(">Q", bits & 0xFFFFFFFFFFFFFFFF))[0]


def single_bits(value):
    if math.isnan(value):  # keep the payload (and signaling NaNs) exactly
        bits = f_bits(value)
        mantissa = (bits >> 29) & 0x7FFFFF
        return ((bits >> 63) << 31) | 0x7F800000 | (mantissa or 0x400000)
    try:
        return struct.unpack(">I", struct.pack(">f", value))[0]
    except OverflowError:
        return 0xFF800000 if value < 0 else 0x7F800000


def single_value(bits):
    if (bits >> 23) & 0xFF == 0xFF and bits & 0x7FFFFF:
        return f_from_bits(((bits >> 31) << 63) | (0x7FF << 52) | ((bits & 0x7FFFFF) << 29))
    return struct.unpack(">f", struct.pack(">I", bits))[0]


def to_single(value):
    return struct.unpack(">f", struct.pack(">I", single_bits(value)))[0]


def f_div(a, b):
    if b == 0.0 and not math.isnan(a):
        if a == 0.0:
            return math.nan
        return math.copysign(math.inf, a) * math.copysign(1.0, b)
    return a / b


def f_fused(a, c, b, subtract):
    """a*c +/- b with one rounding."""
    if math.isfinite(a) and math.isfinite(b) and math.isfinite(c):
        exact = Fraction(a) * Fraction(c) + (-Fraction(b) if subtract else Fraction(b))
        try:
            return float(exact)
        except OverflowError:
            return math.inf if exact > 0 else -math.inf
    return a * c - b if subtract else a * c + b


def hash_byte(text):
    return hashlib.blake2b(text.encode(), digest_size=1).digest()[0]


def in_stack(address):
    return isinstance(address, int) and STACK_LO <= address < STACK_HI


# --- data layout --------------------------------------------------------------

def identity_names(layout):
    """Side-independent identity of each layout object name."""
    counts = collections.Counter(re.sub(r"\$\d+$", "", e["name"]) for e in layout)
    names = {}
    for e in layout:
        name = e["name"]
        if name.startswith("@") and e.get("data") is not None:
            digest = hashlib.sha1(f"{e['size']}:{e['data']}".encode()).hexdigest()[:16]
            names[name] = "@lit:" + digest
        else:
            stem = re.sub(r"\$\d+$", "", name)
            names[name] = stem + "$" if stem != name and counts[stem] == 1 else name
    return names


def is_object(entry):
    return entry["size"] > 0 and entry["type"] in (0, 1)


class Space:
    """Fake addresses shared by both sides of one function."""

    def __init__(self, sides):
        identities = {}  # identity -> span
        sections = set()
        for side in sides:
            for e in side.layout:
                sections.add(e["section"])
                if is_object(e):
                    ident = side.identity[e["name"]]
                    identities[ident] = max(identities.get(ident, 0), e["size"])
            for r in side.relocs:
                if r[2] not in side.by_name:
                    identities.setdefault(r[2], 0)
        self.base = {}
        address = OBJECT_BASE
        for ident in sorted(identities):
            self.base[ident] = address
            address += ((identities[ident] + 0x10000 + 0xFFFFF) // 0x100000) * 0x100000
        self.limit = address
        self.starts = sorted((b, i) for i, b in self.base.items())
        self.section_base = {s: SECTION_BASE + n * 0x1000000 for n, s in enumerate(sorted(sections))}
        self.section_starts = sorted((b, s) for s, b in self.section_base.items())


class Side:
    """One side's relocations, layout, and the resolved code."""

    def __init__(self, words, relocs, layout):
        self.words = list(words)
        self.relocs = relocs or []
        self.layout = layout or []
        self.by_name = {}
        for e in self.layout:
            self.by_name.setdefault(e["name"], e)
        self.identity = identity_names(self.layout)
        self.objects = collections.defaultdict(list)  # section -> sorted (value, size, identity, data)
        self.contents = {}  # identity -> bytes
        for e in self.layout:
            if is_object(e):
                ident = self.identity[e["name"]]
                self.objects[e["section"]].append((e["value"], e["size"], ident))
                if e.get("data") is not None:
                    self.contents[ident] = bytes.fromhex(e["data"])
        for rows in self.objects.values():
            rows.sort()
        self.object_starts = {s: [row[0] for row in rows] for s, rows in self.objects.items()}
        self.ea_override = {}
        self.calls = {}

    def bind(self, space):
        self.space = space
        for offset, kind, symbol, addend in self.relocs:
            index = offset // 4
            if not 0 <= index < len(self.words):
                continue
            word = self.words[index]
            if kind == R_PPC_REL24:
                if word >> 26 != 18:
                    raise Unsupported("REL24 on non-branch")
                self.calls[index] = symbol
                continue
            address = self.resolve(symbol, addend)
            if kind == R_PPC_ADDR16_HA:
                word = (word & 0xFFFF0000) | (((address + 0x8000) >> 16) & 0xFFFF)
            elif kind in (R_PPC_ADDR16_LO, R_PPC_ADDR16):
                word = (word & 0xFFFF0000) | (address & 0xFFFF)
            elif kind == R_PPC_ADDR16_HI:
                word = (word & 0xFFFF0000) | ((address >> 16) & 0xFFFF)
            elif kind == R_PPC_ADDR32:
                word = address
            elif kind == R_PPC_EMB_SDA21:
                self.ea_override[index] = address
            else:
                raise Unsupported(f"reloc {kind}")
            self.words[index] = word & MASK

    def resolve(self, symbol, addend):
        entry = self.by_name.get(symbol)
        if entry is None:
            return (self.space.base[symbol] + addend) & MASK
        if is_object(entry):
            return (self.space.base[self.identity[symbol]] + addend) & MASK
        return (self.space.section_base[entry["section"]] + entry["value"] + addend) & MASK

    def canonical(self, address):
        """Memory key of a byte address: (identity, offset) for modeled data."""
        space = getattr(self, "space", None)
        if space is None:
            return address
        if OBJECT_BASE <= address < space.limit:
            i = bisect.bisect_right(space.starts, (address, "￿")) - 1
            if i >= 0:
                base, ident = space.starts[i]
                return ("o", ident, address - base)
        if SECTION_BASE <= address < SECTION_BASE + 0x1000000 * len(space.section_starts):
            i = bisect.bisect_right(space.section_starts, (address, "￿")) - 1
            base, section = space.section_starts[i]
            offset = address - base
            rows = self.objects.get(section, [])
            j = bisect.bisect_right(self.object_starts.get(section, []), offset) - 1
            if j >= 0 and offset < rows[j][0] + rows[j][1]:
                return ("o", rows[j][2], offset - rows[j][0])
            return ("s", section, offset)
        return address

    def canonical_value(self, value):
        """A register/stored value with region addresses turned into object addresses."""
        key = self.canonical(value)
        if isinstance(key, tuple) and key[0] == "o":
            return (self.space.base[key[1]] + key[2]) & MASK
        return value


# --- machine ------------------------------------------------------------------

GPR_RD_OPS = {7, 8, 12, 13, 14, 15, 32, 33, 34, 35, 40, 41, 42, 43}
GPR_RA_OPS = {20, 21, 23, 24, 25, 26, 27, 28, 29}
UPDATE_OPS = {33, 35, 37, 39, 41, 43, 45}
X_LOADS = {23: (4, False), 55: (4, False), 87: (1, False), 119: (1, False), 279: (2, False),
           311: (2, False), 343: (2, True), 375: (2, True)}
X_STORES = {151: 4, 183: 4, 215: 1, 247: 1, 407: 2, 439: 2}
X_UPDATE = {55, 119, 311, 375, 183, 247, 439}
XO_ARITH = {266, 40, 8, 10, 138, 136, 202, 200, 232, 234, 104, 235, 75, 11, 491, 459}
X_LOGICAL = {28, 60, 444, 412, 316, 124, 284, 476, 24, 536, 792, 824, 26, 954, 922}


def gpr_targets(word):
    op = word >> 26
    rd, ra = (word >> 21) & 31, (word >> 16) & 31
    if op in GPR_RD_OPS:
        return (rd, ra) if op in UPDATE_OPS else (rd,)
    if op in GPR_RA_OPS:
        return (ra,)
    if op in UPDATE_OPS or op in (49, 51, 53, 55, 57, 61):
        return (ra,)
    if op == 46:
        return tuple(range(rd, 32))
    if op == 31:
        xo = (word >> 1) & 0x3FF
        if xo in X_LOADS or xo == 534:
            return (rd, ra) if xo in X_UPDATE else (rd,)
        if xo in X_UPDATE or xo in (567, 631, 695, 759):
            return (ra,)
        if (xo & 0x1FF) in XO_ARITH or xo in (339, 19):
            return (rd,)
        if xo in X_LOGICAL:
            return (ra,)
    return ()


class Machine:
    def __init__(self, gpr, memory_seed, side):
        self.gpr = list(gpr)
        self.cr = 0  # 8 fields, cr0 in the top nibble: LT GT EQ SO
        self.ca = 0
        self.ctr = 0
        self.memory_seed = memory_seed
        self.side = side
        self.mem = {}  # canonical key -> byte
        self.lr = RETURN_LR
        self.epoch = 0
        self.calls = []  # (callee, args, written argument registers, global writes before)
        self.out_pointers = []  # (stack address, tag) of stack buffers passed to calls
        self.written = set()  # GPRs written since entry / the last call
        self.r3_source = None  # what last wrote r3: "insn" or "call"
        self.consumed = set()  # GPRs whose current value an instruction has read
        self.narrow_returns = False
        self.fpr = [1000.0 + i for i in range(32)]
        self.ps1 = [1000.0 + i for i in range(32)]  # Gekko paired-single second slots
        self.f1_source = None
        self.f_consumed = set()

    @property
    def writes(self):
        return {k: v for k, v in self.mem.items() if not in_stack(k)}

    def seed_byte(self, key):
        if isinstance(key, tuple):
            if key[0] == "o":
                data = self.side.contents.get(key[1])
                if data is not None and key[2] < len(data):
                    return data[key[2]]
            return hash_byte(f"{self.memory_seed}|{self.epoch}|{key}")
        if in_stack(key):
            best = None
            for pointer, tag in self.out_pointers:
                if pointer <= key < pointer + 0x100 and (best is None or pointer > best[0]):
                    best = (pointer, tag)
            if best:
                return hash_byte(f"{self.memory_seed}|{best[1]}|{key - best[0]}")
            return random.Random(self.memory_seed * 1000003 + key).randrange(256)
        if self.epoch:
            return hash_byte(f"{self.memory_seed}|{self.epoch}|{key}")
        return random.Random(self.memory_seed * 1000003 + key).randrange(256)

    def load(self, address, size, signed=False):
        value = 0
        for offset in range(size):
            key = self.side.canonical((address + offset) & MASK)
            byte = self.mem[key] if key in self.mem else self.seed_byte(key)
            value = (value << 8) | byte
        if signed and value & (1 << (size * 8 - 1)):
            value -= 1 << (size * 8)
        return value & MASK

    def store(self, address, value, size):
        if size == 4:
            value = self.side.canonical_value(value)
        for offset in range(size):
            shift = 8 * (size - 1 - offset)
            self.mem[self.side.canonical((address + offset) & MASK)] = (value >> shift) & 0xFF

    def set_field(self, field, value):
        shift = (7 - field) * 4
        self.cr = (self.cr & ~(0xF << shift) & MASK) | (value << shift)

    def set_cr0(self, value):
        value = s32(value)
        self.set_field(0, 8 if value < 0 else 4 if value > 0 else 2)

    def compare(self, a, b, signed, field=0):
        if signed:
            a, b = s32(a), s32(b)
        else:
            a, b = a & MASK, b & MASK
        self.set_field(field, 8 if a < b else 4 if a > b else 2)

    def cr_bit(self, bit):
        return (self.cr >> (31 - bit)) & 1

    def branch_taken(self, bo, bi):
        ctr_ok = True
        if not bo & 4:
            self.ctr = (self.ctr - 1) & MASK
            ctr_ok = (self.ctr != 0) != bool(bo & 2)
        cond_ok = bool(bo & 16) or self.cr_bit(bi) == ((bo >> 3) & 1)
        return ctr_ok and cond_ok

    def call(self, callee):
        """Model a call to an unknown function."""
        r = self.gpr
        helper = SAVE_REST_GPR.match(callee)
        if helper:  # runtime register save/restore: r(N)..r31 at r11 - 4*(32-i)
            for register in range(int(helper.group(2)), 32):
                address = (r[11] - 4 * (32 - register)) & MASK
                if helper.group(1) == "save":
                    self.store(address, r[register], 4)
                else:
                    r[register] = self.load(address, 4)
            return
        helper = SAVE_REST_FPR.match(callee)
        if helper:  # f(N)..f31 at r11 - 8*(32-i)
            for register in range(int(helper.group(2)), 32):
                address = (r[11] - 8 * (32 - register)) & MASK
                if helper.group(1) == "save":
                    bits = f_bits(self.fpr[register])
                    self.store(address, bits >> 32, 4)
                    self.store((address + 4) & MASK, bits & MASK, 4)
                else:
                    self.fpr[register] = f_from_bits((self.load(address, 4) << 32) | self.load((address + 4) & MASK, 4))
            return
        if callee in PURE_CALLS:
            # No side effects: the result depends on the arguments only, so
            # the order of such calls does not matter.
            key = (self.observable(r[3]), self.observable(r[4]), f_bits(self.fpr[1]), f_bits(self.fpr[2]))
            self.clobber(random.Random(f"{callee}|{key}|{self.memory_seed}"), 0, callee.endswith("f"))
            return
        args = tuple(self.observable(r[i]) for i in range(3, 11))
        written = frozenset(i for i in self.written - self.consumed if 3 <= i <= 10)
        before = self.writes
        self.calls.append((callee, args, written, before))
        index = len(self.calls)
        for i in range(3, 11):
            if in_stack(r[i]):
                self.out_pointers.append((r[i], f"{callee}#{index}#{i}"))
        for key in before:
            del self.mem[key]
        self.epoch += 1
        # Results are keyed on the callee's own call count, so calls made in
        # a different order still return the same values.
        nth = sum(1 for c in self.calls if c[0] == callee)
        self.clobber(random.Random(f"{callee}|{nth}|{self.memory_seed}"), index, True)

    def clobber(self, rng, index, single):
        """A call's results and its effect on the volatile registers."""
        r = self.gpr
        r[3] = [0, 1, MASK, rng.randrange(-40, 40) & MASK, rng.randrange(1 << 32), rng.randrange(1 << 32)][rng.randrange(6)]
        if self.narrow_returns:  # values every 8/16-bit extension agrees on
            r[3] = rng.randrange(128)
        junk = rng.getrandbits(32 * 24)
        words = [(junk >> (32 * i)) & MASK for i in range(24)]
        r[4] = words[0]
        for n, i in enumerate([0] + list(range(5, 13))):
            r[i] = words[1 + n]
        self.fpr[1] = rng.uniform(-1000.0, 1000.0)
        if single:
            self.fpr[1] = to_single(self.fpr[1])
        if self.narrow_returns:
            self.fpr[1] = float(rng.randrange(128))
        for n, i in enumerate([0] + list(range(2, 14))):
            self.fpr[i] = (words[10 + n] % 2000000) - 1e6 + 0.375
        self.f1_source = "call"
        self.f_consumed = set()
        for n, field in enumerate((0, 1, 5, 6, 7)):
            self.set_field(field, (words[23] >> (4 * n)) & 0xF)
        self.ctr = words[23] ^ words[0]
        self.ca = 0
        self.lr = 0xC0DE0000 | index
        self.written = set()
        self.r3_source = "call"
        self.consumed = set()

    def observable(self, value):
        if in_stack(value):
            return "stack"
        return self.side.canonical_value(value)


FP_LOADS = {48: (4, False), 49: (4, True), 50: (8, False), 51: (8, True)}
FP_STORES = {52: (4, False), 53: (4, True), 54: (8, False), 55: (8, True)}
FP_X_LOADS = {535: (4, False), 567: (4, True), 599: (8, False), 631: (8, True)}
FP_X_STORES = {663: (4, False), 695: (4, True), 727: (8, False), 759: (8, True), 983: (-4, False)}


FP_X_INDEXED = set(FP_X_LOADS) | set(FP_X_STORES)


def fp_load(machine, address, size, rd):
    if size == 4:
        machine.fpr[rd] = machine.ps1[rd] = single_value(machine.load(address, 4))
    else:
        high, low = machine.load(address, 4), machine.load((address + 4) & MASK, 4)
        machine.fpr[rd] = f_from_bits((high << 32) | low)


def fp_store(machine, address, size, rs):
    value = machine.fpr[rs]
    if size == 4:
        machine.store(address, single_bits(value), 4)
    elif size == -4:  # stfiwx: the low word
        machine.store(address, f_bits(value) & MASK, 4)
    else:
        bits = f_bits(value)
        machine.store(address, bits >> 32, 4)
        machine.store((address + 4) & MASK, bits & MASK, 4)


def fp_arith(machine, word, op):
    """Primary opcodes 59/63. Returns (target fpr or None, source fprs)."""
    f = machine.fpr
    d, a, b, c = (word >> 21) & 31, (word >> 16) & 31, (word >> 11) & 31, (word >> 6) & 31
    if word & 1:
        raise Unsupported("fp record form")
    xo5 = (word >> 1) & 0x1F
    single = op == 59
    if xo5 >= 18 or single:
        fa, fb, fc = f[a], f[b], f[c]
        if xo5 == 18:
            value, sources = f_div(fa, fb), (a, b)
        elif xo5 == 20:
            value, sources = fa - fb, (a, b)
        elif xo5 == 21:
            value, sources = fa + fb, (a, b)
        elif xo5 == 25:
            value, sources = fa * fc, (a, c)
        elif xo5 in (28, 29, 30, 31):
            value = f_fused(fa, fc, fb, xo5 in (28, 30))
            if xo5 in (30, 31):
                value = -value
            sources = (a, b, c)
        elif xo5 == 23 and not single:  # fsel
            value, sources = (fc if fa >= 0.0 else fb), (a, b, c)
        else:
            raise Unsupported(f"op{op} {xo5}")
        f[d] = to_single(value) if single else value
        return d, sources
    xo = (word >> 1) & 0x3FF
    if xo in (0, 32):  # fcmpu / fcmpo
        fa, fb = f[a], f[b]
        machine.set_field(d >> 2, 1 if math.isnan(fa) or math.isnan(fb) else 8 if fa < fb else 4 if fa > fb else 2)
        return None, (a, b)
    fb = f[b]
    if xo == 72:  # fmr
        f[d] = fb
    elif xo == 40:  # fneg
        f[d] = f_from_bits(f_bits(fb) ^ (1 << 63))
    elif xo == 264:  # fabs
        f[d] = f_from_bits(f_bits(fb) & ~(1 << 63))
    elif xo == 136:  # fnabs
        f[d] = f_from_bits(f_bits(fb) | (1 << 63))
    elif xo == 12:  # frsp
        f[d] = to_single(fb)
    elif xo in (14, 15):  # fctiw / fctiwz
        if math.isnan(fb):
            value = -(1 << 31)
        elif math.isinf(fb):
            value = (1 << 31) - 1 if fb > 0 else -(1 << 31)
        else:
            value = math.trunc(fb) if xo == 15 else round(fb)
            value = max(-(1 << 31), min((1 << 31) - 1, value))
        f[d] = f_from_bits((0xFFF80000 << 32) | (value & MASK))
    else:
        raise Unsupported(f"op63 {xo}")
    return d, (b,)


def run(side, gpr, memory_seed, limit=20000, narrow_returns=False, fpr=None):
    words = side.words
    machine = Machine(gpr, memory_seed, side)
    machine.narrow_returns = narrow_returns
    if fpr is not None:
        machine.fpr = list(fpr)
        machine.ps1 = list(fpr)
    r = machine.gpr
    pc = 0
    steps = 0
    while True:
        steps += 1
        if steps > limit:
            raise Unsupported("step limit")
        if pc < 0 or pc // 4 >= len(words):
            raise Unsupported("pc out of range")
        index = pc // 4
        word = words[index]
        op = word >> 26
        rd = (word >> 21) & 31
        ra = (word >> 16) & 31
        rb = (word >> 11) & 31
        simm = s32(word & 0xFFFF if not word & 0x8000 else (word & 0xFFFF) - 0x10000)
        uimm = word & 0xFFFF
        rc = word & 1
        next_pc = pc + 4
        override = side.ea_override.get(index)

        if op == 14:  # addi
            if override is not None:
                r[rd] = override
            elif ra == 0:
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
            machine.compare(r[ra], simm, True, rd >> 2)
        elif op == 10:  # cmpli
            machine.compare(r[ra], uimm, False, rd >> 2)
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
        elif 32 <= op <= 45:  # loads/stores D-form, with update forms
            update = op in UPDATE_OPS
            if override is not None:
                if update:
                    raise Unsupported("sda21 update form")
                address = override
            elif ra == 0:
                raise Unsupported("relocated access")
            else:
                address = (r[ra] + simm) & MASK
            if op in (32, 33):
                r[rd] = machine.load(address, 4)
            elif op in (34, 35):
                r[rd] = machine.load(address, 1)
            elif op in (40, 41):
                r[rd] = machine.load(address, 2)
            elif op in (42, 43):
                r[rd] = machine.load(address, 2, signed=True)
            elif op in (36, 37):
                machine.store(address, r[rd], 4)
            elif op in (38, 39):
                machine.store(address, r[rd], 1)
            elif op in (44, 45):
                machine.store(address, r[rd], 2)
            if update:
                r[ra] = address
        elif op in FP_LOADS or op in FP_STORES:
            size, update = FP_LOADS.get(op) or FP_STORES[op]
            if override is not None:
                if update:
                    raise Unsupported("sda21 update form")
                address = override
            elif ra == 0:
                raise Unsupported("relocated access")
            else:
                address = (r[ra] + simm) & MASK
            if op in FP_LOADS:
                fp_load(machine, address, size, rd)
                fp_written(machine, rd)
            else:
                fp_store(machine, address, size, rd)
                machine.f_consumed.add(rd)
            if update:
                r[ra] = address
        elif op in (56, 57, 60, 61):  # psq_l / psq_lu / psq_st / psq_stu
            if (word >> 12) & 7:
                raise Unsupported("psq with GQR != 0")
            w = (word >> 15) & 1
            offset = word & 0xFFF
            if offset & 0x800:
                offset -= 0x1000
            if ra == 0:
                raise Unsupported("relocated access")
            address = (r[ra] + offset) & MASK
            if op in (56, 57):
                machine.fpr[rd] = single_value(machine.load(address, 4))
                machine.ps1[rd] = 1.0 if w else single_value(machine.load((address + 4) & MASK, 4))
                fp_written(machine, rd)
            else:
                machine.store(address, single_bits(machine.fpr[rd]), 4)
                if not w:
                    machine.store((address + 4) & MASK, single_bits(machine.ps1[rd]), 4)
                machine.f_consumed.add(rd)
            if op in (57, 61):
                r[ra] = address
        elif op in (59, 63):
            target, sources = fp_arith(machine, word, op)
            machine.f_consumed.update(x for x in sources if x != target)
            if target is not None:
                fp_written(machine, target)
        elif op in (46, 47):  # lmw / stmw
            if override is not None or ra == 0:
                raise Unsupported("relocated multiple")
            address = (r[ra] + simm) & MASK
            for register in range(rd, 32):
                if op == 46:
                    r[register] = machine.load(address, 4)
                else:
                    machine.store(address, r[register], 4)
                address = (address + 4) & MASK
        elif op == 18:  # b / bl
            callee = side.calls.get(index)
            if callee is not None:
                machine.call(callee)
                if not word & 1:  # tail call: the callee returns to our caller
                    return machine
            else:
                if word & 1:
                    raise Unsupported("call")
                if word & 2:
                    raise Unsupported("absolute branch")
                li = word & 0x03FFFFFC
                if li & 0x02000000:
                    li -= 0x04000000
                next_pc = pc + li
        elif op == 16:  # bc
            if word & 1:
                raise Unsupported("bcl")
            bd = word & 0xFFFC
            if bd & 0x8000:
                bd -= 0x10000
            if machine.branch_taken(rd, ra):
                next_pc = pc + bd
        elif op == 19:
            xo = (word >> 1) & 0x3FF
            if xo == 16:  # bclr
                if word & 1:  # blrl: indirect call
                    if (rd & 0x14) != 0x14:
                        raise Unsupported("conditional blrl")
                    target = machine.observable(machine.lr)
                    machine.call("*" + (hex(target) if isinstance(target, int) else target))
                elif machine.branch_taken(rd, ra):
                    return machine
            elif xo == 528:  # bcctr
                if not word & 1:
                    raise Unsupported("bctr")
                if (rd & 0x14) != 0x14:
                    raise Unsupported("conditional bctrl")
                target = machine.observable(machine.ctr)
                machine.call("*" + (hex(target) if isinstance(target, int) else target))
            elif xo in (257, 449, 193, 225, 33, 289, 129, 417):  # condition register logic
                a, b = machine.cr_bit(ra), machine.cr_bit(rb)
                value = {257: a & b, 449: a | b, 193: a ^ b, 225: 1 - (a & b), 33: 1 - (a | b),
                         289: 1 - (a ^ b), 129: a & (1 - b), 417: a | (1 - b)}[xo]
                machine.cr = (machine.cr & ~(1 << (31 - rd)) & MASK) | (value << (31 - rd))
            elif xo == 0:  # mcrf
                machine.set_field(rd >> 2, (machine.cr >> ((7 - (ra >> 2)) * 4)) & 0xF)
            elif xo == 150:  # isync
                pass
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
            elif xo == 200:  # subfze
                result = ((~a) & MASK) + machine.ca
                machine.ca = 1 if result > MASK else 0
                r[rd] = result & MASK
            elif xo == 234:  # addme
                result = (a & MASK) + MASK + machine.ca
                machine.ca = 1 if result > MASK else 0
                r[rd] = result & MASK
            elif xo == 232:  # subfme
                result = ((~a) & MASK) + MASK + machine.ca
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
                machine.compare(a, b, True, rd >> 2)
            elif xo == 32:  # cmpl
                machine.compare(a, b, False, rd >> 2)
            elif xo in X_LOADS or xo in X_STORES or xo in (534, 662):  # indexed loads/stores
                if xo in X_UPDATE and ra == 0:
                    raise Unsupported("update form with rA=0")
                address = ((0 if ra == 0 else a) + b) & MASK
                if xo in X_LOADS:
                    size, signed = X_LOADS[xo]
                    r[rd] = machine.load(address, size, signed)
                elif xo in X_STORES:
                    machine.store(address, s, X_STORES[xo])
                elif xo == 534:  # lwbrx
                    r[rd] = int.from_bytes(machine.load(address, 4).to_bytes(4, "big"), "little")
                else:  # stwbrx
                    machine.store(address, int.from_bytes(s.to_bytes(4, "big"), "little"), 4)
                if xo in X_UPDATE:
                    r[ra] = address
            elif xo in (339, 467):  # mfspr / mtspr
                spr = ((word >> 16) & 31) | (((word >> 11) & 31) << 5)
                if spr not in (8, 9):
                    raise Unsupported(f"spr {spr}")
                if xo == 339:
                    r[rd] = machine.lr if spr == 8 else machine.ctr
                elif spr == 8:
                    machine.lr = s
                else:
                    machine.ctr = s
            elif xo == 19:  # mfcr
                r[rd] = machine.cr
            elif xo == 144:  # mtcrf
                fxm = (word >> 12) & 0xFF
                mask = 0
                for field in range(8):
                    if fxm & (0x80 >> field):
                        mask |= 0xF << ((7 - field) * 4)
                machine.cr = (machine.cr & ~mask & MASK) | (s & mask)
            elif xo in FP_X_LOADS or xo in FP_X_STORES:
                size, update = FP_X_LOADS.get(xo) or FP_X_STORES[xo]
                if update and ra == 0:
                    raise Unsupported("update form with rA=0")
                address = ((0 if ra == 0 else a) + b) & MASK
                if xo in FP_X_LOADS:
                    fp_load(machine, address, size, rd)
                    fp_written(machine, rd)
                else:
                    fp_store(machine, address, size, rd)
                    machine.f_consumed.add(rd)
                if update:
                    r[ra] = address
            elif xo in (598, 854):  # sync, eieio
                pass
            else:
                raise Unsupported(f"op31 {xo}")
            if rc and xo not in (0, 32):
                target = rd if (xo & 0x1FF) in XO_ARITH else ra
                machine.set_cr0(r[target])
        else:
            raise Unsupported(f"op {op}")
        targets = gpr_targets(word)
        if targets:
            machine.written.update(targets)
        if op in (16, 18, 19, 59, 63):
            sources = ()
        elif op >= 48:
            sources = (ra,)
        elif op == 31 and ((word >> 1) & 0x3FF) in FP_X_INDEXED:
            sources = (ra, rb)
        else:
            sources = (rd, ra, rb)
        machine.consumed.update(field for field in sources if field not in targets)
        machine.consumed.difference_update(targets)
        if 3 in targets:
            machine.r3_source = "insn"
        r[0] &= MASK
        pc = next_pc


def fp_written(machine, register):
    machine.f_consumed.discard(register)
    if register == 1:
        machine.f1_source = "insn"


def words_of(hex_text):
    data = bytes.fromhex(hex_text)
    return [int.from_bytes(data[i:i + 4], "big") for i in range(0, len(data), 4)]


def sides_of(f):
    reference = Side(words_of(f["reference_words"]), f.get("reference_relocs"), f.get("reference_layout"))
    produced = Side(words_of(f["produced_words"]), f.get("produced_relocs"), f.get("produced_layout"))
    if f.get("reference_relocs") or f.get("produced_relocs"):
        space = Space([reference, produced])
        reference.bind(space)
        produced.bind(space)
    return reference, produced


def show(value):
    return hex(value) if isinstance(value, int) else str(value)


def diff_keys(a, b):
    keys = sorted(set(a) | set(b), key=str)
    return [k for k in keys if a.get(k) != b.get(k)]


def compare(ref, ours):
    """First difference between two runs: (bucket, detail) or None."""
    preserved = [1, 2, 13] + list(range(14, 32))
    for reg in preserved:
        if ref.gpr[reg] != ours.gpr[reg]:
            return "preserved", f"r{reg} ref={show(ref.gpr[reg])} ours={show(ours.gpr[reg])}"
    for reg in range(14, 32):
        if f_bits(ref.fpr[reg]) != f_bits(ours.fpr[reg]):
            return "preserved", f"f{reg} ref={ref.fpr[reg]} ours={ours.fpr[reg]}"
    if ref.lr != ours.lr:
        return "preserved", f"lr ref={show(ref.lr)} ours={show(ours.lr)}"
    ref_calls = [c[0] for c in ref.calls]
    our_calls = [c[0] for c in ours.calls]
    if ref_calls != our_calls:
        n = next((i for i, (a, b) in enumerate(zip(ref_calls, our_calls)) if a != b), min(len(ref_calls), len(our_calls)))
        # The same calls in another order: often an unspecified argument or
        # operand evaluation order (f(g(), h()), g() == h()).
        bucket = "call-order" if sorted(ref_calls) == sorted(our_calls) else "memory"
        return bucket, f"call #{n + 1} differs: ref={ref_calls[n:n + 3]} ours={our_calls[n:n + 3]}"
    for n, (rc, oc) in enumerate(zip(ref.calls, ours.calls)):
        if rc[3] != oc[3]:
            keys = diff_keys(rc[3], oc[3])
            return "memory", f"global memory before call #{n + 1} {rc[0]} differs at {[show(k) for k in keys[:4]]}"
    ref_writes, our_writes = ref.writes, ours.writes
    if ref_writes != our_writes:
        keys = diff_keys(ref_writes, our_writes)
        return "memory", f"memory differs at {[show(k) for k in keys[:4]]}"
    # r3 is the return value unless it looks like a temporary: never written,
    # read again after its last write on both sides, or left over from a call
    # on the reference side only.
    returns = (ref.r3_source is not None and not (3 in ref.consumed and 3 in ours.consumed)
               and not (ref.r3_source == "call" and ours.r3_source != "call"))
    if returns and ref.observable(ref.gpr[3]) != ours.observable(ours.gpr[3]):
        return "r3", (ref.observable(ref.gpr[3]), ours.observable(ours.gpr[3]))
    returns = (ref.f1_source is not None and not (1 in ref.f_consumed and 1 in ours.f_consumed)
               and not (ref.f1_source == "call" and ours.f1_source != "call"))
    if returns and f_bits(ref.fpr[1]) != f_bits(ours.fpr[1]):
        if not (math.isnan(ref.fpr[1]) and math.isnan(ours.fpr[1])):
            return "f1", f"ref f1={ref.fpr[1]!r} ours f1={ours.fpr[1]!r}"
    # Argument registers: set since the previous call and not read again by
    # the caller (a read means a temporary), on both sides.
    for n, (rc, oc) in enumerate(zip(ref.calls, ours.calls)):
        for reg in sorted(rc[2] & oc[2]):
            if rc[1][reg - 3] != oc[1][reg - 3]:
                return "call-args", (f"call #{n + 1} {rc[0]} r{reg} ref={show(rc[1][reg - 3])} "
                                     f"ours={show(oc[1][reg - 3])}")
    return None


def trial_registers(trial, size):
    rng = random.Random(trial * 7919 + size)
    gpr = [0] * 32
    gpr[1] = STACK_TOP
    gpr[2] = 0x80200000
    gpr[13] = 0x80300000
    for register in range(14, 32):
        gpr[register] = 0x5A5A0000 + register
    for register in range(3, 11):
        choice = rng.randrange(6)
        gpr[register] = [0, 1, MASK, 0x80000000, rng.randrange(1 << 32), rng.randrange(-40, 40) & MASK][choice]
    frng = random.Random(trial * 104729 + size)
    fpr = [1000.0 + i for i in range(32)]
    for register in range(1, 9):
        fpr[register] = to_single([0.0, 1.0, -1.0, 0.5, -2.5, 1e10, frng.uniform(-100, 100),
                                   float(frng.randrange(-40, 40))][frng.randrange(8)])
    return gpr, fpr


def wide_return_only(reference, produced, trials):
    """Whether the runs agree once calls return small values: the divergence
    then rests on the upper bits of a narrow (char/short/bool) call result,
    which MWCC callees extend."""
    if not reference.calls and not produced.calls:
        return False
    for trial in range(trials):
        gpr, fpr = trial_registers(trial, len(reference.words))
        try:
            result = compare(run(reference, gpr, trial, narrow_returns=True, fpr=fpr),
                             run(produced, gpr, trial, narrow_returns=True, fpr=fpr))
        except Unsupported:
            return False
        if result is not None and result[0] not in ("call-args", "call-order"):
            return False
    return True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("json")
    parser.add_argument("--trials", type=int, default=64)
    parser.add_argument("--filter", help="only functions whose name or source contains this")
    parser.add_argument("--verbose", action="store_true", help="print skip reasons")
    args = parser.parse_args()
    data = json.load(open(args.json))
    checked = skipped = 0
    skip_reasons = collections.Counter()
    divergent = []  # (source, name, trial, args, bucket, detail)
    call_args = []
    call_order = []
    narrow_calls = []
    for f in data:
        if not f.get("reference_words"):
            continue
        if args.filter and args.filter not in f["name"] and args.filter not in f["source"]:
            continue
        try:
            reference, produced = sides_of(f)
            bad = None
            soft = None
            for trial in range(args.trials):
                gpr, fpr = trial_registers(trial, len(reference.words))
                ref_machine = run(reference, gpr, trial, fpr=fpr)
                our_machine = run(produced, gpr, trial, fpr=fpr)
                result = compare(ref_machine, our_machine)
                if result is None:
                    continue
                entry = (trial, [hex(g) for g in gpr[3:8]], result[0], result[1])
                if result[0] in ("call-args", "call-order"):
                    soft = soft or entry
                    continue
                bad = entry
                break
            if bad and bad[2] != "preserved" and wide_return_only(reference, produced, args.trials):
                narrow_calls.append((f["source"], f["name"], bad))
                bad = None
            checked += 1
            if bad:
                divergent.append((f["source"], f["name"], bad))
            elif soft and soft[2] == "call-order":
                call_order.append((f["source"], f["name"], soft))
            elif soft:
                call_args.append((f["source"], f["name"], soft))
        except Unsupported as error:
            skipped += 1
            skip_reasons[re.sub(r"\d+$", "N", str(error)) if str(error).startswith("reloc") else str(error)] += 1

    # A narrow return value's upper bits are the caller's to extend: differences
    # confined to them are not divergences.
    def narrow_only(bad):
        if bad[2] != "r3" or not all(isinstance(v, int) for v in bad[3]):
            return False
        ref, ours = bad[3]
        return (ref ^ ours) & 0xFF == 0 or (ref ^ ours) & 0xFFFF == 0
    narrow = [d for d in divergent if narrow_only(d[2])]
    divergent = [d for d in divergent if not narrow_only(d[2])]
    print(f"checked {checked}, skipped {skipped}, divergent {len(divergent)} "
          f"(+{len(narrow)} differing only above a narrow return, +{len(narrow_calls)} only with wide "
          f"call results, +{len(call_order)} only in call order, +{len(call_args)} only in call arguments)")
    for source, name, bad in divergent:
        trial, arguments, bucket, detail = bad
        if bucket == "r3":
            detail = f"ref r3={show(detail[0])} ours r3={show(detail[1])}"
        print(f"  DIVERGES {source} {name}: trial {trial} args {arguments} [{bucket}] {detail}")
    if narrow_calls:
        print("differ only when a call returns a value outside 0..127 (callee narrow return extension assumed):")
        for source, name, bad in narrow_calls:
            print(f"  WIDE-RESULT {source} {name}: trial {bad[0]} [{bad[2]}] {bad[3]}")
    if call_order:
        print("same calls in a different order (often unspecified evaluation order):")
        for source, name, bad in call_order:
            print(f"  CALL-ORDER {source} {name}: trial {bad[0]} args {bad[1]} {bad[3]}")
    if call_args:
        print("call args differ (argument registers either side set before a call):")
        for source, name, bad in call_args:
            print(f"  CALL-ARGS {source} {name}: trial {bad[0]} args {bad[1]} {bad[3]}")
    if args.verbose:
        print("skip reasons:")
        for reason, count in skip_reasons.most_common(30):
            print(f"  {count:6d}  {reason}")
    return 1 if divergent else 0


if __name__ == "__main__":
    sys.exit(main())
