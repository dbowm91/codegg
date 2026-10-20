# Eggwork Worker Telemetry and Targeting M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggwork-worker-telemetry-and-targeting/004-production-schedule-materialization-and-occurrence-idempotency.md`

Source subsystem roadmap:

- `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`

Repository baseline reviewed: `51d1a0b7` (branch `planning/eggwork-targeting-and-telemetry-m001-m003`)

Implementation commits or pull requests:

- `25af0c05` — feat(scheduler): materialize schedule occurrences exactly once (Eggwork M004)

## 1. Executive finding

The milestone's capability boundary is complete: a schedule now fires, and
an occurrence owns exactly one job, across a daemon crash, with the target
it was created with and with the administrator's approval re-checked at
firing time.

The gap was total, not partial. Before this milestone nothing in
production called `ScheduleStore::claim_due` — its only call sites were
tests — so a recurring schedule was durable data with no trigger. M001
had given `JobTemplate` a target and proved that the target reached the
durable job spec; the path that *creates* that job existed only inside
test materializers. M001's own closure §10 recorded the consequence as a
low-severity residual ("the materializer's job creation and the
occurrence insert are not in one transaction today… dormant, because no
production materializer is wired into the daemon"), and this milestone
is the one that made it live and then removed the wrinkle.

Two defects that the dormant state had hidden were fixed rather than
inherited:

- **A refused occurrence was consumed without running.** The materializer
  error was swallowed (`.ok()`) and the occurrence was still written, with
  a `NULL` job id and status `queued`. Because the occurrence row's
  `(schedule_id, scheduled_for)` primary key is the durable "this already
  happened" marker, the later retry was a no-op: the occurrence was lost
  outright. This was reachable in production precisely because the
  materializer could fail — it was not a theoretical path.
- **The cursor drifted off the schedule's grid.** `last_occurrence_at`
  advanced to the *tick time* rather than to the occurrence just fired, so
  an interval schedule anchored at `T0` left its own grid after its first
  tick and permanently skipped the occurrence at `T0+120`.

The occurrence is now idempotent because its job id is a pure function of
`(schedule_id, scheduled_for)` and the job store derives a scheduled job's
id from its source — so the `job.id` primary key *is* the durable
uniqueness constraint, and `JobStore::create_job` is idempotent on it.
That is a database guarantee rather than a convention, and it is what
makes the tick safe to split into separate phases.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A due occurrence produces exactly one durable job and one occurrence row | `tests/schedule_materialization.rs::occurrence_materializes_one_durable_job_and_is_recorded` | pass | The job's id equals `occurrence_job_id(schedule_id, occurrence)`; the job records its `schedule_id`. |
| A crash between the job row and the occurrence row leaves exactly one job | `tests/schedule_materialization.rs::a_crash_after_the_job_row_leaves_exactly_one_job` | pass | The job row exists with no occurrence row; the replay converges on the same job id; the job table holds one row. |
| The job store refuses to create a second job for one occurrence | `tests/schedule_materialization.rs::the_job_store_treats_a_replayed_occurrence_as_the_same_job` | pass | The second `create_job` returns the same record; the table holds one row. |
| A refused occurrence is not recorded and stays due | `tests/schedule_materialization.rs::a_refused_occurrence_is_not_recorded_and_stays_due` | pass | No job, no occurrence row, cursor unmoved, one-shot not completed; the next tick materializes it. The refusal carries the stable reason code `test_refused`. |
| A recurring schedule keeps its own grid | `tests/schedule_materialization.rs::a_recurring_schedule_keeps_its_own_grid` | pass | The occurrence is on the anchor's grid and the cursor records the occurrence, not the tick time. |
| **The daemon wires the whole path** | `tests/schedule_materialization.rs::the_daemon_fires_a_schedule_it_created` | pass | `CoreDaemon::with_deps` installs the driver; a `ScheduleCreate` request through the daemon's own surface is claimed by that driver; the job is a real durable row whose id is the occurrence's derived identity and whose `schedule_id` is the schedule that produced it. |
| The target is carried into the materialized job | `tests/schedule_materialization.rs::an_approved_node_materializes_the_remote_job` | pass | `target_kind = eggwork_node`, `target_node_id = builder`. |
| The host boundary is re-applied at materialization time, with no local fallback | `tests/schedule_materialization.rs::an_unapproved_node_refuses_the_occurrence_and_creates_no_local_job` | pass | No node approved ⇒ the occurrence is refused and the job table is empty. |
| The driver claims due occurrences, claims nothing when nothing is due, and stops on shutdown | `tests/schedule_materialization.rs::the_driver_claims_due_occurrences_and_stops_on_shutdown` | pass | One claim, then an empty tick, then shutdown. |
| A paused schedule materializes nothing | `tests/schedule_materialization.rs::a_paused_schedule_is_not_materialized` | pass | No job. |
| The occurrence's job goes through the single production admission facade | `src/scheduler/schedule_materializer.rs` → `JobSubmissionService::submit` | pass | The facade validates the payload, acquires the workspace lease, applies the canonical resource profile, creates one durable record, and wakes the scheduler. |
| The driver creates work; the scheduler runs it | `src/scheduler/schedule_driver.rs`; `architecture/scheduler.md` | pass | The driver owns no executor and no queue; the durable job store is the queue the scheduler reconciles from. |
| The commit transaction takes the write lock at BEGIN | `scripts/check_queue_write_transaction_shape.py` | pass | `ImmediateWrite::begin_with` → `BEGIN IMMEDIATE`. |
| No scheduler bypass, execution-ownership or target-routing violation | `scripts/check_scheduler_bypass.py`, `scripts/check_execution_ownership.py`, `scripts/check_eggwork_target_routing.py` | pass | The new modules live under `src/scheduler/` and construct no target and no executor. |
| The whole workspace still compiles and the canonical gate passes | `scripts/verify.sh quick` | pass | "Quick verification passed." (exit 0). |
| The workspace clippy gate | `cargo clippy --workspace --all-targets --locked` | fail (pre-existing) | **No finding is in any file this milestone changed** (verified by diffing the finding locations against the pre-milestone baseline). The findings are in files this milestone never touched. |
| The pre-existing `/nodes` registry defect | `tests/schedule_materialization.rs` sibling: `src/tui/command.rs::every_builtin_action_is_referenced_by_a_canonical_command` | pass (corrected here) | M002 added `BuiltinSlashAction::Nodes` to the enum and the registry but omitted it from `all_builtin_variants()`, so the "no surplus" arm failed with 159 registry variants against 158 listed ones. Corrected here; see §9. |

## 3. Production implementation evidence

**Ownership.** The materialization driver is daemon-owned
(`CoreDaemon::schedule_driver`), installed in `src/core/daemon_construct.rs`
and stopped in `src/core/daemon_shutdown.rs`. Exactly one loop per
daemon; it is held as an `Arc` so `wake()` and `request_shutdown()` stay
reachable, and the handle is kept so the task stays alive.

**The trigger.** `src/scheduler/schedule_driver.rs` sleeps until the
earliest durable due horizon, performs one bounded claim, and exits on
shutdown. `ScheduleCreate` and `ScheduleResume` wake it, so a schedule
that is already due fires promptly rather than waiting out the sleep.
The sleep is bounded (60 s ceiling, 250 ms floor) so a horizon
miscomputation or a clock jump cannot park the loop indefinitely.

**The materializer.** `src/scheduler/schedule_materializer.rs` implements
`OccurrenceMaterializer`. It computes the occurrence's job id, returns the
existing job if the occurrence was already materialized (the crash-replay
path), re-applies the host-boundary gates, builds the durable spec, and
submits through `JobSubmissionService` with a submission key derived from
the occurrence. A concurrent materialization of the same occurrence
converges on one job rather than racing to create two.

**The occurrence identity.** `occurrence_job_id` in
`crates/codegg-core/src/jobs/schedule.rs`; the job store derives a
scheduled job's id from `JobSource::Scheduled` in
`crates/codegg-core/src/jobs/store.rs`. The identity is *derived* — no
column, no migration.

**Refusal semantics.** `OccurrenceStatus::Failed` and
`ClaimedOccurrence::job_id: Option<JobId>` make "this occurrence
produced no job" explicit. The driver logs a refused occurrence with the
schedule id only — no reason detail, no endpoint, no policy text.

**The tick.** `claim_due` in `crates/codegg-core/src/jobs/schedule_store.rs`
runs in three phases (plan → materialize → commit) in both the in-memory
and SQLite stores. The commit transaction opens with `BEGIN IMMEDIATE`.

**Planned but absent.** Nothing about *remote* execution changed in this
milestone: the remote target still reaches the executor's own preflight
and posture agreement exactly as M001/M003 left it. What is new is that a
remote occurrence is now materialized at all, and that the host boundary
is re-entered on the way.

## 4. Verification executed

### Commands run

```bash
scripts/verify.sh quick

cargo test --locked --test schedule_materialization
cargo test --locked --test durable_jobs_phase4 --test scheduler_restart_recovery --test schedule_materialization
cargo test --locked -p codegg-core
cargo test --locked --lib
cargo test --locked -p codegg-client -p codegg-protocol

python3 scripts/check_eggwork_target_routing.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_queue_write_transaction_shape.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_adr_integrity.py

cargo fmt --all -- --check
git diff --check
```

### Results

| Command | Result |
|---|---|
| `scripts/verify.sh quick` | **pass** — "Quick verification passed." (exit 0), re-run after the final edits |
| `cargo test --locked --test schedule_materialization` | **pass** — 15 passed, 0 failed |
| `cargo test --locked --test durable_jobs_phase4 --test scheduler_restart_recovery --test schedule_materialization` | **pass** — 45 + 15 + 15 passed, 0 failed |
| `cargo test --locked -p codegg-core` | **pass** — 878 lib tests passed; every suite passed, 0 failed |
| `cargo test --locked --lib` (whole lib) | **6686 passed, 5 failed** — the five are exactly the pre-existing failures the M003 closure recorded with a baseline proof (two macOS `/tmp` workspace-root rejections, one drifted tool-advisor receipt fingerprint, two load-sensitive TUI terminal tests). The M003 sweep recorded **six**; the sixth, `tui::command::tests::every_builtin_action_is_referenced_by_a_canonical_command`, was the `/nodes` defect corrected in this milestone (§9). |
| `cargo test --locked -p codegg-client -p codegg-protocol` | **pass** — 33 + 9 + 16 + 259 passed, 0 failed |
| `cargo clippy --workspace --all-targets --locked` | **findings, none in a file this milestone changed** — the finding locations were diffed against the pre-milestone baseline in an isolated worktree, and the set of locations attributable to this milestone is empty |
| all six guards | **pass** |
| `cargo fmt --all -- --check`, `git diff --check` | **pass** — clean |

The first `verify.sh quick` run failed at the `fmt` step on two
formattings in the new test file; `cargo fmt --all` was applied and the
gate was re-run to a pass. No test or guard failed at any point.

The **Linux live fixture** was **not run**: this host is macOS, and the
Eggwork live fixture is `#![cfg(target_os = "linux")]`-gated. It is
untouched by this milestone and carries the same outstanding qualification
as M001/M002/M003.

## 5. Invariant review

| Plan invariant | Evidence | Holds |
|---|---|---|
| Scheduler and `JobSubmissionService` remain the sole execution admission; no second queue | The driver creates a job through `JobSubmissionService` and owns no executor; the durable job store is the queue `JobScheduler::reconcile` pulls from | yes |
| No silent local fallback for a remote occurrence | `an_unapproved_node_refuses_the_occurrence_and_creates_no_local_job` — the job table is empty after a refused remote occurrence | yes |
| Explicit named node persists durably and uses configuration-owned endpoints and mTLS only | Unchanged by this milestone; the target travels as a node id and the credentials stay daemon-owned config | yes |
| Existing local jobs/schedules and serialized DTOs remain local when the target is omitted | `JobTemplate.target` is `#[serde(default)]` → `Local`; no wire change in this milestone | yes |
| The occurrence's job id carries no endpoint, credential path, or node detail | `occurrence_job_id` composes a schedule id and an instant only | yes |
| Refusals carry no secret | `MaterializerError::Refused { code }` carries a slug; the submission error's `Display` (which can hold a workspace path or a payload fragment) is never logged or persisted — `submission_error_code` maps it to a slug, and only the slug reaches `ClaimedOccurrence::failure_code` and the log | yes |
| No schema migration | `STORAGE_LAYOUT_VERSION` is unchanged; the occurrence identity is derived; `schedule_occurrence` keeps its shape | yes |
| The principal gate is not re-derived | The materializer applies shape, administrator and resource-ceiling gates, and no principal gate — a timer-driven occurrence has no live principal | yes |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** The occurrence's job id is a
  pure function of `(schedule_id, scheduled_for)`; the `job.id` primary
  key is the durable uniqueness constraint; `JobStore::create_job` returns
  the existing job on a conflict instead of failing or overwriting. A
  replayed materialization converges on one job. Proven by
  `a_crash_after_the_job_row_leaves_exactly_one_job` and
  `the_job_store_treats_a_replayed_occurrence_as_the_same_job`.
- **Partial persistence failure.** The job row is written before the
  occurrence row, and the occurrence's identity is idempotent, so a crash
  in that window replays into the same job. The reverse order was
  deliberately *not* used: an occurrence row written without a job would
  consume the occurrence, and its primary key would make the retry a
  no-op.
- **Daemon restart.** The driver is daemon-lifecycle-owned and claims the
  due schedules on its first tick after a restart; a schedule whose cursor
  did not advance is re-planned and converges on the same job.
- **Cancellation races.** Unchanged. The milestone does not cancel jobs;
  the overlap policy still skips an occurrence while the schedule's job
  is running, and the skip is recorded so it is never re-decided.
- **Stale generation or lease.** Unchanged — `DaemonGeneration` still
  gates the submission facade, and the materializer passes the daemon's
  own generation through it.
- **Contention and resource release.** The materializer goes through the
  submission facade, so the workspace lease and the canonical resource
  profile are applied exactly as for a submitted job. The node-scoped
  remote reservation from M003 is unaffected.
- **Malformed or unauthorized input.** An unapproved node, an
  unknown node, a below-ceiling posture, a widened resource limit, and a
  remotely-unrunnable kind are all refused at materialization time with a
  typed code, before any job becomes durable.
- **Bounded event and artifact behavior.** The driver performs one
  bounded claim per tick and sleeps; it does not poll. Its sleep is
  bounded on both ends.

## 7. Migration and compatibility review

- **Schema migration.** None. `STORAGE_LAYOUT_VERSION` is unchanged. The
  occurrence identity is *derived* from data that already existed
  (`schedule.id` and the occurrence instant) rather than stored, and the
  `schedule_occurrence` table keeps its columns and its
  `(schedule_id, scheduled_for)` primary key.
- **Backward compatibility.** A schedule persisted before this milestone
  materializes exactly as it did: its template carries the target, the
  job carries the target, and its occurrence identity is derived from
  data that predates the milestone. No wire format changed — no request,
  response, event, DTO, or configuration key was added or altered.
- **Protocol negotiation.** Not applicable — no protocol change.
- **Configuration behavior.** Unchanged. `[eggwork.allowed_target_nodes]`
  and `[eggwork.nodes.<id>]` are read at daemon construction exactly as
  before; the milestone reads the *already-resolved* policy again at
  materialization time.
- **Rollback limitations.** Rolling back the milestone leaves a durable
  schedule that nothing claims — the pre-milestone behavior. Durable jobs
  created by a rolled-back milestone keep their deterministic `occ-…` ids,
  which are opaque and self-describing.
- **Legacy-path status.** The in-memory schedule store keeps the same
  three-phase semantics as the SQLite store; both were changed together so
  the two cannot disagree.

## 8. Security review

- **Authorization enforcement.** The host boundary is re-applied at
  materialization time: the remotely-runnable shape
  (`validate_remote_job`), the administrator's approval
  (`RemoteTargetPolicy::check`), and the operator's resource ceiling
  (`check_resource_request`). The checks run in that order, so an
  unauthorized occurrence learns nothing about the configured fleet beyond
  the fact that it was refused.
- **Privilege boundary.** The *principal* gate is deliberately not
  re-applied: a timer-driven occurrence has no live principal, and
  re-deriving one would manufacture authority rather than check it. The
  principal's authority was established when the schedule was created.
- **Secret handling.** The occurrence's job id contains a schedule id and
  an instant — no endpoint, no credential path, no node detail. A refusal
  carries a slug (`remote_target_not_approved`, `invalid_template`,
  `job_store_error`, …), never a message that could contain a path or a
  payload fragment.
- **Path validation.** Unchanged; the milestone does not resolve or
  validate any path.
- **Denial-of-service bounds.** One bounded claim per tick; the sleep is
  bounded at both ends; a refused occurrence does not spin (the schedule's
  next horizon stays at the refused occurrence, so the retry rate is the
  schedule's own period, not the tick rate).
- **Audit behavior.** The materializer creates no separate durable record
  and does not bypass the submission facade, so the existing job-creation
  audit path applies unchanged. No new operation was added, so the
  authorized-operation count is unchanged.
- **Redaction.** `tracing` output from the driver carries the schedule id
  and the job id only.

## 9. Documentation and operations

- `architecture/jobs.md` — the three-phase `claim_due`, the occurrence
  identity, the refusal semantics, the grid alignment, and the production
  materializer (including the host-boundary re-check and the deliberate
  non-re-derivation of the principal).
- `architecture/scheduler.md` — the driver's ownership boundary against
  `JobScheduler`.
- `architecture/overview.md` — the verified-counts table was remeasured
  against **every** listed `Source`. Seven rows had drifted and were
  corrected: architecture docs (86 → 87), `CoreEvent` variants (88 → 92),
  tool source files (78 → 80), TUI state modules (31/32 → 32/33),
  storage layout version (79 → 82), CI guard scripts (53 → 57), and
  integration test files (267 → 268, this milestone's new file). The
  remaining rows were confirmed unchanged.
- `architecture/command.md` — the `BuiltinSlashAction` variant count was
  corrected (158 → **159**) in both places it is claimed, and the
  correction is recorded rather than made quietly: M002 added `/nodes` to
  the enum and the registry but omitted it from `all_builtin_variants()`,
  so the registry referenced 159 distinct builtin actions while the
  exhaustive list held 158, and the "no surplus" arm of
  `every_builtin_action_is_referenced_by_a_canonical_command` failed.
  M002's closure recorded that both count-guard tests passed; they did
  not. The registry-length guard cannot see this class of defect because
  it asserts a count against a *different* count.
- `plans/implementation/eggwork-worker-telemetry-and-targeting/004-…md` —
  the milestone plan.
- `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md` —
  M004 added, the dependency graph extended, and a milestone-number
  disambiguation added (this line's M004 is not the fixed-target line's
  deferred M004, nor the `async-background-jobs` line's closed M004).
- `plans/registry.md` — the subsystem row corrected (it still described
  M002 as "ready for handoff" after M002 had closed) and the M004 closure
  row added.
- **Operator diagnostics.** A refused occurrence is logged by the driver
  with the schedule id; the schedule's own state, cursor and occurrence
  rows remain visible through the existing schedule read/list surfaces.
  There is no new command.
- **Recovery instructions.** If a schedule appears stuck, read its
  `next_run_at` and `last_occurrence_at`: a schedule whose cursor did not
  advance past a refused occurrence is *by design* still due, and the next
  tick retries it. A schedule with no occurrence rows and a due horizon
  means the materializer refused every attempt; the refusal code is in
  the daemon log.
- **Static guards.** No guard was widened. `check_queue_write_transaction_shape.py`
  still pins `ImmediateWrite` to `user_prompt_queue.rs`; the schedule store
  reuses it through `begin_with`/`commit_with` without moving it.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | A refused occurrence leaves **no durable trace**: it is not recorded, so the schedule's durable state cannot answer "why did my schedule not fire?". The reason code reaches the daemon log only. | An operator with a schedule that never runs sees an idle schedule and a log line. The refusal is visible only to someone reading the daemon log at the moment of the tick. | Record the refusal durably — an occurrence row with a `failed` status that the claim path can re-claim, or a `last_error` column on the schedule — and expose it on the existing schedule read surface. The "not recorded" property is what keeps the retry working today, so this is a semantics change, not a field to bolt on. |
| medium | The `MissedRunPolicy::Skip` arm makes `missed_run_targets` return **no** candidates, so a schedule created with `skip` never fires at all. The policy is reachable on the wire (`missed_run_policy` is a `serde_json::Value` in the DTO) but no in-repo producer sets it — every in-repo creator uses `RunOnceNow`. | A schedule created with `skip` by an external client silently never runs. The three policies are not distinct: `Skip` currently means "never". | Fix the policy's semantics (an on-time occurrence must still fire; only *missed* ones are skipped) and add a test per policy. It is a pre-existing defect, **not** introduced by this milestone, and it was left unchanged deliberately: changing policy semantics is a separate decision from wiring the trigger, and the milestone's own scope is the trigger plus the idempotency. |
| medium | The Linux live fixture was **not executed** on this host (macOS; the fixture is `#![cfg(target_os = "linux")]`-gated). | The live remote path remains qualified only by the Linux CI job. | Run the Linux CI job for the commit; the fixture is untouched by this milestone. |
| medium | The upstream client's new transport deadlines (120 s unary total; 60 s per-chunk-gap read on streaming `execute`/`events`/`renew`) change the failure modes of the **existing** M001/M003 fixed-target remote path. | The fixed-target path needs requalification on Linux; a schedule that targets a remote node inherits those deadlines. | Requalify the fixed-target remote path on Linux (carried forward from M002's closure). |
| low | A remote occurrence's materialization performs the host-boundary checks, but the **node's live posture** is still checked only later, in the executor's preflight — so a node that went offline between the schedule's creation and the occurrence's firing is refused at execution, not at materialization. | The occurrence is recorded as queued and then fails at dispatch. This matches M001's contract (creation-time validation plus execution-time policy recheck) and is not a regression. | None. Recorded so the boundary is not mistaken for a materialization-time guarantee. |
| low | The whole-lib sweep has five pre-existing failures (two macOS `/tmp` workspace-root rejections, one drifted tool-advisor receipt fingerprint, two load-sensitive TUI terminal tests). The M003 closure recorded six; this milestone repaired the sixth (the `/nodes` registry defect). | Unrelated to this milestone's own scope. | Repair separately; recorded in §4 with the baseline proof the M003 closure already established. |
| low | The workspace clippy gate is red on findings in files this milestone never touched. The finding locations were diffed against the pre-milestone baseline and the set attributable to this milestone is empty. | The branch-level `-D warnings` gate is red for unrelated reasons. | Repair separately; the M001/M003 precedent is to report, not repair, out-of-scope clippy debt. |
| low | `ImmediateWrite` is now shared by two stores, but the C002 guard pins it to `user_prompt_queue.rs`, so the helper cannot move to a neutral home without the guard being widened. | A shared primitive lives in the module of the store that first needed it. | Accepted. The guard's pinning is deliberate; widening it is a recorded decision, not a quiet change. |

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.** Every milestone on
the `eggwork-worker-telemetry-and-targeting` line (M001–M004) is now
closed, and M004 delivers the capability M001's closure deferred. No
downstream milestone on this line became newly eligible: the remaining
roadmap items (cross-node resource-aware placement, automatic multi-node
placement, whole remote `AgentRun`/`Subagent`/`Test`) are explicitly
deferred and require separate scoped planning.

The `eggwork-fixed-target-remote-execution` line's M004 (whole remote
`AgentRun`) remains deferred and unregistered; this milestone does not
name, close, or unblock it.

## 12. Registry updates

- `plans/registry.md` — the subsystem row for "Eggwork worker targeting
  and host telemetry" now reads "**active planning; M001 closed, M002
  closed, M003 closed, M004 closed**"; its "Current milestone" column
  previously described M002 as "**ready for handoff**" after M002 had
  already closed, and now lists all four closures; the blockers column
  records what M004 landed and states the fixed-target line's remote
  AgentRun M004 stays deferred.
- `plans/registry.md` — a closure row for M004 was added to "Recently
  closed or conditionally closed work", with an unblock audit noting that
  nothing became newly eligible.
- `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md` —
  status line, dependency graph, milestone list, and the
  milestone-number disambiguation.
