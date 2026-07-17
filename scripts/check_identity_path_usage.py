#!/usr/bin/env python3
"""Narrow, opt-in guard seam for path-derived ProjectId construction.

This script is deliberately not wired into CI in the typed-identity
foundation milestone. It provides a focused check that later project-storage
work can extend without making this milestone's compatibility fields fail a
repository-wide heuristic scan.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
IDENTITY_MODULE = ROOT / "crates/codegg-core/src/identity.rs"
SOURCE_ROOT = ROOT / "crates/codegg-core/src"
MARKER = "identity-path-guard: project IDs must not be derived from paths"
FORBIDDEN_PATTERNS = (
    re.compile(r"ProjectId::new_unchecked\b"),
    re.compile(r"ProjectId::from_path\b"),
    re.compile(r"ProjectId::parse\(\s*&?(?:path|directory|canonical_root)\b"),
)


def main() -> int:
    if MARKER not in IDENTITY_MODULE.read_text(encoding="utf-8"):
        print("identity path guard marker is missing", file=sys.stderr)
        return 1

    violations: list[str] = []
    for source in SOURCE_ROOT.rglob("*.rs"):
        if source == IDENTITY_MODULE:
            continue
        text = source.read_text(encoding="utf-8")
        for pattern in FORBIDDEN_PATTERNS:
            if pattern.search(text):
                violations.append(f"{source.relative_to(ROOT)}: {pattern.pattern}")

    if violations:
        print("path-derived ProjectId construction detected:", file=sys.stderr)
        print("\n".join(violations), file=sys.stderr)
        return 1

    print("identity path guard seam: no forbidden ProjectId construction found")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
