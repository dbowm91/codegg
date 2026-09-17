#!/usr/bin/env python3
"""Static guard: WorkOrder coordinator ownership (Project Work Orders M002).

The `WorkOrderCoordinator` is a readiness/materialization coordinator, not a
scheduler or executor:

- every initial turn enters `JobSubmissionService` + the existing global
  scheduler (`JobKind::AgentTurn`, scheduler-owned `AgentTurnExecutor`);
- the coordinator never constructs an `AgentLoop` directly;
- the coordinator never creates a second scheduler loop/queue
  (`JobScheduler::new`, `BackgroundScheduler`, `spawn_loop`);
- the coordinator never bypasses admission (`JobScheduler::submit`,
  `JobStore::create_job`, `pool.spawner().send`);
- the coordinator never invokes the test runner directly
  (`resolve_and_run_test`, `dispatch_to_test_runner`);
- the coordinator never copies directories ad hoc as "isolation"
  (`fs::copy`, `copy_dir`, `cp -r` shell-outs);
- the work-order core module never executes (no session/job/worktree
  creation from `crates/codegg-core/src/work_order/`).

Run:

  python3 scripts/check_work_order_coordinator.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

COORDINATOR_PATHS = [
    ROOT / "src" / "core" / "work_order_coordinator.rs",
    ROOT / "src" / "core" / "daemon_work_orders.rs",
]

CORE_WORK_ORDER_ROOT = ROOT / "crates" / "codegg-core" / "src" / "work_order"

# Patterns forbidden in the daemon coordinator. Each entry maps a regex to a
# human-readable reason. `JobSubmissionService::submit` is the only allowed
# admission path (it is allow-listed separately, not forbidden here).
FORBIDDEN_COORDINATOR = [
    (r"\bAgentLoop::new\s*\(", "direct AgentLoop construction from WorkOrderCoordinator"),
    (r"\bAgentLoop\s*::\s*run\s*\(", "direct AgentLoop execution from WorkOrderCoordinator"),
    (r"\bJobScheduler::new\s*\(", "second scheduler owner in WorkOrderCoordinator"),
    (r"\bBackgroundScheduler\b", "second scheduler loop in WorkOrderCoordinator"),
    (r"\.spawn_loop\s*\(", "second scheduler loop in WorkOrderCoordinator"),
    (r"\bscheduler\s*\.\s*submit\s*\(", "JobScheduler::submit bypasses JobSubmissionService admission"),
    (r"\bJobStore::create_job\s*\(", "direct job creation bypasses JobSubmissionService"),
    (r"\.spawner\(\)\.send(?:_async)?\s*\(", "direct subagent pool send bypasses the scheduler"),
    (r"\btest_runner::runner::resolve_and_run_test\b", "direct test-runner invocation from WorkOrderCoordinator"),
    (r"\bdispatch_to_test_runner\b", "direct test-runner dispatch from WorkOrderCoordinator"),
    (r"\bstd::fs::copy\s*\(", "ad-hoc directory copying is not worktree isolation"),
    (r"\bcopy_dir_all\s*\(", "ad-hoc directory copying is not worktree isolation"),
    (r"\btokio::fs::copy\s*\(", "ad-hoc directory copying is not worktree isolation"),
]

# Patterns forbidden in the core work-order module (it owns domain +
# durable state only; execution lives daemon-side behind JobSubmissionService).
FORBIDDEN_CORE = [
    (r"\bAgentLoop\b", "core work-order module must not reference AgentLoop"),
    (r"\bJobScheduler\b", "core work-order module must not own a scheduler"),
    (r"\bJobSubmissionService\b", "core work-order module must not submit jobs"),
    (r"\bSessionStore\b", "core work-order module must not create sessions"),
    (r"\bWorktreeService\b", "core work-order module must not allocate worktrees"),
    (r"\bProviderRegistry\b", "core work-order module must not resolve providers"),
]

# Required production boundary markers: the coordinator must route initial
# turns through the canonical submission service.
REQUIRED_COORDINATOR = [
    (r"JobSubmissionService", "coordinator must submit initial turns through JobSubmissionService"),
    (r"AgentTurn", "coordinator must submit JobKind::AgentTurn initial turns"),
]


def is_comment_line(line: str) -> bool:
    return line.lstrip().startswith("//")


def check_file(path: Path, rules: list[tuple[str, str]]) -> list[str]:
    failures: list[str] = []
    try:
        content = path.read_text(encoding="utf-8")
    except OSError as error:
        return [f"{path}: could not read source: {error}"]
    lines = content.splitlines()
    for pattern, reason in rules:
        for match in re.finditer(pattern, content):
            line_no = content[: match.start()].count("\n") + 1
            if 1 <= line_no <= len(lines) and is_comment_line(lines[line_no - 1]):
                continue
            failures.append(f"{path.relative_to(ROOT)}:{line_no}: {reason} (`{match.group(0)}`)")
    return failures


def check_required(path: Path) -> list[str]:
    failures: list[str] = []
    try:
        content = path.read_text(encoding="utf-8")
    except OSError as error:
        return [f"{path}: could not read source: {error}"]
    for pattern, reason in REQUIRED_COORDINATOR:
        if not re.search(pattern, content):
            failures.append(f"{path.relative_to(ROOT)}: {reason}")
    return failures


def main() -> int:
    failures: list[str] = []
    for path in COORDINATOR_PATHS:
        if not path.exists():
            failures.append(f"{path.relative_to(ROOT)}: coordinator file is missing")
            continue
        failures.extend(check_file(path, FORBIDDEN_COORDINATOR))
    if CORE_WORK_ORDER_ROOT.exists():
        for child in sorted(CORE_WORK_ORDER_ROOT.glob("*.rs")):
            failures.extend(check_file(child, FORBIDDEN_CORE))
    else:
        failures.append("crates/codegg-core/src/work_order/: module is missing")
    coordinator = ROOT / "src" / "core" / "work_order_coordinator.rs"
    if coordinator.exists():
        failures.extend(check_required(coordinator))

    if failures:
        print("work-order coordinator ownership guard failed:")
        for line in failures:
            print(f"  {line}")
        return 1
    print("work-order coordinator ownership guard ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
