# Windows Native Sandbox Milestone 001 — Launch Contract and Fail-Closed Authority

Status: implemented

Repository baseline: `a02cd9ade7ffca775efc1567669aae728ad9ff72`

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Primary class: invariant

## 1. Objective

Make platform-created constrained child processes expressible within `ManagedProcessService` without silently broadening sandbox authority or rewriting Unix Landlock/Seatbelt. This is a contract and routing change, not Windows sandbox availability.

## 2. Why this milestone is ready / dependency gate

ADR-0004's immutable profile/approval separation and ADR-0016's platform-owned launch decision are accepted. No Windows runtime precondition. Existing `ManagedProcessService`, backend registry, status codec and profile types are stable.

## 3. Current implementation evidence

- `backend.rs` takes `fn apply(&SandboxLaunchSpec)` and expects Unix helper `exec`.
- `managed_process.rs` rejects `SandboxRequest::Required` for `cfg(not(unix))`; the compatibility `DegradedUncontained` request must be rejected before spawn.
- `sandbox.rs` reports `resolve_sandbox_enforcement` before launch and has filesystem/network dimensions, but the advertised backend selected by probe alone is not necessarily a real launch guarantee.

## 4. Invariants that must not regress

- `ManagedProcessService` owns all process creation; `JobSubmissionService` owns scheduled heavy work; no alternative shell executor.
- Requested profile and obtained enforcement remain distinct; no automatic FullHost, even under Yolo.
- Child/worktree policy ceiling, trusted helper identity and status integrity remain stable.
- Unix helper protocol and filesystem containment must remain functional.

## 5. Scope

### In scope

- Define a platform launcher contract handling spawn/setup/status/owned process and cancellation, with Unix adapter retaining current semantics.
- Specify required and obtained filesystem-read, filesystem-write, network, and process-tree facts (backward-compatible typed representation).
- Gate constrained-unavailable execution behind an explicit authorized escalation, not a mere warning.

### Explicitly out of scope

- Implementing token ACLs, Windows Jobs, firewall, GUI/IDE, WSL, or an MXC dependency.
- Replacing session/scheduler authority or revisiting durable runtime preferences.

## 6. Required production changes

### Core/domain
Review `SandboxLaunchSpec`, `SandboxExecutionPath`, `SandboxEnforcement`, `SandboxLaunchOutcome`; add an internal typed platform launch path and an enforceable capability predicate. Leave public protocol unchanged if possible, otherwise version explicitly.

### Runtime and concurrency
Split `prepare_launch_argv`/`run_inner` so Unix can retain helper-FD semantics while a Windows adapter later creates a suspended child with owned handles. Do **not** make a noop Windows adapter appear available. Keep existing spawn limits and cancellation.

### Security and authorization
Ensure `DegradedUncontained` cannot silently execute for a constrained profile; resolve through `ApprovalRouter` with explicit human escalation or fail with typed unavailable in non-interactive contexts. Do not route to FullHost from a model reviewer.

### Storage/protocol/frontends
No migration expected. If a status dimension is introduced, keep stable backward compatibility and update all consumers. Frontend only reflects requested/obtained state.

### Documentation/static guards
Update execution-ownership and sandbox static guards to prevent a second process owner or blind downgrade.

## 7. Ordered work packages

### WP-A — Define authority and guarantee matrix
Record exact requested profile → minimal obtained filesystem guarantees, network truth and process-supervision facts, including partial backend reporting. Acceptance: table-driven unit tests reject write-only token as full ReadOnly/WorkspaceWrite.

### WP-B — Introduce launch adapter seam
Encapsulate Unix helper + future Windows suspended-spawn behind one internal interface while retaining existing outputs/provenance. Acceptance: Unix containment tests remain unchanged and adapter handles are owned by `ManagedProcessService`.

### WP-C — Hard-deny unavailable containment
Ensure every constrained-unavailable path denies or explicitly escalates through the canonical router, including headless, automatic, and descendant runs. Acceptance: negative tests prove no uncontained process spawn when escalation absent.

### WP-D — Static ownership/doc guard
Extend `scripts/check_execution_ownership.py` and `scripts/check_sandbox_contract.py` as needed. Acceptance: guard self-tests catch bypass and unsupported-backend laundering.

## 8. Failure, cancellation, restart, and contention semantics

Spawn/setup failure is terminal and cannot trigger a direct-exec retry. Cancellation before admitted launch never spawns. During launch, mutable user mode changes affect subsequent snapshots only. A restarted daemon reprobes capability and never trusts stale 'Enforced'. Concurrent agents do not merge roots or re-use an earlier approval receipt.

## 9. Compatibility and migration

Preserve `SandboxLaunchSpec` and Unix private helper frame unless there is a compelling versioned addition. No DB migration unless enforcement DTO truly requires persistence. Annotate compatibility of the previously reported `DegradedUncontained` state without reclassifying historical events.

## 10. Required tests

- Focused unit: requested/obtained matrix; unavailable decision under interactive/automatic/yolo; FullHost explicit.
- Integration: Linux supported backend launches through canonical path; constrained unsupported denies without escalation; trusted helper failure terminal.
- Security negative: verifier rejects fabricated backend id, status frame, partial guarantee and stale capability.
- Regression: all prior Unix containment cases. Record tests that cannot execute on non-Linux hosts.

## 11. Required verification commands

`cargo test -p codegg --lib security::sandbox`
`cargo test -p codegg --test sandbox_policy_wiring`
`cargo test -p codegg --test sandbox_containment` (on a supported Unix host)
`python3 scripts/check_sandbox_contract.py`
`python3 scripts/check_sandbox_policy_wiring.py`
`python3 scripts/check_execution_ownership.py`
`cargo fmt --all -- --check`
`cargo clippy --workspace --all-targets -- -D warnings`
`bash scripts/verify.sh quick`

## 12. Documentation updates

`architecture/security.md`, `architecture/process-tool-execution-ownership.md`, `architecture/tool.md`, subsystem roadmap status and closure record.

## 13. Acceptance criteria

- Constrained launch cannot silently use `DegradedUncontained`; `FullHost` remains explicit.
- Per-dimension enforcement states retain truthful backend identity and no implicit network isolation.
- Linux/macOS helper passes existing behavioral probes.
- Windows remains 'unavailable' until a subsequent milestone proves real enforcement.

## 14. Stop conditions

Stop for any new daemon-level launcher, altered principal/approval semantics, unversioned incompatible status change, or failed Unix containment tests that cannot be explained. Record a corrective ADR rather than bypassing.

## 15. Closure evidence required

Source diff, ownership/static guard results, approval/profile matrix, Unix live observed tests with host/ABI and any skips, noninteractive negative denial trace, closure decision and downstream M002 unblock audit.

## 16. Handoff notes

Do not mark Windows sandbox supported in this milestone. Integration files: `src/managed_process.rs`, `src/security/sandbox.rs`, `src/security/sandbox/backend.rs`; root crate has `#![deny(unsafe_code)]` so Win32 unsafe must be scoped and reviewed. Follow planning conventions on status transition after actual implementation.
