# Linux Trusted Git Cost Corrective C005 — Git Workflow Parity, Cost Gate and Independent Admission

Status: blocked — disposition recorded at `plans/closure/linux-trusted-git-cost-corrective/005-status.md`; the C004 production publisher (C007) and the M005 production-owner qualification (C014, C023) are now closed, while accepted ADR-0044, the tool-program child-job route, the supported-host policy, and separate Linux C002/M003 admission remain incomplete
Primary class: capability
Baseline: `planning/linux-sandbox-next-admission-and-adr-cleanup` `1041f41d (rebased review baseline)` (2026-10-18).
Source: `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md`, parent Linux execution isolation roadmap; existing Linux C002 plan `plans/implementation/linux-execution-isolation/010-m001-contained-local-git-recovery.md`, C003 remote `011-m001-authorized-remote-git-recovery.md`, M008 `008-qualification-and-rollout.md`.
Authority: long-term §§4,17,27–29; accepted ADR-0004/0016/0034/0035; proposed ADR-0044 (**must be accepted separately before any production switch**).
Target future closure: `plans/closure/linux-trusted-git-cost-corrective/005-status.md`.

## Objective

Integrate the least-cost qualified read/staging backends through existing typed Git tools, restore representative local Git mutations with valid authority and exact-native security evidence, and prepare a **separate explicit** M003/C010 admission/rollback decision. This plan supplies implementation/evidence to the existing Linux C002 and M008 owner; it does **not** declare those previously blocked milestones closed.

## Inputs / boundary

M004 process/IPC/PTY/LSP is closed for a recorded supported Linux host; M005 network offline is only a candidate, so complete network-owner/fail-closed qualification is a hard condition. C001 must establish a credible security threat model; C002 a contained-read implementation with no immutable-generation claim; C003 measurement chooses CLI or gix; C004 private staging promotion must survive actual external writers and crash faults. Historical C012 rejection of no-writer lock remains valid.

## Invariants and non-goals

Never silently downgrade old immutable trusted reads or replace their obtained guarantees. No default switch to unqualified backend or bypass of approved `GitOperation`/`DestructiveRepositoryChange`; no user-supplied helper trust/host sockets/network credentials; no arbitrary `.git` write admission. Remote fetch/pull/push, credential mediation and M006 proxy are **Linux C003 scope**, not this plan. No requirement to make gix a dependency if measured benefit is insufficient; no SBC compilation benchmark.

## Ordered packages

1. Freeze regression corpus: historical 9/12 Git mutation failures, `git_execution_origin_matrix` row-3 `BackendUnavailable[RepositoryAlias]`, intermittent `ChangedFiles` and `log` payload tests, status/diff/log ordinary positives, linked worktree/common-dir and hostile metadata fixtures. Preserve typed refusal for reset/clean without `DestructiveRepositoryChange` and remote unavailable until C003.
2. Integrate authorized `ContainedGitOperation` dispatch with `GitMutationExecutor`/GitTool and unchanged `ManagedProcessService` owner. Implement small parity groups: status/log/show/diff then stage/add/commit and restore/checkout; only after safe staged publication extend merge/rebase/cherry-pick/revert/stash/recovery. Document inherently non-atomic working-tree mutation and user-visible partial/conflict outcomes. No trusted host execution of hooks/filters/merge drivers.
3. Run actual benign and hostile clean/smudge/process filters, configured and traditional Git hooks, merge drivers, fsmonitor, custom diff/textconv, credential/remote helper vectors under isolated child; prove no host secret read, no host loopback/external TCP/UDP/DNS, no host proc/socket/FD and no persistent poisoning of later daemon Git. Feature-dependent Git versions must be pinned and recorded.
4. Compare performance against the **same repo contents/operations**: native Git, current immutable snapshot, contained live Git and optional gix; >=100 warm/30 cold per packed and >=4k-loose-object >=100 MiB fixture, ext4 + optional CoW. Record setup-only and end-to-end p50/p95/max, CPU/RSS/I/O/bytes copied/scratch, multi-agent 1/4/8 contention. Check existing C010 budgets and reject a candidate that misses them unless an explicit new admission decision revises *future* policy; do not retroactively rewrite C010.
5. Native acceptance matrix: supported Ubuntu kernel Landlock ABI6/M004 with actual M005 offline, default Linux userns/AppArmor enabled policy as shipped, no-userns refused before target, older ABI refused when required, ext4, linked/nested worktrees, simultaneous outside-host `git gc`/repack/index/ref edit, cancellation/restart and crash between private object/index/ref publication phases. Require no skipped security-negative test in positive qualification.
6. Validate cross-platform nonregression (Windows/macOS production legacy routes), audit/receipt versioning and migration, model visibility of typed unavailable/remedy, rollback to legacy immutable backend, and removal of temporary state. Prepare a precise M003/C010 *new admission record* for independent review; do not mark C012 earlier negative evidence superseded.
7. Handoff evidence to Linux C002 positive local Git closure and M008 exact release gate. Keep C003 remote transport blocked until independent mediated-network/credential tests. Production defaults and operator-visible policy require separate approval after safe opt-in acceptance.

## Code ownership / compatibility

Possible `src/git_service.rs`, `src/git_mutations*.rs`, `src/tool/git.rs`, `crates/egggit/src/process.rs`, `src/managed_process.rs`, `src/security/sandbox/{linux_launch,linux_mount_view}.rs`, RunStore typed results, tests and architecture/docs. Reuse canonical launcher and authorization, no new database schema by assumption. If consistency/obtained guarantee is serialized, version it and test older clients; do not change the meaning of old `SandboxEnforcement`. Durable journal and scratch ownership must survive daemon shutdown/restart and cannot delete another agent's in-flight outputs.

## Verification and closure

`cargo fmt --all -- --check`; `cargo test --locked -p egggit`; `cargo test --locked --test git_mutations_integration -- --test-threads=1`; `cargo test --locked --test git_execution_origin_matrix -- --test-threads=1`; `cargo test --locked --test git_network_integration -- --test-threads=1`; `cargo test --locked --test sandbox_landlock -- --test-threads=1`; focused native M004/M005/contained Git suite, `python3 scripts/check_execution_ownership.py`, `python3 scripts/check_git_forbidden_patterns.py`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_adr_integrity.py`, `scripts/verify.sh quick`, exact-head full CI plus native host metadata. Future `005-status.md` must include requirement-to-evidence matrix, precise positive/negative counts, resource/concurrency results, rollback dry-run, migration/security review and links to *separately accepted* Linux C002/M003/M008 decisions.

## Stop / disposition

Stop on unapproved release-default changes, false immutable consistency reporting, broad protected-gitdir writes, any host authority leak, stale-index overwrite, unbounded recheck/poll loop, reliable git mutation regressions, or unqualified offline-network success. A **conditionally closed or blocked** result is preferable to mislabeling a candidate production. C005 cannot alone override the historical C010 no-admission or finish C003 remote Git.
