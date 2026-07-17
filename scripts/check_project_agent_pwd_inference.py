#!/usr/bin/env python3
"""Static guard that rejects new PWD-inference in project-agent resolution paths.

Runtime Assets Milestone 2 requires that agents, skills, and project
instructions are resolved from an explicit `AssetContext` rather than
from `std::env::var("PWD")` or `std::env::current_dir()`. This script
scans the agent resolution surface and fails CI if a new caller
introduces PWD-based inference that should have been threaded through
the explicit context.

Allowed exceptions (allowlist below):
  - The deprecated CLI-bootstrap `AgentRegistry::load` constructor at
    `src/agent/registry.rs` keeps a single PWD read for backward
    compatibility. New production code MUST NOT call it.
  - The legacy `resolve_agents()` boundary in `src/agent/mod.rs` reads
    cwd once at the boundary to convert to an `AssetContext`. Internal
    callers must use `resolve_agents_with_context()` instead.
  - Test modules (`#[cfg(test)]`) and test files are exempt.
  - Doc comments mentioning PWD are exempt.

Exit code 1 if violations are found.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

# ── Configuration ────────────────────────────────────────────────────

REPO_ROOT = Path(__file__).resolve().parent.parent
SRC = REPO_ROOT / "src"

# Glob patterns for files that are scanned for violations. The agent
# resolution surface includes:
#   - the agent registry
#   - the asset context / instructions / snapshot modules
#   - the prompt-loader (which used to do its own PWD walk)
#   - the asset-snapshot builder
#   - the project skill loader (used to read PWD indirectly via
#     SkillIndex::load)
PROTECTED_GLOBS: list[str] = [
    "agent/asset_context.rs",
    "agent/asset_snapshot.rs",
    "agent/asset_snapshot_builder.rs",
    "agent/instructions.rs",
    "agent/registry.rs",
    "agent/prompt.rs",
    "agent/mod.rs",
    "tool/skill.rs",
]

# Regex: matches `std::env::var("PWD")` or `env::var("PWD")` style
# reads, plus `std::env::current_dir()` / `env::current_dir()` inference.
# We treat these as equivalent: both are process-global cwd inference.
PWD_RE = re.compile(
    r"""(?x)
    (?:
      std::env::var\s*\(\s*"PWD"\s*\)
      |
      std::env::current_dir\s*\(
      |
      env::current_dir\s*\(
    )
    """
)

# ── Allowlist ────────────────────────────────────────────────────────
# Patterns matched against each *matched line*. If the line matches any
# allowlist entry, the finding is suppressed.
ALLOWLIST: list[re.Pattern] = [
    re.compile(r"#\[cfg\(test\)\]"),                  # test module guard
    re.compile(r"#\[test\]"),                          # unit test function
    re.compile(r"#\[tokio::test"),                     # async unit test
    re.compile(r"mod tests\s*\{"),                     # test module open
    # Doc comments mentioning PWD or current_dir.
    re.compile(r"///.*PWD"),
    re.compile(r"///.*current_dir"),
    re.compile(r"//!.*PWD"),
    re.compile(r"//!.*current_dir"),
    re.compile(r"//.*PWD"),
    # The single, deprecated CLI-bootstrap `AgentRegistry::load`
    # constructor is the only production-path PWD read; new callers
    # must use `load_for_context`.
    re.compile(r"fn load\(config:\s*&Config\)"),
    # Legacy `resolve_agents()` boundary reads cwd once to produce an
    # `AssetContext`; downstream code must use `resolve_agents_with_context`.
    re.compile(r"pub fn resolve_agents\(config:\s*&Config\)"),
    # Legacy single-file `find_instructions_file` helper.
    re.compile(r"pub fn find_instructions_file"),
    # Legacy `find_all_instruction_files` walker; superseded by
    # `ProjectInstructionResolver`. Marked deprecated below; new
    # callers must use `load_agent_prompt_with_context`.
    re.compile(r"pub fn find_all_instruction_files"),
    # CLI bootstrap: skill tool builds an `AssetContext` from cwd
    # exactly once. New code paths should thread the context in via
    # the tool registry instead.
    re.compile(r"with_workspace_root\(cwd\)"),
    re.compile(r"\.with_workspace_root\(\s*std::env::current_dir"),
]


@dataclass(frozen=True)
class Finding:
    file: Path
    line_no: int
    line_text: str


def _in_test_module(lines: list[str]) -> set[int]:
    """Return the set of line numbers (1-indexed) inside test modules."""
    in_test: set[int] = set()
    depth = 0
    test_depth = -1
    for i, line in enumerate(lines, 1):
        if test_depth == -1 and re.search(r"mod tests\s*\{", line):
            test_depth = depth + 1 if depth else 1
            depth = test_depth
            in_test.add(i)
            continue
        if test_depth != -1:
            in_test.add(i)
            # Track brace depth to know when we exit the test module.
            depth += line.count("{") - line.count("}")
            if depth <= test_depth - 1:
                test_depth = -1
                depth = 0
        else:
            depth += line.count("{") - line.count("}")
    return in_test


def _preceding_lines_with_pattern(
    lines: list[str], idx: int, patterns: list[re.Pattern]
) -> bool:
    """Return True if any of `patterns` match a line within the 30
    lines immediately preceding the matched line. Used to recognize
    whole-function boundary annotations (e.g. `pub fn resolve_agents`,
    `#[deprecated(...)]`) that may sit several lines above the PWD read.
    """
    start = max(0, idx - 30)
    for j in range(start, idx):
        if any(pat.search(lines[j]) for pat in patterns):
            return True
    return False


# Patterns that mark a *function or block boundary* in the surrounding
# 30 lines. When one of these is present, the PWD read is the
# intentional CLI/bootstrap boundary read.
BOUNDARY_PATTERNS: list[re.Pattern] = [
    re.compile(r"pub fn resolve_agents\(config:\s*&Config\)"),
    re.compile(r"CLI bootstrap reads cwd exactly once"),
    re.compile(r"with_workspace_root\(cwd\)"),
]


def _preceding_lines_with_deprecated(lines: list[str], idx: int) -> bool:
    """Return True if a `#[deprecated(...)]` attribute appears within
    the 30 lines immediately preceding the matched line. Used to
    recognize whole-function deprecation that may be several lines
    above the PWD read.
    """
    start = max(0, idx - 30)
    return any("#[deprecated" in lines[j] for j in range(start, idx))


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

            test_lines = _in_test_module(lines)
            for i, line in enumerate(lines, 1):
                if i in test_lines:
                    continue
                if not PWD_RE.search(line):
                    continue
                if any(pat.search(line) for pat in ALLOWLIST):
                    continue
                # If the surrounding function is annotated `#[deprecated]`,
                # the read is intentional and tracked.
                if _preceding_lines_with_deprecated(lines, i - 1):
                    continue
                # CLI/bootstrap boundary reads (e.g. legacy
                # `resolve_agents(config)` boundary) read cwd exactly
                # once to construct an `AssetContext`.
                if _preceding_lines_with_pattern(
                    lines, i - 1, BOUNDARY_PATTERNS
                ):
                    continue
                findings.append(Finding(path, i, line.strip()))

    return findings


def main() -> int:
    findings = collect_findings()

    if findings:
        print(
            "PWD inference found in project-agent resolution modules.\n"
            "Runtime Assets Milestone 2 requires that agents, skills,\n"
            "and instructions be resolved from an explicit AssetContext,\n"
            "not from std::env::var(\"PWD\") or std::env::current_dir().\n"
            "Use AgentRegistry::load_for_context and\n"
            "resolve_agents_with_context instead. Add an allowlist\n"
            "exemption only for CLI bootstrap / boundary conversions.\n"
        )
        for f in findings:
            rel = f.file.relative_to(REPO_ROOT)
            print(f"  {rel}:{f.line_no}: {f.line_text}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1

    print(
        "PWD-inference check passed — no new std::env::var(\"PWD\") "
        "or std::env::current_dir() in project-agent resolution modules"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())