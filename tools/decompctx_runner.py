#!/usr/bin/env python3
"""Run a reference project's decompctx with cross-platform include paths.

Several projects retain Windows-style backslashes in C/C++ include directives.
Their bundled decompctx scripts pass those strings directly to POSIX path APIs,
silently dropping headers that do exist. This adapter preserves each project's
own context generator and only normalizes the include name at its lookup seam.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
from types import ModuleType
from typing import Callable


def load_script(path: Path) -> ModuleType:
    spec = importlib.util.spec_from_file_location("mwcc_reference_decompctx", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def normalize_lookup(module: ModuleType) -> None:
    original: Callable[..., str] = module.import_h_file

    def import_h_file(include: str, *args: object, **kwargs: object) -> str:
        normalized = include.replace("\\", "/")
        if normalized.endswith(".mch") and hasattr(module, "defines"):
            # Generated MWCC precompiled headers are absent in a clean checkout,
            # while their same-path textual `.pch` sources are present. Context
            # generators do not evaluate `#if`, so they visit both the `.mch`
            # and `.pch` arms and globally remember include guards from the first
            # arm. Expand the textual source for the `.mch` arm, but roll back
            # that speculative guard state so the mutually exclusive `.pch` arm
            # can independently expand too; the real preprocessor will select
            # exactly one populated arm afterward.
            saved_defines = set(module.defines)
            module.defines.clear()
            rendered = original(f"{normalized[:-4]}.pch", *args, **kwargs)
            module.defines.clear()
            module.defines.update(saved_defines)
            if rendered:
                return rendered
        return original(normalized, *args, **kwargs)

    module.import_h_file = import_h_file


def adapt_arguments(module: ModuleType, arguments: list[str]) -> list[str]:
    """Feed include roots to older scripts whose CLI predates ``-I``."""
    include_dirs = getattr(module, "include_dirs", None)
    if not include_dirs:
        return arguments
    adapted: list[str] = []
    index = 0
    while index < len(arguments):
        argument = arguments[index]
        if argument == "-I" and index + 1 < len(arguments):
            include_dirs.append(str((Path.cwd() / arguments[index + 1]).resolve()))
            index += 2
            continue
        if argument.startswith("-I") and len(argument) > 2:
            include_dirs.append(str((Path.cwd() / argument[2:]).resolve()))
            index += 1
            continue
        adapted.append(argument)
        index += 1
    return adapted


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: decompctx_runner.py <project decompctx.py> <decompctx arguments...>")
        return 2
    script = Path(sys.argv[1]).resolve()
    arguments = sys.argv[2:]
    module = load_script(script)
    if not hasattr(module, "import_h_file") or not hasattr(module, "main"):
        print(f"unsupported decompctx interface: {script}")
        return 2
    normalize_lookup(module)
    arguments = adapt_arguments(module, arguments)
    sys.argv = [str(script), *arguments]
    module.main()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
