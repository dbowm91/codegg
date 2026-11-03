# S017 status — Muse Spark v17 H007 requalification

Status: **authorized; 7 of 20 runs started**

The user authorized up to 20 sequential runs on `opencode_go/muse-spark-1.3-contributor`. Per-run limits: 750 requests, 3,600 seconds, 10 GiB free disk, and cancellation at a 300-second activity stall. USD cost is recorded when available and `unknown` otherwise. Initial v17 SHA-256: `402b3c1981a3ba5a53a4331a20825e5f6a7fd8479a5de6cae399aa0e618ba4bc`; v18 SHA-256: `2558b3d03f0adc9b272462c2aae27e07bae4d4e292a8091996fb8646e0485589`. Authorization ledger: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-muse-live-authorization-20261103.json`.

## Attempt 1

Attempt 1 used v17 and ended normally after 26m36s active with 233/750 requests. The task verifier failed one of five tests; horizon outcome was `not_qualified` because active work was 1,596,475 ms (<1,800,000 ms). Exact-ID job lifecycle, productive overlap, continuation, 18 repair cycles, and complete event capture passed. Maximum activity gap was 59,759 ms; minimum free disk was 220,531,945,472 bytes. No provider, cap, stall, event-gap, or disk failure occurred. USD cost was reported as $0.106883.

The lane fixture's README was byte-identical to the stale runtime staging source and differed from the corrected frozen starter. This is the likely cause of the single verifier failure, but the private output remains uninspected. The actual runtime fixture README is synchronized and both packaged-fixture and frozen-hash tests pass. Attempt 2 is prepared on v18; it remains within the same authorization. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-01-summary.json`.

## Attempt 2

Attempt 2 used v18 and passed the independent verifier (5/5). It completed normally after 18m46s active, so horizon outcome was `not_observed` (`task_completed_before_threshold`). Exact-ID job lifecycle, productive overlap, continuation, nine repairs, and complete event capture passed. Maximum activity gap was 57,140 ms; free disk remained at least 221,271,543,808 bytes. It used 231/750 requests, with $0.090256 reported USD and no provider, cap, stall, event-gap, or disk failure. The runtime README matched the corrected frozen input. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-02-summary.json`.

Prompt v19 adds reverse-impact analysis and digest-bound read-only policy evaluation as dependent product work. Attempt 3 will use its versioned manifest under the existing authorization.

## Attempt 3

Attempt 3 used v19 and completed normally after 28m56s active. The hidden verifier failed one of five tests. Frozen inputs matched. The agent completed 14 phases and a 49-test release workspace suite, but the designated job was deferred until the end; the safe trace showed `running` then `completed` without an intervening productive edit. Horizon reason was `job_lifecycle_missing`; active span was 1,735,972 ms, 64 seconds short of 30 minutes. Event pairing was complete (196/196), maximum gap 226,616 ms, minimum free disk 223,228,329,984 bytes; it used 176/750 requests and reported $0.079510.

Public code review found a likely task regression in the existing `diff` output: v19 adds revision fields to `added` and `removed` rows, unlike the v18/reference form. The private verifier output remains uninspected. Prompt v20 locks the original row shapes, moves the priority job before delta and requires productive edits during the observed running state. It adds incremental bounded impact views. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-03-summary.json`.


## Attempt 4

Attempt 4 used v20 and completed normally after 34m21s of measured active work. It completed 18 phases, 14 repair cycles, and continuation evidence. The final workspace job passed 44 tests. The priority job (`6cda90e1-3d74-49b1-8ac6-27c6b05d594d`) was observed running while two source edits occurred, but it terminated failed (20 passed, 1 failed); later repairs and the final sweep passed.

A blocking `job wait` on repair job `fcf8f8ac-d58c-4f89-a0f8-74f256bfa532` lasted 300,138 ms, matching the measured maximum activity gap of 300,138 ms (138 ms over the limit). The runner did not cancel at that boundary, but complete event capture remained available (154/154, no gaps). The run therefore did not qualify.

The independent verifier exited after 240,465 ms with `tests_run=0` and `(errors=1)`. Its private output remains uninspected; the cause is unknown. Provider usage was 147/750 requests; reported allowance was $0.071682; minimum free disk was 218,441,805,824 bytes.

The run receipt's `manifest_sha256` identifies the fixture's canonical H007 manifest, not the selected v20 campaign manifest. The S017 evidence records the selected SHA separately. The runner now records `requested_manifest_sha256` from the selected suite so future receipts preserve both identities.

Prompt v21 bans blocking `job wait`, requires nonblocking exact-ID status checks for all jobs, moves the priority job until after known test failures are repaired, and records the prior verifier setup error without exposing private output. Manifest SHA-256: `11370f39d0acc75b90363ad340b116ab0f9dd7bf9760b91faae2e76e3cb070f6`. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-04-summary.json`.


## Attempt 5

Attempt 5 was cancelled after the authorized 300-second tool-activity stall. It had 636 completed tool calls, no unpaired starts, and over 43 minutes of observed tool work before its last tool event. Provider request admission reached request 745 of 750 while the ACP `session/prompt` call remained open and no native tool event arrived for more than five minutes. No verifier ran and no final receipt was produced. Minimum sampled free disk remained above 218 GiB. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-05-summary.json`.

The stall watcher reset on every `session/update`, including streamed assistant message chunks, while the qualification gap metric counts only native tool-call boundaries. Provider traffic could therefore keep the watcher alive during a tool-free gap. The watcher now resets only on `tool_call` and `tool_call_update`; qualification also includes trailing silence from the last tool event to turn termination. Focused production-shaped regressions pass.

Prompt v22 explains that streamed text and provider traffic do not count as productive activity and retains nonblocking exact-ID job polling. Manifest SHA-256: `ccbe0d8585c3bd6a965ad0a0a89c8e1e34c9681758e9856817bca6b760b4b128`.


## Attempt 6

Attempt 6 used v22 and qualified the active-work horizon after 53m49s, with 22 repair cycles, complete event capture, and continuation evidence. The priority workspace job (`52fe9651-feb2-4532-8fda-7e759aa81e74`) was observed running and completed with 41 tests while two edits occurred; the final release workspace sweep passed 45 tests, and targeted release query/scale checks passed.

The independent verifier again exited after 240,408 ms with zero tests and one error. Private verifier output remains uninspected. A separate fresh public debug workspace run passed its earlier targets and then stayed at full CPU in the frozen `catalog-query/tests/impact_scale.rs` test for over seven minutes; that local run was operator-cancelled. This makes debug-path runtime in the verifier setup a likely cause, not a confirmed one. The run used 738/750 requests, reported $0.390768, and retained at least 222,916,632,576 bytes free. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-06-summary.json`.

Prompt v23 asks the agent to profile and reduce debug-path impact-view cost while preserving the frozen scale test and all correctness properties. Manifest SHA-256: `c644badea0601ace04765acf3ed7bfc9e8628dfed40532535f1d93b9150d842e`.


## Attempt 7

Attempt 7 used v23 and ran for 39m11s, with a 35m03s active-work span. It used 107/750 provider requests, reported $0.065766, retained at least 220,197,535,744 bytes free, and captured all 107 native tool starts and terminals with no event gaps. Maximum activity gap was 210,258 ms. The duration and exact-job gates were observed, with productive work during a running job and three repair cycles, but horizon measurement was incomplete and continuation was false. The receipt is `task_outcome=inconclusive`, `solved=false`; this is not a task or qualification pass.

The public `CONTINUATION.md` listed fifteen implementation phases but remained at `next_phase: verify_workspace`. The priority full-workspace job (`aae97a76-2103-4975-9bb7-d02fef2a5d4b`) failed before compilation because the locked Cargo invocation required a lockfile update. Its public stderr stated that `Cargo.lock` needed updating while `--locked` was passed. A later package test without `--locked` passed and refreshed the lockfile, and a locked package-scoped query test passed, but no full locked workspace sweep was recorded after the refresh. The independent verifier did not run.

The run then terminated with `provider_timeout`. Safe CodeGG diagnostics show four OpenCode Go streams with zero decoded events reaching the 120-second stream idle timeout, followed by retry-chain expiry. The 300-second native-activity stall guard did not trigger. This is an upstream stream timeout observed by the harness, while the immediate task-corrective is to refresh and freeze `Cargo.lock` before submitting the locked workspace gate. No provider/model switch or timeout expansion is justified by this evidence. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-07-summary.json`.

Prompt v24 preserves the task and qualification gates, explicitly requires offline lockfile generation before the locked workspace job, and forbids manifest/lockfile edits after that job starts. Manifest SHA-256: `84771c106cb08a9c75ead9b6b7eed7c0b8ba572b5c720e42bf4d434d0927419c`.
