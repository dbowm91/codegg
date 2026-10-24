# Repository Audit Corrective Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/repository-audit-corrective/004-job-state-terminality-and-status-projection.md`

Source subsystem roadmap:

- `plans/subsystems/repository-audit-corrective-roadmap.md`

Repository baseline reviewed: `8590c17c5110e83ed4aaddbb272ae62a1930e69d`

Implementation commits or pull requests:

- `fix(core): align job-state projections with the domain terminality` — the domain's terminality becomes the one authority; the session projection's hand-written "terminal" list is replaced by the domain's derived sets; the three host-evidence copies become one shared function; the model-facing status distinguishes the four unstarted states; a property test, a guard, and the architecture note land with it.

## 1. Executive finding

The milestone's capability boundary is complete. `JobState::is_terminal()` was
already the domain's authority and already decided the disputed question;
nothing was redefined. What changed is that every projection of a job state
now asks that authority instead of keeping its own list:

- the session projection splits a session's jobs with
  `JobState::terminal_states()` / `JobState::non_terminal_states()` instead
  of a hand-written list that called `Interrupted` terminal;
- the three host-evidence copies became one function whose name states the
  question it answers, with **unchanged** verdicts;
- the model-facing status distinguishes `Scheduled`, `Queued`, `Running` and
  `Blocked` instead of reporting all four as "running".

The scratch sweep stays conservative: a session holding an interrupted job
still keeps its scratch, because an interrupted attempt may still be
requeued. No ADR was required.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP1 — every terminality claim about a job enumerated | §3 inventory below | pass | 17 sites, each with the question it asks |
| WP2 — the requeue path read and cited | §4 requeue-path analysis | pass | An interrupted job **can** become live; the sweep's set is right, the projection's was wrong |
| WP3 — one evidence function, three call sites, unchanged verdicts | `crates/codegg-core/src/jobs/evidence.rs::job_success_evidence`; `src/goal_verification.rs`, `src/goal_continuation.rs`, `src/work_plan_evidence.rs` | pass | Verdicts unchanged; the two pinned fail-closed tests still pass unchanged |
| WP4 — the model-facing status distinguishes the four unstarted states | `src/scheduler/job_notifications.rs::job_status_projection` | pass | `scheduled` / `queued` / `running` / `blocked`; every consuming surface checked (§5) |
| WP5a — a property test over the job-state enum | `crates/codegg-core/src/jobs/mod.rs` (3 unit tests), `crates/codegg-core/src/jobs/evidence.rs` (3 unit tests), `tests/core_daemon/job_state_terminality_c004.inc` (2 tests) | pass | §6 records the captured baseline failure |
| WP5b — a guard that fails on a hand-written list and on an uncovered variant | `scripts/check_job_state_terminality_agreement.py` | pass | §7 records the `--self-test` output; wired into `scripts/verify.sh` and `.github/workflows/ci.yml` |
| WP6 — the architecture note | `architecture/core.md` §"Job-state terminality and its three projections" | pass | States the authority, the different evidence question, and the distinct status projection; links the guard |
| §13.1 — the four unstarted states are distinguishable | `tests/scheduler_jobs/job_status_projection_c004.inc` | pass | |
| §13.2 — `Interrupted` is not reported as terminal | `tests/scheduler_jobs/job_status_projection_c004.inc::an_interrupted_job_is_not_reported_as_a_terminal_one`; `crates/codegg-core/src/jobs/mod.rs` unit tests | pass | |
| §13.3 — the terminal-jobs query and the live-job query agree with `is_terminal()` | `tests/core_daemon/job_state_terminality_c004.inc::the_session_projection_queries_agree_with_the_domain_terminality` | pass | |
| §13.4 — a session whose only job was interrupted: reclamation decided, cited and pinned | `src/core/scratch_sweep.rs::the_sweep_refuses_a_session_with_an_interrupted_job` | pass | The **opposite** behaviour (scratch **not** reclaimed) is the decided one; §4 cites the requeue path |
| §13.5 — the evidence verdicts are unchanged | `src/goal_verification.rs::terminal_job_states_are_fail_closed`; `src/work_plan_evidence.rs::job_state_mapping_is_fail_closed`; `crates/codegg-core/src/jobs/evidence.rs` unit tests | pass | The two pinned test bodies are byte-identical; only a delegating helper was added beside them |
| §13.6 — no ADR required, and none raised | This record; the plan's §2 | pass | The domain's authority already existed and already decided |

## 3. WP1 — the inventory of job-state terminality claims

Every site in the tree that makes a terminality claim about a job, and the
question it asks. "Agrees" means the site's answer is the domain's answer
for the question it actually asks.

| # | Site | The question it asks | Agrees with the domain |
|---|---|---|---|
| 1 | `crates/codegg-core/src/jobs/mod.rs:1058` `JobState::is_terminal()` | Is the job finished? | **the authority** (`Completed`, `Failed`, `Cancelled`, `TimedOut`, `Expired`) |
| 2 | `crates/codegg-core/src/jobs/mod.rs:1113` `AttemptState::is_terminal()` | Is the *attempt* finished? | own type, own contract — not a `JobState` projection |
| 3 | `crates/codegg-core/src/jobs/store.rs:119` `job_state_transitions` | Which transitions are legal? | not terminality — a transition table (`Interrupted => [Queued]`) |
| 4 | `crates/codegg-core/src/jobs/store.rs:2250` `eligible_states` | May this job get another attempt? | different question — includes two terminal states on purpose |
| 5 | `crates/codegg-core/src/jobs/store.rs:2153` cancel precondition | From which states may a cancel be requested? | different question |
| 6 | `crates/codegg-core/src/jobs/store.rs:2401` `recover_generation` | Skip the jobs that are already finished? | yes — it calls the domain predicate |
| 7 | `crates/codegg-core/src/jobs/store.rs:2416` requeue eligibility | May this stale job be requeued? | different question — `Interrupted` is eligible |
| 8 | `crates/codegg-core/src/jobs/store.rs:2426` requeue SQL | Which rows may be parked on `interrupted`? | yes — the SQL list is the domain's terminal set |
| 9 | `src/core/daemon_refresh.rs:206` live-jobs query | May this job still run? | **no, before** — it omitted `Interrupted` |
| 10 | `src/core/daemon_refresh.rs:251` `terminal_jobs` query | Is this job finished? | **no, before** — it included `Interrupted` |
| 11 | `src/core/scratch_sweep.rs:578` live-job query | May this job still run? | yes — `Interrupted` is included |
| 12 | `src/core/daemon_sessions.rs:1332` live-job query | May this job still run? | yes — `Interrupted` is included |
| 13 | `src/scheduler/job_notifications.rs:1731` `is_final_job_state` | Is the job finished (so it notifies)? | yes — the same set as the domain |
| 14 | `src/scheduler/job_notifications.rs:1745` `terminal_status_str` | What is the status string? | not terminality — a projection, misnamed; flattened four states |
| 15 | `src/scheduler/job_notifications.rs:1046` backfill query | Which jobs are finished? | yes — the same set as the domain |
| 16 | `src/goal_verification.rs:77`, `src/goal_continuation.rs:113`, `src/work_plan_evidence.rs:139` | Does this job count as **evidence that the work succeeded**? | yes for that question — and deliberately **not** terminality |
| 17 | `src/scheduler/scheduler.rs:1059` Python-orphan query | Which jobs still hold their Python source? | **no, before** — it omitted `Interrupted` while claiming "all states except terminal" |
| 18 | `src/scheduler/scheduler.rs:1922` cancel-pending query | Which jobs are pending? | different question — a deliberate narrowing |
| 19 | `src/scheduler/scheduler.rs:834`, `:2220`; `src/core/daemon.rs:4836`; `src/core/daemon_construct.rs:127` | Which executor status / agent-run outcome / event / failure class? | different questions — they consume the domain's predicate or map to their own types |

Sites 9, 10 and 17 were the divergent ones. Sites 13 and 15 were
hand-maintained copies of the domain's set and are now derived.

## 4. WP2 — the requeue-path analysis

**The question.** Is an interrupted job still live for reclamation purposes?

**The code path.**

1. `crates/codegg-core/src/jobs/store.rs:119` — `job_state_transitions`
   allows `JobState::Interrupted => &[JobState::Queued]`.
2. `crates/codegg-core/src/jobs/store.rs:2401` — `recover_generation` skips
   only the jobs the domain calls terminal
   (`JobState::from_str_lossy(&current_state).is_terminal()`), so an
   interrupted job is *not* skipped.
3. `crates/codegg-core/src/jobs/store.rs:2415-2418` — a job is requeued when
   `wants_requeue` and `current == JobState::Interrupted || validate_state_transition(current, JobState::Interrupted).is_ok()`.
4. `crates/codegg-core/src/jobs/store.rs:2438-2448` — the requeue itself:
   `UPDATE job SET state = 'queued' ... WHERE id = ? AND state = 'interrupted'`.
5. `crates/codegg-core/src/jobs/store.rs:2250` — `start_next_attempt`'s
   `eligible_states` includes `JobState::Interrupted`, so an interrupted job
   may also take a fresh attempt.
6. `src/scheduler/job_notifications.rs:1731-1733` — "Truly final job states
   produce terminal notices. `Interrupted` means an eligible attempt may
   still be requeued and must never be announced as final."
7. `.opencode/skills/jobs/SKILL.md:122,170` — "Interrupted → Queued
   (recovery only)"; "`recover_generation` marks non-terminal attempts from a
   stale generation as `Interrupted`. The parent job is requeued only if
   `RecoveryPolicy` permits based on `IdempotencyClass`."

**The conclusion.** An interrupted job **can** become live again. It is
therefore a live job for reclamation purposes, and the sweep's live set
(which includes `Interrupted`) is the correct one. The session projection's
`terminal_jobs` query, which included `Interrupted`, contradicted the domain
and was the wrong one. This is the direction the plan expects (plan §14, §16);
**no stop condition was hit**.

The consequence is that the reconciliation goes against the *projection* and
not against the *caution*: the sweep is unchanged in behaviour, and the
projection is corrected.

## 5. WP4 — the model-facing status, before and after

**Before** (`terminal_status_str`, one value per state):

| State | Before | After |
|---|---|---|
| `Scheduled` | `running` | `scheduled` |
| `Queued` | `running` | `queued` |
| `Running` | `running` | `running` |
| `Blocked` | `running` | `blocked` |
| `Completed` | `completed` | `completed` |
| `Failed` | `failed` | `failed` |
| `Cancelled` | `cancelled` | `cancelled` |
| `TimedOut` | `timed_out` | `timed_out` |
| `Interrupted` | `interrupted` | `interrupted` |
| `Expired` | `expired` | `expired` |

The change is additive: `scheduled`, `queued` and `blocked` are new values,
and every value a consumer already parsed is preserved — `running` still
means `Running`, and the six terminal spellings are untouched.

**Every consuming surface, and what it does with the value.**

| Surface | What it does with the status | Result |
|---|---|---|
| `src/tool/job.rs:161` — the `job` tool's `status` action | returns `"status": job_state_str(&job.state)` to the model | the four unstarted states are now distinguishable; the change is the point |
| `src/tool/job.rs:247` — the `job` tool's `output` action | returns `"status"` for a job with no final output yet | same |
| `src/tool/job.rs:103-108` — `execute_structured` | `success = !matches!(status, "failed" \| "timed_out" \| "denied" \| "error")` | unaffected: `scheduled`/`queued`/`blocked` are not in the failure set, exactly as `running` was not |
| `src/scheduler/job_notifications.rs:867` — `record_terminal` | persists the notice's `status` | unaffected: `record_terminal` is reached only for a state `is_final_job_state` accepts, so the persisted values are the six terminal spellings, unchanged |
| `src/scheduler/job_notifications.rs:1002` — `reconcile_terminals` | the same, for a backfilled notice | unaffected, for the same reason |
| `src/tool/job.rs` `wait` action | returns `executor_status_str(&completion.status)` | `ExecutorStatus` is its own type and is out of scope |
| `src/tui/app/state/tasks.rs` `is_live_state` | filters the TUI's job rows by the protocol DTO's state | the TUI reads `JobState::as_str()` through the protocol DTO, which already distinguishes every state; it never consumed the model-facing status |
| `src/work_plan_evidence.rs` / `src/goal_*.rs` | consume `HostEvidenceStatus`, not the status string | unaffected |

## 6. The property tests, and their captured baseline failures

### The test that fails against the baseline

`tests/core_daemon/job_state_terminality_c004.inc::the_session_projection_queries_agree_with_the_domain_terminality`
reads the session projection's two queries and checks their state lists
against the domain's own predicate. It uses only symbols the baseline
already has — `JobState::is_terminal()`, the `JobState` variants, and the
production source — so it compiles against the baseline and fails on the
assertion.

Baseline demonstration: the thirteen production files were reverted
(`git stash push -u -- <production paths>`, leaving the new test files in
place) and the narrow run was made:

```
$ CARGO_BUILD_JOBS=3 cargo test --test core_daemon -- job_state_terminality_c004

running 2 tests
test job_state_terminality_c004::the_domain_does_not_call_an_interrupted_job_terminal ... ok
test job_state_terminality_c004::the_session_projection_queries_agree_with_the_domain_terminality ... FAILED

---- job_state_terminality_c004::the_session_projection_queries_agree_with_the_domain_terminality stdout ----
thread '...' panicked at tests/core_daemon/job_state_terminality_c004.inc:121:5:
assertion `left == right` failed: the projection's terminal-jobs query must select exactly the states the domain calls terminal
  left: ["cancelled", "completed", "expired", "failed", "interrupted", "timed_out"]
 right: ["cancelled", "completed", "expired", "failed", "timed_out"]
```

The divergence the assertion names is the one the milestone fixed: the
baseline's `terminal_jobs` query selected `interrupted` as well as the five
states the domain calls terminal.

### The other new tests

| Test | Where | Non-vacuity evidence |
|---|---|---|
| `the_session_projection_queries_agree_with_the_domain_terminality` | `tests/core_daemon/job_state_terminality_c004.inc` | the captured baseline failure above |
| `the_domain_does_not_call_an_interrupted_job_terminal` | `tests/core_daemon/job_state_terminality_c004.inc` | it asserts the domain's own answer for `Interrupted`; it fails if the domain ever calls an interrupted job terminal |
| `terminal_state_sets_agree_with_is_terminal_for_every_variant` | `crates/codegg-core/src/jobs/mod.rs` | **mutation**: inverting `JobState::terminal_states()`'s filter makes it fail — `assertion \`left == right\` failed: Scheduled is in the terminal set but the domain disagrees` |
| `the_two_state_sets_partition_every_variant` | `crates/codegg-core/src/jobs/mod.rs` | the same mutation makes it fail — `a state cannot be both terminal and not terminal` |
| `the_domain_enumeration_covers_every_variant` | `crates/codegg-core/src/jobs/mod.rs` | it compares `JobState::ALL` against the enum's variants; the guard's rule 2 checks the same thing from the source |
| `the_evidence_projection_is_fail_closed_for_every_variant` | `crates/codegg-core/src/jobs/evidence.rs` | it checks the verdict for every variant; the two pinned fail-closed tests check three of them from the other side |
| `only_a_completed_job_is_positive_evidence` | `crates/codegg-core/src/jobs/evidence.rs` | it asserts that no state other than `Completed` is positive evidence |
| `an_interrupted_job_fails_closed_even_though_it_is_not_terminal` | `crates/codegg-core/src/jobs/evidence.rs` | it pins the one state whose verdict differs from terminality |
| `a_scheduled_job_is_reported_as_scheduled` | `tests/scheduler_jobs/job_status_projection_c004.inc` | **mutation**: flattening `Scheduled` back to `"running"` makes it fail — `assertion \`left == right\` failed  left: "running"  right: "scheduled"` |
| `a_queued_job_is_reported_as_queued` | `tests/scheduler_jobs/job_status_projection_c004.inc` | the same mutation family |
| `a_running_job_is_reported_as_running` | `tests/scheduler_jobs/job_status_projection_c004.inc` | it pins the spelling a consumer already parsed |
| `a_blocked_job_is_reported_as_blocked` | `tests/scheduler_jobs/job_status_projection_c004.inc` | the same mutation family |
| `an_interrupted_job_is_not_reported_as_a_terminal_one` | `tests/scheduler_jobs/job_status_projection_c004.inc` | it pins that an interrupted job does not read as finished |
| `the_sweep_refuses_a_session_with_an_interrupted_job` | `src/core/scratch_sweep.rs` | it plants an interrupted job and asserts the tree survives; it fails if the sweep ever widens reclamation to a session holding a job |

The scratch-reclamation test does **not** fail against the baseline, and
does not need to: the baseline's behaviour is the one this milestone keeps.
The plan asks for that test to be shown failing only "if the baseline
behaviour is the one being changed", and it is not.

## 7. The guard

`scripts/check_job_state_terminality_agreement.py`

Two rules: no hand-written job-state terminality list in production code
outside the domain, and no new `JobState` variant that the projections do
not cover. Rust is parsed with `scripts/rust_test_scan.py`.

**Self-test** (the guard's own proof that it is not vacuous):

```
$ python3 scripts/check_job_state_terminality_agreement.py --self-test
check_job_state_terminality_agreement --self-test
  ok: a clean tree passes
  ok: a query named `terminal_jobs` that selects `Interrupted` is rejected
  ok: a hand-written copy of the domain's non-terminal states is rejected
  ok: a new job state the projections do not cover is rejected
  ok: a missing projection is rejected
check_job_state_terminality_agreement --self-test: all checks pass
```

**Against the baseline tree** (the same production files reverted), the
guard reports exactly the divergence this milestone fixed:

```
$ python3 scripts/check_job_state_terminality_agreement.py
job-state terminality agreement check failed:
  src/core/daemon_refresh.rs:200: the query named `live_jobs` selects ['Blocked', 'Queued', 'Running', 'Scheduled'], which is not the set of states the domain does not call terminal (['Blocked', 'Interrupted', 'Queued', 'Running', 'Scheduled'])
  src/core/daemon_refresh.rs:252: the query named `terminal_jobs` selects ['Cancelled', 'Completed', 'Expired', 'Failed', 'Interrupted', 'TimedOut'], which is not the set of states the domain calls terminal (['Cancelled', 'Completed', 'Expired', 'Failed', 'TimedOut'])
  src/core/daemon_sessions.rs:1333: a hand-written copy of the domain's non-terminal job states ...
  src/core/scratch_sweep.rs:579: a hand-written copy of the domain's non-terminal job states ...
  src/scheduler/job_notifications.rs:1047: a hand-written copy of the domain's terminal job states ...
  src/scheduler/job_notifications.rs:1737: a hand-written copy of the domain's terminal job states ...
```

**Against the fixed tree**: `job-state terminality agreement check passed`.

The guard is wired into `scripts/verify.sh`'s `run_guards()` (both the
check and its self-test) and into `.github/workflows/ci.yml`'s guard step
(both as well).

## 8. Verification executed

### Commands run

```bash
CARGO_BUILD_JOBS=3 cargo check -p codegg --all-targets
CARGO_BUILD_JOBS=3 cargo test -p codegg-core --lib
CARGO_BUILD_JOBS=3 cargo test -p codegg-core --lib -- jobs::tests jobs::evidence::tests
CARGO_BUILD_JOBS=3 cargo test --test scheduler_jobs -- job_status_projection_c004
CARGO_BUILD_JOBS=3 cargo test --test core_daemon -- job_state_terminality_c004
CARGO_BUILD_JOBS=3 cargo test --lib -- scratch_sweep::tests
CARGO_BUILD_JOBS=3 cargo test --lib -- goal_verification::tests work_plan_evidence::tests
CARGO_BUILD_JOBS=3 cargo test --lib -- job_notifications
python3 scripts/check_job_state_terminality_agreement.py
python3 scripts/check_job_state_terminality_agreement.py --self-test
cargo fmt
scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo check -p codegg --all-targets` | pass, no errors and no new warnings |
| `cargo test -p codegg-core --lib` | pass — 930 passed, 0 failed |
| `cargo test -p codegg-core --lib -- jobs::tests jobs::evidence::tests` | pass — 31 passed, 0 failed (includes the three new domain tests and the three new evidence tests) |
| `cargo test --test scheduler_jobs -- job_status_projection_c004` | pass — 5 passed, 0 failed |
| `cargo test --test core_daemon -- job_state_terminality_c004` | pass — 2 passed, 0 failed |
| `cargo test --lib -- scratch_sweep::tests` | pass — 21 passed, 0 failed (includes the new interrupted-job test) |
| `cargo test --lib -- goal_verification::tests work_plan_evidence::tests` | pass — 2 passed, 0 failed (the two pinned fail-closed tests, unchanged) |
| `cargo test --lib -- job_notifications` | pass — 8 passed, 0 failed |
| the guard and its self-test | pass (see §7) |
| `python3 scripts/rust_test_scan.py --validate` | pass — "all checks pass" (16 481 test attributes parsed, residual 0 in every tree; the mask lost and gained no newline) |
| `scripts/verify.sh quick` | **pass** — "Quick verification passed." (rustfmt, the two generated-asset checks, 55 guard scripts, and the workspace `cargo check`) |
| `cargo check -p codegg --all-targets` warning audit | pass — the nine warnings the build reports are all pre-existing (`src/interactive_process.rs`, `src/managed_process.rs`, `src/lsp/m004.rs`, `src/security/sandbox.rs`, `src/bin/codegg-sandbox-helper.rs`); none is in a file this milestone changed |
| `cargo clippy -p codegg-core -p codegg --all-targets -- -D warnings` | **fails on the pre-existing macOS set only** — see the lint statement below |

**Scope not run, and why.** Two of the plan's §11 commands were **not** run:

- `scripts/verify.sh scoped HEAD~1` — the selector was run read-only
  (`python3 scripts/scoped_selection.py --base HEAD --format text`) and it
  reports that the scope **widens to the whole workspace**: a changed file
  lies outside every workspace member (the root package's integration test
  files are not attributed to it by the selector), so no test target can be
  ruled out. `verify.sh scoped` would therefore run the workspace sweep.
- `scripts/verify.sh full` — it runs `cargo clippy --workspace --all-targets
  --locked -- -D warnings`, i.e. the workspace sweep plus the lint gate.

Both were withheld deliberately: this milestone was executed on a machine
shared with three parallel milestones, and the run order for that machine
excludes the workspace-wide sweep and `verify.sh full`. The **substitute
evidence** for the surfaces those two commands would have covered is:
`scripts/verify.sh quick` (rustfmt, both generated-asset checks, all 55
guards, and the workspace `cargo check`) is green; `cargo test -p
codegg-core` runs the changed crate's whole library test surface; the two
affected families' new members, the root package's library tests for every
changed module (`scratch_sweep`, `goal_verification`, `work_plan_evidence`,
`job_notifications`), and the guard's own self-test all pass; and the lint
gate was run on the two changed crates (see below). CI runs the full
workspace sweep and the workspace lint gate on every push, so the gap is a
local-machine condition and not an unrecorded assumption.

**The lint statement.** `cargo clippy -p codegg-core -p codegg --all-targets
-- -D warnings` was run on the two changed crates rather than on the whole
workspace. It fails, and the failure set is the pre-existing macOS one the
plan describes (C005 owns that asymmetry):

```
error: unused variable: `shutdown`                     --> src/interactive_process.rs:1469:5
error: unneeded `return` statement                     --> src/interactive_process.rs:1131:9
error: variable does not need to be mutable             --> src/managed_process.rs:870:9
error: unused variable: `close_unapproved_fds`          --> src/managed_process.rs:1977:5
error: method `sandbox_interactive_entries` is never used --> src/managed_process.rs:235:19
error: function `spec_with` is never used               --> src/managed_process.rs:2715:8
error: associated functions `new` and `new_with_network_intent` are never used --> src/lsp/m004.rs:106,115
error: associated function `capture` is never used      --> src/security/sandbox.rs:781:8
```

**The set is identical before and after, and the change added and removed no
lint finding.** Every one of those nine findings is in a file this milestone
did not change, and they are the same nine locations the ordinary
`cargo check` reports; `codegg-core`, the only crate whose source the
milestone changed besides the root package, produces **no** clippy finding
at all. The root package's changed files produce none either.

## 9. Invariant review

| Invariant | Evidence |
|---|---|
| The domain's contract is unchanged | `JobState::is_terminal()` is byte-identical to the baseline. The enum gained no variant. `JobState::ALL`, `terminal_states()` and `non_terminal_states()` are **additive** and derive from the existing predicate. |
| The scheduler's requeue behaviour is unchanged | No store, transition-table or scheduler-execution code changed. The requeue path was read, not edited. |
| No evidence verdict moved | The two pinned fail-closed tests pass unchanged, and `crates/codegg-core/src/jobs/evidence.rs::the_evidence_projection_is_fail_closed_for_every_variant` checks the verdict for all ten variants against the baseline's mapping. |
| `ExecutorStatus` and `AgentRunTerminalOutcome` are untouched | Neither type nor its mapping changed; `src/scheduler/scheduler.rs:834` and `:2220` are as they were. |
| The scratch sweep stays conservative | The sweep's live set is the domain's non-terminal set, which is the same set the baseline had. `the_sweep_refuses_a_session_with_an_interrupted_job` pins it. |
| No query's `limit` or scope changed | Only the `states:` arguments changed. Both `daemon_refresh` queries keep their limits (`MAX_PROJECTION_JOBS + 1` and `MAX_PROJECTION_JOBS - jobs_added`) and their `session_id` scope; the sweep and the session-cleanup query keep `limit: Some(1)` and their `session_id` scope; the notification backfill keeps `limit: Some(256)` and its unbounded scope; the Python-orphan query keeps `limit: Some(1000)` and `workspace_id: None`. |
| The dead routing abstraction is untouched | `src/agent/router.rs` is not in the diff. |
| The projection loses nothing | The two queries now partition the enum: the live query is the domain's complement and the terminal query is the domain's terminal set. A job in any state appears in exactly one of them. |
| The model-facing status is additive | `scheduled`, `queued` and `blocked` are new values; `running` and the six terminal spellings are unchanged. |

## 10. Failure and recovery review

- **Restart.** The session projection's two queries are a partition of the
  enum, so a restart cannot lose a job the projection used to show: the
  inclusive `terminal_jobs` query used to carry the interrupted jobs, and
  the live query now carries them. A job's position in the snapshot moves
  from the terminal group to the live group, which the code's own comment
  asks for ("live jobs ahead of recent terminals so history cannot hide
  active work").
- **Stale generation.** The recovery path (`recover_generation`) is
  unchanged, and it is the path that makes an interrupted job live again.
  The milestone read it rather than altering it.
- **Contention.** No lock, lease or permit changed. The queries are the
  same bounded, indexed store reads on the same paths.
- **Malformed input.** Nothing parses new input. The projection's state
  lists are derived from a closed enum.
- **Bounded behaviour.** No scan, buffer or event was added.

## 11. Migration and compatibility review

- No schema migration, no `STORAGE_LAYOUT_VERSION` change, no new durable
  row, no wire-protocol or DTO change.
- `JobState::ALL` and the two derived accessors are new **API surface** on
  the domain type, not a change to stored values: the state strings in the
  database are untouched.
- The model-facing status change is a projection. A consumer that already
  received `"running"` still receives it for a running job; the three other
  unstarted states now have their own values, which is additive.

## 12. Security review

- **Authorization.** Unchanged. The projection and the sweep read the same
  session-scoped rows they read before.
- **Path validation.** Unchanged; no path handling was touched.
- **Privilege boundaries.** Unchanged. The domain crate gained a pure
  function over its own enum and a trait; `scripts/check-core-boundary.sh`
  is green, and the new module imports nothing from the root crate.
- **Denial of service.** No unbounded work was added. The derived sets are
  computed from a ten-element constant.
- **Redaction and audit.** Unchanged.
- **The conservative direction.** The one behavioural change in a
  reclamation path is that the Python-orphan cleanup now keeps the source
  file of an interrupted Python job, because such a job may be requeued.
  That deletes fewer files, never more.


## 13. Documentation and operations

- `architecture/core.md` — new section "Job-state terminality and its three
  projections".
- `architecture/overview.md` — the three count rows this milestone moves are
  re-measured and updated (integration test files 327 → 329, guard scripts
  80 → 81, CI guard scripts 40 → 41), with a reconciliation paragraph
  appended to that document's audit history. `scripts/check_test_target_references.py`
  fails on a stale integration-test count, so the row is enforced.
- `AGENTS.md` — the two guard counts corrected (quick 54 → 55, CI 40 → 41).
- `scripts/check_job_state_terminality_agreement.py` — new static guard,
  wired into `scripts/verify.sh`'s `run_guards()` and into
  `.github/workflows/ci.yml`.
- `plans/registry.md` — the C004 row's status moved from *ready for handoff*
  to *closed*, with a link to this record.
- No operator diagnostic, configuration key, or recovery instruction changed.

## 14. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `src/scheduler/scheduler.rs`'s cancel-pending query keeps a hand-written `{Scheduled, Queued, Blocked}` list. It is a narrowing of the live set for a different question ("which jobs may be cancelled?"), not a terminality claim, so the guard leaves it alone. | none | none; recorded so a future reader does not mistake it for a divergence |

The `architecture/overview.md` count rows that this milestone moves are
**updated, not left stale**: *Integration test files* **327 → 329**, *Guard
scripts (all)* **80 → 81**, *CI guard scripts* **40 → 41**, with a
reconciliation paragraph appended to that document's audit history and the
`AGENTS.md` guard counts corrected to match. `scripts/check_test_target_references.py`
fails on a stale integration-test count, so the row is enforced rather than
advisory. The `codegg-core` modules row stays at **54**: the new
`crates/codegg-core/src/jobs/evidence.rs` is a sub-module of `jobs`, not a
top-level `pub mod`.

## 15. Roadmap disposition

Milestone closed and the next dependency may proceed. No ADR was required:
the domain's authority already existed and already decided the disputed
question.

## 16. Registry updates

- `plans/registry.md` — **made**: the C004 row's status moved from *ready
  for handoff* to **closed**, and the row now names this closure record.
- `plans/subsystems/repository-audit-corrective-roadmap.md` — **no change
  required**: the roadmap's milestone sections carry no per-milestone status
  field, and C004's dependency row is already `—`. The roadmap's dependency
  table records that **C005 depends on C001–C004 being closed**; this record
  discharges C004's half of that gate, and the registry's C005 row stays
  *ready for handoff* until the other three close.
- **Unblock audit of the registry's Blocked work section.** No row in
  `plans/registry.md`'s "Blocked work" section is blocked on C004 or on the
  repository-audit-corrective workstream. C004 has no blocked dependent, so
  nothing is unblocked by this closure. The only dependent is C005, which is
  in the dependency-ready section and is gated on C001–C003 as well.
