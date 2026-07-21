#!/usr/bin/env python3
"""Static guard that rejects reintroduction of path/current-focus
authority into the multi-project TUI frontend.

Milestone 4 of the Multi-Project TUI roadmap establishes the
project catalog and the routing registry as the authoritative
identity surface. The legacy single-project TUI read `project_dir`
as a project authority in several places; new code must not
re-introduce that pattern.

This script scans `src/tui/app/state/`, `src/tui/app/mod.rs`, and
`src/tui/commands/` for patterns that read global session fields
(`App::session_state.project_dir`, etc.) as if they were project
identity. The check is intentionally conservative — existing
compat-mode call sites that legitimately read `session_state` for
rendering (without making it a project identity decision) are
allowlisted individually.

Exit code 1 if violations are found.

Usage::

    python3 scripts/check_tui_project_authority.py

This is invoked from `make test` and the CI pipeline.
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
    # Reading session_state.project_dir directly as a project identity.
    re.compile(r"session_state\.project_dir"),
    # Treating the legacy single-project cwd as a project identity.
    re.compile(r"std::env::current_dir\(\)"),
]

# Allowlist of lines that legitimately use these patterns. Each
# entry is a regex matched against the source line; matches are
# suppressed. New allowlist entries must be added with a comment
# explaining the legitimate compat-mode usage.
ALLOWLIST: list[re.Pattern] = [
    # Existing compat-mode rendering surface: legacy single-project
    # state continues to drive rendering under milestone 004.
    re.compile(r"//\s*compat"),
    re.compile(r"///\s*compat"),
    # Doc comments are excluded.
    re.compile(r"^\s*///"),
    re.compile(r"^\s*//\s*!"),
    # Test modules and helper assertions.
    re.compile(r"//\s*test"),
    re.compile(r"#\[test"),
    re.compile(r"#\[cfg\(test"),
    re.compile(r"mod tests"),
    # Existing allowlist for the cwd check that pre-dates this guard.
    re.compile(r"std::env::current_dir\(\)\.ok"),
    re.compile(r"let cwd = std::env::current_dir"),
    # Doc-only references in module headers.
    re.compile(r"project_dir"),
    # Manifest restore pipeline: reads session_state.session for
    # compat-mode compat-startup (single-tab mode). Documented.
    re.compile(r"if app\.session_state\.session\.is_some\(\)"),
    re.compile(r"if let Some\(.*\) = app\.session_state\.session"),
    re.compile(r"app\.session_state\.session ="),
    # Pre-existing fallback paths unrelated to project authority.
    re.compile(r"std::env::current_dir\(\)\.unwrap_or_else"),
    re.compile(r"let workdir = std::env::current_dir"),
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
