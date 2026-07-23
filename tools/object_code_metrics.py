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


@dataclass(frozen=True)
class TextFunction:
    address: int
    size: int
    name: str


@dataclass(frozen=True)
class FunctionParity:
    reference_functions: int
    candidate_functions: int
    comparable_functions: int
    text_exact_functions: int
    code_exact_functions: int
    reference_function_bytes: int
    text_exact_reference_bytes: int
    code_exact_reference_bytes: int
    missing_functions: int
    candidate_only_functions: int


@dataclass(frozen=True)
class FunctionMatch:
    reference_bytes: int
    candidate_present: bool
    text_exact: bool
    code_exact: bool


def run_objdump(objdump: Path, *arguments: str) -> str:
    result = subprocess.run(
        [str(objdump), *arguments],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        # Data-only translation units legitimately have no .text section.
        # Binutils reports that absence as an error for both `-s -j .text` and
        # `-r -j .text`; semantically the requested bytes/relocations are empty.
        if ".text" in arguments and "not found" in result.stderr:
            return result.stdout
        raise subprocess.CalledProcessError(
            result.returncode,
            [str(objdump), *arguments],
            output=result.stdout,
            stderr=result.stderr,
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


def parse_text_functions(output: str) -> list[TextFunction]:
    """Read defined, non-empty ``.text`` function symbols from ``objdump -t``."""

    functions: list[TextFunction] = []
    pattern = re.compile(
        r"^\s*([0-9A-Fa-f]+)\s+.*?\bF\s+\.text\s+"
        r"([0-9A-Fa-f]+)\s+(\S.*?)\s*$"
    )
    for line in output.splitlines():
        if match := pattern.match(line):
            size = int(match.group(2), 16)
            if size:
                functions.append(
                    TextFunction(int(match.group(1), 16), size, match.group(3))
                )
    return functions


def function_parity(
    reference_bytes: bytes,
    candidate_bytes: bytes,
    reference_relocations: Sequence[TextRelocation],
    candidate_relocations: Sequence[TextRelocation],
    reference_functions: Sequence[TextFunction],
    candidate_functions: Sequence[TextFunction],
) -> FunctionParity:
    """Compare named function bodies independently of whole-section placement."""

    reference_by_name = {function.name: function for function in reference_functions}
    candidate_by_name = {function.name: function for function in candidate_functions}
    matches = function_matches(
        reference_bytes,
        candidate_bytes,
        reference_relocations,
        candidate_relocations,
        reference_functions,
        candidate_functions,
    )

    return FunctionParity(
        reference_functions=len(reference_by_name),
        candidate_functions=len(candidate_by_name),
        comparable_functions=sum(match.candidate_present for match in matches.values()),
        text_exact_functions=sum(match.text_exact for match in matches.values()),
        code_exact_functions=sum(match.code_exact for match in matches.values()),
        reference_function_bytes=sum(match.reference_bytes for match in matches.values()),
        text_exact_reference_bytes=sum(
            match.reference_bytes for match in matches.values() if match.text_exact
        ),
        code_exact_reference_bytes=sum(
            match.reference_bytes for match in matches.values() if match.code_exact
        ),
        missing_functions=sum(not match.candidate_present for match in matches.values()),
        candidate_only_functions=len(candidate_by_name.keys() - reference_by_name.keys()),
    )


def function_matches(
    reference_bytes: bytes,
    candidate_bytes: bytes,
    reference_relocations: Sequence[TextRelocation],
    candidate_relocations: Sequence[TextRelocation],
    reference_functions: Sequence[TextFunction],
    candidate_functions: Sequence[TextFunction],
) -> dict[str, FunctionMatch]:
    """Return named per-function evidence for checkpoint-to-checkpoint deltas."""

    reference_by_name = {function.name: function for function in reference_functions}
    candidate_by_name = {function.name: function for function in candidate_functions}

    def body(section: bytes, function: TextFunction) -> bytes:
        return section[function.address : function.address + function.size]

    def function_relocations(
        relocations: Sequence[TextRelocation], function: TextFunction
    ) -> list[TextRelocation]:
        end = function.address + function.size
        return [
            TextRelocation(
                relocation.offset - function.address,
                relocation.kind,
                relocation.target,
            )
            for relocation in relocations
            if function.address <= relocation.offset < end
        ]

    result: dict[str, FunctionMatch] = {}
    for name, reference in reference_by_name.items():
        candidate = candidate_by_name.get(name)
        if candidate is None:
            result[name] = FunctionMatch(reference.size, False, False, False)
            continue
        bytes_equal = (
            reference.size == candidate.size
            and body(reference_bytes, reference) == body(candidate_bytes, candidate)
        )
        reference_function_relocations = function_relocations(
            reference_relocations, reference
        )
        candidate_function_relocations = function_relocations(
            candidate_relocations, candidate
        )
        # Anonymous ordinals are object-local numbering, not symbol identity.
        # Partial-TU projection can omit unrelated functions and consequently
        # renumber an otherwise identical literal-pool target.  Its section
        # data is measured independently; code parity must not count that
        # bookkeeping drift as a function regression.
        relocations_equal = bytes_equal and [
            (relocation.offset, relocation.kind, relocation.normalized_target)
            for relocation in reference_function_relocations
        ] == [
            (relocation.offset, relocation.kind, relocation.normalized_target)
            for relocation in candidate_function_relocations
        ]
        result[name] = FunctionMatch(
            reference.size,
            True,
            bytes_equal,
            relocations_equal,
        )
    return result


def describe_function_delta(
    baseline: dict[str, FunctionMatch],
    candidate: dict[str, FunctionMatch],
) -> list[str]:
    """Describe coverage and exactness movement against a prior checkpoint."""

    dimensions = [
        ("COVERAGE", lambda match: match.candidate_present),
        ("TEXT", lambda match: match.text_exact),
        ("CODE", lambda match: match.code_exact),
    ]
    lines: list[str] = []
    for label, predicate in dimensions:
        baseline_names = {name for name, match in baseline.items() if predicate(match)}
        candidate_names = {name for name, match in candidate.items() if predicate(match)}
        gained = sorted(candidate_names - baseline_names)
        regressed = sorted(baseline_names - candidate_names)
        function_delta = len(candidate_names) - len(baseline_names)
        byte_delta = sum(
            candidate[name].reference_bytes for name in candidate_names
        ) - sum(baseline[name].reference_bytes for name in baseline_names)
        gained_names = ", ".join(gained) if gained else "none"
        regressed_names = ", ".join(regressed) if regressed else "none"
        lines.append(
            f"FUNCTION_{label}_DELTA {function_delta:+d} functions, "
            f"{byte_delta:+d} reference bytes; gained: {gained_names}; "
            f"regressed: {regressed_names}"
        )
    return lines


def describe_function_parity(parity: FunctionParity, relocation_aware: bool) -> str:
    exact_functions = (
        parity.code_exact_functions if relocation_aware else parity.text_exact_functions
    )
    exact_bytes = (
        parity.code_exact_reference_bytes
        if relocation_aware
        else parity.text_exact_reference_bytes
    )
    if not parity.reference_functions and not parity.candidate_functions:
        return "EMPTY — neither object has named .text functions"
    exact = (
        exact_functions == parity.reference_functions
        and parity.missing_functions == 0
        and parity.candidate_only_functions == 0
    )
    status = "BYTE" if exact else "DIFF"
    function_percent = 100.0 * exact_functions / parity.reference_functions if parity.reference_functions else 0.0
    byte_percent = (
        100.0 * exact_bytes / parity.reference_function_bytes
        if parity.reference_function_bytes
        else 0.0
    )
    qualifier = "relocation-aware " if relocation_aware else ""
    return (
        f"{status} — {exact_functions}/{parity.reference_functions} {qualifier}functions exact "
        f"({function_percent:.1f}%); {exact_bytes}/{parity.reference_function_bytes} "
        f"reference function bytes exact ({byte_percent:.1f}%); "
        f"{parity.comparable_functions} comparable, {parity.missing_functions} missing, "
        f"{parity.candidate_only_functions} candidate-only"
    )


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


def measure_functions(objdump: Path, object_path: Path) -> list[TextFunction]:
    return parse_text_functions(run_objdump(objdump, "-t", str(object_path)))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("objdump", type=Path)
    parser.add_argument("reference", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--context", default="")
    parser.add_argument(
        "--baseline",
        type=Path,
        help="prior candidate object used to report paired function movement",
    )
    args = parser.parse_args()

    reference_bytes, reference_relocations = measure(args.objdump, args.reference)
    candidate_bytes, candidate_relocations = measure(args.objdump, args.candidate)
    parity = function_parity(
        reference_bytes,
        candidate_bytes,
        reference_relocations,
        candidate_relocations,
        measure_functions(args.objdump, args.reference),
        measure_functions(args.objdump, args.candidate),
    )
    suffix = f" in {args.context}" if args.context else ""
    for line in statuses(
        reference_bytes,
        candidate_bytes,
        reference_relocations,
        candidate_relocations,
    ):
        print(f"{line}{suffix}")
    print(f"FUNCTION_TEXT {describe_function_parity(parity, False)}{suffix}")
    print(f"FUNCTION_CODE {describe_function_parity(parity, True)}{suffix}")
    if args.baseline is not None:
        baseline_bytes, baseline_relocations = measure(args.objdump, args.baseline)
        baseline_matches = function_matches(
            reference_bytes,
            baseline_bytes,
            reference_relocations,
            baseline_relocations,
            measure_functions(args.objdump, args.reference),
            measure_functions(args.objdump, args.baseline),
        )
        candidate_matches = function_matches(
            reference_bytes,
            candidate_bytes,
            reference_relocations,
            candidate_relocations,
            measure_functions(args.objdump, args.reference),
            measure_functions(args.objdump, args.candidate),
        )
        for line in describe_function_delta(baseline_matches, candidate_matches):
            print(f"{line}{suffix}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
