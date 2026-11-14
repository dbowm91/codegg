# Harness Lab Self-Improvement Resumption S059 — Closure Status

Status: **conditionally closed at typed tool-execution and safe verifier-attribution scope; H007 remains unqualified**

Source implementation plan:

- `plans/implementation/harness-lab-self-improvement-resumption/059-typed-tool-execution-and-verifier-setup-attribution.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#54-s059--typed-tool-execution-and-verifier-setup-attribution`

Repository baseline reviewed: `22c1d481ec8cefe8a29b275f3ce232dbb059c185`

Implementation commit:

- `1761f8689bdaa36f29ba3b9dd2545929a1e0c9b8` — preserve ordinary execution-error terminal typing and project only an allowlisted verifier setup category.

## 1. Executive finding

S059's bounded source and privacy requirements are implemented and passed their focused offline checks. Attempt 80 exercised the exact ARM64 pair at the pushed implementation head, using the unchanged v97 H007 task, independent verifier, Muse Spark 1.3 Contributor route, and authorized caps. Attempt 80 did not qualify H007. Its safe evidence proves a task-sequencing failure and controlled prompt timeout, not lost ACP/tool events, a job-ID correlation defect, a disk failure, or an external watchdog stop. S060 owns one controlled repeat on the same task and source behavior; no broader runtime correction is justified by this attempt.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Map ordinary `ToolError::Execution` to terminal `Error` in both broker fallback paths | Commit `1761f8689`; 22 focused Rust broker tests | pass | Explicit backend-unavailable remains infrastructure-typed. |
| Preserve only fixed verifier setup categories in safe campaign output | Focused H007 and campaign-authorization tests; 113 tests passed | pass | Hostile/unrecognized strings are rejected; raw verifier material is not projected. |
| Preserve H007 task, verifier, route, and run controls | Attempt 80 preflight and safe diagnostic | pass | v97 manifest digest unchanged; Muse/OpenCode Go; exact approved limits. |
| Exercise the exact correction in a long live run | Attempt 80 safe diagnostic and monitor | pass | 3,299.791 s qualification duration; 385 provider requests; 3,282.832 s active-work span. |
| Qualify H007 | Attempt 80 unchanged verifier | fail | Agent timed out; one named repair pair was out of order; verifier reported assertion failures. |

## 3. Production implementation evidence

`src/tool/broker.rs` now preserves ordinary `ToolError::Execution` as a terminal `Error` in both fallback branches, while explicit `ToolError::BackendUnavailable` remains `InfrastructureError`. The `ToolValue` helper provides the typed execution-error construction. The safe H007 reconciliation and campaign allowlists now preserve only a recognized fixed verifier setup category. These changes alter no retry, scheduler, test-submission, task, verifier, provider, or resource-limit behavior.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib tool::broker
python3 -m unittest dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_campaign_authorization
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
cargo fmt --check
python3 -m compileall -q dev/harness_lab
git diff --check
```

### Results

The ARM64 broker suite passed 22 tests. The H007 and campaign-authorization suites passed 113 tests. Both test-quality guards, formatting, Python compilation, and diff checks passed. Strict Clippy was attempted and failed on the unrelated existing `clippy::too_many_arguments` finding in `src/agent/worker.rs:1568`; it did not identify a changed S059 line. No full verification wrapper is claimed.

## 5. Invariant review

- Generic execution failures remain distinguishable from actual backend unavailability in both broker fallback branches: pass.
- Timeout, permission, cancellation, disabled-tool, and missing-tool behavior were not altered; focused mappings remain covered.
- Verifier setup evidence is reduced to the fixed allowlist; arbitrary strings, exception material, paths, and raw output are rejected.
- H007 task, verifier, route, and limits are unchanged: pass.

## 6. Failure and recovery review

Attempt 80 admitted 385/1,200 requests and exited naturally with process status 2 after 3,309.172 seconds. Its external stop reason was `runner_exit`; the external wall stop, 300-second inactivity stop, and disk floor did not fire. The agent terminal was `timeout` at the 3,300-second prompt-work boundary. The safe timing record shows 18 prompt requests, 17 autonomous followups, and the final prompt request timing out with the 300-second verifier reserve intact.

The seven task stages, five contract phases, twelve gates, required review, productive release overlap, and both required final test jobs completed. The final workspace-test and Clippy jobs each had a matching completed state. However, only one of the two required named repair pairs qualified. `catalog_format` qualified. For `catalog_store`, `repair_followed_failure` was false: source inspection of the public predicate confirms this means the repair submission preceded the first observed terminal failed status. The productive-change and repair-correlation predicates consequently failed. The safe data does not establish why the agent submitted in that order.

The continuation document was invalid because `summarize_required_repair_jobs(...)[complete]` was false, and `continuation_document_confirmed` makes that a required document predicate. The separate `pointer_block_is_terminal: false` field is informational; the existing layout test explicitly expects it when job and review evidence follows the progress pointers. It is not the failure cause.

All 428 projected native tool starts had terminals, event measurement was complete with zero gaps, all 123 applicable job request/result ID correlations matched, all tool calls were bound, and the exact release job lifecycle and productive overlap were observed. The independent verifier ran once and reported `assertion_failures` in `continuation_boundary` and `transcript_job_lifecycle`. The safe public predicates agree with those failure domains. No daemon, scheduler, ACP, event-pairing, or job-identity failure was reproduced.

## 7. Migration and compatibility review

No schema, migration, persisted format, or wire protocol change was introduced. Terminal error status is more precise for ordinary execution errors; the recognized setup category is an optional additive safe-diagnostic value.

## 8. Security review

Attempt 80 preflight verified the ARM64 binary pair, exact source identity, frozen manifest digest, route presence, and 30-GiB reserve. Runtime minimum free disk was 96,255,148,032 bytes and ending free disk was 96,252,854,272 bytes. The private receipt and runner stdout/stderr were retained with restrictive permissions and left uninspected. No transcript, raw tool arguments/results, verifier source/output, or credential value was retained or inspected. External billed USD was unreported and is treated as unknown.

## 9. Documentation and operations

Attempt 80 evidence is recorded in:

- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt80-preflight-20261114.json` — SHA-256 `419d47b86f4b0f454d49728e7ddbb406252871c985508906feeec0991ec8ab4d`
- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt80-safe-diagnostic-20261114.json` — SHA-256 `7224dc7479f8435499b8e2180c20017f1464246a89aff02a0167575a6c16a689`
- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt80-monitor-summary-20261114.json` — SHA-256 `0c775aeca4f9deba2cb5f99c9dcee28bf36af5f28647863ac0745fd90f33d3ca`

The reconciled authorization ledger records 385 requests and one counted start for attempt 80. Counters after the run are 80/200 combined starts, 36/40 starts in grant `s019-supplemental-20261111-04`, and 51/120 task-authorized starts. Remaining: 120 combined, 4 grant, and 69 task-authorized starts.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | H007 remains unqualified after the independent verifier reported assertions in the continuation and transcript/job-lifecycle domains. | Sustained model execution has not passed the frozen acceptance contract. | S060 runs one unchanged-task controlled requalification under the existing grant. |
| medium | Attempt 80 submitted the named `catalog_store` repair before observing its seed job's terminal failure. | Only one required repair pair qualified; continuation and lifecycle verification failed. | Use the existing exact pair-specific feedback and unchanged prompt in attempt 81; classify from safe evidence, without assuming a daemon defect. |
| low | `pointer_block_is_terminal: false` can be misread as a failed requirement despite being informational for the interleaved continuation layout. | Reviewers may misattribute the continuation failure. | S060 records the correct interpretation; no schema change is warranted absent a separate demonstrated consumer bug. |

## 11. Roadmap disposition

S059 is conditionally closed at its implemented broker-typing and safe-projection boundary. H007 is not closed. Attempt 80 produced no evidence requiring a production harness redesign; S060 is a controlled live requalification with the same task, verifier, route, and limits.

## 12. Registry updates

- Mark S059 implemented and conditionally closed at its exact technical scope in `plans/registry.md` and the subsystem roadmap.
- Register S060 as active for attempt 81 under the existing grant.
- Keep S019 active and H007 explicitly unqualified until the unchanged independent verifier passes all gates.
