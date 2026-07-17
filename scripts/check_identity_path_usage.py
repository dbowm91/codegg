#!/usr/bin/env python3
"""Guard against path-derived canonical ProjectId construction.

The canonical project-storage module is included in the scan. ``--fixture``
lets closure tests prove that an intentionally invalid path-derived identity
is rejected without putting that invalid code in the production source tree.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
IDENTITY_MODULE = ROOT / "crates/codegg-core/src/identity.rs"
SOURCE_ROOT = ROOT / "crates/codegg-core/src"
AUTHORITATIVE_MODULES = {ROOT / "crates/codegg-core/src/project_storage.rs"}
MARKER = "identity-path-guard: project IDs must not be derived from paths"
FORBIDDEN_PATTERNS = (
    re.compile(r"ProjectId::new_unchecked\b"),
    re.compile(r"ProjectId::from_path\b"),
    re.compile(r"ProjectId::parse\(\s*&?(?:path|directory|canonical_root)\b"),
    re.compile(r"ProjectId::parse\(\s*&?\w+\.(?:to_string_lossy|display)\b"),
)


def find_violations(text: str) -> list[str]:
    return [pattern.pattern for pattern in FORBIDDEN_PATTERNS if pattern.search(text)]


def main() -> int:
    if MARKER not in IDENTITY_MODULE.read_text(encoding="utf-8"):
        print("identity path guard marker is missing", file=sys.stderr)
        return 1

    if len(sys.argv) == 3 and sys.argv[1] == "--fixture":
        fixture = Path(sys.argv[2]).resolve()
        if not fixture.exists():
            print(f"fixture does not exist: {fixture}", file=sys.stderr)
            return 2
        violations = [
            f"{fixture.relative_to(ROOT) if fixture.is_relative_to(ROOT) else fixture}: {pattern}"
            for pattern in find_violations(fixture.read_text(encoding="utf-8"))
        ]
        if violations:
            print("path-derived ProjectId construction detected:", file=sys.stderr)
            print("\n".join(violations), file=sys.stderr)
            return 1
        print("fixture passed identity path guard")
        return 0

    violations: list[str] = []
    for source in SOURCE_ROOT.rglob("*.rs"):
        if source == IDENTITY_MODULE:
            continue
        text = source.read_text(encoding="utf-8")
        for pattern in find_violations(text):
            violations.append(f"{source.relative_to(ROOT)}: {pattern}")

    for source in AUTHORITATIVE_MODULES:
        if not source.exists():
            violations.append(f"missing authoritative identity module: {source.relative_to(ROOT)}")

    if violations:
        print("path-derived ProjectId construction detected:", file=sys.stderr)
        print("\n".join(violations), file=sys.stderr)
        return 1

    print("identity path guard seam: no forbidden ProjectId construction found")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
