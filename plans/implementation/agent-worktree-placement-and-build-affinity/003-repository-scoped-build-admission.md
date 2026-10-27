# Agent Worktree Placement and Build Affinity A003 — Repository-Scoped Build Admission and Evidence

Status: **conditionally closed** — closure `plans/closure/agent-worktree-placement-and-build-affinity/003-status.md`; production-executor Cargo comparison remains open
Repository baseline: `80171f45295bebb9ab0b521262c0b295acb93074`
Source roadmap: `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md` A003
Long-term: `plans/000-long-term-specification.md` §§17–18, 26–29; `plans/001-terminology-and-domain-model.md`.
Applicable ADRs: ADR-0002, ADR-0032, ADR-0047. Primary class: **infrastructure**.

## 1. Objective
Give the scheduler an **opt-in**, bounded concurrent-build budget shared by sibling worktrees of one canonical repository, while retaining existing per-workspace admission, fair cancellation and independence of unrelated repositories. Measure actual resource/wait cost and explicitly record which shell/build routes are recognized; do not pretend arbitrary uninspected Bash commands are fully governed.

## 2. Why ready
Blackboard M007 implemented typed keyed quotas, JobSubmissionService normalization, cancellation-safe admission and group status. `RepositoryId` and workspace→repository lineage are durable in project catalog. No new cache, worktree writer or model tool is required for this independent addition.

## 3. Current evidence
`src/scheduler/config.rs` defaults to one `max_concurrent_builds_per_workspace`; `src/scheduler/submission.rs` binds the typed build class to `WorkspaceId`, not repository. `architecture/build_admission.md` states arbitrary Bash is not automatically classified. `crates/codegg-core/src/jobs/quota.rs` / `src/scheduler/admission.rs` already have a typed weighted quota seam. Sibling worktrees may therefore run independent Cargo builds and duplicate dependency compilation despite workspace-level limits. The M008 synthetic same-target lock test is not evidence of a shared cache or cross-worktree admission cost.

## 4. Invariants
Repository scope comes from daemon-verified canonical lineage, **never** a model-provided path or string. A repo budget supplements, never replaces, the workspace quota. Unknown/unsupported jobs do not silently gain classification or escape existing safety. Waiting does not consume process/CPU permits; cancellation releases only what it held after process termination. Cross-project and cross-principal metrics must not reveal sensitive job metadata.

## 5. Scope
In: typed repository-scoped keyed quota, config/validation (disabled by default or explicitly conservative in a separately approved decision), scheduler policy derivation at submission, bounded status, representative Rust fixture measurements. Out: cache installation, shared writable target directory, interception of arbitrary external processes, automatic worktree selection, OS-global CPU governor, unsafe process execution on serialized_shared checkout.

## 6. Production changes
Core: extend typed quota namespace with a `RepositoryId` discriminator and policy; preserve serde/defaults and failure paths for unresolved lineage. Scheduler: per-workspace and per-repository slots acquired atomically, deterministic lock order / starvation bounds, structured waiting reason; use supported canonical inspected job kinds only, with an explicit accounting of `bash`/terminal/test/verify routes and unsupported cases. Config: optional max concurrent builds per repository (validation, 0 refusal, bounded integer). Projection: coarse authorized count/queued/active/terminal, no command/env/path leakage. Storage only if required by durable JobRecord representation, with migration evidence.

## 7. Work packages
A. Inventory genuine production build submissions (`test`, `verify`, typed scheduler jobs, inspectable `bash` routes) and measure baseline two-worktree same-repo contention, plus unrelated-repo control. B. Introduce typed repository quota and backward-compatible config. C. Bind verified repository ID at submission and enforce two dimensions atomically; do not create scheduler bypass or hold job permits while queued. D. Add fair waiting, cancellation/restart and group-visible coarse status. E. Publish measurements and explicit classification gaps; do not infer project identity from a relative checkout path.

## 8. Failure/cancellation/restart/contention
Unknown repository lineage fails closed for requested repository budget or takes an explicitly documented conservative fallback; never silently treats two siblings as unrelated because a path differs. Waiting requests canceled or timed out retain no permit; running cancellation waits for process death before release; restart does not report nonterminal work complete. Jobs in different repositories are independent except global machine budgets.

## 9. Compatibility
Missing new config field preserves historical default. Old serialized jobs decode; no behavior change until opt-in unless separate accepted operator policy. No new lease owner or permission surface. Preserve WorkOrder typed execution semantics.

## 10. Tests
Two sibling linked worktrees share one repository budget; unrelated repositories run concurrently; same workspace remains bounded; malformed/unknown identity and principal isolation; mixed eligible/unsupported Bash route; queue fairness and no held parent permits; cancellation before start and during execution; restart/recovery. Real `cargo test` + synthetic executor timing with 2+ worktrees, report CPU seconds, peak RSS, wall time, disk growth and observed Cargo lock waits.

## 11. Verification
```bash
cargo test -p codegg-core --lib jobs
cargo test --test scheduler_jobs -- build_quota_admission
cargo test --test scheduler_jobs -- scheduler_resource_profiles
cargo test --test scheduler_jobs -- scheduler_permit_lifecycle
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets -- -D warnings
```
Resolve actual family/member names and assert nonzero tests; do not claim commands executed now.

## 12. Documentation
`architecture/build_admission.md`, `architecture/scheduler.md`, `docs/configuration.md`, `docs/tools.md` — config, scope, unsupported routes, cancellation and telemetry.

## 13. Acceptance
Two checked build jobs in distinct worktrees of same repository respect configured aggregate bound; separate repos do not serialize; default config unchanged; a random Bash command is not misrepresented as governed; waiting/cancel/restart produce truthful status and usable measurements.

## 14. Stop
A policy needing arbitrary shell parsing or global interception to be truthful, guessed repo identity, inaccessible process terminality, cross-principal status leakage, silent cache sharing, or a deadlock from nested quotas.

## 15. Closure
Baseline measured fixture + post-change comparison (including negative cases), exact source and config, test matrix, status/authorization proofs, cancellation/restart evidence, quick/Clippy/CI separation. A004 remains blocked until this evidence exists.

## 16. Handoff
Prefer extending existing `keyed_quotas`/admission over a new build scheduler. This is **not** authorization to share target directories or install `sccache` automatically.
