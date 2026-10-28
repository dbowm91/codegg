# Agent Worktree Placement and Build Affinity Corrective Addendum

Status: **closed** — A001/A002/A003 strictly closed; A004 and A005 closed no-go
Repository planning baseline: `d4241f08dcfede0534df3ea08c83d762fa1c55f7` (current plan branch at A002/A004 handoff). Earlier A001/A003 baseline `80171f45295bebb9ab0b521262c0b295acb93074` is historical.
Source of truth: `plans/registry.md`

Long-term references: `plans/000-long-term-specification.md` §§9, 16–18, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/002-long-term-roadmap.md`.
Accepted authorities: ADR-0002 (run/worktree leases), ADR-0032 (coordination), ADR-0033 (client cwd), ADR-0047 (restricted shared writer), ADR-0048 (contained creation). Accepted ADR-0055 covers **main-agent session-relative cwd** (W004 closed), not delegated placement.
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

## 4. Implementation evidence and remaining gaps (baseline review; see current milestone status below)
- `crates/codegg-core/src/run_placement.rs` declares `Auto | IsolatedNew | SerializedShared { group_id }`. `src/tool/task.rs` exposes group-level and per-child placement. `src/scheduler/executors.rs` now honors `IsolatedNew` for read-only children and explicitly refuses missing repository/contained creator; A001 hosted Linux contained creator + live scheduler qualification is strictly closed. A missing Git repository can still run an **Auto** child in place by compatibility; explicit isolated is fail-closed.
- `src/agent/worker.rs::requires_isolation` classifies write-capable child tools. `WorktreeService` owns checkout creation and releases, with explicit integration; `SessionLaunchPlacement::ExistingManagedWorktree` is session-only, not a delegated-run option.
- `SerializedShared` owns a **group** checkout and denies process-capable tools; a permit serializes CodeGG-mediated final writers. Its scope must **not** be expanded to unrestricted Cargo processes under this plan.
- A003 extends `src/scheduler/submission.rs` and typed scheduler quotas with optional `max_concurrent_builds_per_repository` for Build/Format jobs, bound from canonical `RepositoryId` (default disabled). Arbitrary Bash commands are **not** automatically classified. Hosted production Cargo comparison of separate targets: siblings quota-off 331 ms, quota-one 486 ms, unrelated quota-one 277 ms, with zero build-directory lock waits. This was a dependency-free, single-sample per-arm fixture and does not establish cache economics.
- M008's opt-in synthetic Cargo contention measured 1,369 ms serial vs 1,373 ms concurrent with two lock waits in concurrent arm, **not** a live benchmark or proof of cross-worktree cache economics. Cargo's documented `target-dir` defaults to workspace root; its build-cache guidance names `sccache` for shared dependencies across workspaces (https://doc.rust-lang.org/cargo/reference/build-cache.html).

## 5. Target architecture
Preserve `Auto` as backwards-compatible. `IsolatedNew` is an **unconditional** isolated-checkout request, and fails if no qualified managed checkout is creatable. A separate explicit-existing placement may target an authorized managed checkout **read-only**; any mutating shared execution must continue through `SerializedShared` and its restrictive profile, not a new alias. Build admission is layered: existing per-workspace caps plus configurable repository-scoped resource budgets with accurately classified jobs, and coarse observable outcomes. Cache reuse is optional, measured, and trust-partitioned.

## 6. Dependency graph
A001 explicit-isolation semantics (closed)
  └─ A002 explicit existing managed checkout, read-only (closed)
A003 repository-scoped build admission (closed, independent)
  └─ A004 optional cache economics pilot (closed no-go; use A003's production measurements)
A002 + A003 (+ A004 decision evidence) → A005 conservative placement/affinity advisor (closed no-go after paired live qualification)
W002 and W004 in the workspace-authority roadmap are closed and independent of A001/A003.

## 7. Milestones and boundaries

| ID | Class | Objective | Dependency | Exit gate |
|---|---|---|---|---|
| A001 | invariant | Honor explicit `isolated_new` even for read-only profiles; fail closed without qualified repository/worktree; preserve `auto` and `serialized_shared`. | Closed M004/C008 | **Closed:** hosted Linux live-scheduler evidence creates two distinct contained checkouts; no fallthrough |
| A002 | capability | Typed existing-managed-worktree selection for authorized **exclusive read-only** delegated inspection; reject occupied/active-session-owned/dirty/foreign/unmanaged/mutating intents. | A001 strictly closed | **Closed:** local and exact-head hosted Linux production-scheduler success/refusal matrix passed; test-only creator means no native containment claim; see `plans/closure/agent-worktree-placement-and-build-affinity/002-status.md` |
| A003 | infrastructure | Add canonical repository-scoped quotas/metrics for supported build jobs across sibling worktrees, with opt-in policy/default unchanged. | Closed M007 | **Closed:** production scheduler Cargo runs cover quota off/on for sibling worktrees and quota-on unrelated repositories; reports include CPU, RSS, wall time, disk growth, and lock waits |
| A004 | polish/performance | Opt-in **measured** cross-worktree Cargo/sccache economics pilot with controlled cache/quota arms, realistic dependency graph and explicit go/no-go; no shipped cache change. | Closed no-go; A003 strictly closed | `plans/closure/agent-worktree-placement-and-build-affinity/004-status.md` records the scheduler pilot and no-go; keep isolated writable target directories |
| A005 | capability/polish | Explicit conservative orchestrator strategy selection and reasoned recommendation; no inference of shared authorization. | Closed no-go | `plans/closure/agent-worktree-placement-and-build-affinity/005-status.md` records the completed paired live evaluation: zero non-control task-verifier passes in either arm, no substantive correctness improvement, and a corrected offline recheck confirming all six unavailable-checkout controls held. Keep the deterministic guidance within its current authority boundary; no performance claim. |

## 8. Cross-cutting contract
Storage: only additive versioned placement/config/status records; old payloads decode `Auto`. Protocol: placement intent typed, no model-supplied paths as identities. Authorization: same project/repository/principal, generation and lease checks; no operation on external worktrees. Concurrency: lease and group lifetime retained, queued admission holds no CPU/process permits, cancellation cannot release another job's budget. Observability: bounded, authorized placement choice/refusal, job waiting reasons, build resource and disk evidence; never leak command args, env, paths or cross-tenant cache material. UX: distinguish root checkout, managed existing checkout, isolated new and group-shared restricted checkout; display host-unavailable explicitly.

## 9. Verification strategy
Use baseline regression tests and real scheduler/worktree service fixtures; add two-worktree same-repo builds and separate-repo control; Linux/macOS and unavailable-host negative tests; cancellation/restart/dirty checkout, symlink, stale lease and project/principal mismatch. Run `scripts/verify.sh quick`, workspace Clippy and family-accurate focused tests. M008 economics is synthetic; mark every new measurement's denominator, host, confounders and missing live-model claims.

## 10. Risks and decisions
The existing shared profile **cannot run arbitrary process-capable tools**, so a "three Rust writers one checkout" strategy is not currently safe. A002 does not weaken that guard. The accepted ADR-0055 affects *main-agent* directory defaults only. A004's measured no-go keeps writable Cargo targets isolated. A005 remains advisory after its no-go evaluation: it showed no correctness improvement and produced no basis for a performance claim.

### Remaining integration and design gate
A001/A003's **strict** hosted qualification and closure updates are on this planning branch, while `main` may still record their earlier *conditional* closure. Before merging a later A002/A004 implementation, reconcile the plan branch's closure records against current main and obtain exact resulting-head verification. This is a merge/qualification gate, **not** a new implementation milestone or permission to change the runtime.

A005's deterministic implementation and live qualification are closed as **no-go**. Its final comparison used 384 of 480 authorized provider requests, $0.182919 of the $10 cumulative OpenCode Go allowance, and 2,724.928 of 7,200 seconds. Both arms had zero verified task successes; the scoring contract found no substantive correctness improvement. A corrected offline recheck confirms all six saved unavailable-checkout controls held. The closure records the protected refusal control, provider allowance evidence, and unavailable ACP scheduler-wait/collision measures. A004's no-go decision is also closed; neither milestone authorizes cache integration or a performance claim.

## 11. Completion
Every ready milestone receives separate `plans/closure/agent-worktree-placement-and-build-affinity/NNN-status.md` evidence and registry transition, blocked milestones become ready only after closure/decision gates. No performance promise before paired trials.

## 12. Milestone status
| Milestone | Status | Plan | Blocker |
|---|---|---|---|
| A001 | closed | `plans/implementation/agent-worktree-placement-and-build-affinity/001-explicit-isolated-placement-contract.md` | Closure `plans/closure/agent-worktree-placement-and-build-affinity/001-status.md`; hosted Linux production contained-creator and live-scheduler success passed |
| A002 | closed | `plans/implementation/agent-worktree-placement-and-build-affinity/002-existing-managed-read-only-placement.md` | Closure `plans/closure/agent-worktree-placement-and-build-affinity/002-status.md`; exact-head hosted Linux run `40697779631` passed the production-scheduler success/refusal matrix |
| A003 | closed | `plans/implementation/agent-worktree-placement-and-build-affinity/003-repository-scoped-build-admission.md` | Closure `plans/closure/agent-worktree-placement-and-build-affinity/003-status.md`; hosted production Cargo comparison passed with resource, target-size, and lock-wait evidence |
| A004 | closed (no-go) | `plans/implementation/agent-worktree-placement-and-build-affinity/004-rust-cross-worktree-cache-economics-pilot.md` | Closure `plans/closure/agent-worktree-placement-and-build-affinity/004-status.md`; no Rust sccache hits and material time/CPU overhead in the measured scheduler pilot; any production cache proposal requires a separate plan |
| A005 | closed (no-go) | `plans/implementation/agent-worktree-placement-and-build-affinity/005-conservative-placement-advisor.md` | Closure `plans/closure/agent-worktree-placement-and-build-affinity/005-status.md`; three paired repetitions across six cases completed under the approved budget; zero non-control task-verifier passes, corrected 6/6 control holds, and no substantive correctness improvement. |
