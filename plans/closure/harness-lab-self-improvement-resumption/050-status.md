# Harness Lab Self-Improvement Resumption S050 — Closure Status

Status: **closed at event-measurement cause projection scope; attempt 62 unqualified**

Source implementation plan:

- `plans/implementation/harness-lab-self-improvement-resumption/050-event-measurement-cause-projection.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#44-s050--event-measurement-cause-projection`

Repository baseline reviewed: `26c0aebcfa5168b015491b9ef4254f945feec124`

Implementation commit: `6ee1e1a1ec1a279125a26c646f949cbf4a428b92`

## 1. Executive finding

The H007 campaign diagnostic now distinguishes explicit ACP event gaps, observer drain state, unpaired starts, and tool calls without terminal statuses using bounded counts only. Attempt 62 shows complete event measurement: the observer was drained, 273 projected tool calls matched 273 starts and 273 terminals, there were zero explicit gaps, zero unpaired starts, and zero unknown terminal statuses.

S050 is closed at event-measurement diagnosis scope. Attempt 62 remains unqualified because the agent timed out with the required reviewer, two final success jobs, and public artifacts incomplete; the independent verifier reported assertion failures. The attempt also exposed a continuation-layout parser mismatch, assigned to S051.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Distinguish explicit gaps and drain state | Attempt 62 safe event-measurement diagnostic | pass | `observed_complete`, zero event gaps and dropped events |
| Compare observer and projected tool-call pairing | Same safe diagnostic | pass | 273 starts, 273 terminals, 273 projected calls, zero unpaired and unknown calls |
| Reject incomplete or malformed measurement | 219 focused Python tests and projection tests | pass | Tests cover gaps, undrained state, unmatched/unknown calls, malformed fields, and private data removal |
| Qualify H007 | Attempt 62 unchanged verifier | fail | Agent timed out, required completion evidence was missing, and the verifier reported assertion failures |

## 3. Production implementation evidence

The H007 projection derives event completeness from a drained observer, zero gap and dropped counts, equal observer start/terminal/projected-call counts, no unpaired starts, and no call lacking an allowed terminal status. Only fixed state and bounded counters flow through the verifier and campaign projection; event arrays, sources, and call identities are omitted.

## 4. Verification executed

- `python3 -m unittest dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_verifier_diagnostics dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_campaign_authorization dev.harness_lab.tests.test_event_observation` — **219 tests passed**.
- `python3 scripts/check_test_assertion_quality.py` — passed.
- `python3 scripts/check_test_duplication.py` — passed.
- `python3 -m py_compile` on the four changed harness modules — passed.
- `git diff --check` — passed.

These are local focused checks; no hosted CI claim is made here.

## 5. Invariant review

Attempt 62 used the unchanged v97 `build-provenance-catalog` task, the Muse Spark 1.3 Contributor OpenCode Go route, and the same recorded ARM64 CodeGG binary and helper. The Python-only change did not require rebuilding either artifact. The shell ran translated x86_64 on the M4 host; both executable architectures were independently checked as ARM64.

## 6. Failure and recovery review

Attempt 62 lasted 3,299.912 seconds, with a 3,268.281-second active-work span. It used 210/1,200 provider requests and stopped at `turn_timeout`; the independent verifier reported `assertion_failures`. Seven of seven stages and twelve of twelve gates were marked complete, both repair pairs qualified across four observed repair cycles, 50/50 applicable job request/result identities matched, exact job lifecycle and productive overlap were present, and all event measurement checks passed.

The task nevertheless ended incomplete: required final success-job completion was false, required review was unobserved, public artifacts were absent, the continuation document was invalid, and one failed submit had no result ID. Five failed job-status calls were classified as generic `tool_execution_failed`; no `job_not_found` class was observed. External disk monitoring sampled 67 times with a 121.44-GiB minimum, above the 10-GiB runtime floor.

The continuation projection reported three progress sections and four pointers, but could not report progress ordering or pointer placement. Source review found that the actual field is `contract_phases` while `summarize_continuation_line_layout` searches for `completed_contract_phases`. S051 owns that confirmed parser mismatch and adds safe per-label status for the fixed final success jobs.

## 7. Migration and compatibility review

The new diagnostic is additive. Existing receipts without it remain unmeasured in the campaign projection; no product storage schema or migration changed.

## 8. Security review

The public projection retains only fixed observation-state values and bounded counts. Raw event records, source strings, call identifiers, prompts, tool outputs, hidden verifier text, credentials, private receipt, and runner log were not inspected or retained.

## 9. Documentation and operations

Attempt 62 preflight and safe diagnostic:

- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt62-preflight-20261112.json`
- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt62-safe-diagnostic-20261112.json`

Provider USD cost was not reported and remains unknown.

## 10. Unresolved findings

- **Medium:** H007 remains unqualified. Required success jobs, the independent review, and public artifacts were incomplete at timeout; the verifier failed. S051 also fixes the confirmed continuation-layout field-name mismatch.
- **Low:** One failed job submit remained unbound. This is separate from the fully measured event stream.

## 11. Roadmap disposition

S050 closes at measurement-cause projection scope. It establishes that attempt 62's event stream was complete and shifts the next corrective to continuation-layout parity and final-job attribution. H007 remains unqualified.

## 12. Registry updates

The registry marks S050 closed at measurement diagnosis scope and S051 active. No other blocked work is unblocked.

Attempt 62 accounting: campaign 62/200 starts; supplemental grant `s019-supplemental-20261111-04` 18/40 consumed, 22 remain; task authorization 33/120 consumed, 87 remain.
