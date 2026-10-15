# Agent Workstream Blackboard M001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-workstream-blackboard/001-work-order-workspace-policy-corrective.md`

Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md#milestones--deliverables`

Repository baseline reviewed: `73799dff1fc17cd3a3c7fb93b26d14af3df68733`

Implementation commits:

- `4bc6035ed10e42b3df8a9d865146fc546e4afad2` — preserve WorkOrder mutation capability and deny unguarded shared mutation

## 1. Executive finding

The unsafe WorkOrder classification is corrected. The materializer always submits the mutation-capable `build` agent; requested `Shared` placement no longer changes the mutation signal. Both `Shared` and `Serialized` mutation now resolve to `NeedsAttention` because the scheduler's former global key is neither checkout-scoped nor acquired by ordinary AgentTurn writers. No checkout is allocated, session is created, or AgentTurn is submitted after this attention result. Read-only sharing and AutoIsolated Git mutation remain available.

This closes the safety corrective by denying the unsupported capability. It does not authorize serialized shared mutation. M005 retains the bounded follow-up to establish a universal checkout-scoped writer reservation after its independent dependencies and ADR gate are satisfied.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reproduce Shared-as-read-only classification | Baseline `src/core/work_order_coordinator.rs` treated Shared as `is_mutation = false`; `resolve_workspace_action(Shared, _, false)` returned `ShareReadOnly`, while the submitted payload selected `build`. | pass | The defect was in the placement-to-authority inference, not the workspace policy parser. |
| Verify scheduler reservation bypass | `ResourceRequest::for_kind(AgentTurn)` has no exclusivity key; Build/Format used the global `exclusive:workspace-mutation` key. A unit regression pins the empty AgentTurn key list. | pass | The global key cannot prove checkout scope or universal writer admission. |
| Shared mutation cannot claim read-only or serialized safety | `workspace_policy_never_claims_non_git_isolation` now asserts Shared and Serialized mutation return `NeedsAttention`; Shared read-only returns `ShareReadOnly`. Materialization integration test checks the actionable writer-reservation diagnostic. | pass | No shared mutation executes while the common guard is absent. |
| AutoIsolated Git mutation remains isolated; non-Git mutation fails closed | Core WorkOrder test and M007 trajectory containment test. | pass | Managed worktree allocation remains unchanged for AutoIsolated Git jobs. |
| No worktree/session/job allocation on unsupported Shared mutation | Coordinator handles `NeedsAttention` before workspace linking, session creation, and `submit_initial_turn`; the materialization path regression passes. | pass | The attention state is durable and the occurrence is not launched. |
| Preserve release-gate, idempotency, restart, and permission behavior | Existing WorkOrder unit suite and full M002/M007 trajectory test selected below; no persistence, approval, sandbox, or lifecycle code was changed. | pass | Existing focused WorkOrder materialization and recovery coverage remains in the suite. |

## 3. Production implementation evidence

`src/core/work_order_coordinator.rs` now classifies the fixed `build` agent as mutation-capable regardless of requested workspace policy. `crates/codegg-core/src/work_order/coordinator.rs` keeps shared read-only placement but returns `NeedsAttention` for shared or serialized mutation. The submission path no longer attaches the global mutation key to a WorkOrder AgentTurn. Architecture notes in `architecture/worktree.md` and `architecture/scheduler.md` explain the enforced behavior and the limited meaning of the legacy key.

No schema, protocol, lease, scheduler, or authorization change was needed. This is an infrastructure/safety corrective: shared mutation remains unavailable until a common enforcement boundary exists.

## 4. Verification executed

### Commands run

Verification ran from a native arm64 shell with Rust 1.89 and an isolated target directory because the default login shell and existing `target/` contained mixed x86_64/arm64 artifacts.

```bash
arch -arm64 /bin/zsh -c 'export PATH=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin:$PATH CARGO_TARGET_DIR=/tmp/codegg-blackboard-arm64-target CARGO_BUILD_JOBS=2; cargo test --locked -p codegg-core work_order && cargo test --locked --test work_orders_m002_materialization non_git_mutation_and_sharing_follow_the_truthful_policy && cargo test --locked --test work_orders_m007_trajectory f_worktree_containment_gate_and_truthfulness && python3 scripts/check_scheduler_bypass.py && python3 scripts/check_execution_ownership.py'
arch -arm64 /bin/zsh -c 'export PATH=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin:$PATH CARGO_TARGET_DIR=/tmp/codegg-blackboard-arm64-target CARGO_BUILD_JOBS=2; cargo fmt --all -- --check && cargo clippy --workspace --all-targets --locked -- -D warnings && bash scripts/verify.sh quick'
arch -arm64 /bin/zsh -c 'export PATH=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin:$PATH CARGO_TARGET_DIR=/tmp/codegg-blackboard-arm64-target CARGO_BUILD_JOBS=2; bash scripts/verify.sh quick'
```

### Results

- `cargo test -p codegg-core work_order`: 44 passed, 0 failed.
- `work_orders_m002_materialization::non_git_mutation_and_sharing_follow_the_truthful_policy`: 1 passed.
- `work_orders_m007_trajectory::f_worktree_containment_gate_and_truthfulness`: 1 passed.
- Scheduler-bypass and execution-ownership guards passed.
- `cargo fmt --all -- --check` passed.
- `scripts/verify.sh quick` passed all guards and `cargo check --workspace --all-targets --locked`.
- Workspace Clippy did not complete: it failed on the pre-existing `clippy::explicit_auto_deref` warning at `crates/codegg-providers/src/provider_core.rs:2941` (`*secret` should be `secret`). The finding is outside the changed files and is recorded as low severity; no Clippy success is claimed.
- An initial attempt with the Homebrew x86_64 Rust 1.98 toolchain failed at link time against arm64 system libraries. The corrected native-arm64 Rust 1.89 runs above passed; no source change was made in response to the environment failure.

## 5. Invariant review

- Placement selection cannot reduce the effective authority of the configured WorkOrder agent.
- Shared mutation and unverified serialization do not execute.
- AutoIsolated remains the default for mutation-capable Git WorkOrders; no ad hoc copy is made for non-Git projects.
- Cross-principal authorization, approval, sandbox ceilings, worktree leases, and child execution ownership are unchanged.
- No global key is described as checkout isolation or universal writer exclusion.

## 6. Failure and recovery review

An unsupported Shared/Serialized mutation transitions to durable `NeedsAttention` before session/job creation. Duplicate wake and occurrence idempotency behavior is unchanged. No new lock or lease exists, so cancellation and daemon restart continue through the existing occurrence and managed-worktree recovery paths. AutoIsolated allocation failures continue to record attention without submitting the agent turn.

## 7. Migration and compatibility review

No database migration, protocol change, or new configuration field was introduced. Existing WorkOrders retain their stored policy. Their shared/serialized mutation occurrences now fail closed with an actionable diagnostic; operators can use `auto_isolated` for Git-backed mutation. Read-only shared work remains compatible.

## 8. Security review

The former placement-based mutation downgrade is removed. The former global scheduler key is not treated as an authorization boundary. No path, tool, approval, sandbox, project, or Git privilege is widened, and no shared mutation runs while arbitrary AgentTurn writers can bypass a common reservation.

## 9. Documentation and operations

- `architecture/worktree.md` records WorkOrder placement behavior and the fail-closed shared-write policy.
- `architecture/scheduler.md` documents that `exclusive:workspace-mutation` is global and is not acquired by all AgentTurn writers.
- `plans/registry.md` and the subsystem roadmap record the closure and dependency disposition.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Workspace Clippy is blocked by existing `explicit_auto_deref` in `codegg-providers/src/provider_core.rs:2941`. | The full workspace Clippy command is not green on this checkout. | Resolve in the owning provider workstream; unrelated to M001. |
| low | No common checkout-scoped reservation covers all arbitrary AgentTurn mutation surfaces. | Shared/Serialized mutation remains intentionally unavailable by design. | M005 may propose a guarded capability only after M004 closes and ADR-0032 is accepted; preserve fail-closed behavior unless every writer is covered. |

## 11. Roadmap disposition

M001 is closed as a safety corrective with shared mutation denied pending a universal writer guard. No M001 production behavior remains open. M005 remains blocked by M004 and explicit ADR-0032 acceptance; closure of M001 alone does not authorize shared mutation.

## 12. Registry updates

- M001 moved to recently closed with implementation commit `4bc6035ed10e42b3df8a9d865146fc546e4afad2`.
- M002 remained eligible and was moved to active as the requested next sequential plan.
- M003 remains blocked on strict M002 closure; M004 remains blocked on M003.
- M005 remains blocked on M004 and accepted ADR-0032. No other plan became newly eligible.
