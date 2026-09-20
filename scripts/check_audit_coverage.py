#!/usr/bin/env python3
"""Static guard for audit instrumentation coverage (identity M005).

Ensures the Phase-11 required-event matrix stays executable: every
known audit action has a coverage row, every daemon operation is either
live-mapped or explicitly uninstrumented, live-mapped actions have at
least one operation mapping, instrumentation stays append-only with
trusted attribution, and the operator matrix doc exists.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
AUDIT_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "audit.rs"
INSTR_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "audit_instrumentation.rs"
# Canonical authorization descriptor source. The descriptor table moved
# from `authorization.rs` to `authorization/policy.rs` (team-collaboration
# post-closure M001); this guard must parse the canonical module, never
# the historical file location.
AUTHZ_POLICY_MODULE = (
    REPO_ROOT / "crates" / "codegg-core" / "src" / "authorization" / "policy.rs"
)
DAEMON_MODULE = REPO_ROOT / "src" / "core" / "daemon.rs"
AUDIT_DOC = REPO_ROOT / "architecture" / "audit.md"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _audit_actions() -> list[str]:
    source = _read(AUDIT_MODULE)
    match = re.search(r"pub const ALL:\s*\[&'static str;\s*\d+\]\s*=\s*\[(.*?)\];", source, re.DOTALL)
    if not match:
        print("  FAIL: AuditAction::ALL not found")
        return []
    return re.findall(r'"([^"]+)"', match.group(1))


def _coverage_actions() -> list[str]:
    source = _read(INSTR_MODULE)
    # Anchor on REQUIRED_AUDIT_COVERAGE so the M004 executor-hook table
    # (which also uses `action:` keys) is not double-counted.
    match = re.search(
        r"pub const REQUIRED_AUDIT_COVERAGE[^=]*=\s*&\[(.*?)\];", source, re.DOTALL
    )
    if not match:
        print("  FAIL: REQUIRED_AUDIT_COVERAGE not found")
        return []
    return re.findall(r'action:\s*"([^"]+)"', match.group(1))


def _instrumented_pairs() -> list[tuple[str, str]]:
    source = _read(INSTR_MODULE)
    match = re.search(
        r"pub const INSTRUMENTED_OPERATIONS[^=]*=\s*&\[(.*?)\];", source, re.DOTALL
    )
    if not match:
        print("  FAIL: INSTRUMENTED_OPERATIONS not found")
        return []
    # Avoid the doc-comment mention of UNINSTRUMENTED_OPERATIONS: slice
    # from the const definition only (match already starts there).
    return re.findall(r'\(\"([^\"]+)\",\s*\"([^\"]+)\"', match.group(1))


def _uninstrumented_ops() -> list[str]:
    source = _read(INSTR_MODULE)
    # Anchor on the const definition to avoid the doc-comment mention
    # inside the INSTRUMENTED block.
    match = re.search(
        r"pub const UNINSTRUMENTED_OPERATIONS[^=]*=\s*&\[(.*?)\];", source, re.DOTALL
    )
    if not match:
        print("  FAIL: UNINSTRUMENTED_OPERATIONS not found")
        return []
    return re.findall(r'"([^"]+)"', match.group(1))


def _authz_operations() -> set[str]:
    # Canonical source of truth: `authorization/policy.rs`
    # (`operation_descriptor` + `operation_capability_matrix`). Fail
    # closed with a clear message if the descriptor table moves again
    # instead of silently returning an empty set as success.
    if not AUTHZ_POLICY_MODULE.is_file():
        print(f"  FAIL: canonical descriptor module missing: {AUTHZ_POLICY_MODULE}")
        return set()
    source = _read(AUTHZ_POLICY_MODULE)
    if "pub fn operation_descriptor" not in source:
        print("  FAIL: operation_descriptor not found in canonical policy module")
        return set()
    ops = set(re.findall(r'OperationDescriptor::new\(\s*\"([^\"]+)\"', source))
    ops.discard("projection_scope")
    if not ops:
        print("  FAIL: no operations discovered in canonical policy module")
        return set()
    return ops


def check_descriptor_source_is_canonical() -> bool:
    """Pin the guard to the canonical descriptor source.

    Regression for the M003 stale-guard failure where this script read
    `authorization.rs` after the table moved to `policy.rs` and silently
    inventoried an empty/noncanonical set. Fails if the policy module
    moves, the constructor shape changes, or known post-M001 operations
    disappear from the inventory.
    """
    if not AUTHZ_POLICY_MODULE.is_file():
        print(f"  FAIL: canonical descriptor module missing: {AUTHZ_POLICY_MODULE}")
        return False
    source = _read(AUTHZ_POLICY_MODULE)
    if "pub fn operation_descriptor" not in source:
        print("  FAIL: operation_descriptor not found in canonical policy module")
        return False
    if "OperationDescriptor::new" not in source:
        print("  FAIL: OperationDescriptor::new not found in canonical policy module")
        return False
    ops = _authz_operations()
    # Spot-check corrected M001 semantics plus breadth: the inventory
    # must be non-empty and name the LocalOwner-only registration ops.
    for required in ("workspace_register", "workspace_list", "project_register"):
        if required not in ops:
            print(f"  FAIL: canonical inventory missing {required}")
            return False
    if len(ops) < 130:
        print(f"  FAIL: canonical inventory too small ({len(ops)} ops)")
        return False
    return True


def check_matrix_covers_every_action() -> bool:
    known = _audit_actions()
    if not known:
        return False
    covered = _coverage_actions()
    missing = [action for action in known if action not in covered]
    if missing:
        print(f"  FAIL: coverage matrix missing actions: {', '.join(missing)}")
        return False
    if len(covered) != len(known):
        print(f"  FAIL: matrix has {len(covered)} rows for {len(known)} actions")
        return False
    return True


def check_every_operation_is_classified() -> bool:
    ops = _authz_operations()
    if not ops:
        print("  FAIL: no daemon operations discovered (descriptor source moved?)")
        return False
    pairs = _instrumented_pairs()
    instrumented = {op for op, _ in pairs}
    uninstrumented = set(_uninstrumented_ops())
    both = instrumented & uninstrumented
    if both:
        print(f"  FAIL: operations in both lists: {', '.join(sorted(both))}")
        return False
    missing = sorted(ops - instrumented - uninstrumented)
    if missing:
        print(f"  FAIL: unclassified daemon operations: {', '.join(missing)}")
        return False
    # Every mapped action must be a known audit action.
    known = set(_audit_actions())
    bad = sorted({action for _, action in pairs if action not in known})
    if bad:
        print(f"  FAIL: instrumented mappings to unknown actions: {', '.join(bad)}")
        return False
    return True


def _executor_hook_actions(source: str | None = None) -> list[str]:
    """Declarative executor-hook actions from EXECUTOR_LIVE_AUDIT_HOOKS."""
    text = source if source is not None else _read(INSTR_MODULE)
    match = re.search(
        r"pub const EXECUTOR_LIVE_AUDIT_HOOKS[^=]*=\s*&\[(.*?)\];", text, re.DOTALL
    )
    if not match:
        return []
    return re.findall(r'action:\s*"([^"]+)"', match.group(1))


def _future_distributed_actions(source: str | None = None) -> list[str]:
    """Intentionally future/distributed actions from the core table."""
    text = source if source is not None else _read(INSTR_MODULE)
    match = re.search(
        r"pub const FUTURE_DISTRIBUTED_AUDIT_ACTIONS[^=]*=\s*&\[(.*?)\];",
        text,
        re.DOTALL,
    )
    if not match:
        return []
    return re.findall(r'"([^"]+)"', match.group(1))


def _coverage_live_map(source: str | None = None) -> dict[str, bool]:
    """Map coverage action -> live_mapped flag."""
    text = source if source is not None else _read(INSTR_MODULE)
    blocks = re.split(r"AuditCoverageEntry\s*\{", text)[1:]
    out: dict[str, bool] = {}
    for block in blocks:
        action_match = re.search(r'action:\s*"([^"]+)"', block)
        live_match = re.search(r"live_mapped:\s*(true|false)", block)
        if not action_match or not live_match:
            continue
        out[action_match.group(1)] = live_match.group(1) == "true"
    return out


def check_live_mapped_actions_have_operation_mapping() -> bool:
    source = _read(INSTR_MODULE)
    # Split the coverage table into per-action blocks: each block starts
    # with `action: "..."` and contains a `live_mapped: true/false`.
    blocks = re.split(r"AuditCoverageEntry\s*\{", source)[1:]
    pairs = _instrumented_pairs()
    mapped_actions = {action for _, action in pairs}
    executor_actions = set(_executor_hook_actions(source))
    ok = True
    for block in blocks:
        action_match = re.search(r'action:\s*"([^"]+)"', block)
        live_match = re.search(r"live_mapped:\s*(true|false)", block)
        if not action_match or not live_match:
            continue
        action, live = action_match.group(1), live_match.group(1) == "true"
        if not live:
            continue
        # `authorization_decision` is live via the denial seam (every
        # denied operation), not via one operation-table row.
        if action == "authorization_decision":
            continue
        # M004: executor-owned live hooks carry executable evidence in
        # EXECUTOR_LIVE_AUDIT_HOOKS plus owner pins; they must NOT be
        # satisfied by a daemon operation-table row. `job_complete` keeps
        # its `job_retry` request mapping alongside the terminal hook,
        # so it still appears here; command/git are executor-only.
        if action in executor_actions:
            continue
        if action not in mapped_actions:
            print(f"  FAIL: live-mapped action has no operation mapping: {action}")
            ok = False
    return ok


def check_executor_hook_table_is_authoritative() -> bool:
    """M004: executor/future/daemon categories stay distinct and live.

    - EXECUTOR_LIVE_AUDIT_HOOKS names exactly the three corrected
      single-host actions, each live_mapped and never in UNINSTRUMENTED;
    - command_execute/git_operation have no daemon operation mapping
      (executor-only); job_complete keeps job_retry as the retry request;
    - FUTURE_DISTRIBUTED_AUDIT_ACTIONS names exactly node/remote, each
      unmapped and not live;
    - adding an executor action name to UNINSTRUMENTED never satisfies
      the guard (checked explicitly here).
    """
    source = _read(INSTR_MODULE)
    ok = True
    executor = _executor_hook_actions(source)
    if executor != ["command_execute", "git_operation", "job_complete"]:
        print(f"  FAIL: executor hook table must be [command_execute, git_operation, job_complete], got {executor}")
        return False
    future = _future_distributed_actions(source)
    if future != ["node_enrollment", "remote_execute"]:
        print(f"  FAIL: future table must be [node_enrollment, remote_execute], got {future}")
        return False
    live = _coverage_live_map(source)
    for action in executor:
        if live.get(action) is not True:
            print(f"  FAIL: executor action {action} must be live_mapped=true")
            ok = False
    for action in future:
        if live.get(action) is not False:
            print(f"  FAIL: future action {action} must be live_mapped=false")
            ok = False
    uninstrumented = set(_uninstrumented_ops())
    for action in executor:
        if action in uninstrumented:
            print(f"  FAIL: executor action {action} must never hide in UNINSTRUMENTED_OPERATIONS")
            ok = False
    pairs = _instrumented_pairs()
    mapped_actions = {action for _, action in pairs}
    for action in ("command_execute", "git_operation"):
        if action in mapped_actions:
            print(f"  FAIL: {action} must stay executor-only with no daemon operation mapping")
            ok = False
    if ("job_retry", "job_complete") not in pairs:
        print("  FAIL: job_retry -> job_complete retry-request mapping must be retained")
        ok = False
    for action in future:
        if action in mapped_actions:
            print(f"  FAIL: future action {action} must have no daemon mapping")
            ok = False
    # Interactive-process daemon operations stay explicitly uninstrumented
    # (Global execution surface): their live evidence is the interactive
    # command_execute hook, not a daemon mapping.
    for op in ("interactive_process_create", "interactive_process_input"):
        if op not in uninstrumented:
            print(f"  FAIL: {op} must stay explicitly uninstrumented (executor hook owns the event)")
            ok = False
    return ok


def check_instrumentation_stays_append_only_and_trusted() -> bool:
    source = _read(INSTR_MODULE)
    if re.search(r"(?i)\b(UPDATE|DELETE)\s+.*audit_event\b", source):
        print("  FAIL: instrumentation must never UPDATE/DELETE audit_event rows")
        return False
    if re.search(r"(?i)DELETE\s+FROM\s+audit_event", source):
        print("  FAIL: instrumentation must never delete structural rows")
        return False
    for required in (
        "AuditDecisionProvenance",
        "AuthenticatedPrincipal",
        "deterministic_event_id",
        "AuditChainContext",
        "operation_to_audit_action",
        "TrustedExecutionAuditContext",
        "ExecutionAuditEmitter",
        "EXECUTION_AUDIT_EMIT_TIMEOUT",
    ):
        if required not in source:
            print(f"  FAIL: instrumentation missing trusted-attribution marker {required}")
            return False
    # Core boundary: the instrumentation module must not import the
    # authorization service (see check-core-boundary.sh). The daemon
    # seam owns the operation_descriptor -> operation_to_audit_action
    # step plus the audit_provenance bridge. Test-only coverage pins
    # (`#[cfg(test)]`) may reference the canonical matrix; strip them
    # before enforcing the production boundary.
    production = source.split("#[cfg(test)]")[0]
    if "crate::authorization" in production or "crate::auth" in production:
        print("  FAIL: instrumentation must not import crate::authorization (core boundary)")
        return False
    daemon = _read(DAEMON_MODULE)
    for required in (
        "emit_audit_for_authorized",
        "emit_audit_for_denial",
        "append_audit_event",
        "audit_chain_for_request",
        "audit_emitter",
        "execution_audit_context",
    ):
        if required not in daemon:
            print(f"  FAIL: daemon seam missing {required}")
            return False
    # M001: execution owners receive the trusted context by injection;
    # they must never reconstruct it from principal_ref or payloads.
    for path_name, marker in (
        ("src/tool/broker.rs", "execution_audit"),
        ("src/tool/backend.rs", "execution_audit"),
        ("src/git_mutations.rs", "execution_audit"),
        ("src/scheduler/scheduler.rs", "audit_emitter"),
    ):
        path = REPO_ROOT / path_name
        if not path.is_file():
            print(f"  FAIL: execution owner missing: {path_name}")
            return False
        if marker not in path.read_text(encoding="utf-8"):
            print(f"  FAIL: {path_name} missing M001 seam marker {marker}")
            return False
    return True


def check_m002_live_execution_hooks_present() -> bool:
    """Pin the M002 live executor hooks at their canonical owners.

    The coverage matrix above tracks daemon operation mappings; M002
    hooks live one layer down (tool dispatch, Git executor, interactive
    create). This check keeps those hooks from silently regressing to
    builder-only declarations: every canonical owner must reference
    its emit path, the deterministic idempotency helper must be used
    outside the daemon seam, and the daemon must build the interactive
    transport-bound hook. Full live-vs-builder guard tightening (event
    samples, duplicate/secret negatives) is M004 scope.
    """
    expected = (
        ("src/live_execution_audit.rs", "emit_command_execute"),
        ("src/live_execution_audit.rs", "command_family_for_tool"),
        ("src/live_execution_audit.rs", "invocation_scope"),
        ("src/tool/bash.rs", "emit_command_audit"),
        ("src/tool/terminal.rs", "emit_command_audit"),
        ("src/tool/test.rs", "emit_test_audit"),
        ("src/interactive_process_attach.rs", "emit_interactive_create_audit"),
        ("src/interactive_process_attach.rs", "InteractiveAuditHook"),
        ("src/git_mutations.rs", "emit_git_operation"),
        ("src/git_mutations.rs", "git_audit_op_label"),
        ("src/git_mutations.rs", "git_audit_ref_digest"),
        ("src/git_mutations.rs", "emit_git_operation_parts"),
        ("src/git_mutations_ops.rs", "emit_git_operation"),
        ("src/tool/git.rs", "emit_raw_git_audit"),
        ("src/tool/backend.rs", "live_audit_hook"),
        ("src/tool/broker.rs", "audit_emitter"),
        ("src/core/daemon.rs", "interactive_audit_hook"),
    )
    ok = True
    for rel, marker in expected:
        path = REPO_ROOT / rel
        if not path.is_file():
            print(f"  FAIL: live-hook owner missing: {rel}")
            ok = False
            continue
        if marker not in path.read_text(encoding="utf-8"):
            print(f"  FAIL: {rel} missing live-hook marker {marker}")
            ok = False
    return ok


def check_m003_scheduler_job_complete_present() -> bool:
    """Pin the M003 scheduler terminal `job_complete` hook.

    The true terminal owner is the durable scheduler attempt transition
    (`persist_completion` + `mark_unschedulable` + queued
    `request_cancel`), not the `job_retry` request mapping. This check
    keeps the hook from silently regressing to builder-only status: the
    scheduler must reference the terminal emit path, resolve durable
    attribution with the explicit legacy fallback, scope terminals by
    attempt, and bound outcome labels without payload content.
    """
    expected = (
        ("src/scheduler/job_complete_audit.rs", "emit_terminal_completion"),
        ("src/scheduler/job_complete_audit.rs", "trusted_context_for_terminal_job"),
        ("src/scheduler/job_complete_audit.rs", "terminal_scope"),
        ("src/scheduler/job_complete_audit.rs", "job_outcome_label"),
        ("src/scheduler/job_complete_audit.rs", "legacy_local"),
        ("src/scheduler/scheduler.rs", "emit_terminal_completion"),
        ("src/scheduler/scheduler.rs", "mark_unschedulable"),
        ("src/scheduler/scheduler.rs", "persist_completion"),
        ("crates/codegg-core/src/transport_auth.rs", "reconstructed"),
        ("crates/codegg-core/src/audit_instrumentation.rs", "pool_snapshot"),
    )
    ok = True
    for rel, marker in expected:
        path = REPO_ROOT / rel
        if not path.is_file():
            print(f"  FAIL: job-complete owner missing: {rel}")
            ok = False
            continue
        if marker not in path.read_text(encoding="utf-8"):
            print(f"  FAIL: {rel} missing job-complete marker {marker}")
            ok = False
    return ok


def _owner_pins(source: str | None = None) -> list[tuple[str, str, str]]:
    """Declarative executor-hook owner pins from src/executor_audit_hooks.rs.

    Each row is (action, owner_file, emit_symbol). The guard consumes this
    table instead of ad-hoc call text so the owner move fails closed.
    """
    path = REPO_ROOT / "src" / "executor_audit_hooks.rs"
    text = source if source is not None else _read(path)
    pins: list[tuple[str, str, str]] = []
    for block in re.split(r"ExecutorHookPin\s*\{", text)[1:]:
        action = re.search(r'action:\s*"([^"]+)"', block)
        owner = re.search(r'owner_file:\s*"([^"]+)"', block)
        emit = re.search(r'emit_symbol:\s*"([^"]+)"', block)
        if action and owner and emit:
            pins.append((action.group(1), owner.group(1), emit.group(1)))
    return pins


def check_m004_executor_owner_pins_present() -> bool:
    """M004: declarative executor-hook table has executable owners.

    Consumes src/executor_audit_hooks.rs: every pinned (action, file,
    symbol) must exist with the symbol present, every executor-live core
    action must have at least one pin, and future actions must have none.
    Removing a hook (or its owner) fails closed; listing the action in
    UNINSTRUMENTED_OPERATIONS never satisfies this check.
    """
    pins_path = REPO_ROOT / "src" / "executor_audit_hooks.rs"
    if not pins_path.is_file():
        print("  FAIL: src/executor_audit_hooks.rs missing (declarative owner table)")
        return False
    pins = _owner_pins()
    if not pins:
        print("  FAIL: no executor owner pins parsed (table moved?)")
        return False
    executor = _executor_hook_actions()
    if not executor:
        print("  FAIL: core EXECUTOR_LIVE_AUDIT_HOOKS missing (table moved?)")
        return False
    future = set(_future_distributed_actions())
    ok = True
    for action in executor:
        if not any(pin_action == action for pin_action, _, _ in pins):
            print(f"  FAIL: executor action {action} has no owner pin")
            ok = False
    for pin_action, _, _ in pins:
        if pin_action not in executor:
            print(f"  FAIL: owner pin for non-executor action {pin_action}")
            ok = False
        if pin_action in future:
            print(f"  FAIL: future action {pin_action} must not have an owner pin")
            ok = False
    for pin_action, rel, marker in pins:
        path = REPO_ROOT / rel
        if not path.is_file():
            print(f"  FAIL: hook owner missing: {rel} (action {pin_action})")
            ok = False
            continue
        if marker not in path.read_text(encoding="utf-8"):
            print(f"  FAIL: {rel} missing hook marker {marker} (action {pin_action})")
            ok = False
    return ok


def check_audit_doc_has_matrix() -> bool:
    if not AUDIT_DOC.is_file():
        print("  FAIL: architecture/audit.md missing")
        return False
    source = _read(AUDIT_DOC)
    for required in (
        "coverage",
        "causation",
        "correlation",
        "redact",
        "backpressure",
        "audit.read",
    ):
        if required.lower() not in source.lower():
            print(f"  FAIL: audit doc missing {required}")
            return False
    return True


def run_self_test() -> int:
    """Fail-closed self-test for the M004 declarative-table parsing.

    Proves the guard distinguishes live executor hooks from builder-only
    declarations and future scope without silently passing on moved
    tables or UNINSTRUMENTED evasion.
    """
    failures: list[str] = []

    def check(name: str, passed: bool) -> None:
        if not passed:
            failures.append(name)

    # Executor table parsing: exact triple required.
    sample = 'pub const EXECUTOR_LIVE_AUDIT_HOOKS: &[ExecutorAuditHook] = &[ExecutorAuditHook { action: "command_execute", owner: "executor:tool", emit: "emit_command_execute", }, ];'
    check("executor parse finds command_execute", "command_execute" in _executor_hook_actions(sample))
    check("executor parse rejects empty", _executor_hook_actions("no table here") == [])
    # Future table parsing.
    future_sample = 'pub const FUTURE_DISTRIBUTED_AUDIT_ACTIONS: &[&str] = &["node_enrollment", "remote_execute"];'
    parsed_future = _future_distributed_actions(future_sample)
    check("future parse finds both", parsed_future == ["node_enrollment", "remote_execute"])
    # Owner-pin parsing.
    pin_sample = 'ExecutorHookPin { action: "command_execute", owner_file: "src/live_execution_audit.rs", emit_symbol: "emit_command_execute", }'
    pins = _owner_pins(pin_sample)
    check("pin parse finds one row", pins == [("command_execute", "src/live_execution_audit.rs", "emit_command_execute")])
    check("pin parse rejects empty", _owner_pins("no pins") == [])
    # Live-map parsing distinguishes true/false.
    live_sample = 'AuditCoverageEntry { action: "command_execute", live_mapped: true, } AuditCoverageEntry { action: "node_enrollment", live_mapped: false, }'
    live_map = _coverage_live_map(live_sample)
    check("live map true", live_map.get("command_execute") is True)
    check("live map false", live_map.get("node_enrollment") is False)
    # Current tree must satisfy the new checks (fail closed on drift).
    try:
        check("authoritative table passes on current tree", check_executor_hook_table_is_authoritative())
    except Exception as exc:  # noqa: BLE001
        failures.append(f"authoritative check raised {exc}")
    try:
        check("owner pins pass on current tree", check_m004_executor_owner_pins_present())
    except Exception as exc:  # noqa: BLE001
        failures.append(f"owner-pin check raised {exc}")

    if failures:
        print("audit-coverage self-test failed:")
        for line in failures:
            print(f"  {line}")
        return 1
    print("audit-coverage self-test ok (executor/future/pin parsing fail closed)")
    return 0


def main() -> int:
    if "--self-test" in sys.argv:
        return run_self_test()
    verbose = "--verbose" in sys.argv or "-v" in sys.argv
    checks: list[tuple[str, object]] = [
        ("descriptor source is canonical policy module", check_descriptor_source_is_canonical),
        ("coverage matrix covers every audit action", check_matrix_covers_every_action),
        ("every daemon operation is classified", check_every_operation_is_classified),
        ("live-mapped actions have operation mappings", check_live_mapped_actions_have_operation_mapping),
        ("instrumentation stays append-only and trusted", check_instrumentation_stays_append_only_and_trusted),
        ("M002 live execution hooks are present at canonical owners", check_m002_live_execution_hooks_present),
        ("M003 scheduler job-complete hook is present at canonical owner", check_m003_scheduler_job_complete_present),
        ("M004 executor/future/daemon categories are authoritative", check_executor_hook_table_is_authoritative),
        ("M004 executor owner pins have executable owners", check_m004_executor_owner_pins_present),
        ("audit operator matrix doc exists", check_audit_doc_has_matrix),
    ]
    results: list[tuple[str, bool]] = []
    for name, check_fn in checks:
        if verbose:
            print(f"CHECK: {name} ... ", end="", flush=True)
        try:
            passed = bool(check_fn())  # type: ignore[operator]
        except Exception as exc:  # noqa: BLE001 - guard must fail loudly
            print(f"  FAIL: exception: {exc}")
            passed = False
        results.append((name, passed))
        if verbose:
            print("PASS" if passed else "FAIL")
    failed = [name for name, passed in results if not passed]
    if verbose:
        print()
        for name, passed in results:
            print(f"  [{'PASS' if passed else 'FAIL'}] {name}")
        print(f"\n{len(results) - len(failed)}/{len(results)} checks passed.")
    else:
        for name in failed:
            print(f"FAIL: {name}")
    if failed:
        print("Audit coverage invariants violated.")
        return 1
    print("All audit coverage invariants verified.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
