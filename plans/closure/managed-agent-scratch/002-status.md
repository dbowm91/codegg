# Managed Agent Scratch M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/managed-agent-scratch/002-delegated-shared-scratch-and-job-leases.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`

Accepted implementation head: `2e0aa549233dff1ef0caeb759690738161e7f80d`

Implementation commit:

- `2e0aa549233dff1ef0caeb759690738161e7f80d` — durable private and shared scratch scope, direct-parent grants, scheduler attempt leases, tool/process wiring, tests and documentation.

## 1. Executive finding

M002 is closed. Each accepted durable child run receives a private scratch root, and a parent can create a session/run-owned shared resource and grant it to selected direct children by opaque `ScratchId`. The accepted run stores an immutable grant snapshot. Structured tools and Required sandbox writes receive only the child’s private root and explicit shared roots. Standard temp environment variables remain private to the child. Scheduler attempts acquire private/shared leases before child dispatch and retain them until the scheduled child returns; revoke prevents later lease admission while an already admitted attempt retains its lease. Revoked shared directories are reclaimed only after durable revocation and absence of active leases are established.

The permission correction from M001 is preserved: validated structured writes within the workspace, the current session scratch, or an explicitly granted shared scratch root do not inherit the implicit default Ask. Explicit Ask/Deny rules remain authoritative, and writes outside these authorized roots keep the existing decision behavior. There is no raw-path or model-argument route to create a grant.

macOS Required sandbox containment was exercised on the available x86_64 host. Some host sandbox backends allow broader reads for compatibility; documentation claims OS write containment for authorized roots and does not claim OS-level read confidentiality where the backend does not enforce it. Linux and Windows native runtime qualification remains for M003.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed opaque shared resource identity and immutable accepted grant snapshot | `ScratchId`; `AgentRunRecord.shared_scratch`; `spawn_request_fingerprint`; M002 TaskTool admission integration | pass | Identical retries preserve the same run/job and conflicting grant changes conflict with the stored fingerprint. |
| Parent ownership, session boundary and direct-child checks | `agent_run::tests::{shared_scratch_requires_direct_owner_grant_and_holds_attempt_lease,shared_scratch_rejects_alias_collision_and_non_owner_grant}` and SQLite parity test | pass | Unknown/foreign/non-owner grants and alias collisions fail; only the resource owner’s direct child is admitted. |
| Private run roots and explicitly granted shared roots in structured tools | `tool::write::delegated_scratch_tests::only_explicitly_granted_child_tools_can_read_or_write_shared_scratch` | pass | One child writes, a separately granted child reads, and an ungranted child cannot read or write the same path. |
| Required sandbox grants selected roots and rejects an ungranted sibling | `cargo test -p codegg --test sandbox_containment delegated_shared_scratch_is_explicitly_granted_to_the_contained_child --locked` | pass | Native macOS Seatbelt host. The child writes its private and explicit shared roots; the ungranted sibling write is denied. |
| Standard temp variables always identify private scratch; shared aliases are explicit | `managed_process::tests::delegated_scratch_keeps_temp_private_and_exposes_only_granted_aliases` | pass | `TMPDIR`, `TMP`, `TEMP`, and `CODEGG_SCRATCH_DIR` remain private; only a bounded granted alias is exported. |
| Real TaskTool spawn is durably admitted with the grant snapshot | `tests/managed_scratch_job_admission.rs::task_spawn_persists_explicit_scratch_grant_in_admitted_job` | pass | Calls `scratch_create`, then real TaskTool `spawn` through `JobSubmissionService`; verifies the run snapshot and persisted `SubagentRun` job identity. |
| Attempt leases protect admitted shared artifacts across revoke until release | `agent_run::tests::{shared_scratch_requires_direct_owner_grant_and_holds_attempt_lease,sqlite_shared_scratch_grants_are_direct_and_attempt_leased}` | pass | An admitted lease remains after revoke, new admission is refused, and final lease release removes the lease. Scheduler executor holds acquisition across `send_and_wait_scheduled`. |
| Durable lease reconciliation and storage migration | `STORAGE_LAYOUT_VERSION` 80; `migrate_v80`; full core suite | pass | Migration v80 adds immutable grants/resources/attempt leases and safely handles the existing migration replay fixtures. |
| Existing permission precedence | `ToolBatch::check_trusted_root_write` and M001 `trusted_root_write_relaxes_only_the_default_ask` coverage | pass | This milestone adds explicit authorized roots to the trusted-root set without changing explicit decisions or hard denies. |
| Documentation and guards | Updated agent/jobs/process ownership/security docs, tools guidance and agent skill; quick/static verification | pass | Broader native read semantics are described accurately. |

## 3. Authority and lifecycle provenance

| Operation | Authority source | Durable evidence / lifetime |
|---|---|---|
| Create shared resource | Host-owned `TaskTool` orchestration owner and `AgentRunStore`; session owner for a top-level turn or the current durable run for a delegated parent | Resource row stores `ScratchId`, session, owner, alias, idempotency key and revoke state. Model input supplies only the bounded alias and retry key. |
| Grant to child | Parent-owned `TaskTool` spawn request naming an existing `ScratchId`; store verifies session and exact direct parent | The accepted child `AgentRunRecord.shared_scratch` is immutable and participates in the spawn fingerprint. Paths are resolved by host code only after acceptance. |
| Access through tools/sandbox | Host builds `AgentScratchScope` from the persisted run and checked resources | Structured tools validate against workspace/private/explicit shared roots; Required sandbox receives the same authorized write roots. A path string or prompt text conveys no authority. |
| Attempt lease | `SubagentJobExecutor` after scheduler attempt identity exists and before dispatching scheduled child work | Durable lease is keyed by resource/job/attempt/run. It is released after the child worker returns; revoked resources are reclaimed after no active lease remains. |
| Restart reconciliation | Scheduler startup recovery after canonical `JobStore` attempt recovery | SQLite drops scratch leases whose canonical job attempt is not created/admitted/running. The filesystem manifest is not used as proof of liveness. |

## 4. Verification executed

### Commands run

```bash
rtk scripts/verify.sh quick
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 cargo test -p codegg-core --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 cargo test -p codegg --test sandbox_containment delegated_shared_scratch_is_explicitly_granted_to_the_contained_child --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 cargo test -p codegg --lib delegated_scratch --locked
rtk env MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 cargo test -p codegg --test managed_scratch_job_admission --locked
rtk env CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
rtk python3 scripts/check_sandbox_policy_wiring.py
rtk python3 scripts/check_daemon_cwd_usage.py
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_scheduler_bypass.py
rtk cargo fmt --all -- --check
rtk git diff --check
```

### Results

- `scripts/verify.sh quick`: passed, including configured static guards and workspace all-target locked check.
- `cargo test -p codegg-core --locked`: 861 unit tests and all core integration/doc suites passed. This includes shared-resource memory/SQLite tests and migration compatibility.
- Required sandbox integration: 1 passed. Root delegated structured-tool and process-env tests: 2 passed.
- TaskTool → JobSubmissionService admission integration: 1 passed; persisted child run and job carry the same explicit scratch grant.
- Workspace Clippy `--all-targets --locked -- -D warnings`: passed.
- Core-boundary, execution-ownership, scheduler-bypass, sandbox-policy wiring, daemon-cwd, formatting and diff checks: passed.
- The test host was x86_64 macOS with the native Seatbelt helper. Linux/Windows were not available for native execution and remain unqualified.

## 5. Failure, cancellation, restart and contention review

Resource creation is idempotent by host invocation identity. Alias collisions, normalized environment-name collisions, repeated IDs and oversized grant lists are rejected. Existing durable grant snapshots cannot be widened by a retry with changed inputs. The scheduler acquires leases against the canonical `JobId`/`AttemptId` before dispatch; cancellation/revocation blocks future admission and does not delete scratch under an already-admitted worker. A lease release is followed by a revoked-resource/no-leases check before reclaim. On restart, reconciliation trusts canonical job-attempt state rather than a timestamp, PID, caller-supplied path, or on-disk manifest.

Tests cover store-level admission, revoke, rejection of later admission, release and SQLite persistence. The TaskTool integration proves durable scheduler admission with the accepted snapshot. It does not make a live provider/model call; the supervised work lifetime itself remains owned by the existing scheduler/managed-process path. M003 will extend lifecycle qualification and safe cleanup/race evidence without changing M002’s authorization contract.

## 6. Migration and compatibility review

Storage layout version is 80. Migration v80 adds a default-empty `shared_scratch_json` to durable runs and creates the resource and attempt-lease tables. Legacy spawns deserialize as private-only. The migration checks table/column presence so legacy partial migration fixtures and replayed schema-version markers remain safe. No external daemon protocol field or raw path field was added. Existing run/worktree ownership and job payload identity are preserved.

## 7. Security review

- Scratch resources are named by generated `ScratchId`; model-visible input cannot choose storage paths.
- Shared authority is explicit, session-bound and direct-parent-bound; shared grants are immutable per accepted child run.
- Structured reads/writes fail for ungranted shared paths. Required sandbox writes are narrowed to the same effective roots, with no broad temp, home, sibling, or target-cache grant.
- Standard temp variables never point at shared roots. Aliases are bounded and normalized before environment construction.
- Tool path validation retains symlink/traversal protections; this milestone does not claim same-UID isolation for unsandboxed FullHost children.
- Workspace/private/shared structured writes use the M001 implicit-Ask correction while explicit Ask/Deny and hard denies remain in force.

## 8. Unresolved findings and qualification limits

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Linux and Windows native runtime and Required-sandbox execution were unavailable on this host. | No Linux/Windows native containment claim follows from M002. | M003 records host-specific availability and keeps unsupported backends fail-closed. |
| Low | No live provider-backed scheduled child or full daemon restart was launched. | End-to-end model/runtime timing is inferred from the real TaskTool admission test, durable store tests, and production scheduler executor boundary. | M003 adds lifecycle/restart qualification; do not claim provider-backed evidence from this closure. |
| Informational | Some host sandbox implementations have broader read allowances than the write grants. | Shared scratch is not claimed as OS-level read confidentiality on those backends. | Keep security docs explicit; no grant widening is implied. |

No correctness blocker remains in the M002 direct-parent grant and attempt-lease contract. Safe garbage collection under hostile filesystem races and actionable permission recovery are intentionally M003 scope.

## 9. Roadmap and downstream disposition

M002 is strictly closed at implementation head `2e0aa549233dff1ef0caeb759690738161e7f80d`. Registry audit found M003’s only hard dependency was M002 strict closure; it is now eligible and has been moved from blocked to active with the M002 accepted head as its baseline. No other registered managed-agent-scratch plan was newly eligible, and no unrelated subsystem status changed.

## 10. Planning records updated

- Implementation plan M002 now points to this closure record.
- The managed-agent-scratch roadmap records M002 closed and M003 active.
- `plans/registry.md` records the accepted M002 implementation and M003 unblock disposition.

### Post-rebase schema numbering correction (2026-10-19)

The original branch closure recorded the scratch tables as migration v80 and
`STORAGE_LAYOUT_VERSION` 80. Current `main` had since assigned migrations
v80–v82 to durable prompt-queue storage. During PR #128 reconciliation, the
scratch migration was moved additively to v83 and `STORAGE_LAYOUT_VERSION` is
now 83. The migration contents and private/shared scratch behavior are
unchanged; this correction preserves both histories without reusing a schema
version.
