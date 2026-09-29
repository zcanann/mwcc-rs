#!/usr/bin/env python3
"""Extract MWCC's PCode opcode descriptors and default scheduling model.

Addresses come from the CC0 GC/1.2.5 decompilation
(github.com/JackPriceBurns/mwcc: tools/allocator_snapshot.py,
docs/SCHEDULER.md) and are valid for the stock GC/1.2.5 binary (sha256
0443b5c0...), which GC/1.2.5n shares for these tables. Other builds need
their own addresses.

Output JSON rows, one per opcode:
  mnemonic, operand_format, fixed_operand_count, pick_rank (descriptor
  byte 9: the scheduler's last tie-break), flags,
  unit, latency, occupancy, stage2, stage3, serialize (machine model).

Usage: python tools/extract_mwcc_tables.py <mwcceppc.exe> > out.json
"""

import json
import struct
import sys

DESCRIPTORS = 0x005654B0
MAX_OPCODE = 0x01D1
DEFAULT_MODEL = 0x00574D70
MODEL_RECORDS = 0x00574D90


class PE:
    def __init__(self, data: bytes):
        self.data = data
        pe = struct.unpack_from("<I", data, 0x3C)[0]
        sections = struct.unpack_from("<H", data, pe + 6)[0]
        optional_size = struct.unpack_from("<H", data, pe + 20)[0]
        self.image_base = struct.unpack_from("<I", data, pe + 24 + 28)[0]
        table = pe + 24 + optional_size
        self.sections = []
        for index in range(sections):
            entry = table + index * 40
            virtual_size, virtual_address, raw_size, raw_pointer = struct.unpack_from(
                "<IIII", data, entry + 8)
            self.sections.append((virtual_address, max(virtual_size, raw_size), raw_pointer))

    def read(self, address: int, size: int) -> bytes:
        rva = address - self.image_base
        for virtual_address, length, raw_pointer in self.sections:
            if virtual_address <= rva < virtual_address + length:
                offset = raw_pointer + rva - virtual_address
                return self.data[offset:offset + size]
        raise ValueError(f"address 0x{address:08x} is not mapped")

    def c_string(self, address: int) -> str:
        out = bytearray()
        while True:
            byte = self.read(address + len(out), 1)[0]
            if byte == 0:
                return out.decode("latin-1")
            out.append(byte)


def main() -> int:
    pe = PE(open(sys.argv[1], "rb").read())
    issue_width, register_edge_latency = struct.unpack("<II", pe.read(DEFAULT_MODEL, 8))
    rows = []
    for opcode in range(MAX_OPCODE + 1):
        raw = pe.read(DESCRIPTORS + opcode * 16, 16)
        mnemonic, operand_format = struct.unpack_from("<II", raw)
        model = pe.read(MODEL_RECORDS + opcode * 6, 6)
        rows.append({
            "opcode": opcode,
            "mnemonic": pe.c_string(mnemonic),
            "operand_format": pe.c_string(operand_format),
            "fixed_operand_count": raw[8],
            "pick_rank": raw[9],
            "flags": struct.unpack_from("<H", raw, 10)[0],
            "unit": model[0],
            "latency": model[1],
            "occupancy": model[2],
            "stage2": model[3],
            "stage3": model[4],
            "serialize": model[5],
        })
    json.dump({
        "source": "GC/1.2.5 mwcceppc.exe default machine model (0x574d70)",
        "issue_width": issue_width,
        "register_edge_latency": register_edge_latency,
        "opcodes": rows,
    }, sys.stdout, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
