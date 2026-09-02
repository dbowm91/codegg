# Agent Runtime Correctness Milestone 013 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-correctness-autonomy-simplification/013-goal-evidence-provenance-and-criterion-corrective-pass.md`

Source corrective roadmap:

- `plans/subsystems/agent-runtime-goal-verification-corrective-addendum.md`

Historical predecessor preserved:

- M012 implementation: `plans/implementation/agent-runtime-correctness-autonomy-simplification/012-host-owned-goal-completion-verification.md`
- M012 closure: `plans/closure/agent-runtime-correctness-autonomy-simplification/012-status.md`

Repository baseline reviewed: `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce`

Implementation commit:

- `a64990d` — enforce exact goal evidence provenance and conservative criterion semantics.

## 1. Executive finding

M013 is closed. The M012 host-owned completion authority is preserved, while
the two later-discovered evidence-quality defects are corrected:

- supervised Test/Subagent evidence is host-associated with the exact goal by
  durable reserved metadata, rather than inferred from session/time activity;
- free-form completion criteria are no longer classified by substring
  heuristics, so unsupported natural language remains `AwaitingUser`.

Model `tests_run` and `files_changed` fields remain bounded explanatory claims.
A test claim requires a passing host-owned test for the active goal, but its
free-form name is not treated as invocation identity. File claims never
provide positive proof.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Host-written exact-goal provenance | `JobSubmissionService::new_with_goal_store`, `goal_provenance_labels`, and core `GOAL_PROVENANCE_LABEL_KEY` | pass | Daemon production wiring reads the active goal from `GoalStore`; callers do not supply the reserved label. |
| Durable label persistence | `JobStore::create_job_with_labels` / `set_job_labels` implemented by `InMemoryJobStore` and `SqliteJobStore` | pass | Labels are persisted before scheduler enqueue; legacy jobs remain unlabeled and unavailable for positive proof. |
| Test/Subagent submission coverage | `active_goal_provenance_is_host_written_for_test_and_subagent_jobs` | pass | The test also verifies that clearing the active goal does not leave stale provenance. |
| Exact-goal evidence assembly | `src/goal_verification.rs::assemble(pool, session_id, goal_id, created_at)` | pass | Session/time are secondary bounds; only a matching durable `goal_id` label is eligible. |
| Same-session goal isolation | `test_goal_request_completion_ignores_other_goal_evidence` | pass | Goal A evidence cannot satisfy Goal B. |
| Same-session failure isolation | `test_goal_request_completion_other_goal_failure_does_not_poison_current_goal` | pass | Goal B can complete with its own passing evidence despite Goal A failure. |
| Failed/in-flight matching evidence | Existing M012 integration coverage plus exact-goal assembler filtering | pass | Matching failures and in-flight records remain fail-closed. |
| Conservative criterion semantics | `pass_security_review_is_not_a_test_criterion`; root integration regression | pass | All non-empty free-form criteria are user-verifiable; no `test`/`pass`/`green`/`todo`/`task` guessing remains. |
| Claimed test/file scope | `GoalCompletionProposal` contract comments and no positive file-claim path | pass | Names and paths cannot borrow unrelated host evidence or elevate a verdict. |
| Restart reconstruction | SQLite labels are read by every fresh assembler invocation; no verifier cache exists | pass | Persisted label is the only ownership relation after restart. |
| Authority and CAS | Existing M012 `GoalStore` revision/CAS path unchanged; `GoalRequestCompletionTool` still completes only after `Met` | pass | Pause, cancel, replacement, budget, steering, and continuation ownership remain unchanged. |
| Scheduler/tool boundary | `scripts/check_scheduler_bypass.py`, `scripts/check_tool_broker_boundary.py` | pass | No second scheduler or mutating verifier was introduced. |

## 3. Production implementation evidence

- `codegg_core::jobs::GOAL_PROVENANCE_LABEL_KEY` defines the reserved
  host-owned `goal_id` label.
- `JobSubmissionService` has a daemon-only constructor that carries a
  SQLite-backed `GoalStore`. For `Test` and `Subagent` jobs with a session,
  it reads the active goal snapshot and writes the label through the durable
  job store before calling `enqueue_existing`.
- The production daemon uses the goal-aware constructor whenever a SQLite pool
  exists. Legacy/in-memory daemon construction remains compatible and does
  not invent stale provenance.
- `GoalStore::active_for_session` is checked for `GoalStatus::Active` before
  labeling. Awaiting-user, paused, budget-limited, terminal, or absent goals
  receive no label.
- `assemble` filters by session, creation bound, and exact `goal_id`; missing
  or different labels are ignored for positive evidence. It reconstructs
  solely from SQLite records after restart.
- The core verifier retains global deterministic gates for matching failed,
  in-flight, passed, and unfinished-todo evidence, but makes all free-form
  criterion strings unavailable to automatic semantic proof.
- No public goal protocol, scheduler authority, workflow engine, plugin
  callback, file-history store, or LLM verifier was added.

Evidence-source inventory:

- Host-derived: active goal ID/status, durable job label/kind/state, durable
  todo state, and bounded verifier summaries.
- Model-supplied claims: evidence explanation, file list, test-name list, and
  remaining-risk list. These are bounded data only.
- Unavailable: exact test-name invocation identity and exact goal-level file
  history where no existing host-owned source supplies it. Both remain
  non-authoritative by design.

## 4. Criterion matrix

| Criterion input | Automatic host meaning | Verdict posture |
|---|---|---|
| Empty criteria + passing goal-owned supervised test claim | Global test gate is satisfied | `Met` when no other gate blocks |
| Empty criteria + no test claim and no explicit risk | No positive execution claim | `NotMet` |
| `Pass security review` | No typed deterministic contract exists | `AwaitingUser` |
| `Product owner signs off` | No typed deterministic contract exists | `AwaitingUser` |
| Any other non-empty natural-language criterion | No semantic inference | `AwaitingUser` |
| Any `files_changed` value | Model explanation only | Cannot elevate verdict |
| Any exact `tests_run` string | Model explanation; only goal-owned host pass counts | Cannot borrow unrelated evidence |

## 5. Verification executed

### Commands run

```bash
cargo fmt --check --all
cargo test -p codegg-core goal
cargo check -p codegg --lib
cargo check -p codegg --tests
cargo clippy -p codegg-core --all-targets -- -D warnings
cargo clippy -p codegg --lib -- -D warnings
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_tool_broker_boundary.py
bash scripts/check-core-boundary.sh
scripts/verify.sh quick
git diff --check
```

### Results

- `cargo fmt --check --all`: passed.
- `cargo test -p codegg-core goal -- --test-threads=1`: passed, 37 tests.
  An unconstrained rerun had one intermittent failure in the pre-existing
  checkpoint append test; the isolated test and serialized suite both passed.
- `cargo check -p codegg --lib`: passed.
- `cargo check -p codegg --tests`: passed, including the new submission and
  goal integration test code.
- `scripts/verify.sh quick`: passed, including generated-agent, core-boundary,
  sandbox, execution-ownership, formatting, and locked all-target checks.
- Scheduler-bypass, tool-broker-boundary, core-boundary, and diff checks passed.
- The required Clippy invocations reached pre-existing exact-head findings
  outside this milestone: six `clippy::type_complexity` findings in
  `crates/codegg-core/src/snapshot/checkpoint.rs`, and the root
  `src/agent/tool_batch.rs` `unnecessary_unwrap`/tuple-complexity findings.
  No finding was introduced in the M013 goal/provenance changes. The active
  runtime-safety checked-edit-history M013 plan already owns those baseline
  checkpoint/tool-batch findings.
- `cargo test goal_request_completion --lib` was attempted. The root test
  binary could not link under this x86_64 macOS toolchain because the
  available `/opt/local/lib/liblzma.dylib` is arm64, producing undefined
  `_lzma_*` symbols. The root test code was fully type-checked by
  `cargo check -p codegg --tests`; core goal tests executed successfully.

All passing results are local evidence. No hosted CI result is claimed for
this closure.

## 6. Failure, recovery, and contention review

- A job without `goal_id`, including a legacy job, is unavailable positive
  evidence and is never matched by timestamp, display name, or session alone.
- A failed or in-flight job with the matching goal label remains fail-closed;
  a failure carrying another goal label is excluded from the active goal.
- Goal replacement races use the active goal snapshot read at host submission
  time. Verification later uses the persisted label, not whichever goal is
  active at verification time.
- Restart reopens the durable job records and labels; no in-memory map is
  authoritative.
- Goal status/revision CAS and the existing `NotMet` continuation path were
  not redesigned. A stale completion, pause, cancellation, replacement, or
  budget transition still wins through the existing revision/status checks.
- The verifier performs no tool, process, workspace, network, or plugin
  mutation and does not create retries or autonomous loops.

## 7. Compatibility and migration

The preferred existing `JobRecord.labels` storage is used; no database
migration or public protocol change is required. Existing jobs retain their
labels as `{}` and therefore fail conservatively for goal-specific positive
evidence. Existing `GoalCompleted` behavior and M012 revision/CAS semantics
remain compatible.

## 8. Security review

The reserved label is not accepted in `NewJob`, model tool arguments, or
public completion input. It is written only by the daemon-aware submission
boundary after active-goal lookup. It grants no scheduler priority, tool
permission, filesystem authority, or execution capability. Verification does
not log proposal prose, file bodies, or command output to establish evidence.

## 9. Documentation and planning updates

Updated:

- `architecture/goal.md`
- `architecture/jobs.md`
- `plans/implementation/agent-runtime-correctness-autonomy-simplification/013-goal-evidence-provenance-and-criterion-corrective-pass.md`
- `plans/subsystems/agent-runtime-goal-verification-corrective-addendum.md`
- `plans/registry.md`
- this closure record

M012 closure history was not rewritten. Its direct model self-certification
removal remains intact as historical evidence; this record owns the later
provenance and criterion corrections.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| — | No unresolved M013-scope correctness finding | Closed by implementation and focused evidence. |
| Low / out of scope | Exact-head Clippy baseline findings in snapshot/tool-batch code | Remain with runtime-safety checked-edit-history M013; not introduced or required by goal verification. |
| Environmental | Root executable goal integration tests cannot link with this x86_64/arm64 liblzma setup | Type-check evidence and core executable tests pass; rerun root tests in a matching native toolchain. |

## 11. Roadmap disposition

M013 is closed as the strict corrective disposition for the goal-verification
line. The corrective addendum is closed. A registry audit found no later
registered plan whose blocker is goal-verification M013, so no future plan was
unblocked or had its status changed by this closure. Runtime-safety M013 and
runtime-assets M007 remain independently registered and unchanged.

## 12. Registry updates

- Goal-verification corrective follow-up moved from `active`/`ready` to
  `closed` in `plans/registry.md`.
- M013 was removed from the dependency-ready table and added to recently
  closed work.
- The source corrective addendum now records M013 as closed.
- The implementation plan is marked `implemented`.
- M012 implementation and closure records remain historical and unchanged.
- The dependency audit explicitly records that no future registered plan was
  unblocked by M013.
