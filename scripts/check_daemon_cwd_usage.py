#!/usr/bin/env python3
"""Guard: daemon-owned execution must not take its directory from the process.

The CodeGG daemon is a user-scoped singleton. It inherits whatever directory the
user happened to launch it from and keeps that directory for its whole lifetime
(ADR-0033). Any daemon-owned code path that resolves a working directory from
`std::env::current_dir()` therefore resolves it against an unrelated tree, not
against the session's workspace.

This script enforces the invariant statically: inside daemon-owned execution
surfaces, a working directory must come from propagated workspace state.

Design note — why this script is shaped the way it is
-----------------------------------------------------
The predecessor of this script suppressed findings with a regex allowlist
matched against each offending *line* (`allowed_root: std::env::current_dir`,
`let cwd = std::env::current_dir`, ...). That pattern matched the production
sites themselves, so the script exited 0 with every violation still present: it
was reporting on its own allowlist rather than on the code. A guard that passes
while the defect is live is worse than no guard, because it is cited as evidence.

Two rules follow:

  1. Exemptions are *file-scoped* and each must carry a written justification.
     There is no line-pattern allowlist, because a pattern that matches a real
     violation is indistinguishable from one that does not.
  2. The script self-tests. `--self-test` feeds synthetic sources through the
     same detector used against the real tree and fails if a planted violation
     is not reported. A guard that cannot be shown to fail on an injected
     violation is not accepted as evidence.

Usage:
    python3 scripts/check_daemon_cwd_usage.py            # scan the tree
    python3 scripts/check_daemon_cwd_usage.py --self-test
Exit codes: 0 clean, 1 violations found, 2 self-test failure.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# ── Scanned surfaces ─────────────────────────────────────────────────
#
# Daemon-owned code that can resolve or hand out a working directory. This is
# deliberately broader than the predecessor's list: that list omitted
# `tool/terminal.rs`, `tool/formatter.rs`, `tool/tool_program.rs`,
# `tool/lsp_read.rs`, `tool/git_read.rs` and `tool/mod.rs`, all of which carry
# workspace-scoped defaults.

SCANNED_TREES: list[str] = [
    "src/tool",
    "src/scheduler",
    "src/core",
]
SCANNED_FILES: list[str] = [
    "src/agent/worker.rs",
    "src/agent/turn_runtime.rs",
    "src/git_service.rs",
    "src/git_mutations.rs",
    "src/git_recovery.rs",
]

# ── Exemptions ───────────────────────────────────────────────────────
#
# Whole files, each with the reason it is permitted. Reading the process
# directory is legitimate exactly once per process — at bootstrap, before any
# session exists — and never afterwards.

EXEMPT_FILES: dict[str, str] = {
    "src/main.rs": "CLI bootstrap: derives the project root before any daemon or session exists.",
    "src/exec.rs": "CLI bootstrap: same pre-session entry point as src/main.rs.",
}

# ── Detection ────────────────────────────────────────────────────────

# Matches std::env::current_dir()/set_current_dir(), and the bare forms used
# where `use std::env;` is in scope.
CWD_RE = re.compile(r"(?:std::)?env::(?:current_dir|set_current_dir)\s*\(")
# A bare `current_dir(` with no `env::` qualifier, e.g. after
# `use std::env::current_dir;`. The leading lookbehind rejects a preceding `.`
# so `Command::current_dir(cwd)` — which *sets* a child's directory from an
# explicit argument — is not mistaken for reading the process's own.
BARE_CWD_RE = re.compile(r"(?<![:\w.])current_dir\s*\(")
# `#[cfg(test)]`, `#[test]`, `#[tokio::test]`, or a `mod tests {` open.
TEST_MARKER_RE = re.compile(r"#\[cfg\(test\)\]|#\[(?:tokio::)?test|mod\s+tests\s*\{")


@dataclass(frozen=True)
class Finding:
    file: str
    line_no: int
    line_text: str


def _strip_comment(line: str) -> str:
    """Drop a trailing line comment so prose about cwd is not a violation."""
    idx = line.find("//")
    return line if idx < 0 else line[:idx]


def scan_source(rel_path: str, source: str) -> list[Finding]:
    """Return cwd violations in one source file.

    `#[cfg(test)]` modules are skipped: they run in a test process whose cwd is
    the harness's, and a test asserting on a relative path is not a daemon
    execution path.
    """
    if rel_path in EXEMPT_FILES:
        return []

    findings: list[Finding] = []
    in_test_module = False
    test_module_indent = 0

    for line_no, raw in enumerate(source.splitlines(), 1):
        indent = len(raw) - len(raw.lstrip())
        code = _strip_comment(raw)

        if in_test_module and raw.strip() and indent <= test_module_indent:
            in_test_module = False

        if not in_test_module and TEST_MARKER_RE.search(code):
            in_test_module = True
            test_module_indent = indent
            continue

        if in_test_module:
            continue

        if CWD_RE.search(code) or BARE_CWD_RE.search(code):
            findings.append(Finding(rel_path, line_no, code.strip()))

    return findings


def _iter_sources() -> list[tuple[str, str]]:
    sources: list[tuple[str, str]] = []

    for tree in SCANNED_TREES:
        root = REPO_ROOT / tree
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            sources.append((str(path.relative_to(REPO_ROOT)), path.read_text()))

    for rel in SCANNED_FILES:
        path = REPO_ROOT / rel
        if path.is_file():
            sources.append((rel, path.read_text()))

    return sources


def collect_findings() -> list[Finding]:
    findings: list[Finding] = []
    for rel_path, source in _iter_sources():
        findings.extend(scan_source(rel_path, source))
    return findings


# ── Self-test ────────────────────────────────────────────────────────
#
# Exercises the exact detector used above. Each case states what it proves.

SELF_TEST_CASES: list[tuple[str, str, str, int]] = [
    (
        "a production line that defaults a tool root to the process cwd is caught",
        "src/tool/hypothetical.rs",
        "pub fn new() -> Self {\n"
        "    Self { allowed_root: std::env::current_dir().unwrap_or_default() }\n"
        "}\n",
        1,
    ),
    (
        "a bare current_dir() call is caught too",
        "src/tool/hypothetical.rs",
        "pub fn root() -> PathBuf {\n"
        "    current_dir().unwrap_or_default()\n"
        "}\n",
        1,
    ),
    (
        "set_current_dir (a daemon-wide chdir) is caught",
        "src/scheduler/hypothetical.rs",
        "pub fn go(p: &Path) { std::env::set_current_dir(p).ok(); }\n",
        1,
    ),
    (
        "the same call inside a #[cfg(test)] module is not a violation",
        "src/tool/hypothetical.rs",
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    fn t() { std::env::current_dir().unwrap(); }\n"
        "}\n",
        0,
    ),
    (
        "a comment describing the old behaviour is not a violation",
        "src/tool/hypothetical.rs",
        "/// Used to call std::env::current_dir() here.\n"
        "pub fn new() -> Self { Self {} }\n",
        0,
    ),
    (
        "the declared bootstrap file is exempt",
        "src/main.rs",
        "fn root() -> PathBuf { std::env::current_dir().unwrap_or_default() }\n",
        0,
    ),
    (
        "a file not on the exemption list is still checked even next to one",
        "src/tool/hypothetical.rs",
        "fn root() -> PathBuf { std::env::current_dir().unwrap_or_default() }\n",
        1,
    ),
    (
        "a normal rooted call is not a violation",
        "src/tool/hypothetical.rs",
        "pub fn new(root: PathBuf) -> Self { Self { allowed_root: root } }\n",
        0,
    ),
]


def run_self_test() -> int:
    failures = 0
    for description, rel_path, source, expected in SELF_TEST_CASES:
        found = scan_source(rel_path, source)
        actual = len(found)
        if actual == expected:
            print(f"  ok   {description}")
        else:
            failures += 1
            print(f"  FAIL {description}: expected {expected}, found {actual}")
            for f in found:
                print(f"         {f.file}:{f.line_no}: {f.line_text}")

    if failures:
        print(f"\nself-test FAILED: {failures} case(s) — the detector does not discriminate.")
        return 2

    print(f"\nself-test passed: {len(SELF_TEST_CASES)} case(s).")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return run_self_test()

    findings = collect_findings()

    if findings:
        print(
            "std::env::current_dir()/set_current_dir() in daemon-owned execution.\n"
            "A working directory must come from propagated workspace state\n"
            "(ExecutionContext.workspace_root or JobExecutionContext.workspace_root).\n"
            "The daemon's process directory is where the user launched it and is\n"
            "unrelated to the session being run; it is never an acceptable root.\n"
            "\nIf this really is pre-session bootstrap, exempt the whole file in\n"
            "EXEMPT_FILES above with a written justification.\n"
        )
        for f in findings:
            print(f"  {f.file}:{f.line_no}: {f.line_text}")
        print(f"\n{len(findings)} violation(s) found.")
        return 1

    print("cwd usage check passed — no process-cwd reads in daemon-owned execution")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))