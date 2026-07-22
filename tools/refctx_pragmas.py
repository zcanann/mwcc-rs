#!/usr/bin/env python3
"""Preserve mwcc-rs-modeled pragmas across MWCC preprocessing."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
from typing import Iterable, Optional


PRAGMA = re.compile(
    rb"^[ \t]*#[ \t]*pragma[ \t]+"
    rb"(push|pop|cplusplus|defer_codegen|force_active|peephole)"
    rb"(?:[ \t]+(on|off|reset))?[ \t]*(?://[^\r\n]*)?(\r?\n)?$"
)
SENTINEL = re.compile(
    rb"^[ \t]*extern[ \t]+int[ \t]+__mwcc_refctx_pragma_"
    rb"(push|pop|cplusplus|defer_codegen|force_active|peephole)"
    rb"(?:_(on|off|reset))?;[ \t]*(\r?\n)?$"
)


def _directive(name: bytes, value: Optional[bytes]) -> Optional[bytes]:
    if name in {b"push", b"pop"}:
        return name if value is None else None
    if value in {b"on", b"off", b"reset"}:
        return name + b" " + value
    return None


def mark_pragmas(lines: Iterable[bytes]) -> list[bytes]:
    """Replace modeled pragma lines with preprocessor-stable declarations."""

    output = []
    for line in lines:
        match = PRAGMA.fullmatch(line)
        if match is None:
            output.append(line)
            continue
        name, value, newline = match.groups()
        directive = _directive(name, value)
        if directive is None:
            output.append(line)
            continue
        suffix = b"" if value is None else b"_" + value
        output.append(
            b"extern int __mwcc_refctx_pragma_"
            + name
            + suffix
            + b";"
            + (newline or b"")
        )
    return output


def restore_pragmas(lines: Iterable[bytes]) -> list[bytes]:
    """Restore declarations emitted by :func:`mark_pragmas` to pragmas."""

    output = []
    for line in lines:
        match = SENTINEL.fullmatch(line)
        if match is None:
            output.append(line)
            continue
        name, value, newline = match.groups()
        directive = _directive(name, value)
        if directive is None:
            output.append(line)
            continue
        output.append(b"#pragma " + directive + (newline or b""))
    return output


def transform(source: Path, output: Path, *, restore: bool) -> None:
    lines = source.read_bytes().splitlines(keepends=True)
    transformed = restore_pragmas(lines) if restore else mark_pragmas(lines)
    output.write_bytes(b"".join(transformed))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("mark", "restore"))
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    transform(args.source, args.output, restore=args.mode == "restore")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
