#!/usr/bin/env python3
"""Compare executable bytes and text relocations without conflating them.

Whole-object equality is the parity finish line.  This diagnostic decomposes a
non-equal object into backend-facing facts so debug/metadata gaps do not hide an
otherwise exact instruction stream, and symbol-numbering gaps do not masquerade
as instruction-selection failures.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import re
import subprocess
from typing import Iterable, Sequence


@dataclass(frozen=True)
class TextRelocation:
    offset: int
    kind: str
    target: str

    @property
    def shape(self) -> tuple[int, str]:
        return self.offset, self.kind

    @property
    def normalized_target(self) -> str:
        return re.sub(r"(?<![A-Za-z0-9_$])@\d+\b", "@<anonymous>", self.target)


def run_objdump(objdump: Path, *arguments: str) -> str:
    result = subprocess.run(
        [str(objdump), *arguments],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return result.stdout


def parse_section_bytes(output: str) -> bytes:
    """Read only hex columns from ``objdump -s -j .text`` output."""

    result = bytearray()
    in_contents = False
    for line in output.splitlines():
        if line.startswith("Contents of section .text:"):
            in_contents = True
            continue
        if not in_contents:
            continue
        fields = line.split()
        if not fields or not re.fullmatch(r"[0-9A-Fa-f]+", fields[0]):
            continue
        for field in fields[1:5]:
            if not re.fullmatch(r"(?:[0-9A-Fa-f]{2}){1,4}", field):
                break
            result.extend(bytes.fromhex(field))
    return bytes(result)


def parse_text_relocations(output: str) -> list[TextRelocation]:
    relocations: list[TextRelocation] = []
    pattern = re.compile(r"^\s*([0-9A-Fa-f]+)\s+(R_[A-Za-z0-9_]+)\s+(.+?)\s*$")
    for line in output.splitlines():
        if match := pattern.match(line):
            relocations.append(
                TextRelocation(int(match.group(1), 16), match.group(2), match.group(3))
            )
    return relocations


def first_sequence_difference(reference: Sequence[object], candidate: Sequence[object]) -> int:
    for index, (reference_value, candidate_value) in enumerate(zip(reference, candidate)):
        if reference_value != candidate_value:
            return index
    return min(len(reference), len(candidate))


def describe_byte_difference(reference: bytes, candidate: bytes) -> str:
    index = first_sequence_difference(reference, candidate)
    reference_value = f"0x{reference[index]:02x}" if index < len(reference) else "<end>"
    candidate_value = f"0x{candidate[index]:02x}" if index < len(candidate) else "<end>"
    return f"first difference at .text+0x{index:x}: reference {reference_value}, candidate {candidate_value}"


def statuses(
    reference_bytes: bytes,
    candidate_bytes: bytes,
    reference_relocations: Sequence[TextRelocation],
    candidate_relocations: Sequence[TextRelocation],
) -> list[str]:
    no_code = not reference_bytes and not candidate_bytes
    bytes_equal = reference_bytes == candidate_bytes
    reference_shape = [relocation.shape for relocation in reference_relocations]
    candidate_shape = [relocation.shape for relocation in candidate_relocations]
    shape_equal = reference_shape == candidate_shape
    targets_equal = list(reference_relocations) == list(candidate_relocations)
    normalized_equal = shape_equal and [
        relocation.normalized_target for relocation in reference_relocations
    ] == [relocation.normalized_target for relocation in candidate_relocations]
    no_relocations = not reference_relocations and not candidate_relocations

    lines = []
    if no_code:
        lines.append("TEXT_BYTES EMPTY — neither object has emitted .text bytes")
    elif bytes_equal:
        lines.append("TEXT_BYTES BYTE — raw .text bytes match")
    else:
        lines.append(
            "TEXT_BYTES DIFF — "
            + describe_byte_difference(reference_bytes, candidate_bytes)
        )

    if no_relocations:
        lines.append("TEXT_RELOC_SHAPE EMPTY — neither .text section has relocations")
        lines.append("TEXT_RELOC_TARGETS EMPTY — neither .text section has relocation targets")
    else:
        lines.append(
            "TEXT_RELOC_SHAPE BYTE — relocation offsets and types match"
            if shape_equal
            else (
                "TEXT_RELOC_SHAPE DIFF — first differing relocation index "
                f"{first_sequence_difference(reference_shape, candidate_shape)}"
            )
        )
        if targets_equal:
            lines.append("TEXT_RELOC_TARGETS BYTE — relocation target symbols match")
        elif shape_equal:
            index = first_sequence_difference(
                [relocation.target for relocation in reference_relocations],
                [relocation.target for relocation in candidate_relocations],
            )
            lines.append(
                "TEXT_RELOC_TARGETS DIFF — first difference at relocation "
                f"{index}: reference {reference_relocations[index].target}, "
                f"candidate {candidate_relocations[index].target}"
            )
        else:
            lines.append(
                "TEXT_RELOC_TARGETS DIFF — targets are not comparable because relocation shape differs"
            )
        if not targets_equal and normalized_equal:
            lines.append(
                "ANON_ORDINALS DIFF — anonymous symbol numbers are the only relocation-target difference"
            )

    if no_code and no_relocations:
        aggregate = "CODE EMPTY — neither object has emitted code"
    elif bytes_equal and shape_equal and targets_equal:
        aggregate = "CODE BYTE — .text bytes and text relocations match"
    else:
        aggregate = "CODE DIFF — see TEXT_* component results"
    return [aggregate, *lines]


def measure(objdump: Path, object_path: Path) -> tuple[bytes, list[TextRelocation]]:
    section = parse_section_bytes(run_objdump(objdump, "-s", "-j", ".text", str(object_path)))
    relocations = parse_text_relocations(
        run_objdump(objdump, "-r", "-j", ".text", str(object_path))
    )
    return section, relocations


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("objdump", type=Path)
    parser.add_argument("reference", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--context", default="")
    args = parser.parse_args()

    reference_bytes, reference_relocations = measure(args.objdump, args.reference)
    candidate_bytes, candidate_relocations = measure(args.objdump, args.candidate)
    suffix = f" in {args.context}" if args.context else ""
    for line in statuses(
        reference_bytes,
        candidate_bytes,
        reference_relocations,
        candidate_relocations,
    ):
        print(f"{line}{suffix}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
