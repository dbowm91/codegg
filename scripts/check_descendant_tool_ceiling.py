#!/usr/bin/env python3
"""Descendant tool-ceiling boundary guard (M003).

Every model-driven subagent delegation must carry a host-minted
ParentToolAuthoritySnapshot end to end:

- worker child turns resolve their surface under the snapshot
  (exact names + capability bits) and install the narrowed ceiling on
  the loop for broker admission;
- TaskTool never reads authority from model tool arguments and fails
  closed when the snapshot is missing or invalid at launch;
- the batch executor mints the delegation ceiling live and binds the
  child broker manifest (`allowed_tools`) to it;
- scheduler/durable payloads carry the additive host-persisted field
  and decode it fail-closed.

Root turns and explicit system delegations (research, security review,
legacy payloads) legitimately carry `None`; the model-driven `task`
path must never default to unrestricted `None`.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def read(rel: str) -> str:
    return (ROOT / rel).read_text()


def fail(message: str) -> int:
    print(f"FAIL: {message}")
    return 1


def main() -> int:
    failures = 0

    worker = read("src/agent/worker.rs")
    task = read("src/tool/task.rs")
    batch = read("src/agent/tool_batch.rs")
    backend = read("src/tool/backend.rs")
    surface = read("src/agent/tool_surface.rs")
    jobs = read("crates/codegg-core/src/jobs/mod.rs")
    executors = read("src/scheduler/executors.rs")
    dispatcher = read("src/job_dispatcher.rs")

    # 1. Worker enforces the snapshot on child turns.
    for token in (
        "from_registry_with_parent_authority",
        "set_parent_tool_authority",
        "narrow_for_child",
        "StaleGeneration",
    ):
        if token not in worker:
            failures += fail(f"src/agent/worker.rs lacks {token!r}")

    # 2. No ambient `None` ceiling in the worker child surface path:
    # the only `None` parent-ceiling call must be outside worker.rs
    # (root turns) or behind the explicit system-delegation branch.
    child_surface_calls = [
        line
        for line in worker.splitlines()
        if "from_registry_with_aliases" in line
    ]
    if not child_surface_calls:
        # from_registry_with_aliases may vanish entirely from worker;
        # the parent-authority constructor above is then the only path.
        pass
    for line in worker.splitlines():
        if "parent_ceiling" in line and "None" in line:
            failures += fail(
                "src/agent/worker.rs passes a None parent ceiling "
                f"(line: {line.strip()[:120]})"
            )

    # 3. TaskTool never reads authority from model arguments...
    if re.search(r'input\s*\[\s*"parent_authority"\s*\]', task):
        failures += fail("src/tool/task.rs reads parent_authority from model input")
    if "parent_authority" not in task or "with_parent_tool_authority" not in task:
        failures += fail("src/tool/task.rs lacks the host-side authority carrier")
    # ...and fails closed at launch.
    if "parent tool authority unavailable" not in task:
        failures += fail("src/tool/task.rs lacks the fail-closed delegation guard")

    # 4. Batch executor mints live and binds the child broker manifest.
    for token in ("mint_delegation_ceiling", "child_allowed_tools"):
        if token not in batch:
            failures += fail(f"src/agent/tool_batch.rs lacks {token!r}")

    # 5. Execution context carries the snapshot (host side only).
    if "parent_tool_authority" not in backend:
        failures += fail("src/tool/backend.rs lacks parent_tool_authority")

    # 6. Surface enforces exact names + capabilities with parent-bound fp.
    for token in ("resolve_with_parent_authority", "ParentCeiling"):
        if token not in surface:
            failures += fail(f"src/agent/tool_surface.rs lacks {token!r}")

    # 7. Durable payloads carry the additive field and decode fail-closed.
    if "parent_authority: Option<String>" not in jobs:
        failures += fail("JobPayload Subagent/SubagentRun lack parent_authority")
    if "decode_parent_authority" not in executors:
        failures += fail("src/scheduler/executors.rs does not decode parent_authority")
    if "decode_parent_authority" not in dispatcher:
        failures += fail("src/job_dispatcher.rs does not decode parent_authority")

    if failures:
        print(f"{failures} descendant-ceiling violation(s)")
        return 1
    print("OK: Descendant tool-ceiling invariants hold.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
