# Agent Runtime Correctness Milestone 012 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-correctness-autonomy-simplification/012-host-owned-goal-completion-verification.md`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-goal-verification-addendum.md#6-milestone`

Repository baseline reviewed: `004f136c3e47e9731a872244e69bae5072bde199`

Implementation commits:

- `25b85b7c4ac0ed43098dfea873e22ad9d2a2dc96` — implement host-owned goal proposal, evidence assembly, deterministic verdicts, and revision migration.
- `004f136c3e47e9731a872244e69bae5072bde199` — harden stale progress/status revision checks.

## 1. Executive finding

M012 is complete. `goal_request_completion` is now a bounded model proposal
that is evaluated by a stateless core verifier against durable host-owned
test/delegated-job and todo evidence. Only a `Met` verdict can call the
revision-checked terminal transition. Failed or missing evidence remains
non-terminal, semantic criteria use `AwaitingUser`, and the existing bounded
continuation controller remains the only continuation authority.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Model cannot directly complete an active goal | `GoalStore::update_status` rejects `Complete`; tool uses `complete_if_active`; source review of `src/tool/goal.rs` | pass | Explicit user `/goal done` also uses the host CAS boundary. |
| Typed proposal/verdict boundary | `crates/codegg-core/src/goal/verification.rs`; proposal bounds test; `Met`/`NotMet`/`AwaitingUser` tests | pass | No LLM verifier or mutating tool authority exists. |
| Host-owned evidence overrides claims | Failed host test integration test; assembler reads durable `job` records, not model text | pass | Model file/test/evidence prose is non-authoritative. |
| Passing completion follows the real tool path | `test_goal_request_completion_accepts_with_tests` | pass | Durable completed test job → assembler → verifier → complete event/state. |
| Failed and missing evidence stays non-terminal | Core failed-claim/missing-evidence tests; failed-host-test and unfinished-todo integration tests | pass | `NotMet` stores one bounded next action and open-question summary. |
| Semantic/unavailable criteria require user action | `test_goal_request_completion_semantic_criterion_awaits_user`; core unsupported-criterion test | pass | No semantic guess is treated as proof. |
| Stale proposals and contention are safe | `test_complete_if_active_rejects_stale_or_paused_goal`; monotonic revision CAS in migration v45 | pass | Progress and `AwaitingUser` writes also compare revision. |
| Restart does not require verifier memory | Fresh `SqliteJobStore`/`TodoStore` assembly on each request; verifier has no state/cache | pass | Existing durable records reconstruct evidence; missing records fail closed. |
| Continuation/budget/user control remains authoritative | `NotMet` only updates progress; `should_continue` remains the existing bounded path; status CAS tests | pass | No second autonomy loop was added. |
| Plugin absence cannot weaken core completion | Core verifier has no plugin dependency or plugin callback | pass | Optional plugin evidence is not part of this milestone. |
| Compatibility and migration | Additive nullable-safe v45 revision migration; existing `GoalCompleted` event retained; no protocol DTO break | pass | Old goal rows receive revision zero. |

## 3. Production implementation evidence

- `codegg-core::goal::verification` owns bounded `GoalCompletionProposal`,
  `GoalEvidenceContext`, `GoalVerificationVerdict`, and the deterministic
  `GoalVerificationService`.
- `GoalStore` persists a monotonic `revision` (migration v45), rejects generic
  direct completion, and exposes revision-checked completion, status, and
  progress operations.
- `src/goal_verification.rs` is the single application evidence assembler. It
  reads session-scoped durable test/delegated job records created at or after
  the active goal and durable todo records. It maps terminal job states to
  host evidence and treats failures conservatively.
- `GoalRequestCompletionTool` now submits the proposal, applies the verdict,
  records bounded `NotMet` progress or `AwaitingUser`, and publishes the
  existing update/completion events only after host acceptance.
- Supervised test execution now preserves session provenance in its scheduler
  job, allowing completion verification to reconstruct evidence after service
  recreation.
- Prompt and goal architecture documentation now describe request/host
  verification semantics. No new scheduler, workflow engine, plugin authority,
  or protocol surface was introduced.

Evidence-source inventory:

- Host-derived: goal status/revision, durable scheduler job kind/state,
  durable todo status/content, and bounded verifier summaries.
- Model-supplied claims: evidence explanation, file list, test names, and
  remaining-risk text. These are bounded proposal input only and cannot
  elevate failed or absent host evidence.
- Unavailable: arbitrary semantic completion criteria and evidence not retained
  by an owning durable subsystem; these yield `AwaitingUser` or `NotMet`.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core goal
cargo test goal_request_completion --lib
cargo test goal
scripts/verify.sh quick
cargo clippy -p codegg-core --all-targets -- -D warnings
cargo clippy -p codegg --lib -- -D warnings
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_tool_broker_boundary.py
git diff --check
```

### Results

- `cargo test -p codegg-core goal`: passed, 36 tests, 484 filtered.
- `cargo test goal_request_completion --lib`: passed, 6 tests, 4,276 filtered.
- `cargo test goal`: passed, 17 tests, 7,370 filtered across 165 suites.
- `scripts/verify.sh quick`: passed, including formatting, generated-agent
  checks, core-boundary/sandbox/execution-ownership guards, and locked
  workspace all-target checks.
- Both targeted clippy commands passed with `-D warnings`.
- Daemon-CWD, scheduler-bypass, tool-broker-boundary, and diff-check guards
  passed.

All verification above is local evidence; no new hosted CI lane was required
by the plan.

## 5. Invariant review

- The working model cannot directly transition an active goal to `Complete`:
  generic status updates reject that status and the model tool has no direct
  completion call.
- Completion is a core/domain state-machine operation after deterministic host
  verification and an atomic revision/status check.
- Structured failed, in-flight, and missing evidence overrides prose claims.
- Verification only reads stores and value contexts; it does not execute tools
  or mutate the workspace.
- Existing pause, cancellation, budget, and steering status paths remain
  authoritative; stale verifier writes are rejected by revision CAS.
- Verification has no loop or scheduler ownership; `NotMet` returns control to
  the existing bounded continuation decision.
- Plugin installation or failure is irrelevant to the safe core default.

## 6. Failure and recovery review

- Duplicate/concurrent completion proposals: the first matching revision wins;
  later proposals receive a bounded stale result and cannot publish completion.
- Pause/cancel/replacement during verification: the active/status/revision
  predicate rejects terminal or progress commits from the stale proposal.
- Daemon restart: the stateless verifier reassembles jobs and todos from SQLite;
  no in-memory evidence cache is authoritative.
- Failed, timed-out, cancelled, interrupted, expired, queued, and running jobs
  are fail-closed as failed or incomplete host evidence.
- Repeated identical `NotMet` results replace bounded progress summary and
  next-action/open-question state; they do not spawn a synthetic loop. Existing
  continuation caps remain unchanged.
- Malformed or oversized model input is rejected or bounded before verification
  and is stored/rendered only as data.

## 7. Migration and compatibility review

Migration v45 adds `goal.revision INTEGER NOT NULL DEFAULT 0`, preserving old
rows and requiring no backfill inference. Existing `GoalCompleted` event shape
and client status remain compatible; its evidence field now contains a bounded
host-verification summary. `/goal done` retains explicit user behavior while
using the same host CAS transition. No public protocol DTO or plugin contract
changed.

## 8. Security review

The verifier has no mutating tool, process, network, plugin, or workspace
authority. Model prose and path/file claims are not executed and do not grant
completion. Evidence queries are bounded to the current session and goal
creation boundary. Unknown semantic criteria and absent durable evidence fail
closed to `AwaitingUser`/`NotMet`; plugin presence is not required.

## 9. Documentation and operations

Updated:

- `architecture/goal.md`
- `src/agent/prompt.rs` goal contract wording
- `plans/implementation/agent-runtime-correctness-autonomy-simplification/012-host-owned-goal-completion-verification.md`
- `plans/subsystems/agent-runtime-goal-verification-addendum.md`
- `plans/registry.md`

Operationally, a `NotMet` response exposes a bounded `next_action` and
verification gap summary through the ordinary goal state. A semantic or
remaining-risk request shows `AwaitingUser`; the user can resume or steer via
existing goal controls.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None in M012 scope | — | No corrective follow-up required. |

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. The registered-plan audit
found no downstream plan whose blocker is this M012 closure. The independent
runtime-safety M011 and runtime-assets M005 plans remain ready; runtime-safety
M012 and runtime-assets M006 retain their existing named blockers.

## 12. Registry updates

- Marked the host-owned goal-verification roadmap and M012 implementation
  `closed`/`implemented` as applicable.
- Removed M012 from the dependency-ready implementation table.
- Added the accepted closure record and implementation commit reference to
  `plans/registry.md`.
- Recorded that no future registered plan was unblocked; unrelated blocked
  plans were intentionally left unchanged.
