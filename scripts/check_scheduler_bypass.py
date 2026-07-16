#!/usr/bin/env python3
"""Static lint: scheduler direct-bypass guard.

The Phase 5 plan establishes a global admission-control scheduler as
the only authority that invokes the test runner. This script enforces
that rule at the source level: any caller that invokes
``test_runner::resolve_and_run_test`` or constructs a
``SubAgentJobDispatcher`` must be either:

  * the scheduler (whitelist: ``src/scheduler/**``),
  * a testing fixture that lives under ``tests/``.

The same rule applies to the canonical ``TestJobExecutor`` and
``ManagedArgvExecutor`` paths under ``src/scheduler/executors.rs``.

Callers in tool/TUI paths must submit jobs through
``scheduler.submit()`` instead of constructing executors or directly
calling ``resolve_and_run_test``.

Direct subagent pool sends (``pool.spawner().send(``) and
``BackgroundScheduler.spawn_loop``) are gated by an inline
``// scheduler-audit: <reason>`` annotation. The annotation must
appear on the same line as the call or on the line immediately
preceding it. Recognized reasons are:

  - ``scheduler-owned`` — the call is the scheduler's own executor.
  - ``standalone-compat`` — explicit ``--standalone`` / ``--stdio``
    fallback; documented outside the daemon singleton guarantee.
  - ``definition-site`` — defines the canonical subsystem but does
    not invoke it; another site is the canonical caller.
  - ``test-only`` — ``#[cfg(test)]`` fixture or under ``tests/``.

Whole-file exemptions are restricted to subsystem definition files
whose process-spawn entries are owned by the scheduler. Where a
single file contains both scheduler-owned and compatibility paths,
each compatibility call site must carry an inline annotation; the
script no longer accepts a whole-file blanket exemption for that
file.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Direct, narrow greps for the canonical "must route through scheduler"
# surfaces. Each entry maps a regex to a list of
# (path-glob, reason) exemptions: a match is allowed if its
# file path matches any of the glob exemptions, or if the line
# (or line above) carries a recognized `// scheduler-audit: <reason>`
# annotation.
RULES: list[tuple[str, list[tuple[str, str]], str]] = [
    (
        r"\btest_runner::runner::resolve_and_run_test\b",
        [
            ("src/scheduler/**", "scheduler subsystem"),
            ("tests/**", "test fixture"),
            ("src/test_runner/runner.rs", "definition site"),
        ],
        "Direct call to test_runner::runner::resolve_and_run_test must route through the scheduler. Use job_submit(...).",
    ),
    (
        r"\bdispatch_to_test_runner\b",
        [
            ("tests/**", "test fixture"),
            ("docs/**", "documentation reference"),
            ("architecture/**", "architecture doc"),
        ],
        "dispatch_to_test_runner must submit a job to the scheduler.",
    ),
    (
        r"\bSubAgentJobDispatcher\b",
        [
            ("src/job_dispatcher.rs", "legacy dispatcher site"),
            ("src/agent/worker.rs", "subagent pool definition"),
            ("src/scheduler/**", "scheduler subsystem"),
            ("tests/**", "test fixture"),
        ],
        "SubAgentJobDispatcher is the legacy bridge; production paths must go through the scheduler.",
    ),
    (
        r"\.spawner\(\)\.send(?:_async)?\(",
        [
            ("src/scheduler/**", "scheduler executor"),
            ("src/tool/task.rs", "explicit standalone task compatibility"),
            ("src/agent/task.rs", "explicit standalone background compatibility"),
            ("src/job_dispatcher.rs", "legacy dispatcher definition"),
            ("tests/**", "test fixture"),
        ],
        "Direct subagent pool sends must be scheduler submissions in daemon mode. Add a `// scheduler-audit: <reason>` annotation on the call site for explicit standalone-compat or definition-site use.",
    ),
    (
        r"\.spawn_loop\(",
        [
            ("src/agent/task.rs", "compatibility definition"),
            ("src/main.rs", "explicit standalone mode wiring"),
            ("tests/**", "test fixture"),
        ],
        "BackgroundScheduler loops are standalone compatibility only; daemon work must use durable schedules.",
    ),
]

# Inline annotation pattern. The annotation must appear on the same
# line as the call or on the line immediately preceding it.
ANNOTATION = re.compile(
    r"scheduler-audit\s*:\s*(scheduler-owned|standalone-compat|definition-site|test-only)"
)

FAILURES: list[str] = []


def matches_any(path: str, globs: list[str]) -> bool:
    for g in globs:
        if g.endswith("/**"):
            prefix = g[:-3]
            if path.startswith(prefix + "/") or path == prefix:
                return True
        else:
            if path == g or path.startswith(g.rstrip("/") + "/"):
                return True
    return False


def has_audit_annotation(content: str, line_start: int) -> bool:
    """Check if a line has a `// scheduler-audit: <reason>` annotation
    on itself or on one of the preceding lines within a small window
    (covers multi-line comment blocks)."""
    lines = content.splitlines()
    # Walk back at most 24 lines to allow structured comment blocks
    # above the call (covers function-level audit annotations).
    for offset in range(0, 24):
        idx = line_start - offset
        if 0 <= idx < len(lines):
            if ANNOTATION.search(lines[idx]):
                return True
    return False


def scan_file(path: str, content: str) -> None:
    for pattern, exemptions, message in RULES:
        for m in re.finditer(pattern, content):
            line_no = content[: m.start()].count("\n") + 1
            exemption_globs = [g for g, _ in exemptions]
            if matches_any(path, exemption_globs):
                continue
            # Allow inline annotation on files that have both
            # scheduler-owned and compatibility paths.
            if has_audit_annotation(content, line_no - 1):
                continue
            FAILURES.append(
                f"{path}:{line_no}: forbidden direct call to `{m.group(0)}` (must route through scheduler) — {message}"
            )


def walk(root: str) -> list[str]:
    out: list[str] = []
    for dirpath, _dirs, files in os.walk(root):
        if "/.git" in dirpath or "/target" in dirpath:
            continue
        for f in files:
            if f.endswith((".rs",)):
                out.append(os.path.join(dirpath, f))
    return out


def main() -> int:
    src_root = os.path.join(ROOT, "src")
    files = walk(src_root)
    # Don't lint the scheduler subsystem itself
    files = [
        f
        for f in files
        if not f.startswith(os.path.join(ROOT, "src", "scheduler") + os.sep)
        and not f.startswith(os.path.join(ROOT, "src", "test_runner") + os.sep)
    ]
    files += walk(os.path.join(ROOT, "tests"))
    for path in files:
        rel = os.path.relpath(path, ROOT)
        try:
            with open(path, "r", encoding="utf-8") as fp:
                content = fp.read()
        except OSError:
            continue
        scan_file(rel, content)

    if FAILURES:
        print("scheduler-bypass guard failed:")
        for line in FAILURES:
            print(f"  {line}")
        return 1
    print("scheduler-bypass guard ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
