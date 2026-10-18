# Linux Trusted Git Cost Corrective C002 — Contained Live-ODB Read Pilot

Status: blocked on C001 test-only proof; **production route additionally blocked on ADR-0036 acceptance and M005 native/owner admission**
Primary class: capability
Baseline: `planning/linux-sandbox-next-admission-and-adr-cleanup` `0b36c71f (post-rebase review baseline; the pre-rebase evidence commit was 004d770b8cd23ca33fb724dae95f97de563ba60d)` (2026-10-18).
Source roadmap: `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md`; parent `plans/subsystems/linux-execution-isolation-roadmap.md`.
Long term: `plans/000-long-term-specification.md` §§4,17,27,29, `plans/001-terminology-and-domain-model.md` §8. Accepted ADR-0004/0016/0034/0035; proposed ADR-0036 is **not** implementation authority for promotion.
Historical C010 and C012 negative closures are preserved; M004 closure provides tested process/IPC mechanisms, M005 remains internal.
Target future closure: `plans/closure/linux-trusted-git-cost-corrective/002-status.md`.

## Objective

Build and qualify an **internal opt-in, test-only** contained native-Git read adapter that reads the original ODB via a narrow read-only mount instead of `prepare_private_object_view`. Prove requested-vs-obtained enforcement, bounded latency, result parity and acceptable (explicitly weaker) live-read correctness. Retain the immutable snapshot route as the only ordinary trusted-read production backend unless an independent ADR/rollout admits a new class.

## Current code and issues

`egggit::process::GitEnvPolicy::apply` and `apply_sync` still build Git `Command` plus a private ODB lease. They are used by structured reads, mutation pre/post snapshots and many callers. M004's helper/PTY/LSP orchestration uses `ManagedProcessService`, but `egggit` cannot independently adopt Codegg daemon authority. Avoid a second launcher hidden within `egggit`. Historic `tests/git_execution_origin_matrix.rs` and structured ChangedFiles/log outputs have intermittent `BackendUnavailable[RepositoryAlias]` or payload failures; diagnose reproducibly, do not work around by returning empty data.

## Invariants / non-goals

- Typed operation and authenticated parent/child lease stay daemon-owned; no plain `GitEnvPolicy` call silently promotes to host execution.
- Live ODB and selected gitdir/common-dir must be pinned/validated for launch; allow only trusted reads, no writable `.git`, out-of-scope path, host socket/FD or credential. `M005 OfflineRequired` means obtained actual network namespace denial, not a flag.
- Repository-controlled helpers either disabled deterministically or allowed **only inside** the sandbox without inherited host capabilities; a configuration exception cannot bypass confinement.
- No claim that concurrent source mutation cannot change read bytes. Detect and report stale/incoherent results where necessary. Do not rename old immutable outcomes to fit the new model.
- No default profile switch, remote transport, destructive op authorization, custom daemon service, broad host bind, gix prod dependency, or general M003 admission.

## Ordered implementation packages

1. Freeze `C001` test oracle, expected typed operation kinds and selected repo/gitdir/common-dir/ODB identity. Define internal `GitReadBackend` selection/facts (names illustrative): `ImmutableSnapshotLegacy`, `ContainedLiveTestOnly`. Originating session/agent authorization and immutable job lease are inputs; `egggit` may construct argv but must not bypass `ManagedProcessService`.
2. Build internal, test-gated adapter through `src/git_service.rs` and `src/managed_process.rs`; pass reviewed `SandboxLaunchSpec` with explicit protected metadata, ODB read roots, M004 process/FD/seccomp, M005 offline network. Reuse child-only helper identity+status+ack, cancellation, time/output bounds and tests. Refuse any unknown/unobtained required dimension *before Git runs*. Verify chosen trust-root and path/FD identity and enforce no unapproved secondary ODB, partial-clone/promisor fetch, Git `includeIf`, repo-discovery or alternate escape.
3. Preserve structured Git payload parity (`status`, `diff`, `log`, `show`, `ChangedFiles`, branches/worktree reads) against a static native Git oracle. Use separate receipt field for live vs immutable consistency; while proposed, keep it internal or test-only. Explicitly protect Git config executables and user/credential env using existing `GitEnvPolicy` hardening.
4. Run adversarial/race fixtures from C001 on real children and intentionally mutate/repack ODB concurrently. Treat failures as typed stale/unavailable (do not fabricate spawn error or silently retry as FullHost). Carry output provenance/retry counts into existing audit without secret text.
5. Measure paired end-to-end and child-setup times vs unsandboxed native Git and legacy immutable snapshot. Targets: packed, >=4,000 loose objects/~100 MiB, ext4 and optional CoW; >=100 warm/30 cold per fixture for p50/p95 and CPU/RSS, bytes copied, retained tmp bytes, lock contention with 1/4/8 agent calls. C010 budgets remain comparison thresholds; any revised admission budget must be approved separately.
6. Document M004/M005 host acceptance limits and Android/WSL/no-userns or older-Landlock refusal. Provide a ready-to-review production adapter design but **do not register it or change defaults** without accepted ADR-0036, M005 production evidence, C010/M003 new admission and exact-head CI.

## Change scope / migration

`crates/egggit/src/process.rs` (separate argv construction from snapshot ownership without affecting default), `src/git_service.rs`, `src/managed_process.rs`, narrowly scoped Linux sandbox policy compiler/helper, existing `tests/git_*` plus dedicated native Linux test. No new public wire enum/DB column in test-only phase; a future persisted/transport receipt requires versioning. On Windows/macOS keep snapshot/trusted defaults untouched. Cache retained for existing immutable route, never silently unprotected.

## Verification and tests

`cargo fmt --all -- --check`; `cargo test --locked -p egggit trusted_git -- --test-threads=1`; `cargo test --locked --test git_execution_origin_matrix -- --test-threads=1`; `cargo test --locked --test git_network_integration -- --test-threads=1` (record all expected typed denials); `cargo test --locked --test sandbox_landlock -- --test-threads=1`; focused native M004/M005 contained-read tests; `python3 scripts/check_execution_ownership.py`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_git_forbidden_patterns.py`; `python3 scripts/check_adr_integrity.py`; `scripts/verify.sh quick`; exact-source hosted Linux positive and restricted-userns negative matrix, macOS/Windows compile when shared paths change.

## Closure / stop

Future C002 closure documents complete parity, actual enforced dimension receipts, allow and deny probes, timeout/crash cleanup, concurrency/races and p95 data with code SHA/host facts. **Stop** on any host privilege/channel leak, untrusted ODB path, missing M005 obtained enforcement when required, linked-worktree authority expansion, incorrect structured output masked as success or silent weakening of immutable semantics. Test-only success cannot unblock C010, Linux C002 or public backend promotion.
