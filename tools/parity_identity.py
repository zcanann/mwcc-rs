"""Stable identities shared by the reference parity tools."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any, Dict, Iterable


def configuration_payload(row: Dict[str, Any]) -> Dict[str, Any]:
    """Return only inputs that can change the compiler result.

    Project variants intentionally collapse when they resolve to the same
    source, build, and flags.  Compiler and harness hashes do not belong here:
    this identity must survive implementation changes so a failing frontier can
    be followed until it passes.
    """

    return {
        "project": row["project"],
        "source": row["source"],
        "source_sha256": row.get("source_sha256"),
        "language": row["language"],
        "mw_version": row["mw_version"],
        "cflags": row.get("cflags") or [],
        "extra_cflags": row.get("extra_cflags") or [],
        "shift_jis": bool(row.get("shift_jis")),
        "extab_padding": row.get("extab_padding"),
    }


def configuration_id(row: Dict[str, Any]) -> str:
    encoded = json.dumps(
        configuration_payload(row), sort_keys=True, separators=(",", ":")
    ).encode()
    return hashlib.sha256(encoded).hexdigest()


def observation_id(config_id: str, tool_fingerprint: str) -> str:
    encoded = f"{config_id}:{tool_fingerprint}".encode()
    return hashlib.sha256(encoded).hexdigest()


def files_fingerprint(paths: Iterable[Path]) -> str:
    """Hash named measurement inputs, including boundaries between files."""

    digest = hashlib.sha256()
    for path in paths:
        digest.update(path.name.encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()
