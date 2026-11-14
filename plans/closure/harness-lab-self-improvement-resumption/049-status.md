# Harness Lab Self-Improvement Resumption S049 — Closure Status

Status: **closed at failed job-status error-attribution scope; attempt 61 unqualified**

Source implementation plan:

- `plans/implementation/harness-lab-self-improvement-resumption/049-failed-job-status-error-attribution.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#43-s049--failed-job-status-error-attribution`

Repository baseline reviewed: `7bc59352a0c7a337f442bea5aa64947315042712`

Implementation commit: `26c0aebcfa5168b015491b9ef4254f945feec124`

## 1. Executive finding

The safe ACP projection now retains one fixed error class for each failed native `job status` call. Attempt 61 observed three `tool_execution_failed` results, with no `job_not_found`, `unclassified`, or `unobserved` status errors. This distinguishes a generic tool-execution failure from a missing or foreign job ID without retaining output text.

S049 is closed at that diagnostic scope. H007 remains unqualified. Attempt 61 ran the unchanged v97 task for 41.44 active minutes, but the independent verifier reported assertion failures, event measurement was incomplete, one failed job submission had no result ID, and continuation line ordering could not be established. S050 owns the missing safe distinction between explicit event gaps and unpaired or otherwise unknown tool calls.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Classify only failed `job status` output | Production-shaped ACP regression in `test_autonomy_run.py` | pass | Recognized `job not found`, generic execution errors, unknown strings, and absent output project to fixed classes |
| Preserve no error body or job ID | Same regression and direct diagnostic tests | pass | Serialized calls and sequence contain no supplied private marker, raw ID, or argument digest |
| Bound and allowlist public error-class counts | `test_verifier_diagnostics.py`, `test_h007.py`, `test_campaign_authorization.py` | pass | Unknown class, Boolean count, and count above 1,600 are dropped |
| Distinguish job-not-found from generic failures live | Attempt 61 safe diagnostic | pass | Three failures classified as `tool_execution_failed`; zero as `job_not_found` |
| Qualify H007 | Attempt 61 unchanged verifier | fail | `assertion_failures`; event measurement and continuation evidence are incomplete |

## 3. Production implementation evidence

`_ToolCallProjector` classifies only terminally failed `job`/`status` calls from the already delivered ACP output. It retains only the fixed class. `summarize_job_tool_sequence` counts those classes; the H007 reconciliation and campaign projection accept only the fixed class set and counts from 1 through 1,600. Missing output remains `unobserved`; unknown text is `unclassified`.

No Rust, daemon, scheduler, ACP protocol, task prompt, verifier, model, or cap behavior changed. The implementation is Python-only, so the previously paired ARM64 CodeGG binary and sandbox helper remain applicable.

## 4. Verification executed

On the committed implementation:

- `python3 -m unittest dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_verifier_diagnostics dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_campaign_authorization` — **214 tests passed**.
- `python3 scripts/check_test_assertion_quality.py` — passed.
- `python3 scripts/check_test_duplication.py` — passed.
- `python3 -m py_compile` on the four changed harness modules — passed.
- `git diff --check` — passed.

These are local focused checks; no hosted CI claim is made here.

## 5. Invariant review

The task remains the unchanged v97 `build-provenance-catalog` fixture, using only `opencode_go/muse-spark-1.3-contributor`. Attempt 61 used the recorded paired binary and helper hashes. `file` confirmed both Mach-O artifacts are ARM64; the shell process is translated x86_64 on the M4 host. The artifact architecture was recorded separately from the shell architecture.

## 6. Failure and recovery review

Attempt 61 was counted after 164 provider requests. It lasted 2,508.796 seconds, with a 2,486.388-second active-work span. The agent terminal was `completed`; all seven stages and twelve gates were recorded complete. Both required repair pairs qualified, with three observed and documented repair cycles. Exact job request/result correlation matched for 34 applicable calls, but one failed submit had no returned job ID. The transcript projection matched its source; required review, productive job overlap, and terminal test-job observations passed.

The continuation had 84 lines, three progress sections, and four pointer fields. The pointer fields were in expected order, but their placement relative to progress sections and the section ordering were unavailable; the pointer block was not terminal. Event measurement was incomplete. Safe output does not identify whether that came from an explicit event-gap notification or a tool call lacking a terminal event; S050 adds only those bounded counts and observation state.

The independent verifier reported `assertion_failures`; task outcome was `failed`, despite the agent ending normally. This is not a successful H007 task. No daemon/scheduler lifecycle defect is established by these observations.

## 7. Migration and compatibility review

The campaign projection is additive and continues to drop fields outside its explicit allowlists. Existing receipts without the new histogram remain valid and project an empty map. No persisted product schema or migration changed.

## 8. Security review

Only allowlisted error class names and bounded integer counts are persisted in safe diagnostics. Raw output, prompts, transcript text, job IDs, argument digests, hidden verifier assertions, credentials, private receipt, and runner log were not inspected or retained in this closure. A content scan of changed files found no key-shaped credential or private run artifact path.

## 9. Documentation and operations

Attempt 61 preflight and safe diagnostic:

- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt61-preflight-20261112.json`
- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt61-safe-diagnostic-20261112.json`

The external disk monitor sampled 49 times; minimum free space was 132,139,053,056 bytes (123.06 GiB), above the 10-GiB runtime floor. Provider cost was not reported and remains unknown.

## 10. Unresolved findings

- **Medium:** H007 remains unqualified. Attempt 61 had an incomplete event-measurement gate, an invalid continuation layout, one unbound failed submit, and independent verifier assertion failures. S050 owns safe event-measurement cause attribution; the failed verifier remains a task failure until a later unchanged verifier passes.
- **Low:** Three failed job-status calls were classified only at the generic `tool_execution_failed` boundary. The fixed class correctly distinguishes them from a `job_not_found` result; detailed error text remains deliberately unavailable.

## 11. Roadmap disposition

S049 closes at error-attribution scope only. It does not qualify H007. S050 is active for the single missing safe measurement-cause projection.

## 12. Registry updates

The registry marks S049 closed at attribution scope and registers S050 active. No other blocked plan is unblocked by this closure.

Attempt 61 accounting: campaign 61/200 starts; supplemental grant `s019-supplemental-20261111-04` 17/40 consumed, 23 remain; task authorization 32/120 consumed, 88 remain.
