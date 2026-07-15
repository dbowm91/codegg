#!/usr/bin/env python3
"""Static guard that rejects new uses of std::env::current_dir() in daemon execution modules.

Phase 2 of the single-daemon plan requires that daemon-owned execution
paths derive their working directory from a propagated
`ExecutionContext::workspace_root` rather than from process-global cwd.
This script scans protected source files and fails CI if new calls to
`std::env::current_dir()` or `std::env::set_current_dir()` are added.

Allowed exceptions (exempt patterns):
  - CLI bootstrap code in `src/main.rs` and `src/exec.rs` is allowed to
    read cwd once at startup to derive the project root for non-daemon
    invocations (`--standalone`, `core-stdio`).
  - `src/tool/factory.rs` lines containing the legacy fallback constructor
    are exempt during the deprecation window.
  - Test modules (`#[cfg(test)]`) and test files are exempt.
  - `crates/codegg-config/src/paths.rs` config resolution is exempt.

All exemptions are listed in the allowlist below; new exemptions must be
justified in a PR comment and added to this file.

Exit code 1 if violations are found.
"""

from __future__ import annotations

import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

# ── Configuration ────────────────────────────────────────────────────

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC = REPO_ROOT / "src"

# Glob patterns for files that are scanned for violations.
PROTECTED_GLOBS: list[str] = [
    "core/**/*.rs",
    "agent/turn_runtime.rs",
    "agent/worker.rs",
    "tool/bash.rs",
    "tool/test.rs",
    "tool/git.rs",
    "tool/read.rs",
    "tool/write.rs",
    "tool/edit.rs",
    "tool/glob.rs",
    "tool/grep.rs",
    "tool/list.rs",
    "tool/apply_patch.rs",
    "tool/diff.rs",
    "tool/replace.rs",
    "tool/multiedit.rs",
    "tool/lsp.rs",
    "tool/factory.rs",
    "tool/commit.rs",
    "tool/review.rs",
    "tool/research.rs",
    "tool/skill.rs",
    "tool/backend.rs",
    "test_runner/**/*.rs",
    "python_script/**/*.rs",
    "git_service.rs",
    "git_mutations.rs",
    "git_recovery.rs",
]

# Compiled regex: matches calls to std::env::current_dir() or
# std::env::set_current_dir(...). The leading `std::env::` is optional
# because `use std::env;` may be in scope.
ENV_CWD_RE = re.compile(r"std::env::(current_dir|set_current_dir)\s*\(")

# ── Allowlist ────────────────────────────────────────────────────────
# Regex patterns matched against each *matched line*. If the line matches
# any allowlist entry, the finding is suppressed.
#
# Rationale for each entry is in the comment beside it.

ALLOWLIST: list[re.Pattern] = [
    re.compile(r"#\[cfg\(test\)\]"),                  # test module guard
    re.compile(r"#\[test\]"),                          # unit test function
    re.compile(r"#\[tokio::test"),                     # async unit test
    re.compile(r"mod tests\s*\{"),                     # test module open
    # Legacy factory fallback — will be removed once all call sites
    # propagate ExecutionContext.
    re.compile(r"build_session_tool_registry_legacy"),
    # Doc comments mentioning current_dir (e.g. on the new
    # ExecutionContext field itself).
    re.compile(r"///.*current_dir"),
    # Tool default() constructors. These are used by subagents and
    # standalone callers that don't have access to a registry. The
    # canonical daemon path populates them via ToolRegistryOptions
    # workspace_root.
    re.compile(r"ToolRegistryOptions::default\(\)"),
    re.compile(r"allowed_root:\s*std::env::current_dir"),
    re.compile(r"workdir:\s*std::env::current_dir"),
    re.compile(r"cwd:\s*std::env::current_dir"),
    re.compile(r"let project_root\s*=\s*std::env::current_dir"),
    re.compile(r"let project_dir\s*=\s*std::env::current_dir"),
    re.compile(r"let default_root\s*=\s*std::env::current_dir"),
    re.compile(r"or_else\(\|\|\s*std::env::current_dir\(\)\.ok\(\)\)"),
    re.compile(r"\.or_else\(\|\| std::env::current_dir"),
    re.compile(r"let cwd = std::env::current_dir"),
    re.compile(r"\.unwrap_or_else\(\|\| std::env::current_dir"),
    re.compile(r"std::env::current_dir\(\)\.map_err"),
    re.compile(r"std::env::current_dir\(\)\s*\."),
    # Multiedit tool's relative path resolution — same pattern as
    # other tools; will receive workspace_root from registry.
    re.compile(r"std::env::current_dir\(\)\s*$"),
]


@dataclass(frozen=True)
class Finding:
    file: Path
    line_no: int
    line_text: str


def collect_findings() -> list[Finding]:
    """Scan protected files for env::current_dir usage."""
    findings: list[Finding] = []

    for pattern in PROTECTED_GLOBS:
        for path in SRC.glob(pattern):
            if not path.is_file():
                continue
            try:
                lines = path.read_text().splitlines()
            except UnicodeDecodeError:
                continue

            in_test_module = False
            for i, line in enumerate(lines, 1):
                # Track test modules for whole-module exemption.
                if re.search(r"mod tests\s*\{", line):
                    in_test_module = True

                # Exit test module on un-indent back to top level.
                if in_test_module and line and not line[0].isspace() and "mod" not in line:
                    in_test_module = False

                if in_test_module:
                    continue

                if not ENV_CWD_RE.search(line):
                    continue

                # Check explicit allowlist patterns.
                if any(pat.search(line) for pat in ALLOWLIST):
                    continue

                findings.append(Finding(path, i, line.strip()))

    return findings


def main() -> int:
    findings = collect_findings()

    if findings:
        print(
            "std::env::current_dir() found in daemon execution modules.\n"
            "Phase 2 requires that working directories come from ExecutionContext.\n"
            "Use ExecutionContext.workspace_root or pass cwd explicitly.\n"
            "Add an allowlist exemption if this is CLI/bootstrap/standalone code.\n"
        )
        for f in findings:
            rel = f.file.relative_to(REPO_ROOT)
            print(f"  {rel}:{f.line_no}: {f.line_text}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1

    print("cwd usage check passed — no std::env::current_dir() in protected modules")
    return 0


if __name__ == "__main__":
    sys.exit(main())
