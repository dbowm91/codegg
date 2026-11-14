# Harness Lab Self-Improvement Resumption Milestone S022 — Closure Status

Status: **conditionally closed; offline reconciliation verified; attempt 29 completed unqualified; S023 corrective active**

Source implementation plan:

- `plans/implementation/harness-lab-self-improvement-resumption/022-verifier-evidence-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`

Repository baseline reviewed: `ac584c886f03847ff6b829f8509667637e42a052` (working tree)

## 1. Executive finding

Attempt 28 completed 35.9 minutes of active work with healthy event/job/provider/resource evidence, but the verifier passed 3/5. S022 added safe domain-to-projection reconciliation. Attempt 29 then reached 54.9 minutes of active work and the 3,300-second turn cap. The duration gate passed, but the agent terminal was timeout, the task failed, and the verifier passed 2/5. Safe projections show the continuation document was invalid because it documented two repair cycles while the trace recorded three. The task_behavior verifier domain was not reproduced by public offline checks. S022 delivered its diagnostic outcome; S023 owns the narrow task/checkpoint corrective. Hidden verifier source and assertion text remain uninspected.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Safe test outcome | Attempt 28 receipt | pass | 5 tests ran; 3 passed, 2 assertion failures; no setup error or timeout |
| Public continuation | Attempt 28 receipt | pass | All required phases, stages, gates, jobs, repair pairs, and reviewer evidence projected complete |
| Native lifecycle | Causal trace | pass | 653 events; 292/292 native calls paired; no gaps or drops |
| Sustained horizon | Attempt 28 receipt | pass | 2,152,015 ms active; maximum observed activity gap 90,163 ms |
| Resource bounds | Attempt 28 receipt | pass | 1,200 request cap not approached; disk floor not triggered; cleanup count zero |
| Independent task acceptance | Attempt 28 receipt | fail | Verifier 3/5; task outcome failed; horizon not qualified because task did not pass |
| S022 safe reconciliation | Focused implementation and regression tests | pass | Failure domains are compared with public continuation and horizon state without assertion text or raw identifiers |
| Attempt 29 | Live receipt and safe reconciliation | fail | `evidence/s019-attempt29-safe-diagnostic-20261110.json` | Duration and event/job gates passed; task timed out, verifier 2/5. |

## 3. Production implementation evidence

No Rust product changes are in S022 scope. The safe verifier reconciliation and v84 manifest are implemented; attempt 29 completed unqualified and its follow-up is S023.

## 4. Verification executed

### Commands run

S022 offline verification passed: 186 focused tests across five harness modules, both quality guards, Python compilation, and `git diff --check`.

### Results

Attempt 28: Muse route; 267 provider requests; reported allowance USD 0.010866; 292 terminal calls; 2,152,015 ms active work; verifier 3/5. v84 manifest SHA-256 is `b95124e46c1ecf325d38d54e6d91723e8043ef3e790b3e2345fd30a2288554ce` with v83 parent `8e36f5c03299e4089d950b0564a019deb99890c2cdccd3d6f352f4861b5c9d6a`.



Attempt 29: same Muse route; 318/1,200 provider requests; reported allowance USD 0.166339; 354/354 native calls paired; 822 causal events with no drops; maximum activity gap 74,646 ms; 3,294,358 ms active work; timeout at the 3,300-second turn cap; verifier 2/5. The duration gate passed, but task outcome failed and horizon was not qualified. Provider attempts had zero classified failures/retries/timeouts; disk and inactivity controls did not fire. Public projection showed all phases, stages, gates, artifacts, required job terminals, exact overlap, and reviewer lifecycle complete. Both required repair pairs qualified, but the trace had three correlated cycles and the continuation recorded two, so `document_valid` was false. Failure domains were `continuation_boundary` and `transcript_job_lifecycle` with incomplete public evidence, plus one unlocalized `task_behavior` domain. The public workspace suite passed 15 tests, CLI acceptance passed 15/15, and an additional combined public-contract sequence passed 10/10. These results do not establish that the verifier is wrong. Safe evidence is `evidence/s019-attempt29-safe-diagnostic-20261110.json`; receipt SHA-256 `0cafae58cd67fc4705fc7e7f85d71e4f9e69b7f4631b577ba1e728b19cf262ca`.

## 5. Invariant review

The live run used the authorized model route and retained no raw credential, hidden verifier source, assertion text, job IDs, or transcript in the closure evidence.

## 6. Failure and recovery review

Attempt 28 ended normally but failed verification; attempt 29 ended at the turn timeout before task completion. Across both, provider, ACP event-capture, scheduler, daemon, disk, and activity-stall evidence remained healthy. Attempt 29 had a complete causal trace and reviewer/job lifecycle; its public artifact count was stale, and one task_behavior verifier category remains unreproduced by public checks.

## 7. Migration and compatibility review

No product storage or protocol migration is in scope.

## 8. Security review

The temporary credential value is not recorded. Hidden verifier source, assertion text, traceback, raw output, and transcript remain excluded.

## 9. Documentation and operations

S022 evidence and ledger are reconciled through attempt 30. The ledger records 31/120 combined starts and 8/40 starts in the latest supplemental grant consumed; 89 combined starts and 32 starts in that grant remain. No credential value is recorded.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Attempt 30 timed out at the 3,300-second turn cap before task completion | H007 remains unqualified despite passing the active-duration gate | S024 supplies current live reconciliation counts and attempt 31 must close the task and verifier gates |
| medium | Continuation documented two repair cycles while the trace recorded seven | Continuation and transcript lifecycle domains remain incomplete | S024 makes the live observed/documented values explicit in each follow-up; the agent must update and read back the note |
| low | No task-behavior verifier domain was reported in attempt 30; all five checks still did not pass | H007 remains unqualified on continuation and transcript lifecycle | Keep task, verifier, and horizon criteria unchanged; use safe public projections only |

## 11. Roadmap disposition

S022 is conditionally closed: its bounded reconciliation is implemented and produced a safe, actionable result, but H007 remains unqualified. S023's v85 attempt is implemented and completed unqualified; S024 is active for numeric live repair-count feedback.

## 12. Registry updates

Register S022 as conditionally closed with attempt 29 completed unqualified. Register S023 as implemented with attempt 30 completed unqualified. Register S024 as active. The S019 ledger records 31/120 combined starts consumed, 89 remaining, and 8/40 starts in the latest supplemental grant consumed, 32 remaining.
