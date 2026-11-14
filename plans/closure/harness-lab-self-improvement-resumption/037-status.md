# Harness Lab Self-Improvement Resumption S037 — Closure Status

Status: **conditionally closed for reviewer admission and lifecycle; H007 qualification remains unachieved**

Source plan: `plans/implementation/harness-lab-self-improvement-resumption/037-reviewer-start-window-correction.md`

## Finding

Attempt 47 identified the scheduling defect. The fixture selected a 30-minute review threshold; its first due prompt arrived at 1,956,558 ms. The reviewer then reached `deadline_exceeded` after 1,205,060 ms, leaving only 138,270 ms before the parent prompt-work boundary. No completed reviewer result or same-run continuation was observed.

S037 prepared v94, which selects the existing 15-minute path while keeping all non-presentation task fields, verifier identity, review scope, route, model, tools, and caps unchanged. The focused fixture regression passed.

Attempt 48 confirmed the reviewer correction. The reviewer spawned after 340,608 ms of active work, before the 900,000-ms threshold prompt was due. Its durable Run and Task completed after 43,098 ms; the parent observed the completed state and fetched the matching Task once. The review lifecycle therefore finished with ample parent-work time remaining. No reviewer deadline, replacement reviewer, or task continuation was needed.

The full H007 task did not qualify. Attempt 48 reached 3,288.055 seconds of active work and completed all seven phases and all three public artifacts, but only the `catalog_format` repair pair qualified. `catalog_store` had no productive source change after the seed failure and its repair did not reach a terminal observation. Clippy completed; the required `workspace_tests` job was never submitted. The public continuation projection did not record the required review, and the unchanged verifier passed 3/5 with safe failure domains `continuation_boundary` and `transcript_job_lifecycle`.

At the 3,300-second prompt-work boundary, one Bash invocation remained without a terminal event. Its safe projection retains only tool class and elapsed time: `bash`, started at 2,572,719 ms. Its command was not retained or inspected, so it is not attributed to the missing workspace-test job. Event capture recorded 253 starts, 252 terminals, zero explicit gaps, and zero drops; the one unresolved start made the horizon measurement incomplete. This is not evidence of a dropped ACP event.

## Requirement matrix

| Requirement | Attempt 48 evidence | Result |
|---|---|---|
| Reviewer starts early enough to finish and be retrieved | Spawn at 340,608 ms; durable completion after 43,098 ms; one Task fetch after completed Run status | Pass at S037 scope |
| At least 30 minutes active work | 3,288.055 seconds | Pass |
| Required phases and public artifacts | Seven phases; all three artifacts present | Pass |
| Exact-ID productive background work | Running and terminal evidence with productive overlap | Pass |
| Two required repair pairs | One of two qualified; `catalog_store` had no source change and no repair terminal | Fail |
| Required workspace and Clippy jobs | Clippy completed; `workspace_tests` was not submitted | Fail |
| Public continuation and independent verifier | Review lifecycle completed, but public review evidence was not recorded; verifier passed 3/5 | Fail |
| Complete tool-event measurement | 253 starts, 252 terminals, zero explicit gaps/drops; one Bash invocation unfinished at turn timeout | Fail for measurement completeness |
| Resource and cancellation controls | No external hard stop, inactivity cancellation, or disk-floor stop; minimum 28,439,916,544 bytes | Pass; below preflight reserve during run |

## Offline verification

Passed before attempt 48: 83 `dev.harness_lab.tests.test_h007` tests, the test assertion-quality and duplication guards, Python compilation, and `git diff --check`.

## Authorization and cost

Attempt 48 used `opencode_go/muse-spark-1.3-contributor`, start number 49, and 232 provider requests. The provider allowance receipt reported $0.321957; separately billed USD remains unknown. The credential value was not retained. Grant `s019-supplemental-20261111-04` has 5/40 starts consumed and 35 remaining. The campaign counter records 49/200 combined starts consumed and 151 remaining; one historical start-count offset is preserved in the authorization ledger rather than silently dropped.

Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt47-safe-diagnostic-20261111.json` and `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt48-safe-diagnostic-20261111.json` (mode 0600). Raw transcripts, prompts, commands, verifier output, and private logs were not inspected.

## Disposition and registry updates

- Close S037 conditionally at the reviewer admission/lifecycle scope. Do not claim H007 qualification.
- Register S038 for the independently reproduced repair-pair, workspace-test, continuation-record, and unfinished-Bash observations. Keep production daemon, scheduler, ACP, provider, and task-tool architecture out of scope unless new evidence identifies a defect there.
- No other blocked implementation plan was unblocked by this closure. S019 remains active and qualification requires a later run to pass the unchanged verifier and every H007 gate.
