#!/usr/bin/env python3
"""Reject direct YAML parser usage outside the compatibility codec.

The production YAML dependency is intentionally owned by
``crates/codegg-config/src/document.rs``.  Consumers use the typed,
format-neutral ``codegg_config::parse_yaml`` boundary instead.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CODEC = ROOT / "crates" / "codegg-config" / "src" / "document.rs"
PARSER_RE = re.compile(r"\bserde_(?:yaml|norway)\b|serde-saphyr")


def rust_files() -> list[Path]:
    paths: list[Path] = []
    for root in (ROOT / "src", ROOT / "crates", ROOT / "tests"):
        if not root.exists():
            continue
        paths.extend(path for path in root.rglob("*.rs") if "target" not in path.parts)
    return sorted(paths)


def findings_for(path: Path, lines: list[str]) -> list[str]:
    findings: list[str] = []
    for line_no, line in enumerate(lines, start=1):
        if PARSER_RE.search(line):
            findings.append(f"{path.relative_to(ROOT)}:{line_no}: {line.strip()}")
    return findings


def self_test() -> int:
    injected = ROOT / "src" / "__yaml_parser_boundary_self_test__.rs"
    findings = findings_for(injected, ["use serde_yaml::Value;"])
    if not findings:
        print("yaml parser boundary guard self-test failed: injected violation was not found")
        return 1
    print("yaml parser boundary guard self-test passed")
    return 0


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "--self-test":
        return self_test()
    if len(sys.argv) > 1:
        print("usage: check_yaml_parser_boundary.py [--self-test]", file=sys.stderr)
        return 2

    findings: list[str] = []
    for path in rust_files():
        if path == CODEC or "tests" in path.relative_to(ROOT).parts:
            continue
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except (OSError, UnicodeError) as error:
            print(f"yaml parser boundary guard could not read {path}: {error}", file=sys.stderr)
            return 2
        findings.extend(findings_for(path, lines))

    if findings:
        print("yaml parser boundary guard failed:")
        print("\n".join(f"  {finding}" for finding in findings))
        return 1

    print("yaml parser boundary guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
