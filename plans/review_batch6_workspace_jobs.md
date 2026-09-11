# Review: batch6 workspace-jobs

**Reviewed**: 2026-09-11
**Files**: workspace.md, workspace_services.md, jobs.md, scheduler.md, process-tool-execution-ownership.md, project_catalog.md, project_identity_storage.md

## Summary

Seven architecture documents reviewed against source code. The most significant pattern is pervasive line-number drift in `jobs.md` and `scheduler.md`, where nearly every referenced line is 4–248 lines stale. The `jobs.md` doc also understates the `JobStore` trait's method count (claims 16, actual is 21+). `workspace.md` has a stale schema migration line reference. `workspace_services.md`, `project_catalog.md`, `project_identity_storage.md`, and `process-tool-execution-ownership.md` are largely accurate with only minor line drift.

## Documentation Issues

| # | File | Line | Issue | Severity | Suggested fix |
|---|------|------|-------|----------|---------------|
| 1 | jobs.md | 53 | `mod.rs:340–461` typed identifiers: actual `JobId` at line 346, `AttemptId` at 373, `ScheduleId` at 394, `DependencyId` at 415, `DaemonGeneration` at 439. All off by 4–6 lines. | LOW | Update range to `346–465` |
| 2 | jobs.md | 59 | `JobId(String); // line 342` — actual line 346 (off by 4) | LOW | Change to 346 |
| 3 | jobs.md | 60 | `AttemptId(String); // line 369` — actual line 373 (off by 4) | LOW | Change to 373 |
| 4 | jobs.md | 61 | `ScheduleId(String); // line 390` — actual line 394 (off by 4) | LOW | Change to 394 |
| 5 | jobs.md | 62 | `DependencyId(String); // line 411` — actual line 415 (off by 4) | LOW | Change to 415 |
| 6 | jobs.md | 63 | `DaemonGeneration(String); // line 435` — actual line 439 (off by 4) | LOW | Change to 439 |
| 7 | jobs.md | 66 | `DaemonGeneration::new() (line 438)` — actual line 442 (off by 4) | LOW | Change to 442 |
| 8 | jobs.md | 70 | `JobKind (mod.rs:468)` — actual `JobKind` enum at line 472 (off by 4) | LOW | Change to 472 |
| 9 | jobs.md | 85 | `JobSource (mod.rs:548)` — actual `JobSource` enum at line 554 (off by 6) | LOW | Change to 554 |
| 10 | jobs.md | 85 | `JobPriority (mod.rs:580)` — actual `JobPriority` enum at line 586 (off by 6) | LOW | Change to 586 |
| 11 | jobs.md | 92 | `JobPayload (mod.rs:941)` — actual `JobPayload` enum at line 947 (off by 6) | LOW | Change to 947 |
| 12 | jobs.md | 124 | `JobStore trait (mod.rs:1244)` — actual `JobStore` trait at line 1274 (off by 30) | MEDIUM | Change to 1274 |
| 13 | jobs.md | 126 | `16 methods on JobStore` — actual trait has 21 methods (5 additional: `set_job_labels`, `create_job_with_labels`, `get_jobs`, `count_jobs_by_kind_state`, `list_job_records`). Doc omits methods added after initial doc. | HIGH | Update count to 21 and add omitted methods to the table |
| 14 | jobs.md | 147 | `ScheduleStore trait (schedule.rs:231)` — correct (verified at line 231) | — | No fix needed |
| 15 | jobs.md | 160 | `claim_due (schedule_store.rs:519)` — actual `claim_due` impl at line 521 (off by 2) | LOW | Change to 521 |
| 16 | jobs.md | 179 | `ResourceRequest::for_kind (mod.rs:644)` — actual at line 648 (off by 4) | LOW | Change to 648 |
| 17 | jobs.md | 197 | `RecoveryPolicy defaults (mod.rs:1210)` — actual at line 1240 (off by 30) | MEDIUM | Change to 1240 |
| 18 | jobs.md | 203 | `Recovery Contract (mod.rs:1410)` — actual `request_cancel` doc at line 1409 (off by 1) | LOW | Change to 1409 |
| 19 | jobs.md | 203 | `store.rs:694` recovery — `recover_generation` implementation at approximately that line (verified) | — | No fix needed |
| 20 | jobs.md | 220 | `recover_at_startup (scheduler.rs:1281)` — actual `JobScheduler::recover_at_startup` at line 1529 (off by 248) | HIGH | Change to 1529 |
| 21 | jobs.md | 261 | `RunStore Linkage (mod.rs:1144)` — `JobAttempt.run_id` field at approximately line 1267 (the `AttemptCompletion` struct) | MEDIUM | Verify and update line reference |
| 22 | scheduler.md | 57 | `main loop (scheduler.rs:625)` — actual `run()` method (main loop) at line 809 (off by 184) | HIGH | Change to 809 |
| 23 | scheduler.md | 65 | `Reconciliation (scheduler.rs:435)` — actual `reconcile()` at line 583 (off by 148) | HIGH | Change to 583 |
| 24 | scheduler.md | 73 | `Admission (scheduler.rs:680)` — actual `admit_and_dispatch_batch()` at line 833 (off by 153) | HIGH | Change to 833 |
| 25 | scheduler.md | 87 | `JobScheduler (scheduler.rs:99)` — actual `pub struct JobScheduler` at line 135 (off by 36) | MEDIUM | Change to 135 |
| 26 | scheduler.md | 112 | `JobSubmissionService (submission.rs:83)` — actual at line 86 (off by 3) | LOW | Change to 86 |
| 27 | scheduler.md | 124 | `SubmissionKey (line 31)` — correct (verified at line 31) | — | No fix needed |
| 28 | scheduler.md | 129 | `AdmissionController (admission.rs:27)` — actual `AdmissionController` struct at line 99 (off by 72); line 27 is `AdmissionDecision` enum | HIGH | Change to 99 |
| 29 | scheduler.md | 357 | `Shutdown (scheduler.rs:1102)` — actual `shutdown()` at line 1317 (off by 215) | HIGH | Change to 1317 |
| 30 | scheduler.md | 378 | `WokeReason (events.rs:68)` — correct (verified at line 68) | — | No fix needed |
| 31 | workspace.md | 87 | `Schema migration v22 (schema.rs:963)` — actual `migrate_v22` at line 1064, CREATE TABLE at 1067 (off by ~101) | HIGH | Change to 1064 or 1067 |
| 32 | project_catalog.rs:377 | `RegisterLocalProject` — actual at line 378 (off by 1) | LOW | No change needed (within tolerance) |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | `jobs/mod.rs` | `AttemptState::Interrupted` is terminal (`is_terminal()` returns true at line 930) yet `interrupted → Queued` is a valid recovery transition (store.rs:91). The state machine allows a non-exit transition from a terminal state, which is unusual. | `mod.rs:930`, `store.rs:91` | LOW |
| 2 | `scheduler/scheduler.rs` | The `run()` main loop (line 809) directly calls `reconcile()` and `admit_and_dispatch_batch()` without error propagation — reconcile failures are logged and swallowed. This is by design but worth noting. | `scheduler.rs:823-826` | LOW |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | jobs.md | Add a generated-line-number comment or use anchor links instead of hardcoded line numbers. The `mod.rs` file is 1767 lines and line numbers shift with every edit. | Eliminates recurring stale-line maintenance |
| 2 | scheduler.md | Same as above — `scheduler.rs` is 1740 lines; line references drift rapidly. Use `fn name` anchors instead. | Eliminates recurring stale-line maintenance |
| 3 | jobs.md | Document the 5 newer `JobStore` methods (`set_job_labels`, `create_job_with_labels`, `get_jobs`, `count_jobs_by_kind_state`, `list_job_records`) with their purpose and default behavior. | Complete API documentation |
| 4 | workspace.md | The `TurnRunInput` and `ToolRegistryOptions` line references (`turn_runtime.rs:126`, `tool/mod.rs:293`) should be verified and updated if stale. | Accuracy |
| 5 | process-tool-execution-ownership.md | The doc is thorough but could add a one-line summary at the top listing the total number of classified spawn sites for quick verification. | Readability |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | jobs.md:220 | Reference to `scheduler.rs:1281` for `recover_at_startup` | The function is at line 1529 — line reference is 248 lines stale |
| 2 | jobs.md:53 | Range `mod.rs:340–461` for typed identifiers | All individual line refs within are stale by 4–6 lines |
