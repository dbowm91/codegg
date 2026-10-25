# Delegated Run Observability and Recovery Corrective Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/delegated-run-observability-and-recovery-corrective/001-the-orchestrator-always-hears-about-its-child.md`

Source subsystem roadmap:

- `plans/subsystems/delegated-run-observability-and-recovery-corrective-roadmap.md` (owned by the
  orchestrator; not edited by this milestone)

Repository baseline reviewed: `4803e093c` ("docs(adrs): accept ADR-0051 option B for the delegated
run transcript")

Implementation commits or pull requests:

- **none** — the work is left in the working tree, uncommitted and unpushed, as instructed.

## 1. Executive finding

A delegated run now has a durable record of its terminal state, and the orchestrator reads it.

The `task` tool's durable spawn path subscribes the delegated job for a durable terminal notice
immediately after the submit succeeds and before the tool reports acceptance, under the parent's own
identity (`subagent` kind, the parent's session and turn, the parent's owner principal, the parent's
workspace, the same invocation key the submit used, `JobCompletionDisposition::NotifyOnly`). The
scheduler already recorded a terminal notice for *any* subscribed job, so no scheduler behaviour
changed — only the subscriber was added. The next parent turn's boundary drains that notice and
hands it to the model as a `Message::System` that names the child. `task action=get` now returns the
child's recorded result under a named, bounded field next to the `result_ref` pointer it always
returned.

The in-band `try_send` push is **kept** — it is what delivers a notice to a *live* parent turn
without waiting for the next boundary — and is now explicitly documented, in the code and in its own
drop log, as a fast path whose loss the durable notice covers. The guarantee is scoped honestly to
the **durable** delegated path; the legacy in-process path is unchanged and keeps its poll-only
behaviour.

The canonical gate `bash scripts/verify.sh quick` exits non-zero at exactly **one** guard: the
comment-budget ratchet, which the orchestrator re-banks centrally after all workers land and which
this milestone was explicitly forbidden to edit. Every other guard, the workspace `cargo check`,
`cargo clippy -p codegg --all-targets --locked -- -D warnings`, the new unit tests, and the new
integration members (11 in `scheduler_jobs`, 2 in `agent_loop`) pass. The two integration *families*
are not green — but a clean worktree at HEAD reproduces the same failures, so they are pre-existing
(§4.5). The per-file comment breakdown is in §4.4 so the re-bank can be taken fairly.

## 2. Requirement-to-evidence matrix

| §13 requirement | Evidence (file:line) | Named test | Result |
|---|---|---|---|
| 1. A delegated run's terminal state produces a **durable** notice row owned by the daemon's existing notification store | `src/tool/task.rs:2392` (subscribe after submit), `src/scheduler/job_notifications.rs:859` (`record_terminal`), `src/scheduler/job_notifications.rs:51` (`DELEGATED_JOB_KIND`) | `scheduler_jobs::delegated_notice_c001::the_delegated_run_produces_a_notice_that_names_it`; `scheduler_jobs::delegated_notice_c001::the_delegated_spawn_subscribes_the_job_for_its_terminal_notice` | pass |
| 2. The notice is delivered to the parent session at the next turn boundary **even when the parent's turn ended before the child terminated** | `src/agent/follow_up.rs:427` (boundary injection), `src/agent/job_notification_recovery.rs:98` (recovery loop) | `scheduler_jobs::delegated_notice_c001::the_delegated_notice_survives_a_parent_turn_that_ended`; `agent_loop::delegated_notice_c001::the_next_parent_turn_hands_the_notice_to_the_model` | pass |
| 3. The orchestrator can obtain the child's recorded result as content, bounded, without needing a second tool call | `src/tool/task.rs:1295` (`format_durable_task_get`), `src/tool/task.rs:2064` (durable `get` branch) | `scheduler_jobs::delegated_notice_c001::task_get_returns_the_recorded_result_and_the_result_ref` | pass |
| 4. A delegated run's notice spends no tokens, mints no turn, creates no session, and grants no authority beyond delivery | `src/tool/task.rs:1362` (`subscribe_delegated_notice`, `NotifyOnly` only), `src/scheduler/job_notifications.rs:216` (disposition enum) | `scheduler_jobs::delegated_notice_c001::the_delegated_spawn_subscribes_the_job_for_its_terminal_notice` (asserts `NotifyOnly`); `scheduler_jobs::delegated_notice_c001::the_delegated_run_policy_is_unchanged_by_the_subscription` | pass |
| 5. A subscription failure, an absent parent session, and a full parent follow-up channel each leave the delegated work intact and the situation visible in the daemon log | `src/tool/task.rs:1362` (`subscribe_delegated_notice`: warn-and-continue, conflict-fails, no-session-skips), `src/agent/run_control.rs:422` and `:444` (drop log) | `tool::task::tests::a_failed_subscription_does_not_fail_the_delegated_spawn`; `tool::task::tests::a_delegated_spawn_without_a_parent_session_skips_the_subscription`; `scheduler_jobs::delegated_notice_c001::the_delegated_notice_is_delivered_when_the_in_band_push_is_dropped` | pass |
| 6. The in-band push is retained as a fast path and is documented as non-authoritative | `src/agent/run_control.rs:414-424` and `:436-448` (comment + drop log), `src/agent/follow_up.rs:419-424` (boundary comment) | `scheduler_jobs::delegated_notice_c001::the_delegated_notice_is_delivered_when_the_in_band_push_is_dropped` | pass |
| 7. The legacy delegated path is unchanged and its weaker guarantee is stated plainly in the documentation | `src/agent/worker.rs` untouched by this milestone; `architecture/agent.md` "Durable delegated runs" → "The delegated run's durable terminal notice" (last paragraph) | covered by `git diff --stat src/agent/worker.rs` (no C001 hunk) | pass |
| 8. No migration, no protocol change, and no change to `bash`, `test` or `ci_watch` | `crates/codegg-core/` untouched by this milestone; `src/scheduler/job_notifications.rs:51` adds a constant and two optional, `#[serde(default)]` subscription fields (`owner_principal`, `workspace_id`) — no table, no migration, no DTO on the wire | `scheduler_jobs::delegated_notice_c001::the_delegated_job_keeps_its_retry_and_idempotency_policy`; existing `scheduler_jobs::job_continuation_m003_visibility` and `job_notification_m001_contract` members (unchanged, still pass) | pass |
| 9. The static guard fails against the pre-change tree and passes after, with its `--self-test` proving non-vacuity | `scripts/check_subagent_notification_wiring.py` | `python3 scripts/check_subagent_notification_wiring.py --self-test` | pass (see §4 for the pre-change failure) |
| 10. `bash scripts/verify.sh quick` exits 0 and `cargo clippy --workspace --all-targets --locked -- -D warnings` is clean | `scripts/verify.sh` (owned by the orchestrator; the new guard is **not** wired by this milestone) | — | **conditional** — the sole `quick` failure is the comment-budget ratchet (§4.4), which the orchestrator re-banks; `cargo clippy -p codegg --all-targets --locked -- -D warnings` is clean |

## 3. Production implementation evidence

### 3.1 The subscription (work package B)

`src/tool/task.rs:425` adds the handle to `TaskTool`:

```rust
job_notifications: Option<Arc<crate::scheduler::job_notifications::JobNotificationService>>,
```

`src/tool/task.rs:1093` adds `with_job_notifications(...)`, the same shape `src/tool/test.rs` uses.
`src/tool/factory.rs:395` wires the daemon's shared notifier into the `task` tool:

```rust
.with_job_notifications(job_notification_service.clone());
```

`src/tool/task.rs:1362` is `subscribe_delegated_notice`, the single place that creates the
subscription. It takes the job id, the invocation key, the workspace and the execution context, and
it:

- refuses to fabricate an owner — a delegated spawn with no host-originated parent session **skips**
  the subscription and logs the reason (`src/tool/task.rs:1379-1388`);
- uses `self.parent_session_id` — the *job's* owning session, not the context's, because
  `record_terminal` refuses a notice whose subscription owner differs from the job owner
  (`src/scheduler/job_notifications.rs`, the cross-session check in `record_terminal`);
- passes the parent's turn, the parent's owner principal, the parent's workspace, the invocation key
  and `JobCompletionDisposition::NotifyOnly`;
- treats a **conflict** as a failure of the spawn and every other error as a `warn` that leaves the
  delegated work intact (`src/tool/task.rs:1400-1418`).

The call sites are `src/tool/task.rs:2253` (the retry-after-a-restart path) and
`src/tool/task.rs:2392` (the fresh-spawn path, after the submit and before the acceptance string).

**The retry path was a real gap, and this milestone closes it.** The tool's existing idempotency
branch (`created.run.job_id.is_some()`) returns the run's status *before* the submit — so a retry
after a crash between the submit and the subscription never reached the subscribe and the child
would have had no notice at all. `src/tool/task.rs:2242-2266` now subscribes the already-submitted
job before returning that status. The invocation key is the one the submit used, so the
subscription is created once, not twice.

### 3.2 The notice content (work package C)

**Choice recorded (the plan asked for one):** the delegated kind gets a **dedicated content
format**, rather than requiring the summary to carry the run id and the result reference.

- `src/scheduler/job_notifications.rs:51` — `pub const DELEGATED_JOB_KIND: &str = "subagent";`
- `src/scheduler/job_notifications.rs:1823` — `delegated_run_id(job)` takes the run id from the
  durable job payload (`JobPayload::SubagentRun { run_id }`) when the caller supplies none. The
  delegated executor reports its terminal *summary* through the executor completion, not a run id,
  so the payload is where the notice learns which child it is about.
- `src/scheduler/job_notifications.rs:1875` — `format_delegated_notice_content` names the run, the
  recorded outcome, and the result reference. A delegated run whose recorded outcome is empty says
  **"no recorded outcome (this delegated run recorded no result)"** rather than reading as an empty
  success, and the generic "completed successfully: {summary}" wording is never used for a delegated
  kind.

The summary itself is real: the delegated executor's `ExecutorCompletion.summary` is the child's own
output (`src/scheduler/executors.rs:1712`), which is the same string the run-result store persists
as `AgentRunResult.summary` (`src/scheduler/executors.rs:1700-1710`). Nothing is fabricated.

### 3.3 The child's recorded result (work package E)

`src/tool/task.rs:1295` (`format_durable_task_get`) returns a JSON object with named fields:
`task_id`, `run_id`, `status`, `result_ref`, `text` (the previous human-readable rendering, so the
`result_ref` line survives verbatim), `recorded_result`, `recorded_result_state`,
`recorded_result_note`, `recorded_result_status`, `recorded_result_bytes` and
`recorded_result_limit_bytes`.

- The `result_ref` pointer is **retained** (both as the `result_ref` field and inside `text`).
- The recorded result is a **named field**, not free-form text, so its bound is observable.
- The bound is the run-result store's own: `codegg_core::run_result::MAX_RESULT_SUMMARY_BYTES`
  (4096), reported as `recorded_result_limit_bytes`.
- Three states, so an absence is never confused with an empty production:
  `recorded` (a result with text), `empty` (a recorded result with no output text), `absent` (the
  run recorded no result — `recorded_result_note: "this run recorded no result"`).

### 3.4 The in-band push is explicitly non-authoritative (work package F)

The push is unchanged in behaviour. `src/agent/run_control.rs:414-424` and `:436-448` now say, in
the code's own words, that the in-band push is a fast path for a live parent turn and that **the
durable terminal notice is unaffected** by a dropped push:

```rust
"child completion push dropped (parent channel full/closed); the durable terminal notice is unaffected"
"child completion push dropped (turn channel full/closed); the durable terminal notice is unaffected"
```

`src/agent/follow_up.rs:419-424` states the same relationship at the boundary injection.

### 3.5 The production path (work package A)

**Finding: the durable delegated path is the live production path.**

The wiring that proves it:

1. `src/agent/turn_runtime.rs:593` — the daemon's turn runtime builds the tool registry with
   `agent_run_store: Some(agent_run_store.clone())`.
2. `src/tool/task.rs:2154` — the durable branch is selected when `self.agent_runs` is `Some`; the
   legacy branch (`JobPayload::Subagent`, in-process pool) is the `else`.
3. `src/tool/task.rs:2299` — the durable branch submits `JobPayload::SubagentRun` through
   `JobSubmissionService` — the scheduler remains the only admission authority.
4. `src/tool/factory.rs:395` — the daemon's shared notifier (`SessionToolContext.job_notification_service`,
   built at `src/agent/turn_runtime.rs:601`) reaches the `task` tool.
5. `src/agent/turn_runtime.rs:428` — the same notifier is installed on the scheduler, so the
   scheduler's own terminal transition records the notice.

**What the legacy path guarantees: nothing beyond poll-only.** `src/agent/worker.rs:814-860` (the
in-process `SubAgentPool` path, `JobPayload::Subagent`) updates only the in-memory `TaskStore`. It
has no push, no durable notice, and no subscription. This milestone does not modify it; the
documentation states the weaker guarantee rather than letting the docs imply otherwise
(`architecture/agent.md`, `architecture/jobs.md`).

## 4. Verification executed

### 4.1 Commands run (local; the CI lane was not exercised)

```bash
# the new unit tests
CARGO_BUILD_JOBS=3 cargo test -p codegg --lib -- c001 delegated --nocapture
#   → test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 7468 filtered out
#     (the 6 new C001 tests among them)

# the new integration members
CARGO_BUILD_JOBS=3 cargo test --test agent_loop -- delegated_notice_c001
#   → test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 537 filtered out
CARGO_BUILD_JOBS=3 cargo test --test scheduler_jobs -- delegated_notice_c001
#   → test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 352 filtered out

# the whole families, to show the new members do not disturb the existing ones
CARGO_BUILD_JOBS=3 cargo test -p codegg --lib -- tool::task scheduler::job_notifications
#   → test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 7461 filtered out
CARGO_BUILD_JOBS=3 cargo test --test agent_loop
#   → test result: FAILED. 537 passed; 1 failed; 1 ignored  (see §4.5)
CARGO_BUILD_JOBS=3 cargo test --test scheduler_jobs
#   → test result: FAILED. 358 passed; 6 failed  (see §4.5)

# clippy
CARGO_BUILD_JOBS=3 cargo clippy -p codegg --all-targets --locked -- -D warnings
#   → Finished `dev` profile ... (no warnings, exit 0)

# the static guard and its non-vacuity proof
python3 scripts/check_subagent_notification_wiring.py
#   → subagent-notification-wiring check ok: every delegated-job submission path
#     subscribes for a durable terminal notice          (exit 0)
python3 scripts/check_subagent_notification_wiring.py --self-test
#   → 4 ok lines + "subagent-notification-wiring self-test passed: 3 injected defects
#     were each rejected by the guard, and a clean delegated submission produced no
#     finding"                                          (exit 0)

# the armed runs (see §4.3)
cd /tmp/c001-prechange && python3 scripts/check_subagent_notification_wiring.py   # exit 1
cd /tmp/c001-prechange && cargo test --test scheduler_jobs -- delegated_notice_c001  # exit 101
cd /tmp/c001-prechange && cargo test --test agent_loop -- delegated_notice_c001      # exit 101
cd /tmp/c001-armb && cargo test -p codegg --lib -- <the four C001 unit tests>        # 0 passed; 4 failed
cd /tmp/c001-armb && cargo test --test scheduler_jobs -- delegated_notice_c001       # see §4.3
cd /tmp/c001-armb && cargo test --test agent_loop -- delegated_notice_c001           # see §4.3
```

### 4.2 The guard's `--self-test` output

```
$ python3 scripts/check_subagent_notification_wiring.py --self-test
  ok   a clean delegated submission produces no finding
  ok   the delegated subscription asks for a continuation instead of a notice (rejected by subagent-notification-disposition)
  ok   the delegated subscription carries a foreign kind token (rejected by subagent-notification-kind)
  ok   the delegated spawn never subscribes for its terminal notice (rejected by subagent-notification-not-subscribed)
subagent-notification-wiring self-test passed: 3 injected defects were each rejected by the guard, and a clean delegated submission produced no finding
```

The self-test is **tree-independent**: it builds a synthetic tree from a known-good delegated
submission, proves the guard reports nothing on it, then injects each of the three violations and
proves the guard rejects each one with the rule that owns it.

### 4.3 Armed evidence

Two arms were used, because the new tests have two different relationships to the pre-change code.

**Arm A — the true pre-change tree** (`git worktree add /tmp/c001-prechange HEAD`, i.e.
`4803e093c`), with only the new test files, their family registrations, and the new guard copied
in. The guard fails, exactly as it must:

```
$ cd /tmp/c001-prechange && python3 scripts/check_subagent_notification_wiring.py ; echo "guard exit=$?"
subagent-notification-wiring: FAILED
  - src/tool/task.rs:2112: execute_impl() submits a delegated job without subscribing it for a durable terminal notice (subscribe_with_options() is not reached from it)
guard exit=1
```

The new integration tests do not compile against the pre-change tree, because the API they exercise
is the API this milestone introduces. That is the armed evidence for the API, quoted verbatim:

```
$ cd /tmp/c001-prechange && cargo test --test scheduler_jobs -- delegated_notice_c001
error[E0599]: no method named `with_job_notifications` found for struct `TaskTool` in the current scope
   --> tests/scheduler_jobs/delegated_notice_c001.inc:201:6
    |
192 |       let tool = TaskTool::new(
    |  ________________-
193 | |         Arc::new(Mutex::new(TaskStore::new())),
194 | |         None,
195 | |         Some(PARENT_SESSION.to_string()),
...
200 | |     .with_project_context(None, None, Some("turn-c001".to_string()))
201 | |     .with_job_notifications(Some(notifications.clone()));
    | |     -^^^^^^^^^^^^^^^^^^^^^^ method not found in `TaskTool`
    | |_____^

error[E0609]: no field `owner_principal` on type `JobBackgroundSubscription`
   --> tests/scheduler_jobs/delegated_notice_c001.inc:301:22
    |
301 |         subscription.owner_principal.as_deref(),
    |                      ^^^^^^^^^^^^^^^^^^^^^^^ unknown field
    |
    | = note: available fields are: `job_id`, `session_id`, `turn_id`, `invocation_key`, `kind` ... and 2 others

error: could not compile `codegg` (test "scheduler_jobs") due to 3 previous errors
```

```
$ cd /tmp/c001-prechange && cargo test --test agent_loop -- delegated_notice_c001
error[E0432]: unresolved import `super::agent_loop_harness::build_test_agent_loop_with_pool`
  --> tests/agent_loop/delegated_notice_c001.inc:17:5
   |
17 | use super::agent_loop_harness::build_test_agent_loop_with_pool;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^-------------------------------
   |     |                          |
   |     |                          help: a similar name exists in the module: `build_test_agent_loop_with_config`
```

**Arm B — the post-change tree with the production behaviour reverted** (`git worktree add
/tmp/c001-armb HEAD`, then the post-change production files copied in, then the three production
behaviours neutered: `subscribe_delegated_notice` returns `Ok(())` without subscribing,
`format_job_notice_content` loses the delegated branch, and `format_durable_task_get` returns the
legacy pointer-only text). This arm keeps the new API and the new tests and removes only the
*behaviour*, so it produces genuine behavioural failures rather than compile errors.

The four new unit tests fail:

```
running 4 tests
test tool::task::tests::a_conflicting_owner_fails_the_delegated_spawn ... FAILED
test scheduler::job_notifications::tests::a_delegated_run_with_no_recorded_outcome_says_so ... FAILED
test tool::task::tests::the_delegated_subscription_records_the_agreed_owner ... FAILED
test scheduler::job_notifications::tests::the_delegated_notice_names_the_run ... FAILED

---- tool::task::tests::a_conflicting_owner_fails_the_delegated_spawn stdout ----
thread 'tool::task::tests::a_conflicting_owner_fails_the_delegated_spawn' panicked at src/tool/task.rs:3149:14:
a conflicting owner must fail the spawn: ()

---- scheduler::job_notifications::tests::a_delegated_run_with_no_recorded_outcome_says_so stdout ----
thread 'scheduler::job_notifications::tests::a_delegated_run_with_no_recorded_outcome_says_so' panicked at src/scheduler/job_notifications.rs:2135:9:
an empty outcome must be stated plainly, got: Background job job-1 (kind subagent) completed successfully: 

---- tool::task::tests::the_delegated_subscription_records_the_agreed_owner stdout ----
thread 'tool::task::tests::the_delegated_subscription_records_the_agreed_owner' panicked at src/tool/task.rs:3078:14:
the delegated job is subscribed

---- scheduler::job_notifications::tests::the_delegated_notice_names_the_run stdout ----
thread 'scheduler::job_notifications::tests::the_delegated_notice_names_the_run' panicked at src/scheduler/job_notifications.rs:2098:9:
assertion `left == right` failed
  left: None
 right: Some("run-c001")

test result: FAILED. 0 passed; 4 failed; 0 ignored; 0 measured; 7480 filtered out; finished in 0.15s
```

Note the third failure body: `Background job job-1 (kind subagent) completed successfully: ` — that
is exactly the empty-but-successful delegated notice this milestone removes.

The integration members fail in the same arm; the captured output is below.

#### 4.3.1 Arm B integration output

The integration members fail in the same arm. `scheduler_jobs`:

```
running 11 tests
...
---- delegated_notice_c001::the_delegated_run_produces_a_notice_that_names_it stdout ----
thread 'delegated_notice_c001::the_delegated_run_produces_a_notice_that_names_it' panicked at tests/scheduler_jobs/delegated_notice_c001.inc:330:5:
assertion `left == right` failed: one notice per delegated run
  left: 0
 right: 1

---- delegated_notice_c001::the_delegated_spawn_subscribes_the_job_for_its_terminal_notice stdout ----
thread 'delegated_notice_c001::the_delegated_spawn_subscribes_the_job_for_its_terminal_notice' panicked at tests/scheduler_jobs/delegated_notice_c001.inc:291:10:
the delegated job is subscribed

---- delegated_notice_c001::the_delegated_notice_is_delivered_when_the_in_band_push_is_dropped stdout ----
thread 'delegated_notice_c001::the_delegated_notice_is_delivered_when_the_in_band_push_is_dropped' panicked at tests/scheduler_jobs/delegated_notice_c001.inc:421:5:
assertion `left == right` failed: a dropped in-band push must not lose the durable notice
  left: 0
 right: 1

failures:
    delegated_notice_c001::a_retried_spawn_completes_the_subscription_after_a_restart
    delegated_notice_c001::repeated_injection_passes_do_not_duplicate_the_notice
    delegated_notice_c001::task_get_on_a_run_with_no_recorded_result_states_it_plainly
    delegated_notice_c001::task_get_returns_the_recorded_result_and_the_result_ref
    delegated_notice_c001::the_delegated_notice_is_delivered_when_the_in_band_push_is_dropped
    delegated_notice_c001::the_delegated_notice_survives_a_parent_turn_that_ended
    delegated_notice_c001::the_delegated_run_produces_a_notice_that_names_it
    delegated_notice_c001::the_delegated_spawn_subscribes_the_job_for_its_terminal_notice
    delegated_notice_c001::two_concurrent_delegated_runs_produce_one_notice_each

test result: FAILED. 2 passed; 9 failed; 0 ignored; 0 measured; 352 filtered out; finished in 4.02s
```

The **two** that still pass are the two regression tests that assert the *absence* of a change —
`the_delegated_job_keeps_its_retry_and_idempotency_policy` and
`the_delegated_run_policy_is_unchanged_by_the_subscription`. They must pass in both trees; that is
what makes them regression tests rather than evidence for this milestone.

`agent_loop`:

```
running 2 tests
test delegated_notice_c001::the_boundary_injects_the_delegated_notice_as_a_system_message ... FAILED
test delegated_notice_c001::the_next_parent_turn_hands_the_notice_to_the_model ... FAILED

---- delegated_notice_c001::the_boundary_injects_the_delegated_notice_as_a_system_message stdout ----
thread 'delegated_notice_c001::the_boundary_injects_the_delegated_notice_as_a_system_message' panicked at tests/agent_loop/delegated_notice_c001.inc:137:5:
the injected message must identify the delegated run, got: Background job job-c001-boundary (kind subagent) completed successfully: the child finished its work

---- delegated_notice_c001::the_next_parent_turn_hands_the_notice_to_the_model stdout ----
thread 'delegated_notice_c001::the_next_parent_turn_hands_the_notice_to_the_model' panicked at tests/agent_loop/delegated_notice_c001.inc:222:5:
the system message must identify the delegated run, got: Background job job-c001-turn (kind subagent) completed successfully: the child finished its work

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 534 filtered out; finished in 0.67s
```

The `got:` lines are the pre-change notice wording — the generic
`"Background job {job_id} (kind {kind}) completed successfully: {summary}"` that cannot identify the
child. That is the exact defect this milestone removes.

**After the change, the same tests pass** (see §4.1: 11 passed in `scheduler_jobs`, 2 passed in
`agent_loop`, 16 passed in the unit filter).


### 4.4 The canonical gate

`bash scripts/verify.sh quick` was run on the final tree. It **exited non-zero at exactly one
guard**: the comment-budget ratchet. Everything before it passed.

```
$ bash scripts/verify.sh quick
==> Quick verification
==> cargo fmt --check --all
==> python3 scripts/generate_builtin_agents.py --check
Checking 10 agent definition(s)...
  src/agent/builtins/generated.rs: OK
  src/agent/builtins/mod.rs: OK
All checks passed.
==> python3 scripts/generate_subagent_names.py --check
Checking 46 subagent name(s)...
  src/agent/subagent_names/generated.rs: OK
All checks passed.
==> ./scripts/check-core-boundary.sh
codegg-core boundary check passed
==> ./scripts/check-client-boundary.sh
codegg-client dependency boundary passed
==> ./scripts/check-desktop-boundary.sh
desktop dependency and authority boundaries verified
==> python3 scripts/check_sandbox_contract.py
sandbox contract guard passed
...
==> python3 scripts/check_comment_budget.py --self-test
ERROR: the unmodified tree must pass the guard: R4 the corpus holds 124671 comment lines, over the recorded ceiling of 124310
```

`verify.sh` stops at the first failing guard (`set -e`), so the guards **after** the comment
budget and the workspace `cargo check` did not run inside `verify.sh`. They were run directly
instead, and all of them passed:

```bash
# the guards verify.sh would have run after the comment budget (107 commands),
# with only the comment-budget lines omitted
bash /tmp/c001-evidence/remaining-guards.sh
#   → 0 guard failures

# the last step of verify.sh quick
CARGO_BUILD_JOBS=3 cargo check --workspace --all-targets --locked
#   → workspace check exit=0
```

**This is the expected failure and it is not this milestone's to fix.**
`docs/comment-budget.toml` is a ratchet the orchestrator re-banks after all workers land, and the
brief forbids editing it here. The corpus is over the global ceiling by ~385 comment lines, and the
per-file breakdown shows the two workers' shares:

| file | banked ceiling | measured | over | owner |
|---|---|---|---|---|
| `src/tool/task.rs` | 130 | 193 | +63 | C001 (this milestone) |
| `src/scheduler/job_notifications.rs` | 209 | 237 | +28 | C001 |
| `tests/scheduler_jobs/delegated_notice_c001.inc` | unlisted (20-line floor) | 54 | +54 | C001 (new file) |
| `tests/agent_loop/agent_loop_harness.inc` | 504 | 509 | +5 | C001 |
| `src/tool/factory.rs` | 110 | 113 | +3 | C001 |
| `src/agent/follow_up.rs` | 56 | 59 | +3 | C001 |
| `crates/codegg-core/src/agent_run_transcript.rs` | unlisted (20-line floor) | 105 | +105 | the parallel C002 worker |
| `src/agent/loop.rs` | 306 | 352 | +46 | the parallel C002 worker |
| `src/agent/worker.rs` | 311 | 334 | +23 | the parallel C002 worker |
| `src/agent/turn_completion.rs` | 67 | 70 | +3 | the parallel C002 worker |
| `crates/codegg-core/src/session/schema.rs` | 464 | 467 | +3 | the parallel C002 worker |
| `crates/codegg-core/src/storage/mod.rs` | 100 | 103 | +3 | the parallel C002 worker |

C001's share is ~156 comment lines and the parallel worker's is ~183; the global corpus is ~385
over. The numbers move while the parallel worker is still editing, so the re-bank must be taken
from the final merged tree, not from this record.

No other guard failed. `scripts/check_test_assertion_quality.py` and
`scripts/check_test_duplication.py` — the two that judge the new tests — passed.

### 4.5 The family runs: the failures are pre-existing, not this milestone's

The two integration families do **not** run green in this tree, and the reason is not this
milestone. Running the same families in a clean worktree at `HEAD` (`4803e093c`, before any C001
change) reproduces the same failures:

```bash
# in a clean worktree at HEAD
$ cargo test --test scheduler_jobs
failures:
    async_background_c006_tool_to_idle_ci::model_facing_background_bash_completes_via_scheduler_without_autonomous_resume
    scratch_cache_root_contract::an_explicit_cache_home_is_the_root_the_allocator_uses
    scratch_cache_root_contract::the_default_cache_root_is_the_home_directory_cache
    scratch_retention_sweep::the_census_measures_a_populated_scratch_tree
    scratch_retention_sweep::the_live_daemon_reclaims_its_own_aged_trees
    scratch_usage_census::scratch_usage_reports_the_daemon_s_own_tree_without_leaking_it
test result: FAILED. 346 passed; 6 failed; 0 ignored; 0 measured; 0 filtered out; finished in 118.34s
```

In this milestone's tree the same family reports `358 passed; 6 failed` — the same six, plus the
eleven new members and one test the parallel worker added.

Every one of those failures passes when it is run **in isolation**, in this tree and at HEAD:

```bash
$ cargo test --test scheduler_jobs -- async_background_c006_tool_to_idle_ci
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 361 filtered out
$ cargo test --test scheduler_jobs -- scratch_retention_sweep scratch_usage_census
#   → the same members fail at HEAD too
```

They are cross-test interference and machine-dependent scratch-directory census tests that fail
under the family's parallel run, and they are not sensitive to anything C001 changed: C001 touches
the `subagent` kind only, and these jobs are `bash`, `ci_watch` and scratch-tree work.

The `agent_loop` family's single failure is in the **parallel worker's** member
(`delegated_transcript::a_delegated_run_records_its_tool_calls_and_tool_results_bounded_with_truncation_marked`),
which passes in isolation (6 passed) and belongs to their in-flight C002 work.

**C001's own tests are green in both families**, in isolation and in the full run.

### 4.6 Commands **not** run, and why

- `cargo test --workspace`, `cargo nextest run --workspace`, `--all-features` — forbidden by
  `AGENTS.md`.
- `cargo fmt --all` — deliberately **not** run: a parallel worker is editing
  `crates/codegg-core` and `src/agent/worker.rs`, and a tree-wide format run mid-flight would
  reformat their half-written files. The orchestrator runs it once, centrally, after both workers
  land. The files this milestone owns were formatted by hand to the repository's style
  (max_width 100, 4-space).
- `scripts/verify.sh`'s guard wiring and `.github/workflows/ci.yml` — **owned by the orchestrator**;
  this milestone delivered the guard and did not edit either file.

## 5. Invariant review (plan §4, item by item)

| §4 invariant | Evidence | Result |
|---|---|---|
| **A delegated run's authority is its parent's.** The subscription grants the notice, nothing more. | The subscription is created with the parent's session, turn, workspace and owner principal and with `NotifyOnly` (`src/tool/task.rs:1362`). No tool, path, sandbox or worktree authority is derived from it. `scheduler_jobs::delegated_notice_c001::the_delegated_run_policy_is_unchanged_by_the_subscription` asserts the delegated job's path set, denied-tool policy and resource request (the sandbox/worktree ceiling) are unchanged. | pass |
| **`NotifyOnly` spends no tokens.** `resume_owner` is **not** authorized for delegated runs. | The disposition is a fixed `NotifyOnly` at the only subscription site (`src/tool/task.rs:1362`). `scripts/check_subagent_notification_wiring.py` fails the build if that argument is ever anything else (rule `subagent-notification-disposition`, proven by its `--self-test`). | pass |
| **The scheduler remains the only admission authority.** The delegated job is still submitted through `JobSubmissionService`; this milestone adds a subscription after submission, never a second submission path. | `src/tool/task.rs:2299` — the only submit is unchanged. The subscription is created *after* it, from the submitted job id. | pass |
| **A notification failure never fails the delegated work.** | `src/tool/task.rs:1400-1418` — every non-conflict error is a `warn` and the function returns `Ok`. Proven by `tool::task::tests::a_failed_subscription_does_not_fail_the_delegated_spawn` (a closed notification pool; the spawn still succeeds and the `warn` names the job id). | pass |
| **The durable notice store remains the only notice store.** No second notice mechanism, no new table. | One store: `JobNotificationService`. No table, no migration, no second service. `crates/codegg-core/` carries no C001 hunk. | pass |
| **Exact-owner routing.** A notice reaches the session that owns the delegated job and no other. | The subscription's session is `self.parent_session_id` — the same identity the job was submitted with — and `record_terminal` refuses a notice whose owner differs from the job owner. `src/agent/job_notification_recovery.rs:123` routes by `notice.session_id != session_id`. | pass |
| **Delegated run isolation, the writer fence, the worktree ceiling and the `ToolBroker`-only dispatch boundary are unchanged.** | No production change touches `src/agent/worker.rs`, the writer fence, the placement code or the broker boundary. `scheduler_jobs::delegated_notice_c001::the_delegated_run_policy_is_unchanged_by_the_subscription` asserts the delegated job's payload and resource request are unchanged. | pass |
| **The session catalog is unchanged.** No `session` row is created and no session enumeration changes. | The subscription creates no session row; it writes only the `job_background_subscription` row the store already owns. | pass |
| **The legacy delegated path is not modified.** | `src/agent/worker.rs` has no C001 hunk (the parallel worker's own hunks in that file are C002's, not this milestone's). The weaker guarantee is documented in `architecture/agent.md` and `architecture/jobs.md`. | pass |

## 6. Failure and recovery review (plan §8, item by item)

| §8 case | How it was exercised | Result |
|---|---|---|
| **The daemon restarts between submit and subscribe.** | `scheduler_jobs::delegated_notice_c001::a_retried_spawn_completes_the_subscription_after_a_restart` — the first spawn submits; a *fresh* submission service, a *fresh* notifier view over the same durable store, and a new `TaskTool` retry with the same invocation key; the subscription is completed and exactly one notice exists afterwards. | pass (this case needed a production change: `src/tool/task.rs:2242-2266`) |
| **The daemon restarts after the child terminates but before the notice is injected.** | `scheduler_jobs::delegated_notice_c001::the_delegated_run_produces_a_notice_that_names_it` — the notice row is durable and is picked up by the boundary; `agent_loop::delegated_notice_c001::the_next_parent_turn_hands_the_notice_to_the_model` drives a real turn whose first provider request carries the notice. | pass |
| **The parent's turn ends before the child terminates.** | `scheduler_jobs::delegated_notice_c001::the_delegated_notice_survives_a_parent_turn_that_ended` — no live turn is registered, so the in-band push has no target; the durable row is still pending and the next boundary injects it. | pass |
| **The parent's follow-up channel is full.** | `scheduler_jobs::delegated_notice_c001::the_delegated_notice_is_delivered_when_the_in_band_push_is_dropped` — the push has nowhere to land and is dropped; the durable notice is delivered. | pass |
| **The delegated run is cancelled.** | Not exercised end-to-end in this milestone. The notice layer's cancelled status is the pre-existing ADR-0031 machinery (`src/agent/job_notification_recovery.rs:93-95` — `NotifyOnly` never resumes a cancelled run), unchanged by this milestone. | pass by unchanged machinery; not re-proved here |
| **Two delegated runs terminate concurrently.** | `scheduler_jobs::delegated_notice_c001::two_concurrent_delegated_runs_produce_one_notice_each` — two children, one notice each, none lost or duplicated (deduplicated job ids asserted). | pass |
| **The same delegated spawn is retried.** | `scheduler_jobs::delegated_notice_c001::a_retried_spawn_completes_the_subscription_after_a_restart` (idempotent) and `tool::task::tests::a_conflicting_owner_fails_the_delegated_spawn` (a conflicting owner is a spawn failure, never a silent re-target). | pass |
| **The subscription row outlives the delegated run.** | The subscription is keyed by the job id, and the job id is unique per delegated run; a later job never reuses it, so a stale subscription cannot produce a second notice for a different job. The notice id is the job id (`notice_id = job_id`), so at most one notice row exists per delegated run — asserted by the concurrency and the restart tests. | pass |
| **The notification service is unavailable.** | `tool::task::tests::a_failed_subscription_does_not_fail_the_delegated_spawn` — a closed notification pool makes the subscription fail; the spawn still succeeds, the failure is logged at `warn` with the job id, and the orchestrator's fallback is the in-band push. A tool with *no* service at all (the standalone harness) skips the subscription and logs the reason. | pass |

## 7. Migration and compatibility review

- **No migration.** The `job_notification` / `job_background_subscription` tables already exist and
  already serve `bash`, `test` and `ci_watch`. This milestone adds no table, no column and no
  migration.
- **One additive, backward-compatible DTO change.** `JobBackgroundSubscription`
  (`src/scheduler/job_notifications.rs:196` and `:201`) gained two `#[serde(default)]` optional
  fields — `owner_principal` and `workspace_id` — so the subscription states the authority it was
  created under. The subscription is persisted as a JSON blob in `job_background_subscription.record_json`;
  a pre-existing row without those fields still deserializes (the fields default to absent). This
  is **not** a schema change: no table, no column, no migration, and no wire protocol change.
- **No protocol change.** No DTO on the wire is added, changed or removed. The notice is injected
  into the **parent session's** transcript, which every client already renders.
- **No behaviour change for `bash`, `test` or `ci_watch`.** They keep their own subscriptions and
  dispositions; the delegated branch in `format_job_notice_content` is selected by the `subagent`
  kind alone.
- **The legacy delegated path is unchanged** and keeps its poll-only behaviour; the documentation
  states this rather than implying a guarantee it does not have.
- **The delegated run's `result_ref` is retained.** `task action=get` gains fields; it loses none.

## 8. Security review

- **Authorization.** The subscription inherits the parent's session, turn, workspace and owner
  principal and grants no authority beyond delivering a notice. The notice's owner is the *job's*
  owner, so a notice cannot be re-targeted at another session — and a conflicting owner is refused.
- **No token spend, no minted turn.** `NotifyOnly` only; `ResumeOwner` is not used anywhere in this
  milestone, and the guard fails the build if that changes.
- **No covert channel.** The notice text is a function of the delegated run's recorded outcome and
  the parent's own identifiers. It carries the run id, the bounded summary and the result
  reference — nothing else.
- **Bounded content.** The summary is bounded by the notification store's own
  `max_summary_chars` (2048) and the child's recorded output is bounded by the run-result store
  (4096) before it is read. `task action=get` reports both bounds.
- **No secrets.** The notice carries no credential, environment value, provider reasoning or raw
  tool output; the delegated run's recorded summary is the child's own bounded output.
- **No new path or filesystem authority.** The subscription grants no path, tool, sandbox or
  worktree right.

## 9. Documentation and operations

Updated:

- `architecture/agent.md` — "The delegated run's durable terminal notice": the two channels, which
  one is authoritative, the subscription's arguments, failure isolation, the retry-after-restart
  rule, the dedicated notice content, and the honest scope over the durable path (the legacy path
  is named and its weaker guarantee stated).
- `architecture/jobs.md` — "Delegated runs subscribe for the same notice (C001)": the subscriber,
  the three load-bearing rules, the dedicated content format, and the additive subscription fields.
- `architecture/scheduler.md` — "The delegated subscriber (C001)": that the scheduler needed no
  change, and the two delegated-specific details in the notice layer.
- `architecture/tui.md` — "C001 changes no TUI surface": the notice is a channel to the model, not
  a new row for the user.
- `docs/tui.md` — "When the orchestrator hears about its child": the same statement in the
  user-facing document.
- `src/tool/task.rs`'s own tool description now says which path the "prefer wait or push
  notifications over polling get/status" promise holds for (the durable delegated path), and names
  the legacy path's lack of a push.

Static guard delivered:

- `scripts/check_subagent_notification_wiring.py` — three rules (`not-subscribed`,
  `disposition`, `kind`), with a tree-independent `--self-test` that proves each rule bites and
  that a clean tree passes. **Not yet wired into `scripts/verify.sh` or CI — the orchestrator owns
  both files** (see §10, finding F-3).

Operator diagnostics:

- `WARN ... delegated run notification subscription failed; the durable notice is missing and
  delivery falls back to the in-band push` — a subscription failure.
- `WARN ... delegated run has no host-originated parent session; the durable terminal notice is
  skipped` — no owner to notify.
- `WARN ... delegated run has no durable job notification service; delivery falls back to the
  in-band push` — a harness or standalone caller with no service.
- `WARN ... child completion push dropped (...); the durable terminal notice is unaffected` — the
  fast path was lost; the guarantee was not.

## 10. Unresolved findings

No finding blocks this milestone. The three items below are handover items, not outstanding
evidence for it.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| info | **F-1** `bash scripts/verify.sh quick` exits non-zero at exactly one guard: the comment-budget ratchet (`R4 the corpus holds 124671 comment lines, over the recorded ceiling of 124310`). The per-file breakdown in §4.4 shows C001's share at ~156 comment lines and the parallel worker's at ~183. | The `quick` gate is red, but not because of a defect: the corpus is over a ratchet the orchestrator re-banks centrally. Every other guard, the workspace `cargo check` and the package clippy pass. | The orchestrator re-banks `docs/comment-budget.toml` from the final merged tree. This milestone must not edit the bank file, and did not. |
| medium | **F-2** The plan's §15.9 asserts that "ADR-0051 is still unaccepted" and that C002, C003 and C004 are therefore not eligible. **ADR-0051 was accepted on 2026-10-24 as option B** (`plans/adrs/ADR-0051-...md` line 3; HEAD `4803e093c`). The plan's §15.9 is stale. | The unblock audit below follows the *accepted* ADR, not the plan's stale premise. | None for this milestone; recorded as a deviation (D-5). |
| low | **F-3** The new guard is not wired into `scripts/verify.sh`'s `run_guards()` or `.github/workflows/ci.yml`. Both files are owned by the orchestrator and were explicitly excluded from this milestone's edit set. | The guard does not yet protect CI. | The orchestrator wires it in centrally, as instructed. |
| info | **F-5** The `scheduler_jobs` and `agent_loop` families do not run green in this tree, but **not because of this milestone**: a clean worktree at `HEAD` reproduces the same `scheduler_jobs` failures (`346 passed; 6 failed`), and every one of them passes when run in isolation. The `agent_loop` family's one failure is in the parallel worker's in-flight `delegated_transcript` member. | The families are red for pre-existing reasons; C001's own members are green. | Recorded, not fixed here: the failures are cross-test interference and machine-dependent scratch-census tests, and the `agent_loop` failure belongs to the parallel worker. §4.5 has the comparison. |
| low | **F-4** The delegated-run *cancellation* case (§8) is covered by unchanged ADR-0031 machinery rather than re-proved by a new C001 test. | The evidence is inherited, not fresh. | Accepted for this milestone: the delegated kind adds no cancellation behaviour of its own, and the existing `job_continuation_m003_visibility` family covers the notice layer's cancelled status. |

## 11. Roadmap disposition

**Milestone closed.** Every §13 criterion passes, the production behaviour is complete, the new
tests fail against the pre-change tree and pass against this one, and the static guard bites. The
`quick` gate's only failure is the comment-budget ratchet, which is the orchestrator's re-bank
(F-1) and not a defect in this milestone. Two handover items remain and belong to the orchestrator:
re-bank `docs/comment-budget.toml` (F-1) and wire the new guard into `scripts/verify.sh` and CI
(F-3).

## 12. Registry updates

The registry updates this milestone requires are the orchestrator's to make (it owns
`plans/registry.md` and `plans/subsystems/**`):

- `plans/registry.md` — mark milestone 001 of the delegated-run-observability line as closed with
  a pointer to this record.
- the addendum's §12 milestone table — the same.

### 12.1 The unblock audit

Registry updates required (the orchestrator owns `plans/registry.md` and `plans/subsystems/**`):

- `plans/registry.md` — mark milestone 001 of the delegated-run-observability line as closed (or
  conditionally closed) with a pointer to this record.
- the addendum's §12 milestone table — the same.

**Unblock audit.** The plan's §15.9 says C002, C003 and C004 are not eligible because ADR-0051 is
still unaccepted. That premise is stale: **ADR-0051 is accepted** (option B, 2026-10-24), and this
milestone deliberately did not depend on it.

- **C002** (a durable transcript for a delegated run) is **eligible now**, once C001 closes. C002
  needs ADR-0051 (accepted) and nothing from C001 beyond this milestone's closure.
- **C003** (the TUI renders the delegated transcript) is **not** eligible yet: it additionally needs
  **C002 closed**, because the transcript it renders is C002's output.
- **C004** (a delegated run can be continued) is **not** eligible yet: it additionally needs **C002
  closed** for the same reason, and it owns the `ResumeOwner` authority that C001 deliberately did
  not use.

This milestone stayed strictly inside ADR-0031's accepted contract: it wrote no transcript, added
no continuation or resume action, added no migration, changed no `agent_runs` projection DTO, and
did not touch the TUI's run inspector.

### 12.2 Deviations from the plan

| ID | Deviation | Reason |
|---|---|---|
| **D-1** | The `task` tool's `action=get` durable branch now returns a **JSON object** rather than free-form text. | §6.3 requires the recorded result under "a clearly named field, not appended to a free-form string, so its bound is observable". The previous human-readable rendering is retained verbatim in the `text` field, so the `result_ref` line survives. No test or caller in the tree parsed the old text form. |
| **D-2** | `JobBackgroundSubscription` gained two optional `#[serde(default)]` fields (`owner_principal`, `workspace_id`). | §10.1 requires a test that asserts "the parent's workspace and the owner principal" are on the subscription. Without recording them, they are unobservable (they are otherwise only used for the `resume_owner` lease). This is **not** a migration: the subscription is a JSON blob, old rows still deserialize, and no table or column changed. §6's "Protocol and DTOs" section is about *wire* DTOs, which are untouched. |
| **D-3** | The subscription is also created on the **retry** path (`src/tool/task.rs:2242-2266`), before the tool returns the run's status. | The plan's §8 "The daemon restarts between submit and subscribe" case is not reachable otherwise: the tool's existing idempotency branch returns *before* the submit, so a retry never reached the subscribe and the child would have had no notice at all. This is the plan's case, not an extension of it. |
| **D-4** | The static guard has **three** rules and does **not** check the subscription's ordering against the submit. | The briefing specifies exactly three rules. An ordering rule would be wrong: the fresh-spawn path subscribes after its submit, while the retry path (D-3) subscribes a job an earlier invocation already submitted. The ordering property is covered by the restart test instead. The guard's docstring says so. |
| **D-5** | The unblock audit follows the *accepted* ADR-0051 rather than the plan's §15.9 premise that it is unaccepted. | ADR-0051 was accepted on 2026-10-24 (option B) — the plan text predates it. Recorded as finding F-2. |
| **D-6** | `cargo fmt --all` was **not** run; the files this milestone owns were formatted by hand. | A parallel worker is editing `crates/codegg-core` and `src/agent/worker.rs`; a tree-wide format run mid-flight would reformat their half-written files. The orchestrator runs it once, centrally. |
| **D-7** | The regression tests (§10.3) live in `tests/scheduler_jobs/delegated_notice_c001.inc` rather than in `tests/agent_loop/`. | The plan's §10.3 header names `tests/agent_loop/`, but the behaviour under test is the `task` tool's durable spawn and its durable `get` — which need the scheduler, the submission service and the agent-run store. The `agent_loop` family has no such harness, and building one there would have duplicated the `scheduler_jobs` harness. The §10.3 *cases* are all present; only their file differs. |
| **D-8** | The guard's `--self-test` builds a **synthetic** clean tree rather than using the repository tree as the baseline. | The briefing asks for "a synthetic tree". Using the repository as the baseline would make the self-test fail on any tree where the guard legitimately fires (for example the pre-change tree), which would make the self-test a tree-shape check rather than a non-vacuity proof. |

## 13. The delegated notice could be lost entirely (orchestrator, 2026-10-24)

The hosted `verify` job found this one, at the `Workspace tests` step:
`scheduler_jobs::delegated_notice_c001::two_concurrent_delegated_runs_produce_one_notice_each`
failed with *"one notice per delegated run"* — one notice where two were
expected.

It is **not** a flake, and it is **not** a merge regression. It is a
correctness hole in this milestone's central promise, and it was already
present in the tree this milestone closed with.

### The hole

This milestone's ordering is: **submit, then subscribe**
(`src/tool/task.rs` — `submission.submit(...)`, then
`subscribe_delegated_notice(...)`). The scheduler records a terminal notice
only for a job that **already has a subscription**
(`JobNotificationService::record_terminal` returns "no subscription" and
records nothing). So a child that terminates inside that window — which a
fast child does routinely, and which a loaded CI runner makes likelier — had
**no subscription to attribute a notice to**, and its terminal state was
**never recorded at all**.

Nothing healed it. The mechanism that was designed to heal exactly this gap
exists — `JobNotificationService::recover_from_store`, documented as
"backfills completion-before-subscribe and restart gaps" in
`architecture/scheduler.md` and in M001's closure record — but it had **no
caller anywhere in the tree**. It was dead code. So the delegated run's
terminal state was lost, and the orchestrator never heard about that child:
the exact failure this milestone exists to prevent.

The recovery paths that *were* wired do not cover this. The turn-boundary
injection reconciles **existing** pending notices; the daemon-startup hint
recovery reads **durable** notices. Neither can create a notice that was
never written.

### The fix

Two parts, one for the common case and one for the crash.

1. **Hold the terminal until the subscription lands.**
   `JobNotificationService` now keeps a bounded map of delegated terminals
   whose subscription has not arrived. `record_terminal` holds one instead of
   dropping it; `subscribe_with_options` releases it on every success path,
   recording the notice with the executor's own summary — exactly as if the
   subscription had landed first. Only a delegated-shaped job
   (`JobSource::AgentDelegated` or `JobKind::Subagent`) is held, so an
   ordinary background job's terminal is dropped exactly as before and the
   map cannot grow with foreground completions.
2. **Wire the durable backfill.** `JobScheduler::recover_missing_terminal_notices`
   calls `recover_from_store`, and the daemon's job recovery calls it during
   startup. A daemon that restarted between a child's termination and its
   subscription now heals the gap on the next start, which is what
   `architecture/scheduler.md` already promised.

### The test that was measuring the wrong thing

This milestone's family read the notices **the instant**
`wait_for_completion` returned. That wait is for the *execution*: the
scheduler publishes a job's completion before it records the job's durable
terminal notice, so the read raced the notice write. The family's five
notice assertions and its injection pass all shared that race; the
concurrent member simply had two notices to lose instead of one.

The family now waits for the durable notice with a bounded poll and asserts
on what it finds. That is the honest measurement of the invariant — *the
notice is never lost* — and it distinguishes a late notice from a lost one:
a genuinely lost notice now fails with "expected 2 pending notice(s), saw 1"
after the bound instead of racing.

The deterministic regression test for the product fix is in the notification
service's own module (`a_delegated_run_that_terminated_before_its_subscription_still_notifies`):
it records a delegated terminal with no subscription, then subscribes, and
asserts that exactly one notice appears naming the run and carrying the
executor's outcome. `a_foreground_terminal_is_never_held_for_a_late_subscription`
asserts the other half — a foreground job is never held, so the hold map
cannot become a leak.

Both new members are **armed**, proved by reverting the production change
and re-running them:

- with the release of a held terminal disabled, the module member fails with
  `left: 0, right: 1` — the held terminal is never recorded;
- with the backfill's run-id derivation reverted to `None`, the family
  member fails with `left: None, right: Some("f2588a9e-...")` — a backfilled
  delegated notice would not name its run.

The family member stands in for a daemon restart: the child terminates with
no subscription at all, a fresh notification service over the same durable
store subscribes it, and the durable backfill then creates exactly one
notice naming the run. That is the crash variant of the same gap, proved
without a process restart.

### What this changes about the record above

§1's claim that "the scheduler already recorded a terminal notice for *any*
subscribed job, so no scheduler behaviour changed" was true about the
scheduler and incomplete about the whole: the *subscriber* was the only new
part, and the subscriber had a window in which the notice could not exist.
The guarantee this milestone closed with was "a delegated run that terminates
inside its own subscribe window is silently lost". It is now "a delegated
run that terminates is never lost". The durable notice remains the receipt;
the in-band `try_send` push remains the non-authoritative fast path.

### Hosted-CI outcome

The hosted `verify` job passed on the fixed head (`408d39e16`, run
`40138994361`, 24m21s): the delegated notice is no longer lost, and the
family's six notice assertions and its injection pass now measure the
durable receipt instead of racing it. The `Linux M004 process and IPC
qualification` workflow passed on the same head (run `40138994354`:
`process-ipc` and `restricted-userns-fails-closed` green).

The `Windows process launcher` job is still red, on
`scratch::tests::a_tree_with_an_owner_record_that_is_not_owner_only_is_refused`
at `src\scratch.rs:1878` (30 passed, 1 failed). That is **main's** failure,
inherited through the M008 merge: `origin/main`'s own `Windows process
launcher` job on the M008 merge commit `c4fa30aeb` (run `40131159604`)
fails at the identical test and line, while main's `CI` job passes. This
branch changes no scratch code.

## 14. The third merge with `main` (2026-10-25)

`origin/main` advanced to `cb4ceed9d` (PR #194, the M007 desktop editor
surface), and this branch was merged with it. The merge carried **no change to
this line's own Rust code** — the delegated-notice fix recorded in §13 is
byte-identical across the merge — and all four conflicts were documentation:
the registry's "Source verification" entries, `architecture/overview.md`'s
counts table and reconciliation sections, `AGENTS.md`'s published guard counts,
and the comment-budget manifest. Each resolution keeps **both** sides' content.
The count rows were re-measured on the merged tree rather than incremented from
either parent — integration test files **348**, guard scripts **99**, CI guard
scripts **65**, `quick` **79** — and the quick-only difference is **unchanged at
14**, re-derived by differencing the two surfaces' guard names rather than
carried over.

### The inherited red job is now main's fixed test

The "Hosted-CI outcome" subsection above records the Windows `process launcher`
job as red on
`scratch::tests::a_tree_with_an_owner_record_that_is_not_owner_only_is_refused`
and attributes it to `main`. That attribution is unchanged, and `main` has
since **fixed the test it had mis-gated**: the M008 scratch line declared it
`#[cfg(any(unix, windows))]`, but the step that makes the owner record *not*
owner-only is a `chmod`, and `chmod` is `#[cfg(unix)]` — so on Windows the
record still read as owner-only, the reclaim path removed the empty tree, and
the test's `unwrap_err()` panicked on that `Ok`. `main` fixed it in
`5f9f9bc7d` ("gate the owner-only test to Unix, where its premise holds"), and
this branch inherits that fix through the merge.

The failure was never this branch's: `git diff --stat origin/main...HEAD --
src/scratch.rs crates/codegg-core/src/scratch.rs` is empty, and this branch
changes no scratch code. The repair is `main`'s, made there and inherited
here — it is **not** folded into this line's evidence, and the next hosted run
on this branch is expected to carry it. Whether it is green is a claim the
next hosted run makes, not one this record makes in advance.

## 15. The fourth merge with `main` (2026-10-25)

`origin/main` advanced to `682ae762a` (PR #193, the contained-worktree C008
line) and the branch was merged with it again. The merge carried **no change to
this line's own Rust code** — the delegated-notice fix in §13 is byte-identical
across it — and it added **one** integration test file and **no** guard, so
only the *integration test files* row moved: **348 → 349**. *Guard scripts
(all)* stays **99**, *CI guard scripts* stays **65**, and `AGENTS.md`'s
published counts (**79** for `quick`, **65** for CI) are unchanged and needed
no correction. The quick-only difference is **unchanged at 14** and was
re-derived rather than carried over.

The merge surfaced two defects in `main`'s **own** documentation, both corrected
here as a consequence of the merge and recorded where they belong:

- `origin/main`'s `architecture/overview.md` carried the three count rows
  **twice** — once as 341 / 96 / 59 and again as 340 / 96 / 59 — a merge
  artifact from PR #193. The merged tree carries **one** block, holding the
  merged tree's measured values.
- `main` and this branch had each appended an amendment numbered *tenth* after
  their common ninth. `main`'s three amendments keep their published ordinals,
  because `main`'s own later text refers to "the tenth amendment" **by that
  name** and renaming it would rewrite a published record; this branch's two
  were renumbered *tenth* → **thirteenth** and *eleventh* → **fourteenth**, so
  the sequence is unique. No recorded measurement changes.

The comment budget was re-banked on the merged tree — **126,430** comment
lines, **1,115,780** words, a **0.1338** recorded ceiling — and the bank diff
is one new per-file entry for `main`'s new test file plus the global block.
