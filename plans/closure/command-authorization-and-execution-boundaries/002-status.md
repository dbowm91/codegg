# Command Authorization M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/command-authorization-and-execution-boundaries/002-approval-preflight-and-routing-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md#6-dependency-graph`

Repository baseline reviewed: `b01ee2f7`; implementation reviewed at `b415efad`.

Implementation commit:

- `b415efad` — carry accepted authorization state through broker attempts, Bash dispatch and canonical process/Git owners; refuse constrained routes that cannot prove the accepted sandbox.

## 1. Executive finding

M002 is complete. An accepted agent tool call now has one immutable
`ExecutionAuthorizationContext` containing the captured `ExecutionPolicySnapshot`,
permission receipt, and bounded command-preflight result. The agent loop passes
that context through `ToolExecutionContext`, `BrokerInvocationContext`, and each
broker attempt. Retries may receive a reduced timeout budget but reuse the same
snapshot and receipt.

Command preflight runs before approval. Bash reuses its accepted summary rather
than scanning after approval. The actual Bash profile must match the accepted
snapshot; an unavailable constrained profile, a disabled managed-process
sandbox, or an active route that cannot prove constrained enforcement returns a
typed refusal before child execution. Typed Git mutation paths retain the
accepted context while Git's own capability policy remains authoritative.

Active command routing is available only for an explicitly accepted `FullHost`
snapshot. A constrained snapshot uses the raw managed route under the accepted
Landlock policy, and missing filesystem enforcement fails closed. This preserves
the distinction between user approval and obtained OS enforcement. Landlock
continues to report networking as unrestricted; this milestone adds no network,
IPC, or process isolation claim.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| One immutable snapshot and receipt across broker retries | `tool::broker::tests::retry_attempt_contexts_preserve_one_authorization_snapshot_and_receipt` | pass | Attempt timeout changes; authorization context equality does not. |
| Preflight is evaluated before approval and not repeated by production Bash | `src/agent/tool_batch.rs`, `src/tool/bash.rs`; `ToolPermissionOutcome` carries checked/clean separately from unchecked | pass | Direct legacy Bash calls without accepted context retain tool-local preflight. |
| Explicit Deny, Yolo, Automatic, and captured-snapshot precedence remain intact | `cargo test -p codegg --test approval_router -- --test-threads=1` | pass | 17/17; includes Deny and mode-change races. |
| Constrained authority cannot route through FullHost Bash configuration | `tool::bash::tests::accepted_constrained_profile_refuses_full_host_bash_configuration` | pass | The attempted marker remains absent. |
| Active command routes cannot be selected under a constrained snapshot | `tool::bash::process::tests::active_routes_require_an_explicit_full_host_snapshot` | pass | Legacy no-context callers remain compatible; production constrained calls refuse this route. |
| Managed process cannot spawn a constrained call with sandbox disabled | `managed_process::tests::accepted_constrained_context_refuses_disabled_process_sandbox_before_spawn` | pass | The marker remains absent. |
| Required sandbox behavior and profile wiring remain truthful | `cargo test -p codegg --test sandbox_policy_wiring` | pass | 23/23, including unsupported-host refusal and FullHost distinction. |
| Process owner, authorization matrix, audit coverage, Git policy, and approval boundaries remain guarded | `check_execution_ownership.py`, `check_authorization_matrix.py`, `check_audit_coverage.py`, `check_git_forbidden_patterns.py`, `check_approval_router.py`, `check_sandbox_policy_wiring.py` | pass | All guards passed. |
| Canonical repository quick verification | `scripts/verify.sh quick` | pass | Includes workspace all-target check with lockfile. |

## 3. Production implementation evidence

- Added `src/tool/authorization.rs` with the typed snapshot, receipt, and
  bounded command-preflight result.
- The agent permission outcome retains the preflight result through human,
  Automatic, and Yolo resolution. A clean result is distinguishable from a
  scanner that was not configured.
- `ToolBroker` derives each attempt context from the same broker invocation;
  it no longer drops accepted authorization metadata when constructing the
  execution context.
- `BashTool` consumes the pre-approval preflight result, verifies its
  configured sandbox profile against the accepted snapshot, and refuses when
  required filesystem enforcement is unavailable.
- `ManagedProcessRequest` carries accepted authorization locally, and
  `ManagedProcessService` rejects a disabled sandbox for a constrained request
  before spawn. Typed Git mutation and recovery executors carry the accepted
  context from both direct and Bash-routed calls.
- Active command routing has an explicit ceiling check. Under the current
  implementation, constrained Bash calls use the raw managed route; active
  Test/Python/Git/native routes are eligible only under explicit FullHost.
- Updated `architecture/permission.md`, `architecture/tool.md`, and the
  `permission` and `tool-execution` skills.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo check -p codegg --tests
cargo test -p codegg --lib retry_attempt_contexts_preserve_one_authorization_snapshot_and_receipt
cargo test -p codegg --lib accepted_constrained_profile_refuses_full_host_bash_configuration
cargo test -p codegg --lib accepted_constrained_context_refuses_disabled_process_sandbox_before_spawn
cargo test -p codegg --lib active_routes_require_an_explicit_full_host_snapshot
cargo test -p codegg --test approval_router -- --test-threads=1
cargo test -p codegg --test sandbox_policy_wiring
python3 scripts/check_approval_router.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_audit_coverage.py
scripts/verify.sh quick
```

### Results

- Focused retry, route-ceiling, Bash mismatch, and managed-process no-spawn
  regressions passed.
- Approval router: 17 passed; sandbox policy wiring: 23 passed.
- All six static guards passed.
- `scripts/verify.sh quick` passed, including `cargo check --workspace
  --all-targets --locked`.
- Verification was local on the Linux host. No hosted CI result is claimed.

## 5. Invariant review

- Explicit permission/security Deny and child ceilings remain terminal; Yolo
  resolves only escalations, and Automatic continues to use the bounded
  reviewer/fallback behavior.
- The preflight risk result remains advisory and is not treated as a resource
  capability. Typed Git policy and real executor refusals remain separate.
- Retrying does not recapture a newer approval or sandbox snapshot.
- A constrained request cannot become FullHost through a stale registry,
  active-route selection, or a disabled managed-process sandbox.
- The selected FullHost profile remains an explicit operator choice. No
  command string selects it.
- No network or IPC isolation is inferred from filesystem Landlock evidence.

## 6. Failure and recovery review

Profile mismatch, unavailable required enforcement, invalid authority, and
disabled process sandbox return before child spawn. Existing broker retry,
cancellation, process termination, and scheduler ownership paths remain in
place. Accepted receipts are in-memory invocation state and are not replayed by
this change. No permission-store or job-store state was migrated.

## 7. Migration and compatibility review

No database, permission-store, TUI, daemon, or ACP schema migration was added.
New context fields are optional for legacy direct callers. Agent production
calls populate the context after approval. Legacy unbrokered Bash behavior is
retained and uses its local preflight service.

## 8. Security review

The context is trusted only when built by the agent permission boundary and
threaded through the broker. Model command text cannot construct or mutate the
snapshot or receipt. Preflight summaries are bounded; raw command bodies are
not added to the new context. Constrained launches fail closed when the
registered backend cannot satisfy the accepted profile. FullHost remains
uncontained by CodeGG and is not represented as a sandbox.

## 9. Documentation and operations

Updated `architecture/permission.md`, `architecture/tool.md`,
`.opencode/skills/permission/SKILL.md`, and
`.opencode/skills/tool-execution/SKILL.md`. The sandbox wiring, process
ownership, approval-router, authorization-matrix, audit-coverage, and Git
forbidden-pattern guards remain passing.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Structural execution events do not yet join the accepted receipt, risk signal, route, and obtained-enforcement facts into one operator-facing record | Operators can see existing route/effect events and typed tool results, but not a single correlated authorization trace | M003 qualifies operator explanation, route provenance, and retained audit without logging scripts or secrets. |
| medium | Network and IPC effects remain unrestricted/unqualified under Landlock-only operation | Filesystem denial does not establish network, IPC, or inherited-descriptor isolation | Keep those dimensions explicitly unrestricted until a backend and native evidence support stronger claims. |

## 11. Roadmap disposition

M002 is closed. M003 is unblocked to qualify effect equivalence and operator
observability. M003 must preserve the Linux limitations recorded above and may
not turn classifier misses into authority changes.

## 12. Registry updates

- Marked M002 closed and added this closure record.
- Promoted M003 from blocked to active, with native Linux evidence available
  for filesystem effects and network/IPC remaining explicitly unqualified.
