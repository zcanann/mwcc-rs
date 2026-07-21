#!/usr/bin/env python3
"""Identify generated MWCC precompiled headers used by a decomp context."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
from typing import Iterable


PCH_MARKER = re.compile(r'^/\* ".*" line \d+ "([^"]+\.mch)" \*/$')


def generated_pch_paths(lines: Iterable[str]) -> list[str]:
    """Return normalized, unique .mch paths retained in source markers."""

    paths = {
        match.group(1).replace("\\", "/")
        for line in lines
        if (match := PCH_MARKER.fullmatch(line.rstrip("\r\n"))) is not None
    }
    return sorted(paths)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("context", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    with args.context.open(encoding="shift_jis", errors="replace") as source:
        for path in generated_pch_paths(source):
            print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
