#!/usr/bin/env python3
"""Extract relocatable CodeWarrior debug sections into a compact capture.

The capture keeps raw `.line`/`.debug` bytes but represents relocations by
semantic target name rather than ELF symbol index.  The object writer can then
rebind those targets to the symbol table it creates for an otherwise-generated
translation unit.  This is intentionally a debug-lowering corpus format, not a
copy of an ELF object.
"""

from __future__ import annotations

import argparse
import struct
from dataclasses import dataclass
from pathlib import Path


ELF32_HEADER = struct.Struct(">16sHHIIIIIHHHHHH")
SECTION_HEADER = struct.Struct(">IIIIIIIIII")
SYMBOL = struct.Struct(">IIIBBH")
RELOCATION = struct.Struct(">IIi")


@dataclass(frozen=True)
class Section:
    name: str
    section_type: int
    offset: int
    size: int
    link: int
    info: int
    entry_size: int


def c_string(data: bytes, offset: int) -> str:
    end = data.index(0, offset)
    return data[offset:end].decode("utf-8")


def parse_sections(data: bytes) -> list[Section]:
    header = ELF32_HEADER.unpack_from(data)
    section_offset = header[6]
    section_entry_size = header[11]
    section_count = header[12]
    section_names_index = header[13]
    raw = [
        SECTION_HEADER.unpack_from(data, section_offset + index * section_entry_size)
        for index in range(section_count)
    ]
    names_offset = raw[section_names_index][4]
    names_size = raw[section_names_index][5]
    names = data[names_offset : names_offset + names_size]
    return [
        Section(
            name=c_string(names, fields[0]) if fields[0] else "",
            section_type=fields[1],
            offset=fields[4],
            size=fields[5],
            link=fields[6],
            info=fields[7],
            entry_size=fields[9],
        )
        for fields in raw
    ]


def section_bytes(data: bytes, sections: list[Section], name: str) -> bytes:
    section = next(section for section in sections if section.name == name)
    return data[section.offset : section.offset + section.size]


def symbols(data: bytes, sections: list[Section]) -> list[tuple[str, int, int, int, int]]:
    symbol_section = next(section for section in sections if section.name == ".symtab")
    strings = section_bytes(data, sections, sections[symbol_section.link].name)
    result = []
    for offset in range(
        symbol_section.offset,
        symbol_section.offset + symbol_section.size,
        symbol_section.entry_size or SYMBOL.size,
    ):
        name_offset, value, size, info, _other, section_index = SYMBOL.unpack_from(data, offset)
        name = c_string(strings, name_offset) if name_offset else ""
        if info & 0xF == 3 and section_index < len(sections):
            name = sections[section_index].name
        result.append((name, value, size, info, section_index))
    return result


def relocations(
    data: bytes,
    sections: list[Section],
    symbol_table: list[tuple[str, int, int, int, int]],
    name: str,
) -> list[tuple[int, int, bool, str, int]]:
    relocation_section = next(section for section in sections if section.name == name)
    result = []
    for offset in range(
        relocation_section.offset,
        relocation_section.offset + relocation_section.size,
        relocation_section.entry_size or RELOCATION.size,
    ):
        address, info, addend = RELOCATION.unpack_from(data, offset)
        symbol_index = info >> 8
        relocation_type = info & 0xFF
        target, _value, _size, symbol_info, _section_index = symbol_table[symbol_index]
        if not target:
            raise ValueError(f"relocation at {address:#x} has an unnamed non-section target")
        result.append((address, relocation_type, symbol_info & 0xF == 3, target, addend))
    return result


def write_bytes(output: bytearray, value: bytes) -> None:
    output.extend(struct.pack(">I", len(value)))
    output.extend(value)


def write_relocations(
    output: bytearray, records: list[tuple[int, int, bool, str, int]]
) -> None:
    output.extend(struct.pack(">I", len(records)))
    for offset, kind, is_section, target, addend in records:
        encoded_target = target.encode("utf-8")
        output.extend(struct.pack(">IBBHi", offset, kind, is_section, len(encoded_target), addend))
        output.extend(encoded_target)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("object", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--layout",
        choices=("before-grouped", "before-interleaved", "after-interleaved", "after-grouped"),
        default="after-grouped",
    )
    args = parser.parse_args()

    data = args.object.read_bytes()
    if data[:6] != b"\x7fELF\x01\x02":
        raise ValueError("expected a big-endian ELF32 object")
    sections = parse_sections(data)
    symbol_table = symbols(data, sections)
    layout = {
        "before-grouped": 0,
        "before-interleaved": 1,
        "after-interleaved": 2,
        "after-grouped": 3,
    }[args.layout]

    output = bytearray(b"MWDC\x01")
    output.append(layout)
    write_bytes(output, section_bytes(data, sections, ".line"))
    write_bytes(output, section_bytes(data, sections, ".debug"))
    write_relocations(output, relocations(data, sections, symbol_table, ".rela.line"))
    write_relocations(output, relocations(data, sections, symbol_table, ".rela.debug"))

    # Fragmented generations define named symbols inside the debug sections.
    # Preserve them independently of the ELF symbol indexes.
    debug_section_indexes = {
        index for index, section in enumerate(sections) if section.name in (".line", ".debug")
    }
    captured_symbols = [
        symbol
        for symbol in symbol_table
        if symbol[0] not in (".line", ".debug") and symbol[4] in debug_section_indexes
    ]
    output.extend(struct.pack(">I", len(captured_symbols)))
    for name, value, size, info, section_index in captured_symbols:
        encoded_name = name.encode("utf-8")
        section_kind = 0 if sections[section_index].name == ".line" else 1
        binding = info >> 4
        output.extend(
            struct.pack(">HBBIII", len(encoded_name), section_kind, binding != 0, value, size, 1)
        )
        output.extend(encoded_name)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(output)
    print(
        f"wrote {args.output}: line={len(section_bytes(data, sections, '.line'))} "
        f"debug={len(section_bytes(data, sections, '.debug'))} "
        f"relocations={len(relocations(data, sections, symbol_table, '.rela.line')) + len(relocations(data, sections, symbol_table, '.rela.debug'))}"
    )


if __name__ == "__main__":
    main()
