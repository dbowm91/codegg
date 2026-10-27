# Agent Worktree Placement and Build Affinity Corrective Addendum

Status: **active planning** — A001 and A003 ready; later milestones blocked
Repository baseline: `80171f45295bebb9ab0b521262c0b295acb93074` (main, 2026-10-27)
Source of truth: `plans/registry.md`

Long-term references: `plans/000-long-term-specification.md` §§9, 16–18, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/002-long-term-roadmap.md`.
Accepted authorities: ADR-0002 (run/worktree leases), ADR-0032 (coordination), ADR-0033 (client cwd), ADR-0047 (restricted shared writer), ADR-0048 (contained creation). Proposed ADR-0055 covers **main-agent session-relative cwd**, not delegated placement.
Inputs (closed, not reopened): agent-run-worktree-concurrency M003/M004/M005/M009; agent-workstream-blackboard M005/M006/M007/M008/M010/M011; workspace-authority W001.

## 1. Purpose and ownership boundary
Make agent placement explicit and correct while reducing redundant Rust builds. `WorktreeService` continues to own worktrees and generation-fenced leases; `AgentRun`/groups own parent/child lineage and group checkout; `JobScheduler` owns build admission; `SessionLaunchService` owns session checkout launch; the TUI and agent tool supply intent, **not lease authority**. Distinguish separate-worktree parallelism from shared-checkout write mediation and compiler-resource economics.

## 2. Work classification
- **Invariant:** an explicit isolated request must never silently run in an inherited root; existing checkout placement must not borrow another run's write lease; no daemon process cwd; no path/identity-based authority escalation.
- **Capability:** optionally target an existing *managed* checkout for authorized read-only inspection, with a typed refusal for unsupported writable sharing; preserve existing opt-in group-owned `serialized_shared`.
- **Infrastructure:** repository-scoped build admission tied to canonical repository identity; bounded telemetry.
- **Polish/performance:** optional `sccache` study and explicit orchestration guidance after evidence; no unmeasured automatic placement.

## 3. Non-goals
No unrestricted co-located shell/build writers; no arbitrary manual-checkout adoption; no implicit lease transfer; no unconditional shared `CARGO_TARGET_DIR`; no automatic parent merge; no fresh multi-tenant authority model, OS sandbox bypass or daemon `chdir`; no live-model speed claims without trials. WorkOrder shared mutation stays fail-closed under its own contract.

## 4. Evidence at baseline
- `crates/codegg-core/src/run_placement.rs` declares `Auto | IsolatedNew | SerializedShared { group_id }`. `src/tool/task.rs` exposes group-level and per-child placement. `src/scheduler/executors.rs` enters checkout allocation on `needs_isolation || wants_shared_checkout`, apparently ignoring `IsolatedNew` for otherwise read-only children; separately, a missing Git repository may run an Auto child in place. Verify by a failing real scheduler test before correcting.
- `src/agent/worker.rs::requires_isolation` classifies write-capable child tools. `WorktreeService` owns checkout creation and releases, with explicit integration; `SessionLaunchPlacement::ExistingManagedWorktree` is session-only, not a delegated-run option.
- `SerializedShared` owns a **group** checkout and denies process-capable tools; a permit serializes CodeGG-mediated final writers. Its scope must **not** be expanded to unrestricted Cargo processes under this plan.
- `src/scheduler/config.rs` default `max_concurrent_builds_per_workspace = 1` only scopes eligible build-class scheduler jobs to a `WorkspaceId`; arbitrary Bash commands are not automatically classified; separate linked worktrees remain independently admitted.
- M008's opt-in synthetic Cargo contention measured 1,369 ms serial vs 1,373 ms concurrent with two lock waits in concurrent arm, **not** a live benchmark or proof of cross-worktree cache economics. Cargo's documented `target-dir` defaults to workspace root; its build-cache guidance names `sccache` for shared dependencies across workspaces (https://doc.rust-lang.org/cargo/reference/build-cache.html).

## 5. Target architecture
Preserve `Auto` as backwards-compatible. `IsolatedNew` is an **unconditional** isolated-checkout request, and fails if no qualified managed checkout is creatable. A separate explicit-existing placement may target an authorized managed checkout **read-only**; any mutating shared execution must continue through `SerializedShared` and its restrictive profile, not a new alias. Build admission is layered: existing per-workspace caps plus configurable repository-scoped resource budgets with accurately classified jobs, and coarse observable outcomes. Cache reuse is optional, measured, and trust-partitioned.

## 6. Dependency graph
A001 explicit-isolation semantics (ready)
  └─ A002 explicit existing managed checkout, read-only (blocked on A001 closure)
A003 repository-scoped build admission (ready, independent)
  └─ A004 optional cache economics pilot (blocked on A003 measurement)
A002 + A003 (+ A004 decision evidence) → A005 conservative placement/affinity advisor (blocked)
W002 (other roadmap) → W004 (blocked additionally on proposed ADR-0055 acceptance); neither is required by A001 or A003.

## 7. Milestones and boundaries

| ID | Class | Objective | Dependency | Exit gate |
|---|---|---|---|---|
| A001 | invariant | Honor explicit `isolated_new` even for read-only profiles; fail closed without qualified repository/worktree; preserve `auto` and `serialized_shared`. | Closed M004/C008 | Real scheduler preparation and model-tool parity tests; no fallthrough |
| A002 | capability | Typed existing-managed-worktree selection for authorized read-only delegated inspection only; reject occupied/foreign/unmanaged/mutating intents. | A001 closed | End-to-end spawn_many → daemon selector → read-only tool root; no extra worktree or new write authority |
| A003 | infrastructure | Add canonical repository-scoped quotas/metrics for supported build jobs across sibling worktrees, with opt-in policy/default unchanged. | Closed M007 | Real scheduler admitted/waiting/cancel/restart tests across two workspaces in same repo |
| A004 | polish/performance | Comparative Rust build cost study and opt-in cache candidate, after measured inputs; no global writable target reuse. | A003 closed | Published wall/CPU/disk/cache measurements including negative cases; rollback to isolated target dirs |
| A005 | capability/polish | Explicit conservative orchestrator strategy selection and reasoned recommendation; no inference of shared authorization. | A002 + A003, A004 evidence/decision | Real-model before/after multi-language and Rust workloads with failure/collision and cost accounting |

## 8. Cross-cutting contract
Storage: only additive versioned placement/config/status records; old payloads decode `Auto`. Protocol: placement intent typed, no model-supplied paths as identities. Authorization: same project/repository/principal, generation and lease checks; no operation on external worktrees. Concurrency: lease and group lifetime retained, queued admission holds no CPU/process permits, cancellation cannot release another job's budget. Observability: bounded, authorized placement choice/refusal, job waiting reasons, build resource and disk evidence; never leak command args, env, paths or cross-tenant cache material. UX: distinguish root checkout, managed existing checkout, isolated new and group-shared restricted checkout; display host-unavailable explicitly.

## 9. Verification strategy
Use baseline regression tests and real scheduler/worktree service fixtures; add two-worktree same-repo builds and separate-repo control; Linux/macOS and unavailable-host negative tests; cancellation/restart/dirty checkout, symlink, stale lease and project/principal mismatch. Run `scripts/verify.sh quick`, workspace Clippy and family-accurate focused tests. M008 economics is synthetic; mark every new measurement's denominator, host, confounders and missing live-model claims.

## 10. Risks and decisions
The existing shared profile **cannot run arbitrary process-capable tools**, so a "three Rust writers one checkout" strategy is not currently safe. A002 does not weaken that guard. The proposed ADR-0055 affects *main-agent* directory defaults only. A004 must decide cache isolation and portability from measured evidence rather than assuming cross-worktree Cargo target sharing is safe. A005 is advisory until scored against realistic task success and end-to-end wall time.

## 11. Completion
Every ready milestone receives separate `plans/closure/agent-worktree-placement-and-build-affinity/NNN-status.md` evidence and registry transition, blocked milestones become ready only after closure/decision gates. No performance promise before paired trials.

## 12. Milestone status
| Milestone | Status | Plan | Blocker |
|---|---|---|---|
| A001 | ready | `plans/implementation/agent-worktree-placement-and-build-affinity/001-explicit-isolated-placement-contract.md` | none |
| A002 | blocked | to author after A001 | A001 strict closure |
| A003 | ready | `plans/implementation/agent-worktree-placement-and-build-affinity/003-repository-scoped-build-admission.md` | none |
| A004 | blocked | to author after A003 | A003 evidence and cache decision |
| A005 | blocked | to author after A002/A003 | A002+A003 and economics evidence |
