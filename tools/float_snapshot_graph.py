#!/usr/bin/env python3
"""Compare single-precision snapshot operation graphs independently of FPR coloring.

This is a scheduling diagnostic, not an object-parity or general equivalence test.
It accepts straight-line bodies using stable GPR bases and ending in blr. Every
memory read has its own identity; stores separate memory epochs. Unsupported
instructions fail the comparison instead of disappearing from the graph.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import subprocess


class UnsupportedGraph(ValueError):
    pass


ARITHMETIC = {
    "fmuls": 3, "fadds": 3, "fsubs": 3, "fdivs": 3,
    "fmadds": 4, "fmsubs": 4, "fnmadds": 4, "fnmsubs": 4,
    "fmr": 2, "fneg": 2, "fabs": 2, "frsp": 2,
}
INSTRUCTION = re.compile(r"^\s*[0-9a-f]+:\s+(?:[0-9a-f]{2}\s+){4}(\S+)\s*(.*?)\s*$")


def parse_graph(disassembly: str) -> list[dict]:
    """Build exact ordered-operand identities; do not reassociate arithmetic."""
    values = {register: ("parameter", register) for register in range(32)}
    definitions = {}
    occurrences = Counter()
    nodes = []
    epoch = 0
    last_store = None
    pending_loads = []
    returned = False
    for line in disassembly.splitlines():
        match = INSTRUCTION.match(line)
        if not match:
            if re.match(r"^\s*[0-9a-f]+:", line):
                raise UnsupportedGraph(f"unrecognized instruction line: {line.strip()}")
            continue
        opcode, operands = match.groups()
        if returned:
            raise UnsupportedGraph("instructions follow blr")
        if opcode == "blr" and not operands:
            returned = True
            continue
        reads = []
        extra_deps = []
        registers = []
        memory = None
        if opcode in ("lfs", "stfs"):
            memory_match = re.fullmatch(r"f(\d+),(-?\d+)\(r(\d+)\)", operands)
            if not memory_match:
                raise UnsupportedGraph(f"unsupported operands: {opcode} {operands}")
            register, offset, base = map(int, memory_match.groups())
            memory = {"base": base, "offset": offset, "epoch": epoch}
            registers = [register]
            if last_store is not None:
                extra_deps.append(last_store)
            if opcode == "lfs":
                signature = (opcode, base, offset, epoch)
                pending_loads.append(len(nodes))
            else:
                reads = [values[register]]
                signature = (opcode, base, offset, epoch, *reads)
                extra_deps.extend(pending_loads)
                pending_loads = []
                last_store = len(nodes)
                epoch += 1
        elif opcode in ARITHMETIC:
            if not re.fullmatch(r"f\d+(?:,f\d+)+", operands):
                raise UnsupportedGraph(f"unsupported operands: {opcode} {operands}")
            registers = list(map(int, re.findall(r"\d+", operands)))
            if len(registers) != ARITHMETIC[opcode]:
                raise UnsupportedGraph(f"wrong operand count: {opcode} {operands}")
            register, *sources = registers
            reads = [values[source] for source in sources]
            signature = (opcode, *reads)
        else:
            raise UnsupportedGraph(f"unsupported instruction: {opcode} {operands}")
        if any(register > 31 for register in registers):
            raise UnsupportedGraph("FPR outside architectural register file")
        identity = (*signature, occurrences[signature])
        occurrences[signature] += 1
        nodes.append({
            "identity": identity,
            "opcode": opcode,
            "reads": [definitions[value] for value in reads if value in definitions],
            "parameters": [value[1] for value in reads if value[0] == "parameter"],
            "extra_deps": sorted(set(extra_deps)),
            "registers": registers,
            "memory": memory,
            "assembly": f"{opcode} {operands}",
        })
        if opcode != "stfs":
            values[register] = identity
            definitions[identity] = len(nodes) - 1
    if not returned or not nodes:
        raise UnsupportedGraph("expected a nonempty straight-line body ending in blr")
    return nodes


def compare_graphs(reference: list[dict], candidate: list[dict]) -> dict:
    reference_keys = [node["identity"] for node in reference]
    candidate_keys = [node["identity"] for node in candidate]
    if Counter(reference_keys) != Counter(candidate_keys):
        return {"graph_equal": False, "reference_operations": len(reference),
                "candidate_operations": len(candidate)}
    candidate_indices = {identity: index for index, identity in enumerate(candidate_keys)}
    expected = [candidate_indices[identity] for identity in reference_keys]
    positioned = sum(actual == wanted for actual, wanted in enumerate(expected))
    return {"graph_equal": True, "operations": len(candidate),
            "same_positions": positioned, "reference_order": expected}


def read_graph(objdump: str, path: Path, function: str) -> list[dict]:
    result = subprocess.run([objdump, "-d", "--disassemble=" + function, str(path)],
                            text=True, capture_output=True, check=True, timeout=30)
    return parse_graph(result.stdout)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--function", action="append", required=True)
    parser.add_argument("--objdump", default="powerpc-eabi-objdump")
    parser.add_argument("--output", type=Path, help="write graphs and measured reference orders as JSON")
    args = parser.parse_args()
    report = {"diagnostic": "floating snapshot graph; not object parity",
              "objects": {role: {"file": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                          for role, path in [("reference", args.reference), ("candidate", args.candidate)]},
              "functions": []}
    failed = False
    for function in args.function:
        record = {"function": function}
        try:
            reference = read_graph(args.objdump, args.reference, function)
            candidate = read_graph(args.objdump, args.candidate, function)
            comparison = compare_graphs(reference, candidate)
            record.update(comparison)
            for role, graph in [("reference", reference), ("candidate", candidate)]:
                record[role] = [{key: value for key, value in node.items() if key != "identity"}
                                for node in graph]
            if comparison["graph_equal"]:
                print(f"{function}: same graph; {comparison['same_positions']}/{comparison['operations']} operations at reference positions")
            else:
                failed = True
                print(f"{function}: different operation graphs")
        except (UnsupportedGraph, subprocess.SubprocessError) as error:
            failed = True
            record["unsupported"] = str(error)
            print(f"{function}: unmeasured: {error}")
        report["functions"].append(record)
    if args.output:
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
