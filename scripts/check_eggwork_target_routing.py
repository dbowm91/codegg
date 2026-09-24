#!/usr/bin/env python3
"""Static lint: Eggwork fixed-target routing guard (M001).

Enforces the CodeGG-owned target-selection invariants from
``plans/implementation/eggwork-fixed-target-remote-execution``:

  1. ``ExecutorKind::Eggwork`` exists with ``as_str() == "eggwork"``.
  2. ``executor_kind_for_job`` routes ``ExecutionTarget::EggworkNode``
     to ``Eggwork`` before kind dispatch, and never maps ``Local`` to it.
  3. ``eggwork_client`` / ``eggwork_core`` are used only in
     ``src/scheduler/eggwork.rs`` (plus tests): no other module can
     construct remote executions or bypass target selection.
  4. ``src/scheduler/eggwork.rs`` spawns no local process
     (``Command::new`` / ``std::process`` / ``tokio::process``) and never
     delegates to a local executor (no ``executors::`` import).
  5. The executor persists the remote handle
     (``set_attempt_remote_handle``) so restart reconciles instead of
     resubmitting.

Run:

  python3 scripts/check_eggwork_target_routing.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXECUTOR_RS = ROOT / "src" / "scheduler" / "executor.rs"
EGGWORK_RS = ROOT / "src" / "scheduler" / "eggwork.rs"

FAILURES: list[str] = []


def fail(message: str) -> None:
    FAILURES.append(message)


def main() -> int:
    if not EGGWORK_RS.exists():
        fail(f"missing module: {EGGWORK_RS}")
        return 1
    executor_src = EXECUTOR_RS.read_text()
    eggwork_src = EGGWORK_RS.read_text()

    # 1. ExecutorKind::Eggwork with the canonical string form.
    if "Eggwork," not in executor_src and "Eggwork" not in executor_src:
        fail("ExecutorKind::Eggwork variant missing in src/scheduler/executor.rs")
    if 'ExecutorKind::Eggwork => "eggwork"' not in executor_src:
        fail('ExecutorKind::Eggwork as_str() must be "eggwork"')

    # 2. Target-first routing in executor_kind_for_job.
    routing = re.search(
        r"pub fn executor_kind_for_job\(.*?^}",
        executor_src,
        re.MULTILINE | re.DOTALL,
    )
    if routing is None:
        fail("executor_kind_for_job not found")
    else:
        body = routing.group(0)
        eggwork_arm = body.find("ExecutionTarget::EggworkNode")
        first_match = body.find("match (job.kind")
        if eggwork_arm == -1 or "ExecutorKind::Eggwork" not in body:
            fail("executor_kind_for_job must map ExecutionTarget::EggworkNode to ExecutorKind::Eggwork")
        elif first_match != -1 and eggwork_arm > first_match:
            fail("Eggwork target routing must precede kind dispatch in executor_kind_for_job")
        if "ExecutionTarget::Local" in body and "Eggwork" in body.split("ExecutionTarget::Local")[1][:200]:
            fail("Local targets must never route to the Eggwork executor")

    # 3. Eggwork crates confined to the executor module (+ tests).
    # `crates/eggwork-test-node/` is the C001 live-qualification fixture: it
    # hosts the Eggwork node under test and never constructs CodeGG jobs,
    # selections, or executions, so it cannot bypass target selection.
    allowed_prefixes = (
        "src/scheduler/eggwork.rs",
        "tests/",
        "src/scheduler/mod.rs",
        "crates/eggwork-test-node/",
    )
    for path in list(ROOT.joinpath("src").rglob("*.rs")) + list(
        ROOT.joinpath("crates").rglob("*.rs")
    ):
        rel = str(path.relative_to(ROOT))
        if rel.startswith(allowed_prefixes):
            continue
        for i, line in enumerate(path.read_text().splitlines(), 1):
            stripped = line.split("//")[0]
            if "eggwork_client::" in stripped or "eggwork_core::" in stripped:
                fail(f"{rel}:{i}: eggwork crate use outside src/scheduler/eggwork.rs")

    # 4. No local process spawn and no local-executor delegation.
    for pattern, label in [
        (r"\bstd::process::Command::new\s*\(", "std process spawn"),
        (r"\btokio::process::Command::new\s*\(", "tokio process spawn"),
        (r"\bCommand::new\s*\(", "process spawn"),
        (r"use crate::scheduler::executors::", "local executor import"),
        (r"ManagedArgvExecutor|TestJobExecutor", "local executor reference"),
    ]:
        for i, line in enumerate(eggwork_src.splitlines(), 1):
            if line.strip().startswith("//"):
                continue
            if re.search(pattern, line.split("//")[0]):
                fail(f"src/scheduler/eggwork.rs:{i}: forbidden {label}")

    # 5. Remote-handle persistence for restart reconciliation.
    if "set_attempt_remote_handle" not in eggwork_src:
        fail("src/scheduler/eggwork.rs must persist the remote handle via set_attempt_remote_handle")
    if "persist_handle" not in eggwork_src or "persisted_handle" not in eggwork_src:
        fail("src/scheduler/eggwork.rs must reconcile persisted handles on restart")

    if FAILURES:
        print("eggwork target-routing guard FAILED:")
        for failure in FAILURES:
            print(f"  - {failure}")
        return 1
    print("eggwork target-routing guard ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
