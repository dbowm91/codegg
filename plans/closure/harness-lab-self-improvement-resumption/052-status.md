# Harness Lab Self-Improvement Resumption S052 — Closure Status

Status: **closed at diagnostic scope; attempt 66 unqualified**

Source implementation plan: `plans/implementation/harness-lab-self-improvement-resumption/052-completion-disagreement-and-job-status-attribution.md`

Source subsystem roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#46-s052--completion-disagreement-and-job-status-attribution`

Repository baseline reviewed: `190d48be7ca32ae30b6b8a44718ddf11ecce9bfb`

Implementation commit: `190d48be7ca32ae30b6b8a44718ddf11ecce9bfb — add strict continuation-status projection and job-status failure attribution`

## 1. Executive finding

S052 is closed at its declared diagnostic scope. The safe projection now exposes allowlisted phase statuses and attributes failed job-status calls by fixed error class, argument shape, returned-ID presence, and request/result correlation. Attempt 66 exercised those diagnostics against the unchanged v97 workload. H007 remains unqualified: the agent reached the 3,300-second prompt-work boundary, timed out with the 300-second verifier reserve intact, did not submit the two final required-success jobs, and qualified neither required repair pair. The evidence does not establish a general daemon, scheduler, ACP, or job-tool defect.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Only allowlisted complete statuses count as complete | 221 focused tests; attempt 66 phase and contract-phase summaries | Pass: 7/7 public phases and 5/5 contract phases project as complete; `document_valid` remains false because final required jobs are incomplete |
| Attribute failed status calls without retaining IDs or error text | Attempt 66 safe cross-tab | Pass: five `execution_backend_unavailable` rows had string-shaped arguments, no returned ID, and unobserved request/result correlation; seven `tool_execution_failed` rows were matched and had returned IDs |
| Preserve privacy and count bounds | S052 campaign projection tests; safe diagnostic and monitor summary | Pass: only fixed labels, enum values, and bounded counts are retained |
| Exercise corrected diagnostics on unchanged v97 | Attempt 66 exact-head preflight and safe diagnostic | Pass for diagnostic coverage; H007 qualification failed |
| Keep live telemetry and resource limits attributable | Attempt 66 preflight and monitor summary | Pass with caveat: request count was reconciled from the bounded lane-local admission parser because the monitor's per-sample request path was mispointed |

## 3. Production implementation evidence

Implementation commit `190d48be7ca32ae30b6b8a44718ddf11ecce9bfb` added explicit continuation status allowlists and bounded, content-free failed-job-status attribution. No CodeGG runtime, daemon, scheduler, ACP protocol, task fixture, or hidden verifier changes were made. Attempt 66 shows the status projection did not mark the required final jobs complete merely because their continuation document said they were complete: `workspace_tests` and `clippy` were both unsubmitted and unobserved in the actual job lifecycle.

The five backend-unavailable status errors are correlated with absent result IDs and unobserved request/result pairs. A production-shaped native job-lifecycle sequence regression passed with the paired binary, but it did not reproduce those five failures. Therefore the cross-tab improves localization but does not prove a general product failure or establish why those individual calls failed.

## 4. Verification executed

On the S052 implementation commit:

- `python3 -m unittest dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_verifier_diagnostics dev.harness_lab.tests.test_campaign_authorization` — **221 tests passed**.
- `python3 scripts/check_test_assertion_quality.py` — **passed**.
- `python3 scripts/check_test_duplication.py` — **passed**.
- `python3 -m py_compile dev/harness_lab/h007_results.py dev/harness_lab/diagnostics.py dev/harness_lab/autonomy_run.py dev/harness_lab/campaign_authorization.py` — **passed**.
- `git diff --check` — **passed**.
- `CODEGG_H007_CODEGG_BINARY=<paired ARM64 binary> python3 -m unittest dev.harness_lab.tests.test_h007_native` — **1 production-shaped native job-sequence regression passed**; this did not reproduce the live backend-unavailable status errors.

These are local checks; no new hosted CI result is claimed.

## 5. Invariant review

Attempt 66 used `opencode_go/muse-spark-1.3-contributor`, unchanged v97 manifest SHA-256 `e87faea3f5da93ae300f2ae21ec46e6ff1f899cff0fdf6c1b9e860c97ffe57ee`, paired ARM64 CodeGG binary SHA-256 `7c87918b4721b798e43798dda33cf4d3958f595bd5268bdc465733a8884b35cf`, and paired helper SHA-256 `6b4f01d4f842bc42ca58bee54738792918ec2eb6f5604e59bae41649214eebd3`. Preflight admitted the 1,200-request/3,600-second policy with the required 30-GiB free-space reserve. Runtime controls remained unchanged. No private receipt, runner log, prompt, raw transcript, tool arguments, continuation body, or hidden-verifier output was inspected or copied into the repository.

## 6. Failure and recovery review

Attempt 66 ran for 3,299.955 seconds with 3,295.770 seconds of active work and 274 provider requests. The final request was clamped to 37.459 seconds to preserve the planned 300-second verifier reserve; it timed out and the agent terminal was `timeout` with stop reason `turn_timeout`. This is the configured prompt-work boundary, not evidence that the provider request ceiling was exhausted.

Safe monitor evidence contains 167 samples at a nominal 20-second interval. Minimum sampled free disk was 112,076,980,224 bytes, about 104 GiB, well above the 10-GiB runtime floor. Event measurement completed with 314 starts and 314 terminals, zero gaps, and zero unpaired calls. Twelve test jobs reached terminal observation; four were observed both running and completed. The largest sampled sorted event gap was 75,656 ms.

The public continuation projection recorded all seven stages and twelve gates complete, but `workspace_tests` and `clippy` were not submitted. The task documented both as complete; the lifecycle projection correctly kept them incomplete. Only one repair cycle was documented and observed, versus the required two; neither named pair qualified because there was no productive change between its failed job and rerun. The independent verifier therefore failed its continuation-boundary and transcript-job-lifecycle domains. The task outcome is failed and H007 is not qualified.

The live monitor's provider-request field was unavailable because its configured path pointed one directory too deep. The final count of 274 was independently obtained from the lane-local provider admissions using the bounded parser. Correct this monitor path before the next start so request counts are sampled during execution.

## 7. Migration and compatibility review

No CodeGG storage schema, protocol, task fixture, or verifier migration was introduced. Older safe campaign evidence remains readable under the additive optional diagnostic fields.

## 8. Security review

S052 stores only fixed phase/status/error enums, shape/correlation states, and bounded counts. The new attempt evidence contains no credential, job identifier, raw argument, tool output, prompt, transcript, continuation body, or hidden assertion. External billed USD remains **unknown** if unreported.

## 9. Documentation and operations

Safe records:

- Preflight: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt66-preflight-20261113.json`
- Completion diagnostic: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt66-safe-diagnostic-20261113.json`
- Monitor summary: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt66-monitor-summary-20261113.json`
- Campaign accounting: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-muse-diagnostic-authorization-20261109.json`

Campaign counters after attempt 66: 66/200 combined starts, 22/40 supplemental-grant starts, and 37/120 task-authorized starts. The supplemental grant has 18 starts remaining; task authorization has 83 remaining. External billed USD is unknown when unreported.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| Medium | H007 task does not complete the required final job submissions and two repair pairs within the 3,300-second prompt-work window | Continue the unchanged v97 run/fix loop under existing authorization; use each safe result to determine whether a reproducible harness defect exists |
| Medium | Five job-status calls classify as backend-unavailable without returned IDs or observable request/result pairs | No general runtime defect reproduced by the native sequence regression; retain as a live diagnostic finding and investigate only if subsequent safe evidence narrows it |
| Low | Live monitor request-count path was mispointed | Correct the lane-local admissions path before the next run and retain bounded per-sample counts |

## 11. Roadmap disposition

S052 closes at strict status-projection and failed-status-attribution scope. Attempt 66 is preserved as an unqualified H007 run. No qualification gate was relaxed, and no broad harness redesign is warranted by this evidence.

## 12. Registry updates

- Mark S052 closed at diagnostic scope and record attempt 66 as unqualified.
- Keep S019 active; the existing supplemental authorization has 18 starts remaining.
- No other blocked plan was unblocked by this closure.
