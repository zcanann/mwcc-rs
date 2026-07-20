#!/usr/bin/env python3
"""Inventory the authoritative MWCC configuration of reference projects.

The decomp projects already describe every object with its source path, MWCC
version, and exact flag vector in ``configure.py``.  Generating build.ninja is
not a useful way to read that information: it requires an extracted game image
and writes generated files into the project.  This tool intercepts the project's
``generate_build`` call instead and serializes the fully resolved object model.

By default every immediate child of the reference-project root is inspected and
a deterministic JSON document is written to stdout.  ``GC/1.3.2r`` is recorded
but excluded from the required translation-unit matrix: it is the Animal
Crossing rodata-pooling workaround build, not a required parity target.  ProDG
rows are also excluded because they target SN Systems' compiler, not MWCC.
"""

from __future__ import annotations

import argparse
import contextlib
import glob
import hashlib
import importlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any, Dict, Iterable, List, Optional, Sequence, Tuple

from parity_identity import configuration_id


SCHEMA_VERSION = 5
SOURCE_SUFFIXES = {".c", ".cc", ".cp", ".cpp", ".cxx"}
EXCLUDED_MW_VERSIONS = {"GC/1.3.2r", "ProDG/3.5"}


def source_metadata(path: Path) -> Dict[str, Any]:
    if not path.is_file():
        return {
            "source_sha256": None,
            "source_size_bytes": None,
            "source_has_non_whitespace": None,
        }
    data = path.read_bytes()
    return {
        "source_sha256": hashlib.sha256(data).hexdigest(),
        "source_size_bytes": len(data),
        "source_has_non_whitespace": bool(data.strip()),
    }


def source_files(project: Path) -> Iterable[Path]:
    for path in project.rglob("*"):
        if not path.is_file() or path.suffix.lower() not in SOURCE_SUFFIXES:
            continue
        if any(part in {"build", "orig", ".git"} for part in path.parts):
            continue
        yield path


def configuration_variants(project: Path) -> List[Optional[str]]:
    configure = project / "configure.py"
    if configure.is_file() and '"-v"' not in configure.read_text(encoding="utf-8"):
        # Some projects describe every regional build in one configuration and
        # have no version selector (OoT GC is the current example).
        return [None]
    config_root = project / "config"
    variants = sorted(
        path.parent.name
        for path in config_root.glob("*/config.yml")
        if path.is_file()
    )
    return variants or [None]


def jsonable(value: Any) -> Any:
    if isinstance(value, Path):
        return value.as_posix()
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    if isinstance(value, (list, tuple)):
        return [jsonable(item) for item in value]
    return str(value)


def transpile_literal_matches(source: str) -> str:
    """Backport the projects' simple literal ``match`` blocks to Python 3.9.

    The reference projects require no structural matching here: their configure
    scripts only dispatch on strings, ``None``, literal alternatives, and the
    wildcard.  Supporting that narrow grammar avoids imposing a new host Python
    installation merely to inspect build metadata.
    """

    lines = source.splitlines(keepends=True)
    output: List[str] = []
    active: Optional[Tuple[int, int, str, int]] = None
    match_index = 0
    for line in lines:
        stripped = line.lstrip(" ")
        indent = len(line) - len(stripped)
        if active is not None:
            match_indent, case_indent, variable, case_count = active
            if stripped.strip() and indent <= match_indent:
                active = None
            elif indent == case_indent:
                case = re.fullmatch(r"case\s+(.+):\s*", stripped.rstrip("\n"))
                if case is not None:
                    pattern = case.group(1).strip()
                    prefix = "if" if case_count == 0 else "elif"
                    if pattern == "_":
                        output.append(" " * match_indent + "else:\n")
                    elif pattern == "None":
                        output.append(
                            " " * match_indent + f"{prefix} {variable} is None:\n"
                        )
                    else:
                        alternatives = [part.strip() for part in pattern.split("|")]
                        if len(alternatives) == 1:
                            condition = f"{variable} == {alternatives[0]}"
                        else:
                            condition = f"{variable} in ({', '.join(alternatives)},)"
                        output.append(" " * match_indent + f"{prefix} {condition}:\n")
                    active = (match_indent, case_indent, variable, case_count + 1)
                    continue
            if active is not None:
                # The case suite was nested one level below ``match``. It now
                # belongs directly to the synthesized if/elif suite.
                if stripped.strip():
                    output.append(" " * (indent - 4) + stripped)
                else:
                    output.append(line)
                continue

        match = re.fullmatch(r"( *)match\s+(.+):\s*", line.rstrip("\n"))
        if match is not None:
            indentation, expression = match.groups()
            variable = f"__mwcc_inventory_match_{match_index}"
            match_index += 1
            output.append(f"{indentation}{variable} = {expression}\n")
            active = (len(indentation), len(indentation) + 4, variable, 0)
            continue
        output.append(line)
    return "".join(output)


def capture_project(project: Path, variant: Optional[str]) -> List[Dict[str, Any]]:
    """Execute one configure.py while replacing its build writer with a capture."""

    project = project.resolve()
    configure = project / "configure.py"
    if not configure.is_file():
        raise FileNotFoundError(f"no configure.py in {project}")

    old_cwd = Path.cwd()
    old_argv = sys.argv[:]
    old_path = sys.path[:]
    captured: List[Dict[str, Any]] = []

    def collect(config: Any) -> None:
        config_versions = getattr(config, "versions", None)
        resolved_sets = (
            [(version, config.objects(version)) for version in config_versions]
            if config_versions is not None
            else [(str(config.version), config.objects())]
        )
        for config_version, objects in resolved_sets:
            for obj in objects.values():
                source = Path(obj.src_path)
                if source.suffix.lower() not in SOURCE_SUFFIXES:
                    continue
                metadata = source_metadata(source)
                options = obj.options
                completed = getattr(obj, "completed", False)
                matching = (
                    bool(completed(config, config_version))
                    if callable(completed)
                    else bool(completed)
                )
                captured.append(
                    {
                        "project": project.name,
                        "variant": str(config_version),
                        "source": source.as_posix(),
                        **metadata,
                        "language": "c++" if source.suffix.lower() != ".c" else "c",
                        "mw_version": options["mw_version"],
                        "cflags": jsonable(options.get("cflags") or []),
                        "extra_cflags": jsonable(options.get("extra_cflags") or []),
                        "library": options.get("lib"),
                        "shift_jis": bool(options.get("shift_jis")),
                        "extab_padding": jsonable(options.get("extab_padding")),
                        "matching": matching,
                        "source_exists": source.is_file(),
                    }
                )

    try:
        os.chdir(project)
        sys.path.insert(0, str(project))
        project_module = importlib.import_module("tools.project")
        project_module.generate_build = collect
        project_module.calculate_progress = collect
        sys.argv = [str(configure), "configure"]
        if variant is not None:
            sys.argv.extend(["-v", variant])
        source = transpile_literal_matches(configure.read_text(encoding="utf-8"))
        namespace = {
            "__name__": "__main__",
            "__file__": str(configure),
            "__package__": None,
            "__cached__": None,
        }
        # Configure scripts occasionally print disabled-version notices. Keep
        # the capture protocol's stdout as one unambiguous JSON document.
        original_glob = glob.glob
        if project.name == "ocarina_of_time_gc_port":
            # This project's configure script selects variants by checking for
            # extracted disc images. Metadata inventory is independent of those
            # binaries, so expose every declared config without creating fake files.
            def inventory_glob(pattern: str, *args: Any, **kwargs: Any) -> List[str]:
                normalized = pattern.replace("\\", "/")
                if normalized.startswith("orig/") and normalized.endswith("/*"):
                    return [normalized[:-1] + "inventory-placeholder"]
                return original_glob(pattern, *args, **kwargs)

            glob.glob = inventory_glob
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                exec(compile(source, str(configure), "exec"), namespace)
        finally:
            glob.glob = original_glob
    finally:
        os.chdir(old_cwd)
        sys.argv = old_argv
        sys.path[:] = old_path
    return captured


def capture_subprocess(
    script: Path, project: Path, variant: Optional[str], python: str
) -> Tuple[List[Dict[str, Any]], Optional[str]]:
    command = [python, str(script), "--capture", str(project)]
    if variant is not None:
        command.extend(["--variant", variant])
    result = subprocess.run(command, text=True, capture_output=True)
    if result.returncode != 0:
        detail = result.stderr.strip().splitlines()
        return [], detail[-1] if detail else f"capture exited {result.returncode}"
    try:
        return json.loads(result.stdout), None
    except json.JSONDecodeError as error:
        return [], f"capture returned invalid JSON: {error}"


def row_key(row: Dict[str, Any]) -> Tuple[Any, ...]:
    return (
        row["project"],
        row["source"],
        row.get("source_sha256"),
        row["language"],
        row["mw_version"],
        tuple(row["cflags"]),
        tuple(row["extra_cflags"]),
        row["shift_jis"],
        json.dumps(row["extab_padding"], sort_keys=True),
    )


def build_inventory(root: Path, python: str) -> Dict[str, Any]:
    script = Path(__file__).resolve()
    projects: List[Dict[str, Any]] = []
    unique_rows: Dict[Tuple[Any, ...], Dict[str, Any]] = {}
    excluded_rows = 0

    for project in sorted(path for path in root.iterdir() if path.is_dir()):
        all_sources = sorted(
            path.relative_to(project).as_posix() for path in source_files(project)
        )
        configure = project / "configure.py"
        if not configure.is_file():
            projects.append(
                {
                    "name": project.name,
                    "status": "no_mwcc_configure",
                    "source_count": len(all_sources),
                    "mapped_source_count": 0,
                    "unmapped_sources": all_sources,
                    "variants": [],
                    "errors": [],
                }
            )
            continue

        variants = configuration_variants(project)
        errors: List[Dict[str, str]] = []
        project_rows: List[Dict[str, Any]] = []
        for variant in variants:
            rows, error = capture_subprocess(script, project, variant, python)
            if error is not None:
                errors.append({"variant": variant or "<default>", "error": error})
            else:
                project_rows.extend(rows)

        mapped = {
            row["source"]
            for row in project_rows
            if row["source_exists"]
        }
        for row in project_rows:
            if row["mw_version"] in EXCLUDED_MW_VERSIONS:
                excluded_rows += 1
                continue
            row["configuration_id"] = configuration_id(row)
            unique_rows.setdefault(row_key(row), row)

        projects.append(
            {
                "name": project.name,
                "status": "ok" if not errors else "capture_error",
                "source_count": len(all_sources),
                "mapped_source_count": len(mapped),
                "unmapped_sources": sorted(set(all_sources) - mapped),
                "variants": [variant or "<default>" for variant in variants],
                "errors": errors,
            }
        )

    rows = sorted(unique_rows.values(), key=row_key)
    version_counts: Dict[str, int] = {}
    language_counts: Dict[str, int] = {}
    for row in rows:
        version_counts[row["mw_version"]] = version_counts.get(row["mw_version"], 0) + 1
        language_counts[row["language"]] = language_counts.get(row["language"], 0) + 1
    return {
        "schema_version": SCHEMA_VERSION,
        "reference_root": str(root.resolve()),
        "excluded_mw_versions": sorted(EXCLUDED_MW_VERSIONS),
        "excluded_row_count": excluded_rows,
        "projects": projects,
        "translation_unit_count": len(rows),
        "version_counts": dict(sorted(version_counts.items())),
        "language_counts": dict(sorted(language_counts.items())),
        "translation_units": rows,
    }


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root",
        nargs="?",
        type=Path,
        default=Path(__file__).resolve().parents[2] / "Metrowerks" / "reference_projects",
        help="reference_projects directory",
    )
    parser.add_argument(
        "--python",
        default=sys.executable,
        help="Python interpreter used for project configure scripts",
    )
    parser.add_argument("--capture", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--variant", help=argparse.SUPPRESS)
    return parser.parse_args(argv)


def main() -> int:
    args = parse_args()
    if args.capture is not None:
        rows = capture_project(args.capture, args.variant)
        json.dump(rows, sys.stdout, sort_keys=True)
        sys.stdout.write("\n")
        return 0
    if not args.root.is_dir():
        print(f"reference project root not found: {args.root}", file=sys.stderr)
        return 2
    inventory = build_inventory(args.root, args.python)
    json.dump(inventory, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 1 if any(project["errors"] for project in inventory["projects"]) else 0


if __name__ == "__main__":
    raise SystemExit(main())
