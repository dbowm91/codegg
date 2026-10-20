# Eggwork Worker M004 — Production Schedule Materialization and Crash-Safe Occurrence Idempotency

Status: **implemented**
Class: capability / scheduler / storage
Source: `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`
Reviewed baseline: CodeGG `main` (2026-10-20); relevant current source `crates/codegg-core/src/jobs/schedule.rs`, `crates/codegg-core/src/jobs/schedule_store.rs`, `crates/codegg-core/src/jobs/store.rs`, `src/scheduler/submission.rs`, `src/core/daemon_construct.rs`.
Existing closed evidence: `plans/closure/eggwork-worker-telemetry-and-targeting/001-status.md` (M001 — the explicit target/schedule boundary, whose §10 deferred the production materializer and the occurrence idempotency key to "the milestone that wires a production materializer"), `plans/closure/eggwork-worker-telemetry-and-targeting/002-status.md` (M002 — authenticated host telemetry), `plans/closure/eggwork-worker-telemetry-and-targeting/003-status.md` (M003 — remote limits and admission).

> **Naming.** This is the **eggwork-worker-telemetry-and-targeting** line's M004. It is not the `eggwork-fixed-target-remote-execution` line's deferred M004 (whole remote `AgentRun`), and not the `async-background-jobs` line's closed M004 (GitHub Actions completion watch).

## 1. Objective

Make a schedule actually fire, exactly once per occurrence, with the target it was created with — and make that true across a daemon crash, not only in a happy-path test.

M001 gave `JobTemplate` a target and proved that a materialized job carries it. It could not prove that a schedule ever materializes: the durable schedule rows existed, and nothing in production ever called `ScheduleStore::claim_due`. A recurring schedule was durable data with no trigger.

This milestone closes that: a daemon-owned materialization driver, a production materializer that goes through the single admission facade, and an occurrence identity that is idempotent across a crash.

## 2. Verified gap

**No production trigger.** `ScheduleStore::claim_due` had exactly two call sites, both in tests. `CoreRequest::ScheduleCreate` persisted a schedule and returned; `ScheduleResume` un-paused one; neither woke anything. A schedule's `next_run_at` advanced only if something called `claim_due`, and nothing did. `JobTemplate::new_job_for_occurrence` was reachable only from test materializers.

**No occurrence idempotency.** `claim_due` materialized the job through a callback and then wrote the `schedule_occurrence` row in the store's own transaction. Two failure modes followed from the split, both recorded as a low-severity residual in M001's closure §10 ("the materializer's job creation and the occurrence insert are not in one transaction today"):

- a crash between the job write and the occurrence write left a job with no occurrence row, and the retry created a **second** job for the same occurrence;
- a materializer failure was swallowed (`.ok()`), and the occurrence was still written — with a `NULL` job id and status `queued`. The occurrence was consumed *and* had no job: a silent loss.

**A third, larger defect: the occurrence was lost even without a crash.** The `.ok()` path was reachable in production precisely because the materializer could fail. The occurrence row's `(schedule_id, scheduled_for)` primary key then made the retry a no-op, so the occurrence was permanently consumed without ever running.

**The cursor drifted off the schedule's grid.** `claim_due` advanced `last_occurrence_at` to the **tick time** rather than to the occurrence it had just fired. An interval schedule anchored at `T0` with a 60 s period, ticked at `T0+65`, recorded `last_occurrence_at = T0+65`; the next candidate was then derived from `T0+65` instead of the anchor, so the schedule left its own grid and permanently skipped the occurrence at `T0+120`.

**A refused occurrence had no home.** Nothing recorded *why* an occurrence did not fire, so a schedule that could never run looked identical to one that had not run yet.

**The host boundary was decided once.** M001's contract is "explicit creation-time validation plus execution-time policy recheck". The execution-time recheck lives in the executor's preflight, which compares the node's *live posture* against its declared policy. The **administrator's approval** and the **operator's resource ceiling** were checked only at schedule *creation*. A schedule created while a node was approved kept dispatching to that node after the administrator withdrew the approval and the daemon restarted — because materialization never re-entered the host boundary.

## 3. Work packages

A. **Occurrence identity.** Give every occurrence a durable identity that is a pure function of `(schedule_id, scheduled_for)`: `occurrence_job_id` in `crates/codegg-core/src/jobs/schedule.rs`. The job store derives a scheduled job's id from its source, so the `job.id` primary key *is* the durable uniqueness constraint for an occurrence — a database guarantee, not a convention. `JobStore::create_job` becomes idempotent on that id: creating a job whose id exists returns the existing job. A non-scheduled job keeps a random UUID v4, which cannot collide.

B. **A production materializer.** `src/scheduler/schedule_materializer.rs` turns a fired occurrence into a durable job through `JobSubmissionService` — the single production admission facade — so a scheduled job is admitted exactly like a submitted one (workspace validated, canonical resource profile applied, one durable record, scheduler woken). It creates no job behind the facade and invokes no executor. It re-applies the host-boundary gates at materialization time: the remotely-runnable shape, the administrator's approval, and the operator's resource ceiling. It does **not** re-derive a principal — a timer-driven occurrence has no live principal, and inventing one would manufacture authority rather than check it. Refusals carry a stable, secret-free reason code; no endpoint, credential path, or policy detail reaches a log line or a durable field.

C. **A daemon-owned driver.** `src/scheduler/schedule_driver.rs` is one earliest-due wake loop per daemon: it sleeps until the earliest durable due horizon, claims the due occurrences in one bounded tick, and exits on shutdown. It creates work; the scheduler runs it. No per-schedule timer, no second scheduler, no model polling. The daemon installs it only where it can do its job — the scheduler enabled (the submission facade refuses every submission otherwise) and a pool present (a pool-less daemon has no cross-process durability to claim). Schedule creation and resumption wake it.

D. **Honest occurrence semantics.** A refused occurrence is **not** recorded: the occurrence row is the durable "this already happened" marker, and its primary key would turn the retry into a no-op. An unrecorded occurrence stays retryable, the schedule's cursor does not advance past it, and the schedule's next horizon does not move past it. `OccurrenceStatus::Failed` and `ClaimedOccurrence::job_id: Option<JobId>` make the outcome explicit — an occurrence that produced no job says so instead of claiming one. The cursor advances to the **occurrence**, not to the tick time, so a recurring schedule keeps its own grid.

E. **Three phases, and why.** `claim_due` is split into **plan** (a short read), **materialize** (the slow part: a job submission, and for a remote target a node round trip), and **commit** (one write transaction recording the occurrence rows and advancing the cursors together). The split is not cosmetic: materialization needs the job store, the job store needs a pool connection, and materializing while the schedule store held its own connection waits for a second connection a single-connection pool never has — a self-deadlock that no amount of waiting resolves. The phases are safe to separate because the occurrence's job id is a pure function of the occurrence: a crash between the materialization and the commit replays into the *same* job, and the `job.id` primary key keeps it to one. The commit transaction reads each schedule's stored cursor and writes it back, so it opens with `BEGIN IMMEDIATE` rather than a deferred transaction (C002).

F. **Backward compatibility.** A schedule persisted before this milestone materializes exactly as it did: its template carries the target, the job carries the target, and the occurrence identity is derived from data that already existed. No schema migration: `STORAGE_LAYOUT_VERSION` is unchanged, because no persistent representation changed — the occurrence identity is derived, not stored, and the `schedule_occurrence` table keeps its shape.

## 4. Acceptance tests

1. A due occurrence materializes exactly one durable job and records exactly one occurrence row, and the job's id equals `occurrence_job_id(schedule_id, occurrence)`.
2. **Crash replay**: the job row exists and the occurrence row does not (the daemon stopped between the two). The next tick converges on the *same* job, and the job table holds exactly one row for the occurrence.
3. **A refused occurrence** produces no job and no occurrence row; the schedule's cursor does not advance; a one-shot is not completed; the next tick with a working materializer materializes it.
4. **Grid alignment**: an interval schedule anchored at `T0` fires on the anchor's grid, and its `last_occurrence_at` is the occurrence, not the tick time.
5. **The target is preserved** end to end: a schedule targeting a node materializes a job whose `target_kind` is `eggwork_node` and whose `target_node_id` is that node.
6. **The host boundary is re-applied**: with no node approved, the occurrence is refused and **no local job is created** — there is no local fallback. With the node approved, the remote job is materialized.
7. The driver claims the due occurrences, claims nothing when nothing is due, and stops on shutdown.
8. A paused schedule materializes nothing.
9. **The daemon wires the whole path**: `CoreDaemon::with_deps` installs the driver, a `ScheduleCreate` request through the daemon's own request surface is claimed by that driver, and the job it produces is a real durable row carrying its schedule.
10. A refusal carries a stable, secret-free reason code that reaches the operator's log.

## 5. Verification and documentation

- `scripts/verify.sh quick` (fmt, agent schema, core/client/desktop boundaries, sandbox, execution-ownership, TUI authority, http-route-disposition, audit-coverage, queue-delivery-callsites, queue-write-transaction-shape, scheduler-bypass, config-merge-coverage, no-hardcoded-models, provider wire/catalog/resilience + OpenAI endpoint, Eggwork target routing, `cargo check` workspace).
- `cargo test --locked --test schedule_materialization` (new), `--test durable_jobs_phase4`, `--test scheduler_restart_recovery`, `-p codegg-core` (lib + suites), `-p codegg-protocol`, `-p codegg-client`.
- `architecture/jobs.md` — the occurrence identity, the three-phase tick, the refusal semantics, the grid alignment, the host-boundary re-check, and the M004 section.
- `architecture/scheduler.md` — the driver's ownership boundary against `JobScheduler`.

## 6. Stop conditions

- **No second scheduler.** The driver creates work; the scheduler runs it. If materialization ever needs an executor, the milestone is wrong.
- **No silent occurrence loss.** A refused occurrence is never recorded as done, and never produces a local job in place of a remote one.
- **No fabrication.** An occurrence that produced no job says `None`, never a synthetic id.
- **No authority widening.** The host boundary is re-applied, never widened; the principal gate is not re-derived.
- **No secret in a durable field.** The occurrence identity carries a schedule id and an instant. The refusal carries a reason code, never a message.
- **The occurrence identity stays derived.** It is a pure function of the occurrence; storing it would add a migration for no gain.
