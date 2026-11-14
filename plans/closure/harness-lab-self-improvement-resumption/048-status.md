# Harness Lab Self-Improvement Resumption S048 — Closure Status

Status: **closed at safe timeout and failed-check causal-projection scope; attempt 60 unqualified**

Source implementation plan:

- `plans/implementation/harness-lab-self-improvement-resumption/048-timeout-and-failed-check-causal-projection.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#42-s048--timeout-and-failed-check-causal-projection`

Repository baseline: `7bc59352a0c7a337f442bea5aa64947315042712` (pushed checkpoint).

## 1. Result

S048 is complete at its diagnostic scope. Its campaign evidence now preserves the fixed continuation-layout counts, the two named repair-group predicates, bounded job-ID request/result correlation and unbound-call states, and existing prompt-timeout timing. Attempt 60 exercised these projections on the unchanged H007 v97 case. The run did not qualify H007.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Preserve continuation structure using fixed counts | Attempt 60 safe diagnostic | pass | 65 lines, 3 progress sections, 4 pointer fields; document invalid, pointer block not terminal; section ordering unavailable |
| Name each fixed repair group without raw IDs | Attempt 60 safe diagnostic | pass | `catalog_store` qualified; `catalog_format` did not because immediate status after the seed submission was not observed |
| Project bounded job ID correlation and unbound call state | Attempt 60 safe diagnostic | pass | 166 of 167 applicable calls matched; one was unobserved and one call was unbound |
| Report timeout boundary and request timing | Attempt 60 safe diagnostic | pass | 22 followups, 23 prompt requests, last request 62.687 seconds, 299.974 seconds remained at timeout |
| Preserve privacy boundary | Safe diagnostic and preflight records | pass | No prompt, transcript, tool arguments/results, raw job IDs, credential, verifier assertion text, private receipt, or runner log was inspected or retained |
| Qualify H007 | Attempt 60 safe diagnostic | fail | Task incomplete; independent verifier reported assertion failures; H007 remains unqualified |

## 3. Attempt 60 evidence

- Route/model: `opencode_go` / `opencode_go/muse-spark-1.3-contributor`.
- Fixture: unchanged v97 `build-provenance-catalog`; manifest digest `e87faea3f5da93ae300f2ae21ec46e6ff1f899cff0fdf6c1b9e860c97ffe57ee`.
- Binary digest: `7c87918b4721b798e43798dda33cf4d3958f595bd5268bdc465733a8884b35cf`.
- Duration: 3,299.655 seconds; active-work span: 3,288.685 seconds.
- Provider requests: 390/1,200; native calls stayed within the 1,600 cap. Stop reason: `turn_timeout` at the 3,300-second prompt-work ceiling, with the 300-second verifier reserve intact.
- Seven of seven stages and twelve of twelve gates were complete; public artifacts, release overlap, and independent review were observed.
- Task completion remained `agent_incomplete`; the continuation document was invalid and repair pairs were incomplete. `catalog_format` failed the immediate seed-status predicate; `catalog_store` passed. Four repair cycles were observed and matched the documented count.
- Job event measurement was complete and the serialized transcript projection matched. Exact job lifecycle and productive overlap were observed. One failed `job status` call had a string-shaped ID argument but no returned ID; its error class is unavailable, motivating S049. No daemon/scheduler/ACP lifecycle defect is established by this trace.
- External disk samples during the run stayed above 119.303 GiB; the 10-GiB runtime floor was never approached. USD remains unknown if unreported.

## 4. Verification

Before attempt 60, S048's focused Python suites passed (18 campaign/CLI tests and 197 autonomy, verifier-diagnostic, and H007 tests); test assertion-quality and duplication guards, Python compilation, and `git diff --check` passed. Attempt 60 passed the fresh 30-GiB preflight and was admitted under the 1,200-request, 3,600-second wall, 1,600-native-call, 100-followup, 300-second inactivity, 10-GiB runtime-floor, and 300-second verifier-reserve controls.

The safe preflight and diagnostic are:

- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt60-preflight-20261112.json`
- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt60-safe-diagnostic-20261112.json`

The private receipt and runner log remain uninspected.

## 5. Accounting and handoff

Attempt 60 was counted after provider admission. Durable counters after reconciliation: campaign 60/200; active supplemental grant `s019-supplemental-20261111-04` 16/40; task authorization 31/120. Attempt 53 remains a preserved pre-provider refusal. The current active grant has 24 starts remaining; the task authorization has 89 remaining. S049 is the next narrow diagnostic-corrective. H007 is not qualified.
