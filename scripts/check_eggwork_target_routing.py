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
  6. The frontend and tool layers cannot name a remote node at all: no
     file under ``src/tool``, ``src/tui``, ``src/agent``, or ``src/command``
     may mention ``EggworkNode``. A remote-targeted command is only ever
     spawned on the remote node, so a local surface that cannot name one
     cannot spawn one either.
  7. Production code constructs a remote ``ExecutionTarget`` only at the
     two sanctioned boundaries: the protocol conversion
     (``crates/codegg-core/src/protocol_conversions.rs``) that turns a
     client-supplied target into a domain target, and the durable read
     path (``crates/codegg-core/src/jobs/store.rs``) that restores one from
     the ``target_kind`` / ``target_node_id`` columns.

The script self-tests. ``--self-test`` feeds synthetic sources through the
same detectors used against the real tree and fails if a planted violation
is not reported. A guard that cannot be shown to fail on an injected
violation is not accepted as evidence.

Run:

  python3 scripts/check_eggwork_target_routing.py
  python3 scripts/check_eggwork_target_routing.py --self-test

Exit codes: 0 clean, 1 violations found, 2 self-test failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXECUTOR_RS = ROOT / "src" / "scheduler" / "executor.rs"
EGGWORK_RS = ROOT / "src" / "scheduler" / "eggwork.rs"

FAILURES: list[str] = []

# ── Target-construction ownership (rules 6 and 7) ──────────────────────
#
# The frontend/tool trees may not name a remote node at all. A remote
# target is selected by the daemon host boundary and executed by the
# Eggwork executor; a local surface that cannot express one cannot spawn
# a remote-targeted command locally.
FRONTEND_TREES: tuple[str, ...] = (
    "src/tool",
    "src/tui",
    "src/agent",
    "src/command",
)

# The only production sites allowed to build a remote domain target.
SANCTIONED_TARGET_CONSTRUCTION: dict[str, str] = {
    "crates/codegg-core/src/protocol_conversions.rs": (
        "the protocol conversion that turns a client-supplied target into a "
        "domain target"
    ),
    "crates/codegg-core/src/jobs/store.rs": (
        "the durable read path that restores a target from the "
        "target_kind / target_node_id columns"
    ),
}

# A *construction* names the field; a *pattern* binds it. The field-name
# form is what a caller needs in order to build a target.
TARGET_CONSTRUCTION_RE = re.compile(
    r"ExecutionTarget::EggworkNode\s*\{\s*node_id\s*:", re.MULTILINE
)
REMOTE_TARGET_TOKEN = "EggworkNode"


def fail(message: str) -> None:
    FAILURES.append(message)


def _production_source(text: str) -> str:
    """Drop the inline ``#[cfg(test)]`` module from a source file.

    Test modules legitimately construct remote targets; production code
    may not (outside the sanctioned boundaries).
    """
    marker = text.find("#[cfg(test)]")
    return text if marker == -1 else text[:marker]


def _iter_rust_sources(root: Path) -> list[tuple[str, str]]:
    """Every Rust source under ``root`` as ``(relative path, text)``."""
    sources: list[tuple[str, str]] = []
    for path in sorted(root.rglob("*.rs")):
        sources.append((str(path.relative_to(root)), path.read_text()))
    return sources


def find_frontend_remote_target(sources: list[tuple[str, str]]) -> list[str]:
    """Rule 6: the frontend and tool layers may not name a remote node."""
    findings: list[str] = []
    for rel, text in sources:
        if not any(rel.startswith(tree) for tree in FRONTEND_TREES):
            continue
        for i, line in enumerate(text.splitlines(), 1):
            stripped = line.split("//")[0]
            if REMOTE_TARGET_TOKEN in stripped:
                findings.append(
                    f"{rel}:{i}: {REMOTE_TARGET_TOKEN} in a frontend/tool surface; "
                    "a remote target is selected by the daemon host boundary only"
                )
    return findings


def find_unsanctioned_target_construction(sources: list[tuple[str, str]]) -> list[str]:
    """Rule 7: only the sanctioned boundaries may construct a remote target."""
    findings: list[str] = []
    for rel, text in sources:
        if rel.startswith("tests/"):
            continue
        if rel in SANCTIONED_TARGET_CONSTRUCTION:
            continue
        production = _production_source(text)
        for match in TARGET_CONSTRUCTION_RE.finditer(production):
            line = production.count("\n", 0, match.start()) + 1
            findings.append(
                f"{rel}:{line}: remote ExecutionTarget constructed outside the "
                "sanctioned boundaries "
                f"({', '.join(sorted(SANCTIONED_TARGET_CONSTRUCTION))})"
            )
    return findings


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

    # 6 + 7. Target-construction ownership. The crate-scoped sources carry
    # a "crates/..." prefix and the src-scoped ones "src/...", so the
    # sanctioned-path lookup matches repository-relative paths.
    rooted: list[tuple[str, str]] = []
    for rel, text in _iter_rust_sources(ROOT / "src"):
        rooted.append((f"src/{rel}", text))
    for rel, text in _iter_rust_sources(ROOT / "crates"):
        rooted.append((f"crates/{rel}", text))
    for finding in find_frontend_remote_target(rooted):
        fail(finding)
    for finding in find_unsanctioned_target_construction(rooted):
        fail(finding)

    if FAILURES:
        print("eggwork target-routing guard FAILED:")
        for failure in FAILURES:
            print(f"  - {failure}")
        return 1
    print("eggwork target-routing guard ok")
    return 0


# ── Self-test ───────────────────────────────────────────────────────────
#
# The detectors are fed synthetic sources. A detector that cannot be
# shown to fail on a planted violation is not evidence of anything.

SELF_TEST_CASES: list[tuple[str, list[tuple[str, str]], bool, str]] = [
    (
        "frontend naming a remote node is reported",
        [
            (
                "src/tool/bash/process.rs",
                "fn spawn() { let t = ExecutionTarget::EggworkNode { node_id: x }; }",
            )
        ],
        True,
        "frontend_remote_target",
    ),
    (
        "a clean frontend surface is not reported",
        [("src/tool/bash/process.rs", "fn spawn() { run_local(); }")],
        False,
        "frontend_remote_target",
    ),
    (
        "a remote target constructed outside the sanctioned boundaries is reported",
        [
            (
                "src/scheduler/other.rs",
                "let target = ExecutionTarget::EggworkNode { node_id: id };",
            )
        ],
        True,
        "unsanctioned_construction",
    ),
    (
        "a remote target constructed at a sanctioned boundary is not reported",
        [
            (
                "crates/codegg-core/src/protocol_conversions.rs",
                "let target = ExecutionTarget::EggworkNode { node_id };",
            )
        ],
        False,
        "unsanctioned_construction",
    ),
    (
        "a remote target constructed inside a test module is not reported",
        [
            (
                "src/scheduler/other.rs",
                "fn ok() {}\n#[cfg(test)]\nmod tests {\n"
                "    fn t() { let target = ExecutionTarget::EggworkNode { node_id: id }; }\n}",
            )
        ],
        False,
        "unsanctioned_construction",
    ),
    (
        "matching a remote target is not a construction",
        [
            (
                "src/scheduler/other.rs",
                "match job.target { ExecutionTarget::EggworkNode { node_id } => use_it(node_id) }",
            )
        ],
        False,
        "unsanctioned_construction",
    ),
]


def run_self_test() -> int:
    failures: list[str] = []
    for name, sources, should_report, detector in SELF_TEST_CASES:
        if detector == "frontend_remote_target":
            findings = find_frontend_remote_target(sources)
        else:
            findings = find_unsanctioned_target_construction(sources)
        reported = bool(findings)
        if reported != should_report:
            failures.append(
                f"{name}: expected {'a finding' if should_report else 'no finding'}, "
                f"got {findings or 'none'}"
            )
    if failures:
        print("self-test FAILED:")
        for failure in failures:
            print(f"  - {failure}")
        return 2
    print(f"self-test passed: {len(SELF_TEST_CASES)} case(s).")
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        sys.exit(run_self_test())
    sys.exit(main())
