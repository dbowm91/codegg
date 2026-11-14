# Harness Lab Self-Improvement Resumption S058 Closure — Failed Job-Status Identity and Error Typing

Status: **closed at failed job-status identity and error-typing scope; H007 remains unqualified**

Repository baseline reviewed: `22c1d481ec8cefe8a29b275f3ce232dbb059c185`

Implementation commit:

- `22c1d481ec8cefe8a29b275f3ce232dbb059c185` — preserve a valid requested UUID on ordinary failed `job.status` responses and distinguish execution failure from backend unavailability.

Source plan: `plans/implementation/harness-lab-self-improvement-resumption/058-failed-job-status-identity-and-error-typing.md`.

## 1. Executive finding

S058's bounded correction is verified. Attempt 79 exercised the exact S058 binary with the unchanged v97 H007 workload. All 169 applicable job request/result ID correlations matched; 46 failed `job.status` responses retained their requested IDs and were safely classified as one `job_not_found` and 45 `tool_execution_failed`. This closes S058's runtime/diagnostic scope. H007 did not qualify: the agent timed out after approximately 54.75 minutes of active work, and neither required final workspace-test nor Clippy job was submitted and completed. The independent verifier reported a setup error. S059 owns the separate broker error-typing defect and missing safe verifier setup-category projection discovered in this attempt.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Preserve valid requested UUID on ordinary failed status lookup | S058 Rust tests; attempt 79 safe job ID correlation | pass | 169/169 applicable calls matched; no raw IDs retained. |
| Keep ordinary execution error distinct from true backend unavailable | S058 Rust and diagnostic tests; attempt 79 error-class counts | pass | Failed status calls were `job_not_found` or `tool_execution_failed`; this does not fix other tools' broker fallback. |
| Preserve production-shaped ACP request/result identity | S058 ACP regression; attempt 79 | pass | Safe projection records no unbound job calls. |
| Keep H007 task and verifier unchanged and exercise exact corrective head | Attempt 79 preflight and safe diagnostic | pass | Exact S058 pair, Muse/OpenCode Go route, v97 manifest and limits. |
| Qualify H007 | Attempt 79 | fail | Agent terminal was timeout; required successful workspace tests and Clippy were incomplete; verifier state was `setup_error`. |

## 3. Production implementation evidence

`JobTool::execute_structured` returns a bounded unsuccessful result for ordinary execution failures on a valid UUID `status` request and echoes only the ID provided by that request. It does not claim a job state or expose stored job data. The safe classifier reads the first detail line, preserving the stable `job_not_found` class when the ID line follows it. Permission, timeout, malformed-ID, and explicit backend-unavailable paths retain their prior typed behavior.

Attempt 79's safe campaign summary showed one `job_not_found` and 45 `tool_execution_failed` failed status results, with 169/169 job-ID request/result correlations matched. It also exposed two failed required test submissions classified as `execution_backend_unavailable`. That separate source path is addressed by S059; it does not reopen S058.

## 4. Verification executed

### Commands run

The committed S058 verification is recorded in implementation commit `22c1d481ec8cefe8a29b275f3ce232dbb059c185`: focused Rust job-tool tests, focused Python autonomy/diagnostic tests, test-quality guards, formatting, and diff checks passed. The ARM64 exact-head paired binaries were built and preflighted for attempt 79.

### Results

Attempt 79 ran 3,298.972 seconds total, with 3,284.983 seconds active work, and admitted 427 provider requests. It had 466/466 projected tool starts and terminals and zero event gaps. The task completed seven of seven stages, both required repair pairs, and review; it did not submit either required final success job. The agent terminal was timeout. The independent verifier reported `setup_error`; no bounded setup reason was included in the safe campaign projection. The run remained within all resource bounds and had no external-stop reason.

## 5. Invariant review

- Failed status lookup never fabricates a state or terminal success: pass.
- ID echo is limited to the bounded valid UUID supplied by the caller: pass.
- No owner, stored job state, raw ID, error text, or private transcript data appears in public evidence: pass.
- True backend-unavailable status errors keep their typed path: covered by focused tests; attempt 79 did not report such a status error.
- H007 task, verifier, route, and caps remained unchanged: pass.

## 6. Failure and recovery review

S058 performs no retry, job submission, persistence change, or scheduler operation. The failed lookup remains an ordinary unsuccessful status response bound to the request ID. Attempt 79's two failed test submits are outside S058; source analysis found a generic broker fallback that can report ordinary `ToolError::Execution` as infrastructure failure. S059 covers that independent correction.

## 7. Migration and compatibility review

No schema, migration, public argument, or wire-protocol change was introduced. Successful status responses are unchanged. Failed valid-UUID status results add the request ID and fixed error class while remaining failures.

## 8. Security review

The response echoes only a bounded UUID already supplied in the request. It does not disclose owner, state, or stored job details on lookup failure. Campaign evidence retains only fixed classes and correlation counts; private receipt, runner output, transcript, raw tool payloads, verifier output, and credentials were not inspected.

## 9. Documentation and operations

Attempt 79 evidence is recorded in `evidence/s019-attempt79-preflight-20261114.json`, `evidence/s019-attempt79-safe-diagnostic-20261114.json`, and `evidence/s019-attempt79-monitor-summary-20261114.json`. S059 is registered as the next bounded corrective for broker execution classification and verifier setup-reason projection.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Two required final test submissions were not accepted as jobs; the safe class was `execution_backend_unavailable`, which may conflate generic execution failure with an actual backend outage. | Agent could not complete required workspace tests or Clippy; their job IDs and terminal states were unavailable. | S059 preserves the broker's `Execution` versus `BackendUnavailable` distinction; a new live run must establish the actual class. |
| medium | Independent verifier setup reason was omitted from safe campaign evidence. | Attempt 79's setup failure cannot be assigned more narrowly from the safe record. | S059 projects only the existing fixed setup-failure category. |
| high | H007 sustained qualification remains unproven. | The long-horizon task remains unqualified. | Continue only within the remaining explicit authorization and unchanged H007 gates. |

## 11. Roadmap disposition

S058 is closed at its failed job-status identity/error-typing scope. H007 remains unqualified. S059 is the required corrective plan for the separate broker failure-class ambiguity and missing safe verifier setup attribution.

## 12. Registry updates

- Mark S058 closed at its correction scope in `plans/registry.md`.
- Register S059 as active.
- Update the S019 roadmap counters and attempt 79 result without changing prior evidence.
