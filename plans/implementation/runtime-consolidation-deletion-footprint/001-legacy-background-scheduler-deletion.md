# Runtime Consolidation, Deletion, and Footprint M001 — Legacy Background Scheduler Deletion

Status: ready

Source roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Planning governance:

- `plans/003-planning-process.md`

Relevant long-term and architecture references:

- `plans/000-long-term-specification.md` sections 1, 2, 4.2, 5, and 7
- `architecture/scheduler.md`
- `architecture/agent.md`
- `docs/execution-ownership.md`

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Primary class: invariant / simplification

Dependencies:

- hard: none beyond current durable `JobStore` / `ScheduleStore` / `JobSubmissionService` production path;
- interface: existing legacy `TaskSchedule`, `TaskDelete`, and `TaskList` request semantics, if any supported caller still uses them;
- soft: M005 may later remove static ownership ratchets made redundant by this work.

Target closure record:

- `plans/closure/runtime-consolidation-deletion-footprint/001-status.md`

## 1. Objective

Remove the independent legacy `BackgroundScheduler` execution/persistence model from production ownership. Where a compatibility request surface still has a supported caller, retain only a thin adapter that translates the request to the durable scheduler/store contract.

The target state is one scheduler implementation, one persistence model, and one durable task/job identity model.

## 2. Current implementation evidence

Inspect at minimum:

- `src/agent/task.rs`;
- `src/core/runtime_deps.rs`;
- daemon request handlers for `TaskSchedule`, `TaskDelete`, and `TaskList`;
- `crates/codegg-core/src/jobs/schedule.rs`;
- `src/scheduler/`;
- `docs/execution-ownership.toml` and related guards;
- tests covering background tasks/schedules.

Known baseline evidence:

- `BackgroundTask::new()` creates a UUID string ID.
- `BackgroundScheduler::spawn_loop()` attempts to parse that string as `u64` and skips dispatch on failure.
- `save_task()` stores the UUID in `task.parent_id`; `load_tasks()` may restore that value as the task ID, preserving the incompatible representation.
- `spawn_loop()` duplicates readiness/expiry logic already present in `tick()`.
- several async database operations occur while the in-memory task write lock is held.
- `CoreRuntimeDeps::LegacyAgentRuntimeDeps` already labels the scheduler transitional and sets `bg_scheduler_compat_enabled = false` for production SQLite daemons.

These facts make repair of the second scheduler implementation a non-goal unless caller evidence proves that a compatibility adapter must remain.

## 3. Explicit non-goals

Do not:

- redesign durable scheduling, admission control, job attempts, recovery leases, or scheduler resource budgets;
- add a third task identifier or compatibility persistence table;
- preserve the old timer loop merely because tests instantiate it;
- introduce a new cron grammar or scheduling feature;
- change public wire semantics without evidence and a separate protocol decision;
- add a daemon-local timer implementation parallel to `ScheduleStore`;
- add CI lanes or scheduled workflow tests.

## 4. Invariants that cannot regress

- production daemon work remains scheduler-governed;
- schedule persistence is durable through the existing store abstraction;
- scheduler/job identifiers remain typed consistently end to end;
- restart/recovery semantics remain owned by durable scheduler/job infrastructure;
- child/subagent authority remains bounded by existing execution context/policy;
- no direct process dispatch is introduced to bypass `JobSubmissionService` or the canonical scheduler;
- local/test compatibility constructors may remain, but they cannot become a second production authority.

## 5. Ordered work packages

### A. Caller and protocol inventory

1. Search all constructors/usages of `BackgroundScheduler`, `BackgroundTask`, `spawn_loop`, and `bg_scheduler_compat_enabled`.
2. Enumerate daemon/native protocol request handlers that reference the legacy scheduler.
3. Classify each caller as production, supported compatibility, test-only, or dead.
4. Record the exact compatibility semantics that must survive before deleting code.
5. If a public supported client genuinely depends on behavior that cannot map to durable scheduling without a protocol change, stop this milestone and document the blocker.

### B. Durable compatibility mapping

If compatibility requests remain supported:

1. map schedule creation directly onto the existing durable schedule/job API;
2. map list/delete onto the same store/service rather than an in-memory vector;
3. use durable typed identifiers in responses;
4. preserve authorization/session/workspace binding;
5. ensure schedule creation is atomic enough that persistence and enqueue semantics cannot diverge;
6. avoid translating durable IDs through UUID-string/`u64` parsing hacks.

If no supported compatibility caller remains, delete the request handlers or reject them through the repository's existing explicit unsupported/deprecated mechanism according to protocol compatibility policy.

### C. Delete independent scheduler implementation

Delete or reduce `src/agent/task.rs` so it no longer contains:

- an independent timer loop;
- independent task expiry semantics;
- independent database interpretation of the `task` table;
- independent callback/spawner dispatch;
- duplicated `tick()`/`spawn_loop()` readiness logic.

Delete now-unused `LegacyAgentRuntimeDeps` fields and constructors where source compatibility is not required. If a compatibility field must remain, it should hold a durable scheduler adapter/facade, not `BackgroundScheduler`.

### D. Correct concurrency/lifecycle boundaries

For any compatibility adapter that remains:

- never hold an async RwLock guard across database/network/process awaits;
- use store/service methods as the durable source of truth;
- propagate cancellation/shutdown through the existing scheduler lifecycle;
- do not create detached timer tasks that outlive daemon generation ownership.

### E. Tests and documentation

Add focused regression coverage proving:

- a scheduled compatibility request, if retained, creates a durable schedule/job and can actually dispatch;
- list/delete operate on the durable record;
- restart/load does not reconstruct an incompatible alternate ID;
- production `CoreRuntimeDeps::with_jobs` does not instantiate/use a legacy scheduler loop;
- no UUID-to-`u64` parsing path remains in scheduling;
- existing durable schedule behavior remains unchanged.

Update `architecture/scheduler.md` and `architecture/agent.md` to remove any claim that two scheduler implementations are production-owned.

Update execution-ownership documentation/manifest only if source ownership truly changed. Do not add a new scanner.

## 6. Storage, protocol, compatibility, migration

Storage:

- no schema migration is expected;
- reuse `ScheduleStore` / `JobStore` and current schedule schema;
- do not repurpose unrelated `task` rows in an undocumented manner.

Protocol:

- preserve supported request/response semantics where possible by adapting to durable scheduling;
- internal implementation types may disappear;
- if deletion of a public request is proposed, stop and require explicit compatibility decision rather than silently removing it.

Migration:

- no user/operator migration;
- stale legacy in-memory tasks have no production durability guarantee and should not motivate new migration machinery.

## 7. Concurrency, cancellation, restart, failure semantics

Concurrency:

- durable scheduler/store remains authoritative under concurrent schedule create/delete/list;
- no lock may span an await unless the lock is explicitly designed for that operation.

Cancellation/shutdown:

- daemon shutdown must use existing scheduler cancellation/shutdown semantics;
- compatibility calls must not spawn untracked detached loops.

Restart:

- restart behavior derives only from durable schedule/job recovery policy.

Failure:

- schedule persistence/enqueue failures return typed/actionable errors;
- do not leave a compatibility object in memory when durable creation failed.

## 8. Verification

Focused commands should be selected after caller inventory, but expected minimum is:

```bash
cargo test -p codegg-core jobs::schedule -- --nocapture
cargo test -p codegg --lib scheduler -- --nocapture
cargo test -p codegg --lib agent::task -- --nocapture   # only if module remains
scripts/verify.sh quick
git diff --check
```

Run protocol/daemon integration tests that cover `TaskSchedule`/`TaskDelete`/`TaskList` if those handlers are modified.

Do not require broad all-features or new hosted lanes for M001.

## 9. Explicit acceptance criteria

M001 is complete only when all are true:

1. Production SQLite daemon construction no longer owns an independent `BackgroundScheduler` loop.
2. `BackgroundTask` UUID values are never parsed as `u64` to dispatch work.
3. There is exactly one production persistence interpretation for scheduled work: the durable schedule/job store path.
4. A retained compatibility request is a thin adapter to durable scheduler services and has no independent timer, expiry, callback, or dispatch loop.
5. No compatibility adapter holds an in-memory task lock across asynchronous storage/process work.
6. Restart/recovery of scheduled work uses the existing durable scheduler recovery model only.
7. Existing scheduler admission/resource authority is unchanged.
8. Focused schedule/compatibility tests pass.
9. `scripts/verify.sh quick` passes on the final candidate.
10. Architecture docs identify the durable scheduler as the sole production scheduling owner.
11. No new CI workflow, schema, scheduler framework, or static scanner was added.
12. Closure evidence records every retained legacy symbol and the concrete caller that still requires it; unexplained legacy scheduling code is not acceptable.

## 10. Stop conditions

Stop and report blocked/corrective status if:

- a supported external client requires legacy wire semantics that cannot map to the durable API without protocol change;
- the durable scheduler lacks a required capability and implementing it would materially expand this milestone;
- deletion would weaken authorization, workspace binding, restart recovery, or scheduler admission control.

Do not paper over such a conflict with another compatibility scheduler.
