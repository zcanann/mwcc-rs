#!/usr/bin/env python3
"""Compare ELF function text at a pinned original DOL's link addresses.

A layout supplies SDA bases, named symbols, external functions, literals, and initialized data images. Unknown
relocations prevent an exact result. This measures linked function text, not
relocatable object, debug information, or whole-project parity.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import struct

from extract_debug_capture import parse_sections, symbols, relocations
from extract_dol_reference import function_range, read_dol_range


def relocate(word, kind, target, place, sda_bases):
    if kind in (4, 5, 6):  # R_PPC_ADDR16_LO, HI, HA
        immediate = target if kind == 4 else (target + (0x8000 if kind == 6 else 0)) >> 16
        return (word & 0xFFFF0000) | (immediate & 0xFFFF)
    if kind == 10:  # R_PPC_REL24
        delta = target - place
        if word >> 26 != 18 or word & 2 or delta & 3 or not -(1 << 25) <= delta < (1 << 25):
            raise ValueError("invalid relative branch relocation")
        return (word & ~0x03FFFFFC) | (delta & 0x03FFFFFC)
    if kind == 109:  # R_PPC_EMB_SDA21
        choices = [(int(reg), target - base) for reg, base in sda_bases.items()
                   if -32768 <= target - base <= 32767]
        if len(choices) != 1 or choices[0][0] not in (2, 13):
            raise ValueError("SDA relocation needs one unambiguous r2/r13 placement")
        reg, delta = choices[0]
        return (word & ~0x001FFFFF) | (reg << 16) | (delta & 0xFFFF)
    raise ValueError(f"unsupported relocation kind {kind}")


def compare(data, dol, symbol_text, layout):
    for key, content in [('dol_sha256', dol), ('symbols_sha256', symbol_text.encode('utf-8'))]:
        if hashlib.sha256(content).hexdigest() != layout[key]:
            raise ValueError(f"{key} does not match the pinned reference")
    ranges = {name: function_range(symbol_text, name) for name in layout['functions']}
    placements = {name: address for name, (address, _) in ranges.items()}
    for name in layout.get('external_functions', []):
        placements[name] = function_range(symbol_text, name)[0]
    for name, address in layout['data_symbols'].items():
        matches = re.findall(r'^\s*' + re.escape(name) + r'\s*=\s*\.[\w.]+:(0x[0-9a-fA-F]+);', symbol_text, re.MULTILINE)
        if len(matches) != 1 or int(matches[0], 16) != address:
            raise ValueError(f"data symbol placement is not verified: {name}")
        placements[name] = address
    for literal in layout['literals']:
        value = bytes.fromhex(literal['hex'])
        if read_dol_range(dol, literal['address'], len(value)) != value:
            raise ValueError("literal placement does not contain the expected bytes")
    data_images = layout.get('data_images', [])
    for item in data_images:
        value = bytes.fromhex(item['hex'])
        matches = re.findall(r'^\s*' + re.escape(item['reference_symbol'])
                             + r'\s*=\s*\.data:(0x[0-9a-fA-F]+);[^\n]*type:object size:(0x[0-9a-fA-F]+)',
                             symbol_text, re.MULTILINE)
        if len(matches) != 1 or (int(matches[0][0], 16), int(matches[0][1], 16)) != (item['address'], len(value)):
            raise ValueError("initialized data image does not match the pinned symbol range")
        if not value or read_dol_range(dol, item['address'], len(value)) != value:
            raise ValueError("initialized data image does not contain the expected bytes")
    if data[:7] != b'\x7fELF\x01\x02\x01' or struct.unpack_from('>HH', data, 16) != (1, 20):
        raise ValueError("candidate must be a big-endian ELF32 PowerPC relocatable object")
    sections = parse_sections(data)
    table = symbols(data, sections)
    text_sections = [s for s in sections if s.name == '.text']
    if len(text_sections) != 1:
        raise ValueError("candidate needs one text section")
    text = text_sections[0]
    if text.offset + text.size > len(data):
        raise ValueError("candidate text extends outside the object")
    fixups = relocations(data, sections, table, '.rela.text') if any(s.name == '.rela.text' for s in sections) else []

    def placement(name):
        if name in placements:
            return placements[name]
        matches = [s for s in table if s[0] == name]
        if len(matches) != 1:
            raise ValueError("relocation symbol is missing or ambiguous")
        _, offset, size, _, index = matches[0]
        if index >= len(sections) or sections[index].name not in ('.sdata2', '.rodata', '.data') or size <= 0:
            raise ValueError("relocation symbol has no configured placement")
        pool = sections[index]
        if offset + size > pool.size or pool.offset + offset + size > len(data):
            raise ValueError("literal symbol extends outside its section")
        value = data[pool.offset + offset:pool.offset + offset + size]
        images = layout['literals']
        if pool.name == '.data':
            images = data_images
            # An ordinal-independent placement requires one unique object image
            # on each side. Equal initialized objects must not be silently aliased.
            candidates = [(at, length) for _, at, length, info, section_index in table
                          if section_index == index and info & 15 == 1 and length == size
                          and data[pool.offset+at:pool.offset+at+length] == value]
            if len(candidates) != 1:
                raise ValueError("initialized data image is ambiguous in the candidate")
        matches = [item['address'] for item in images if bytes.fromhex(item['hex']) == value]
        if len(matches) != 1:
            raise ValueError("literal placement is missing or ambiguous")
        return matches[0]

    results = []
    for name, (address, original_size) in ranges.items():
        found = [s for s in table if s[0] == name and s[3] & 15 == 2
                 and s[4] < len(sections) and sections[s[4]].name == '.text']
        if len(found) != 1:
            results.append(dict(name=name, exact=False, error='missing or ambiguous candidate function'))
            continue
        _, offset, size, _, _ = found[0]
        if size <= 0 or offset + size > text.size or offset % 4 or size % 4:
            raise ValueError("invalid candidate function range")
        blob = bytearray(data[text.offset + offset:text.offset + offset + size])
        unknown = []
        for where, kind, _, symbol, addend in fixups:
            if not offset <= where < offset + size:
                continue
            at = (where - offset) & ~3
            try:
                if kind in (4, 5, 6) and (where-offset) % 4 != 2:
                    raise ValueError("address-half relocation is not on the immediate field")
                word = struct.unpack_from('>I', blob, at)[0]
                word = relocate(word, kind, placement(symbol) + addend, address + at, layout['sda_bases'])
                struct.pack_into('>I', blob, at, word)
            except ValueError as error:
                unknown.append(dict(offset=where-offset, kind=kind, symbol=symbol, reason=str(error)))
        original = read_dol_range(dol, address, original_size)
        results.append(dict(name=name, candidate_size=size, reference_size=original_size,
                            exact=not unknown and blob == original, unresolved_relocations=unknown,
                            candidate_linked_sha256=hashlib.sha256(blob).hexdigest(),
                            reference_sha256=hashlib.sha256(original).hexdigest()))
    return dict(reference_kind='linked_dol_functions', candidate_sha256=hashlib.sha256(data).hexdigest(),
                dol_sha256=layout['dol_sha256'], symbols_sha256=layout['symbols_sha256'], functions=results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['object', 'dol', 'symbols', 'layout', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    try:
        result = compare(args.object.read_bytes(), args.dol.read_bytes(), args.symbols.read_bytes().decode('utf-8'), json.loads(args.layout.read_text()))
    except (OSError, ValueError, KeyError, IndexError, StopIteration, struct.error) as error:
        parser.error(str(error))
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    exact = sum(row['exact'] for row in result['functions'])
    print(f"{exact}/{len(result['functions'])} functions have exact linked text")
    raise SystemExit(0 if exact == len(result['functions']) and exact else 1)


if __name__ == '__main__':
    main()
