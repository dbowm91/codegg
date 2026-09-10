#!/usr/bin/env python3
"""Static guard that rejects reintroduction of path/current-focus
authority into the multi-project TUI frontend.

Corrective milestone M005 of the Multi-Project TUI roadmap establishes the
project catalog and the routing registry as the authoritative
identity surface. The legacy single-project TUI read `project_dir`
as a project authority in several places; new code must not
re-introduce that pattern.

This script scans the TUI execution surface for direct ambient project
authority. Project-scoped operations must resolve the active tab's explicit
execution context before dispatch; process cwd is allowed only at a clearly
marked bootstrap boundary and test fixtures are allowed only in test code.

Exit code 1 if violations are found.

Usage::

    python3 scripts/check_tui_project_authority.py

This is invoked by `scripts/verify.sh quick` and the CI `verify` job.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC = REPO_ROOT / "src"

# Glob patterns for files that are scanned for violations.
PROTECTED_GLOBS: list[str] = [
    "tui/app/state/**/*.rs",
    "tui/app/mod.rs",
    "tui/commands/**/*.rs",
    "tui/runtime/**/*.rs",
    "tui/command.rs",
    "tui/components/dialogs/command.rs",
]

# Patterns that indicate a session/project identity read. Each
# pattern is matched against individual lines; a line containing
# any of these is a candidate finding.
#
# Only the strongest "this looks like project identity" patterns
# are checked here; compat-mode reads for rendering (e.g.,
# `session_state.session.is_some()`) are explicitly allowlisted
# because the legacy surface continues to drive rendering.
PATTERNS: list[re.Pattern] = [
    # Any direct legacy mirror read can select the wrong project after a tab
    # switch, including reads that do not look like an identity comparison.
    re.compile(r"\bsession_state\.project_dir\b"),
    re.compile(r"std::env::current_dir\(\)"),
]

# Allowlist of lines that legitimately use these patterns. Each
# entry is a regex matched against the source line; matches are
# suppressed. New allowlist entries must be added with a comment
# explaining the legitimate compat-mode usage.
ALLOWLIST: list[re.Pattern] = [
    # Doc comments are excluded.
    re.compile(r"^\s*///"),
    re.compile(r"^\s*//\s*!"),
    # Test modules and helper assertions.
    re.compile(r"//\s*test"),
    re.compile(r"#\[test"),
    re.compile(r"#\[cfg\(test"),
    re.compile(r"mod tests"),
    # One-time bootstrap locators must be marked at the exact source line.
    re.compile(r"//\s*bootstrap\b"),
    # Test-only fixtures may use cwd when constructing isolated test data.
    re.compile(r"//\s*test-fixture\b"),
]


@dataclass(frozen=True)
class Finding:
    file: Path
    line_no: int
    line_text: str
    pattern: str


def collect_findings() -> list[Finding]:
    findings: list[Finding] = []
    for pattern in PROTECTED_GLOBS:
        for path in SRC.glob(pattern):
            if not path.is_file():
                continue
            try:
                lines = path.read_text().splitlines()
            except UnicodeDecodeError:
                continue
            for i, line in enumerate(lines, 1):
                matched = None
                for pat in PATTERNS:
                    if pat.search(line):
                        matched = pat.pattern
                        break
                if matched is None:
                    continue
                if any(allow.search(line) for allow in ALLOWLIST):
                    continue
                findings.append(Finding(path, i, line.strip(), matched))
    return findings


def main() -> int:
    findings = collect_findings()
    if findings:
        print(
            "Path/current-focus TUI authority patterns found in protected modules.\n"
            "Milestone 4 requires that project identity comes from the routing\n"
            "registry and project catalog, not from session_state or process cwd.\n"
            "Add an allowlist exemption if this is a documented compat-mode usage.\n"
        )
        for f in findings:
            rel = f.file.relative_to(REPO_ROOT)
            print(f"  {rel}:{f.line_no} [{f.pattern}] {f.line_text}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1
    print("TUI project-authority guard passed — no path/current-focus reads in protected modules")
    return 0


if __name__ == "__main__":
    sys.exit(main())
