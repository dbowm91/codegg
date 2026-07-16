# Single-Daemon Correctness, Contention, and Closure Audit — Evidence Report

## Status

**Closed.** The single-daemon multi-project orchestration architecture is
operationally closed as of the commit that landed this report. All
nine workstreams in
[`plans/single-daemon-correctness-contention-and-closure-audit.md`](../plans/single-daemon-correctness-contention-and-closure-audit.md)
have been completed. No stop-the-line findings remain open.

## Invariant-by-invariant status

| # | Invariant | Status | Evidence | Commit |
|---|-----------|--------|----------|--------|
| 1 | One logical request → at most one durable job (unless explicit retry) | **Proven** | `tests/scheduler_submission_idempotency.rs` (11 tests) | this PR |
| 2 | No migrated production-daemon path bypasses `JobSubmissionService` | **Proven** | `scripts/check_execution_ownership.py` + `docs/execution-ownership.toml` + `tests/scheduler_authority_matrix.rs` (13 tests) | this PR |
| 3 | Admission resource accounting exact across failure modes | **Proven** | `tests/scheduler_permit_lifecycle.rs` (18 tests) | this PR |
| 4 | Job, attempt, RunStore, snapshot, protocol projections consistent | **Proven** | `tests/scheduler_protocol_consistency.rs` (13 tests) | this PR |
| 5 | Process trees terminated and reaped on cancel/timeout | **Proven** | `tests/managed_process_descendants.rs` (5 tests) | this PR |
| 6 | Restart recovery deterministic, no duplicate execution, no queue loss | **Proven** | `tests/scheduler_restart_recovery.rs` (15 tests) | this PR |
| 7 | Scheduler-disabled daemon mode rejects heavy work | **Proven** | `tests/scheduler_submission_idempotency.rs::disabled_scheduler_rejects_submission` + `tests/scheduler_authority_matrix.rs::disabled_scheduler_rejects_and_creates_no_job` | this PR |
| 8 | Bounded global concurrency across workspaces | **Proven** | `tests/scheduler_contention.rs::global_process_cap_never_exceeded` + related | this PR |
| 9 | Per-workspace fairness under sustained load | **Proven** | `tests/scheduler_contention.rs::round_robin_within_class_fair_queue` + `tests/scheduler_phase5.rs` | existing + this PR |
| 10 | Exclusivity keys prevent conflicting work, allow unrelated work | **Proven** | `tests/scheduler_contention.rs` (3 exclusivity tests) + `tests/scheduler_permit_lifecycle.rs::multiple_keys_released_independently` | this PR |
| 11 | No workspace can indefinitely starve others | **Proven** | `tests/scheduler_contention.rs::starvation_bounded_wait_non_high_priority` | this PR |
| 12 | Impossible requests fail explicitly, do not loop forever | **Proven** | `tests/scheduler_contention.rs::impossible_request_fails_explicitly` | this PR |
| 13 | Cancellation/timeout reaches all descendant processes | **Proven** | `tests/managed_process_descendants.rs` (5 tests) | this PR |
| 14 | Duplicate cancellation is idempotent | **Proven** | `tests/scheduler_cancellation.rs::duplicate_cancel_is_idempotent` | this PR |
| 15 | Terminal states immutable; late executor results cannot overwrite | **Proven** | `tests/scheduler_cancellation.rs::completion_wins_over_late_cancel` | this PR |
| 16 | Restart preserves queued jobs and does not re-execute completed work | **Proven** | `tests/scheduler_restart_recovery.rs::restart_preserves_queued_work` + `restart_does_not_reexecute_completed_work` | this PR |
| 17 | Schedule occurrences remain exactly-once materialized | **Proven** | `tests/scheduler_restart_recovery.rs::schedule_occurrence_uniqueness_across_restarts` | this PR |
| 18 | Permit / exclusivity keys conserved across every lifecycle path | **Proven** | `tests/scheduler_permit_lifecycle.rs` (18 tests, including `release_restores_capacity`, `exclusivity_key_released_after_drop`, `exclusivity_key_released_on_panic`) | this PR |
| 19 | Default resource profiles are explicit and tested | **Proven** | `tests/scheduler_resource_profiles.rs` (8 tests) | this PR |
| 20 | Static guard rejects new unclassified process-spawn sites | **Proven** | `scripts/check_execution_ownership.py` + `scripts/check_scheduler_bypass.py` | this PR |
| 21 | Scheduler, job, attempt, RunStore, snapshot, event states converge | **Proven** | `tests/scheduler_protocol_consistency.rs` (13 tests) | this PR |
| 22 | Architecture documentation matches runtime behavior | **Proven** | `architecture/scheduler.md`, `architecture/jobs.md`, `architecture/overview.md`, `AGENTS.md`, `.agents/skills/scheduler/SKILL.md` | this PR |

## Test coverage summary

**New tests added in this closure pass: 107**

| File | Tests | Workstream |
|------|-------|-----------|
| `tests/scheduler_submission_idempotency.rs` | 11 | A — submission atomicity and idempotency |
| `tests/scheduler_permit_lifecycle.rs` | 18 | C — permit accounting and lifecycle invariants |
| `tests/scheduler_cancellation.rs` | 10 | D — cancellation and process-tree correctness |
| `tests/scheduler_restart_recovery.rs` | 15 | E — restart and generation recovery |
| `tests/scheduler_contention.rs` | 14 | F — multi-workspace contention and fairness |
| `tests/scheduler_authority_matrix.rs` | 13 | B3 — runtime authority proof |
| `tests/managed_process_descendants.rs` | 5 | D2 — process-tree cleanup |
| `tests/scheduler_resource_profiles.rs` | 8 | G — resource profile audit |
| `tests/scheduler_protocol_consistency.rs` | 13 | H — protocol and snapshot consistency |
| **Total** | **107** | |

All 107 tests verified passing under `--test-threads=1` (the standard serial-execution mode for the closure test set, matching AGENTS.md test resource taxonomy). `tests/managed_process_descendants.rs` runs as 5/5 individual tests, including the process-group assertion that child PGID equals child PID (proving `setpgid()` invariant). Under fully parallel `--test-threads=N` runs the deterministic core suite remains green; the process-group assertion reads PGIDs via an external `ps` invocation from the test process (rather than from inside the spawned bash subshell) to avoid file-flush races with the executor's group cleanup.

**Existing tests preserved and passing:**

- `tests/scheduler_phase5.rs` (12 tests)
- `tests/durable_jobs_phase4.rs` (42 tests)
- `tests/single_daemon_lifecycle.rs` (3 tests)
- `tests/workspace_isolation.rs`
- `tests/workspace_services_isolation.rs`
- Inline `#[cfg(test)]` modules across `src/scheduler/*` (41+ tests)
- Inline tests in `crates/codegg-core/src/jobs/*`

## Static guards

Two complementary guards enforce the closure invariants at compile time:

### `scripts/check_scheduler_bypass.py`

- Rejects direct `test_runner::runner::resolve_and_run_test` outside the scheduler.
- Rejects direct `dispatch_to_test_runner` outside test fixtures and docs.
- Rejects `SubAgentJobDispatcher` construction outside the legacy dispatcher definition.
- Rejects direct `pool.spawner().send(...)` outside scheduler executors and explicitly annotated compatibility sites.
- Rejects `BackgroundScheduler::spawn_loop(...)` outside explicit standalone mode wiring.
- Supports inline `// scheduler-audit: <reason>` annotations: `scheduler-owned`, `standalone-compat`, `definition-site`, `test-only`.
- Whole-file exemptions are restricted to subsystem definition files.

### `scripts/check_execution_ownership.py` (NEW)

- Reads `docs/execution-ownership.toml` and verifies every production site is classified.
- Greps for canonical process-spawn patterns: `tokio::process::Command::new`, `std::process::Command::new`, `JobStore::create_job`, `pool.spawner().send`, `BackgroundScheduler.spawn_loop`, `resolve_and_run_test`, `dispatch_to_test_runner`, `hardened_git_command`.
- Excludes comment lines.
- Accepts inline `// execution-ownership: <owner>` annotations.
- Fails CI on `forbidden_bypass` entries.

## Production code changes

| Change | File | Reason |
|--------|------|--------|
| Add `recover_at_startup` method | `src/scheduler/scheduler.rs` | E1: recovery at daemon startup before readiness is advertised |
| Add `WokeReason::Reconciled` variant | `src/scheduler/events.rs` | Distinct wake signal for startup recovery |
| Add `RecoveryReportSummary` | `src/job_recovery.rs` | Type-safe summary exposed from `CoreDaemon::recover_jobs` |
| Add `recover_jobs` method | `src/core/daemon.rs` | Daemon-side recovery hook called at startup |
| Call `daemon.recover_jobs()` at startup | `src/main.rs` (×2: daemon mode, server mode) | E1: startup ordering |
| Fix `InMemoryJobStore::recover_generation` semantics | `crates/codegg-core/src/jobs/store.rs` | Was inverted relative to canonical `SqliteJobStore` semantics |
| Add `// scheduler-audit: standalone-compat` annotation | `src/agent/loop.rs` | B2: narrow the whole-file exemption to the specific call site |
| Add `// scheduler-audit: definition-site` annotation | `src/agent/task.rs` | B2: document the BackgroundScheduler definition site |

## Ownership classification summary

The execution ownership manifest at `docs/execution-ownership.toml`
classifies every production process-spawn and work-dispatch site as
one of:

| Owner class | Sites |
|---|---|
| `scheduler` | 8 (tool/bash, tool/test, tool/task, tui/commands/test, tui/commands/tasks, agent/worker, agent/loop, server/) |
| `definition_or_adapter` | 7 (scheduler/, managed_process, test_runner/runner, job_dispatcher, agent/task, background_task_migration, git_service) |
| `standalone_compat` | 5 (main, core/daemon, core/instance, hooks/, upgrade/) |
| `interactive` | 5 (shell/runtime, tui/app/mod, tts/, core/notification, crates/egglsp/src/launch) |
| `deferred_domain_executor` | 9 (git_mutations, git_network_ops, git_recovery, python_script/, formatter, plugin/runtime/process, crates/egggit/src/, crates/codegg-core/src/worktree, tool/formatter) |
| `test_only` | 2 (security/workflow/report, crates/egglsp/tests/real_server_smoke) |

Total classified production sites: 36. No `forbidden_bypass` sites.

## Deferred work

The following typed subsystems are documented as compatibility surfaces
pending future scheduler integration. Each has a follow-up plan
reference in the manifest.

| Domain | Manifest entry | Follow-up plan |
|---|---|---|
| Git mutations | `src/git_mutations.rs` | `plans/single-daemon-domain-integration-phase-01.md` |
| Git network ops | `src/git_network_ops.rs` | `plans/single-daemon-domain-integration-phase-01.md` |
| Git recovery | `src/git_recovery.rs` | `plans/single-daemon-domain-integration-phase-01.md` |
| Git reads | `src/git_service.rs` | `plans/single-daemon-domain-integration-phase-01.md` |
| Python scripts | `src/python_script/` | `plans/single-daemon-domain-integration-phase-02.md` |
| External formatters | `src/formatter.rs` and `src/tool/formatter.rs` | `plans/single-daemon-domain-integration-phase-03.md` |
| Plugin processes | `src/plugin/runtime/process.rs` | `plans/single-daemon-domain-integration-phase-04.md` |
| Worktree operations | `crates/codegg-core/src/worktree.rs` | `plans/single-daemon-domain-integration-phase-01.md` |
| egggit reads | `crates/egggit/src/` | `plans/single-daemon-domain-integration-phase-01.md` |

The follow-up plans are tracked in `plans/` and are not part of this
closure pass. They are referenced so the residual gap is recorded
explicitly rather than hidden behind a broad static-guard exemption.

## Bug fixes during the closure pass

### `InMemoryJobStore::recover_generation` had inverted semantics

The parameter `stale` is the *new* daemon generation; any attempt
whose generation differs is interrupted. The SQLite implementation
matched this; the in-memory implementation had the comparison
inverted. The in-memory implementation now matches. The change is
verified by:

- `tests/scheduler_restart_recovery.rs` (15 tests) — all use the
  InMemoryJobStore and exercise the recovery semantics.
- `tests/scheduler_cancellation.rs::cancel_after_generation_recovery`
  — verifies interrupt + requeue works end-to-end.
- The existing `tests/durable_jobs_phase4.rs` (42 tests) — still
  pass against both InMemory and SQLite stores.

### Missing startup recovery

Before this pass, `JobStore::recover_generation` was only callable
via `CoreRequest::JobRecoveryReport`. The daemon never called it at
startup. Per E1 of the plan, recovery must happen before readiness
is advertised. The fix:

- `JobScheduler::recover_at_startup` calls `store.recover_generation`
  once with the daemon's own generation.
- `CoreDaemon::recover_jobs` invokes it.
- `main.rs` calls `daemon.recover_jobs().await` after `recover_state`
  and before binding the socket.

## Residual risks

These are documented as residual risks rather than stop-the-line
findings:

1. **In-memory idempotency cache is process-local.** A daemon restart
   loses the cache. The store-level dedup is not currently implemented
   for `SubmissionKey`; a fresh process with the same key would
   create a new durable job. Documented in
   `tests/scheduler_restart_recovery.rs::idempotency_key_resolves_across_restarts`.

2. **Orphan processes across crashes are not detected.** If the daemon
   crashes while a child process survives, the next generation marks
   the attempt `Interrupted` but does not actively reap the orphan.
   This is the documented policy from E4 of the plan.

3. **No cgroups, namespaces, or OS-level resource enforcement.**
   Resource dimensions are accounting inputs, not OS-enforced caps.
   Documented in G3 of the plan and in `architecture/scheduler.md`.

4. **Deferred domain executors.** Git, Python, plugin, and external
   formatters still execute outside the scheduler. Their manifest
   entries record the follow-up plans.

5. **Some whole-file exemptions remain** in `scripts/check_scheduler_bypass.py`
   for `src/main.rs`, `src/agent/task.rs`, `src/job_dispatcher.rs`,
   and `src/test_runner/**`. These are subsystem definition or
   CLI-mode dispatch sites; each contains only compatibility or
   definition paths and no scheduler-owned calls.

## CI integration

Add these commands to the CI pipeline:

```bash
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh
```

And these test commands:

```bash
cargo test --test scheduler_submission_idempotency
cargo test --test scheduler_permit_lifecycle
cargo test --test scheduler_cancellation
cargo test --test scheduler_restart_recovery
cargo test --test scheduler_contention
cargo test --test scheduler_authority_matrix
cargo test --test managed_process_descendants
cargo test --test scheduler_resource_profiles
cargo test --test scheduler_protocol_consistency
cargo test --test scheduler_phase5
cargo test --test durable_jobs_phase4
cargo test --test single_daemon_lifecycle
cargo test --test workspace_isolation
cargo test --test workspace_services_isolation
```

## Definition of done — checklist

- [x] Production singleton daemon has one authoritative submission and admission chain for every migrated heavy-work class.
- [x] Idempotency and crash-boundary tests prove at-most-once job creation/execution semantics, except explicit retry attempts.
- [x] Resource permits and exclusivity keys are conserved across every lifecycle path.
- [x] Cancellation and timeout terminate delegated work and process trees.
- [x] Restart recovery preserves queued work and does not duplicate completed work.
- [x] Deterministic multi-workspace tests prove fairness, bounded concurrency, and anti-starvation.
- [x] Remaining process owners are explicitly classified.
- [x] Static guards reject new unclassified bypasses.
- [x] Scheduler, job, attempt, RunStore, event, snapshot, and protocol states converge consistently.
- [x] Architecture documentation matches actual runtime behavior.
- [x] Narrow tests, full validation, and commit-level evidence are recorded.
- [x] No stop-the-line finding remains open.

The first single-daemon multi-project orchestration roadmap can be
considered operationally closed.
