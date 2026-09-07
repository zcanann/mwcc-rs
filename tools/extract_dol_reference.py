#!/usr/bin/env python3
"""Extract a linked function from a project's original DOL and symbol map.

This is a linked-code reference, not a relocatable compiler-reference object.
The manifest pins both inputs so later comparisons can distinguish that evidence.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import struct


def read_dol_range(data: bytes, address: int, size: int) -> bytes:
    if len(data) < 0x100:
        raise ValueError("truncated DOL header")
    if address < 0 or size <= 0 or address + size > 1 << 32:
        raise ValueError("invalid virtual address range")
    matches = []
    for index in range(18):
        offset = struct.unpack_from(">I", data, index * 4)[0]
        start = struct.unpack_from(">I", data, 0x48 + index * 4)[0]
        length = struct.unpack_from(">I", data, 0x90 + index * 4)[0]
        if length and start <= address and address + size <= start + length:
            if offset < 0x100 or offset + length > len(data):
                raise ValueError("DOL section extends outside file data")
            position = offset + address - start
            matches.append(data[position:position + size])
    if len(matches) != 1:
        raise ValueError("requested range must belong to exactly one file-backed section")
    return matches[0]


def function_range(symbols: str, name: str) -> tuple[int, int]:
    pattern = re.compile(
        r"^\s*" + re.escape(name)
        + r"\s*=\s*\.text:(0x[0-9a-fA-F]+);\s*//([^\n]*)$", re.MULTILINE
    )
    matches = []
    for match in pattern.finditer(symbols):
        attributes = dict(re.findall(r"(\w+):([^\s]+)", match[2]))
        if attributes.get("type") == "function" and "size" in attributes:
            matches.append((int(match[1], 16), int(attributes["size"], 0)))
    if len(matches) != 1:
        raise ValueError("symbol must identify exactly one sized text function")
    return matches[0]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dol", type=Path, required=True)
    parser.add_argument("--symbols", type=Path, required=True)
    parser.add_argument("--symbol", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        dol = args.dol.read_bytes()
        symbol_data = args.symbols.read_bytes()
        address, size = function_range(symbol_data.decode("utf-8"), args.symbol)
        code = read_dol_range(dol, address, size)
    except (OSError, UnicodeError, ValueError) as error:
        parser.error(str(error))
    manifest = {
        "schema_version": 1,
        "reference_kind": "linked_dol_function",
        "dol": str(args.dol.resolve()),
        "dol_sha256": hashlib.sha256(dol).hexdigest(),
        "symbols": str(args.symbols.resolve()),
        "symbols_sha256": hashlib.sha256(symbol_data).hexdigest(),
        "symbol": args.symbol,
        "address": address,
        "size": size,
        "code_sha256": hashlib.sha256(code).hexdigest(),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(code)
    args.output.with_suffix(args.output.suffix + ".json").write_text(
        json.dumps(manifest, indent=2) + "\n"
    )
    print(f"{args.symbol}: {size} bytes at {address:#010x} -> {args.output}")


if __name__ == "__main__":
    main()
