# Agent Worktree Placement and Build Affinity A004 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-worktree-placement-and-build-affinity/004-rust-cross-worktree-cache-economics-pilot.md`
Source subsystem roadmap: `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md#milestones-and-boundaries`
Repository baseline reviewed: `493e5ee111d133eb1cb99debddcc466122c7f7f9`
Implementation commits:
- `0ac17d38c` — production-scheduler A002/A004 implementation and opt-in Cargo/sccache harness.
- `271fd8d56` — A002 ownership guard hardening; A004's existing harness remains unchanged.

Closed: 2026-10-28

Primary class: polish / performance experiment

## 1. Executive finding

A004 is strictly closed as a **no-go** for production compiler-cache integration. Three randomized repetitions submitted 24 real `cargo check --locked --offline -p codegg-core` jobs through the production scheduler and managed argv executor, across cache and repository-quota arms. sccache 0.18.0 recorded **zero Rust hits** across 4,692 Rust misses. Cache-on median per-job wall time increased from 106.80 to 161.89 seconds with quota one and from 107.94 to 178.05 seconds with quota disabled. Median CPU also increased materially. The raw measurements and environment are recorded in `plans/validation/agent-worktree-placement-and-build-affinity/004-cargo-sccache-economics.md`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Actual scheduler and managed Cargo execution | Ignored A004 test submits through JobSubmissionService → JobScheduler → ManagedArgvExecutor | pass | 24 Cargo jobs succeeded |
| Cache and repository quota controlled independently | Three randomized repetitions, cache off/on × quota disabled/one, separate target directory per checkout | pass | No shared writable Cargo target |
| Representative Rust dependency graph | `cargo check -p codegg-core` from frozen sibling worktrees | pass | Real CodeGG workspace and lockfile |
| Resource, disk, hit/miss, and lock evidence | Wall, user/system CPU, process-tree RSS, target bytes, Cargo lock waits, sccache counters in validation record | pass | All 24 Cargo lock-wait counts were zero |
| Evidence-driven decision | Zero Rust hits, higher time/CPU, ~2 GiB cache use | pass | No-go; no production integration |
| Contained-host cache-server qualification | Not run | not claimed | A no-go was reached without granting or requiring a sandbox exception; no cache service is shipped |

## 3. Production scheduler pilot

Host: Apple Silicon macOS 25.6; ARM64 test/rustc process; Cargo 1.89.0; rustc 1.89.0; pinned sccache 0.18.0. Three independent random arm orders produced six jobs in each condition. One unrelated host Rust compilation overlapped one sample; that observation remains in the raw table.

| Cache | Repository quota | Jobs | Median wall / range (s) | Median user+system CPU (s) | Target bytes | Rust cache hits |
|---|---:|---:|---:|---:|---:|---:|
| off | 1 | 6 | 106.80 / 98.52–122.57 | 165.70 | 641,365,146 | — |
| off | disabled | 6 | 107.94 / 102.84–113.69 | 170.98 | 641,365,146 | — |
| on | 1 | 6 | 161.89 / 139.05–181.92 | 280.81 | 641,366,837 | 0 |
| on | disabled | 6 | 178.05 / 145.99–184.21 | 304.54 | 641,366,837 | 0 |

The enabled cache reported 638 non-Rust hits (assembler/C/C++), 4,692 Rust misses, no cache errors/timeouts, and a rounded size of 2 GiB under its 3 GiB cap. The cache-on target difference was 1,691 bytes. Those aggregate hits do not establish Rust compilation reuse.

## 4. Verification executed

- Opt-in production-scheduler Cargo/sccache pilot: **24/24 jobs succeeded**; test passed in 2,493.77 seconds. The final harness now also requires `CODEGG_REGENERATE_ASSETS=1` before writing Git worktree metadata; that guard was added after this completed run to satisfy the repository's ignored-writer test rule. The measurement body and arms did not change.
- `scripts/verify.sh quick`: passed on commit `271fd8d56`; 357 Harness Lab tests ran (356 passed, 1 skipped), all quick guards and workspace all-target check passed.
- `CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings`: passed on commit `0ac17d38c`.
- `python3 scripts/check_test_assertion_quality.py`, `python3 scripts/check_test_duplication.py`, `python3 scripts/rust_test_scan.py --validate`: passed.
- `python3 scripts/check_comment_budget.py`, `cargo fmt --check --all`, and `git diff --check`: passed.

## 5. Invariant review

No cache, service, shared target, global Cargo configuration, dependency, or user-facing cache toggle was added. Per-worktree Cargo targets remain separate. Repository quota remains configuration-owned and unchanged. The no-go is scoped to the measured sccache 0.18.0/macOS/Rust 1.89 configuration; it does not claim every compiler cache on every platform is uneconomic.

## 6. Failure and recovery review

The pilot used task-owned temporary worktrees, targets, cache, and server lifecycle. The corrected runner kept sccache compiler CPU/RSS in the managed Cargo process tree and fixed the scratch SQLite pool lifetime. The complete randomized matrix exited successfully and cleanup ran. No cache failure was converted into an uncached success.

## 7. Migration and compatibility review

No product schema, protocol, config default, or persisted user state changed for cache support. Existing A003 repository admission remains independent.

## 8. Security review

No provider credentials, remote cache, user target directory, or global cache were used. The run did not establish native sandbox permission for a persistent cache service; since the measured decision is no-go, no such permission or service was introduced. Future cache integration requires a separate plan with Rust hits, trust partitioning, containment, lifecycle, and net-benefit evidence.

## 9. Documentation and operations

The pilot protocol and all 24 per-job observations are in `plans/validation/agent-worktree-placement-and-build-affinity/004-cargo-sccache-economics.md`. The implementation plan's exact opt-in command names the pinned Cargo, rustc, and sccache binaries and the repository-writer opt-in.

## 10. Unresolved findings

- **Low:** Linux/Windows economics, contained-server permission, warm changed-source benefit, and cache trust partitioning are unqualified. They do not block this no-go closure because no production cache is recommended or enabled.

## 11. Roadmap disposition

A004 is **closed** with a no-go. Keep isolated writable targets and current repository quota behavior. Any future compiler-cache integration is a new separately reviewed plan, not a continuation of A004.

## 12. Registry updates

- Move A004 from active implementation work to recently closed work as **closed / no-go**.
- A004's decision evidence is available to A005; the A002 strict closure remains independently required.
