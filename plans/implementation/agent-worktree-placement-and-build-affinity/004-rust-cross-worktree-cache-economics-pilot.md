# Agent Worktree Placement and Build Affinity A004 — Cross-Worktree Cargo Cache Economics Pilot

Status: ready for handoff
Repository baseline: `d4241f08dcfede0534df3ea08c83d762fa1c55f7` (`plan/worktree-affinity-cwd-build-admission-20261027`; confirm exact head before edits)

Source roadmap:
- `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md` §7 A004

Long-term requirements:
- `plans/000-long-term-specification.md` (resource predictability, tool execution and agent orchestration)
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md` (scheduling and agent workstreams)

Applicable accepted ADRs: ADR-0002, ADR-0032, ADR-0047, ADR-0048. Primary class: **polish/performance and experimental evidence**, not a shipped cache integration.

## 1. Objective
Determine, from reproducible **real Rust builds**, whether an opt-in `sccache`-style compilation cache offers enough wall-time, CPU and disk benefit across **separate managed worktrees** to justify a later production integration. Compare cache disabled/enabled under repository build admission disabled/enabled, leaving each checkout's writable Cargo target independent. Publish a **go/no-go decision** and explicit operating requirements; do not silently enable a new cache, shared `CARGO_TARGET_DIR`, OS service or dependency.

## 2. Why this milestone is ready
A003 is strictly closed on the branch and already measured production `JobSubmissionService → JobScheduler → ManagedArgvExecutor` Cargo jobs, with per-repository optional quotas, CPU/RSS/target bytes and captured Cargo lock waits. That A003 fixture intentionally had **no dependencies**, used fresh distinct targets and measured 331 ms (two siblings, quota off), 486 ms (siblings, quota one), 277 ms (unrelated repos, quota one), each **one fixture run**. Those observations prove admission and absence of shared-target locks in that small fixture, not cache benefits. A004 investigates the missing economic question independently of A002.

## 3. Current implementation evidence and external constraints
- `src/scheduler/submission.rs`, `src/scheduler/admission.rs`, `src/scheduler/executors.rs`, `src/scheduler/config.rs`: typed Build/Format admission and optional `scheduler.quota.max_concurrent_builds_per_repository`; arbitrary `bash`/terminal/external builds are not automatically classified.
- `architecture/build_admission.md` and `plans/closure/agent-worktree-placement-and-build-affinity/003-status.md`: previous production Cargo fixture and evidence baseline.
- Cargo's reference says target/build output defaults to workspace-local `target` and distinguishes shared external compiler caching from writable Cargo build output: https://doc.rust-lang.org/cargo/reference/build-cache.html .
- Mozilla sccache supports `RUSTC_WRAPPER` or Cargo `build.rustc-wrapper` with a local client/server, but not every `rustc` invocation is cacheable; procedural macros, build scripts/outputs, linkers and incremental compilation impose caveats: https://github.com/mozilla/sccache/blob/main/docs/Rust.md and https://github.com/mozilla/sccache/blob/main/README.md .
- A cache daemon may require a socket/loopback, process-spawn and a writable cache path. Those permissions are **not** implied by Codegg's sandbox, A003's admission, or a successful uncontrolled host-shell sample. Qualify the exact Codegg process executor and native host profile before any production recommendation.

## 4. Invariants
1. Independent worktree `CARGO_TARGET_DIR` and any `CARGO_BUILD_BUILD_DIR` remain separate; never share mutable incremental, build-script, target or fingerprint state across worktrees.
2. Only explicitly authorized experiment invocations alter `RUSTC_WRAPPER`, `CARGO_INCREMENTAL`, cache endpoint/storage or process environment. Respect pre-existing user Cargo config and wrapper (measure/report or skip incompatibility, never clobber silently). No automatic global `~/.cargo/config` modification.
3. A cache has **no authority to execute arbitrary source/build scripts** beyond the already-authorized Cargo process. Cache files, socket and retained outputs obey workload trust/secret boundaries, quota, lifecycle, sandbox and provenance policy. No remote cache upload, credential forwarding or cross-principal writable cache without separate decision.
4. Measurement captures actual total pipeline **wall time**, per-process CPU user/system, peak RSS, disk bytes/inodes (worktrees, targets, cache), cache hits/misses/non-cacheable count, Cargo lock waits and scheduling wait. Avoid using a result merely because `sccache --show-stats` printed a positive ratio.
5. Preserve scheduler default (repository limit disabled), normal tool execution authorization, process isolation, and the restricted group `serialized_shared` profile. If cache overhead exceeds savings or access is denied, report a negative outcome rather than bypass sandboxing.

## 5. Scope
**In:** reproducible opt-in benchmark harness/evidence + decision, isolated cache setup/teardown in an explicitly authorized test location, qualification under Codegg's real managed argv scheduler with two sibling worktrees, representative Rust dependencies, effects of compiler cache and quota.
**Out:** automatic user configuration, an unconditional `sccache` dependency, shared writable targets, long-lived cache service lifecycle in Codegg, interception of arbitrary Bash/external builds, distributed caches, remote credentialed stores, algorithmic auto-placement (A005), changing A003 defaults.

## 6. Required production changes
### Core/domain
No product-domain changes expected. Introduce a bounded opt-in **test/validation harness** (such as an ignored family integration test plus a `plans/validation/` measurement record) that invokes the **existing** typed Build job submission/actual `ManagedArgvExecutor`, preserving the real admission and containment path. A portable helper is fine, not a second scheduler.

### Storage and migrations
None expected. Experiment data lives in a validation report, not in production JobStore schema. If cache installation requires production state, stop and record a separate follow-up design; do not expand A004.

### Protocol and DTOs
None. All modes remain test configuration / offline experiment variants. No model-visible cache toggle or new permission grants.

### Runtime and concurrency
Create distinct linked worktrees at a frozen Git base with reproducible Cargo.lock, features, profile, `rustc`, `cargo`, OS and environment. Include a **dependency-heavy realistic** case (e.g. a bounded selection from Codegg's crate graph) and at least one fast minimal control; use warm/cold samples, shuffled order and multiple paired repetitions. Qualify the cache server/socket/storage in a test-only contained route; when unavailable, emit a supported host/backend refusal as evidence and run the uncontrolled comparison separately labelled. Keep `CARGO_TARGET_DIR` private per worktree, and scope test directories + cache cleanup; do not delete user targets.

### Frontend/operator surface
No shipped toggle. The published evaluation should offer an operator decision: keep disabled, proceed to a separately approved opt-in production milestone, or discard. Do not advertise a benefit unless the chosen workloads, denominators and variance support it.

### Security and authorization
Review `sccache`'s local server, socket binding, subprocesses and cache writes against `docs/execution-ownership.toml`, `docs/writer-disposition.toml` and sandbox enforcement. Partition cache by canonical trust/principal/toolchain/settings **if** production integration is later proposed; merely splitting by repository path is not enough for untrusted projects or secrets. Record what the experiment cannot prove.

### Documentation and static guards
Add a reproducible `plans/validation/agent-worktree-placement-and-build-affinity/` experimental protocol and resulting measurements, reference Cargo/sccache docs and qualify supported host/toolchain. An opt-in test must not run from `scripts/verify.sh quick` or routine CI without explicit resource authorization.

## 7. Ordered work packages
**A — Pre-register experiment:** derive A003's denominator and freeze fixture/toolchain/matrix; measure stable cold/warm runs and cache prerequisites on Linux/macOS (Windows as qualified support only). Define clear failure flags for absent tools, cache server not permitted, mixed toolchain, wrong target, and baseline CARGO settings.

**B — Fair controls:** baseline isolated target with cache off; same-repo sibling builds quota disabled and quota one; unrelated repository control; then identical arms with local `sccache`. Use explicitly pinned version/tool path, no automatic binary installation. Test `CARGO_INCREMENTAL=0` only in the cache arm, and include a fair no-cache arm with the same setting to separate incremental changes from cache benefit.

**C — Representative workloads:** run dependency-free fixture for continuity with A003 **plus** a realistic dependency graph with build scripts/proc-macros and code-change vs unchanged paths. Cold cache; warm independent second checkout; source modification; feature/profile/toolchain change invalidation; optional denial/unavailable sandbox case. At least three paired repetitions per comparable arm (prefer five when resources allow), shuffled order and recorded system load; never make a one-run performance claim.

**D — Measure and validate:** capture end-to-end queue+execution wall time, per-job CPU/RSS, target/cache/disk growth and cleanup, compiler-cache hit/miss/non-cacheable, scheduler quota waits, Cargo lock evidence and failed builds. Verify matching build outputs under cache/no-cache (hash/fixture tests), repeat run, correct provenance, and no unbounded process/server left behind.

**E — Publish decision:** separate raw observations from inference. Report median, spread and worst case, potential cold-start overhead, hit-ratio denominators, disk cost, cache retention/security constraints, and what is unqualified. A **go** is contingent on consistent practical improvement with no correctness/containment regression; a **no-go** or `inconclusive` result is a legitimate strict closure. If go, register a **new** separately authorized opt-in production integration milestone; do not implement it under A004.

## 8. Failure/cancellation/restart/contention
Interrupted benchmark deletes **only its own** scratch worktrees/targets/cache after managed job and process termination; never interferes with a concurrently executing user's Cargo or global sccache daemon. A cache server failure/timeout is recorded as explicit `cache_unavailable`, not secretly rerun as uncached. If the sandbox denies socket/cache/server, stop and record precise denial; no FullHost fallback unless a separately designated and labelled trusted-workstation experiment. Restarted trials must not mix cold/warm cache history; seed/clear only owned test cache.

## 9. Compatibility and migration
No product configuration/schema default change. Do not require `sccache` on Codegg install or CI. New ignored experiment is opt-in and has a bounded maximum wall time, target size and cache retention. A negative result leaves A003 admission intact and still unblocks A005 planning (with no cache recommendation).

## 10. Required tests and evidence
- **Focused:** experimental arm parser/runner dry-run and `--help`, skip if tool absent, version/feature/incremental/environment capture, output equivalence, cache hit parsing, cleanup of owned dirs and server.
- **Integration:** real scheduler + managed argv on two sibling worktrees, production quota off/on, unrelated repository and actual Cargo; backend contained and host-only evidence distinguished.
- **Failure/recovery:** cache daemon not running, denied socket/cache write, corrupt cache entry, interruption, no user-scope target/cache deletion.
- **Statistics:** multiple paired samples and randomized ordering; cold/warm separately, hash/output correctness, median and spread; resource accounting beyond aggregate time.
- **Security:** absent principal separation, path escape, secret/cache storage, long-lived server and external networking denied; no wider sandbox grant.

## 11. Required verification commands
```bash
cargo test --test scheduler_jobs -- --list
cargo test --test scheduler_jobs -- build_quota_admission
cargo test --test scheduler_jobs -- repository_quota
# opt-in only, with exact test/runner name verified during implementation:
CODEGG_RUN_A004_CARGO_ECONOMICS=1 cargo test --locked --test scheduler_jobs -- a004_cargo_economics --ignored --nocapture --test-threads=1
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
```
An unavailable optional binary/environment means a documented skipped or refused arm, never a fabricated result. Confirm test-filter selection is nonzero; no benchmark was run by authoring this plan.

## 12. Documentation updates
Add a validation protocol/result in `plans/validation/agent-worktree-placement-and-build-affinity/`, explain findings and source versions in `architecture/build_admission.md` / `architecture/testing.md` **only after measurement**, update roadmap/registry at closure. Include citations to Cargo official build-cache reference and Mozilla sccache Rust caveats.

## 13. Acceptance criteria
A reproducible, bounded opt-in experiment compares **real** Rust builds across sibling linked worktrees, separate target directories, disabled/enabled repository quota and disabled/enabled local compiler cache, with honest sandbox qualification; data contains CPU/RSS/wall/disk/cache statistics and correctness checks. It produces an explicit go/no-go/inconclusive decision. The shipped default remains unchanged; no shared target or uncontrolled daemon persists.

## 14. Stop conditions
Stop rather than widening scope if native containment forbids cache server/socket, the only available fixture is dependency-free, a representative workload cannot be reproduced, cache key/trust partition cannot be assessed, existing global Cargo wrapper would be overwritten, or real production adapter cannot be exercised. For incomplete evidence close conditionally with explicit future gates, or strictly with a justified **negative** decision if enough evidence supports it.

## 15. Closure evidence required
Exact host/toolchain/cache versions, recorded environment, source/ref/checkouts, test-scoped directories, all arm sample rows, output equivalence, cache hit/miss denominators, CPU/RSS/wall/target/cache metrics, lock waits, skipped arms and reasons, sandbox fact receipt, no tool/sandbox permissions expanded, quick/Clippy results and linked operator decision. Cite A003's production measurements as baseline but do not reuse its single sample as A004 replication.

## 16. Handoff notes
Use the current A003 production Cargo fixture as a starting point rather than inventing a new scheduler; its fixture has **no dependencies**, so add a realistic dependency-heavy arm. `sccache` may require `CARGO_INCREMENTAL=0` and can decline caching proc macros/linker outputs; first check cacheability **before** drawing a conclusion from wall time. Linux hosted qualification is available but resource-intensive ignored benchmark must be explicitly enabled. A005 remains **blocked until the A004 decision and A002 closure**; do not write A005's implementation plan against hypothetical evidence.
