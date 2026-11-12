# S017 status — Muse Spark v17 H007 requalification

Status: **unqualified; authorization exhausted (20/20 runs)**

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


## Attempt 8

Attempt 8 used v24 and ended normally after 18m23s elapsed / 18m13s active. The lockfile correction worked: priority job `7d4b0d0e-6ed3-40be-ac0a-5da771a2fad6` completed with 19 tests after two source edits while running, and final locked workspace job `6de28a51-26d5-4f08-9546-4bbff8f6e597` completed with 42 tests. The continuation recorded all fifteen phases and `next_phase: none`; 15 repair cycles were recorded. Event capture was complete (371/371), with no event gaps and a maximum gap of 89,415 ms. It used 364/750 requests, reported $0.176098, and retained at least 220,616,421,376 bytes free.

The independent verifier ran five tests and returned one failure. Its output remains sealed and uninspected. Therefore `task_outcome=failed`, `solved=false`; the implementation is not qualified even though the workspace suite and continuation passed. Active work was only 18m13s, so the 30-minute horizon was not observed. This is a task failure, not an early successful completion. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-08-summary.json`.

Prompt v25 makes the next live run a diagnostic/corrective pass: it requires a public acceptance matrix mapping every frozen contract and added capability to implementation and assertion-bearing tests, without accessing private verifier material. Manifest SHA-256: `a563b246b9fd0d2ac8a7596988e622dcc373d46e7c0a3e0cc1632832a58f0573`.


## Attempt 9

Attempt 9 used v25 and ended normally after 25m52s active. It completed the priority workspace job (`a7d89382-5fe7-40a6-9289-9268dec79a56`, 42 tests) with two source edits during its running state and a final locked workspace sweep (`53085b34-f7a4-4476-9f54-dc8e8401212e`, 42 tests). `CONTINUATION.md` listed all fifteen phases and `next_phase: none`; ten repairs were recorded. Event capture was complete (187/187) with no gaps; maximum activity gap was 134,607 ms. The run used 181/750 requests, reported $0.074363, and retained at least 218,588,561,408 bytes free.

The independent verifier ran five tests and reported two failures. Its output remains sealed. The task is therefore failed, not a success; active work was only 25m52s, so the 30-minute horizon also did not qualify. The public audit and source review established two incomplete prompt requirements without identifying the verifier's sealed failure names: `ImpactView` has no incremental upsert/remove behavior and does not exercise the required fixed-seed 250k mixed mutation sequence; reconcile's resume assertion is vacuous and restart recovery after each page/checkpoint boundary is unproven. These are plausible explanations, not confirmed mappings to the verifier failures. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-09-summary.json`.

Prompt v26 asks for those two narrow capability corrections and assertion-bearing tests, preserving all frozen inputs and verifier isolation. Manifest SHA-256: `c5ea59a66c02d65ef4bded1ad41d128745e026cfcfa5fb8cbabdb3f88836cb14`.


## Attempt 10

Attempt 10 used v26 and stopped under the authorized 300-second native-tool stall rule. It reached a public 250k ImpactView scale test that ran CPU-bound after each of 200 mutations, rebuilding the full view twice per mutation. The verifier did not run. Attempt 11 started using v27 (`d45257b05c31f97a07bc10c1e3c4932af5c2b367b5132ae7e64c57028d9150c0`) to keep the large fixed-seed mixed sequence while bounding full-rebuild comparisons to checkpoints and to prove restart recovery with real assertions. No hidden verifier material was read.


## Attempt 11

Attempt 11 ended inconclusively on `provider_timeout` after 43 requests. Tool-event capture was complete, no 300-second activity stall occurred, and disk stayed above the floor. The required exact-ID priority lifecycle was not reached. The verifier did not pass; its private output was accidentally exposed during extraction and is excluded from analysis. Attempt 12 will repeat v27 to test whether the attempt-10 workload correction can progress past this transient failure.

Attempt 12 has started on v27 under the same Muse route and request/time/disk/stall caps.


## Attempt 12 result

Attempt 12 repeated v27 and ended on `provider_timeout` after 26/750 admitted requests. It had 388 seconds of active tool span, 31 native starts and terminals with no gaps, a maximum measured activity gap of 128,253 ms, and no disk/stall trigger. The verifier ran zero tests and returned an error; its output remains sealed. No qualifying lifecycle or sustained horizon was established. Attempt 13 will repeat the same manifest to separate a transient provider interruption from the v27 task correction; no timeout or retry-policy changes are justified by this attempt alone.

Attempt 13 is running with the v27 prompt under the same per-run limits.

Attempt 14 started on the arm64 release binary with the first-event timeout corrective.

Attempt 14 was refused before provider admission because its sandbox helper was absent; a matching arm64 helper is now staged and attempt 15 is starting.

Attempt 16 is running on the arm64 binary with its matching sandbox helper.


## Attempt 16 and v28 corrective

Attempt 16 is the first sustained horizon pass: active work exceeded 30 minutes; exact job overlap, continuation, seven repairs, complete events, and disk/activity limits passed. The independent verifier timed out at 240,312 ms with zero tests run, so the task did not qualify. Public source inspection found `ImpactView` mutation scans all live records and recomputes full row/snapshot digests on every update; the 250k mixed test therefore remains too expensive in debug. v28 targets truly incremental per-package/index/digest maintenance while preserving the 250k workload and its ten checkpoints. Attempt 17 uses manifest SHA-256 `6aea30d94b4b10e11e98f0c7cc234393e4e31aea5f6c7a1239ca8e7ea3b5f410`.

## Attempt 17 result

Attempt 17 used v28 and completed normally after 48m56s total / 48m46s active. The sustained horizon qualified: exact-ID job running/terminal, productive overlap, continuation, and 52 repair cycles passed. Native events were complete (643 starts / 643 terminals, zero gaps or drops); maximum activity gap was 175,188 ms. It used 630/750 requests, reported $0.474781, and retained at least 119,304,101,888 bytes free. No stall or disk trigger occurred.

The independent verifier completed five tests in 70.2 seconds, exited 1, and reported `(failures=2)`; it did not time out. The agent ended normally, so the task is failed and not qualified despite the horizon pass. The exact failure categories remain unknown; no verifier source or output tail was inspected. This is task-correctness evidence, not a harness, provider, scheduler, or ACP stall. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-17-summary.json`.

Attempt 18 is a diagnostic/corrective run under the same authorization and route. v29 keeps the workload and qualification gates unchanged, carries only this safe failure summary, and asks one read-only subagent to independently review the public implementation and tests against the frozen contract before the final workspace sweep. It must not broaden the product scope or inspect the hidden verifier. Manifest SHA-256: `bc98975acc0e5b06036ade2611773da14d00c242db6bb3154c5abaec906da340`.

## Attempt 18 result

Attempt 18 used v29 and ended inconclusively after 11m46s total / 7m41s active. It admitted 29/750 requests, captured 42/42 native starts and terminals with no gaps or dropped events, and retained at least 119,577,243,648 bytes free. Maximum activity gap was 118,842 ms; no 300-second activity stall or disk trigger occurred. Exact-ID job lifecycle and continuation were not established; two repairs were observed.

The run ended on `provider_timeout`. Sanitized CodeGG diagnostics show zero decoded events during the 240,000 ms first-event wait. The verifier ran zero tests and returned `(errors=1)`; no verifier output was inspected. This is an inconclusive provider-stream failure, not evidence of a native harness stall or task-correctness result. Attempt 19 repeats v29 under the same route and caps to check whether the stream failure recurs. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-18-summary.json`.

## Attempt 19 result

Attempt 19 repeated v29 and ended inconclusively after 10m03s total / 5m58s active. It used 12/750 requests, captured 25/25 native starts and terminals with no gaps or dropped events, and retained at least 118,083,358,720 bytes free. Maximum activity gap was 180,990 ms; no activity stall or disk trigger occurred. Exact-ID job lifecycle, continuation, and repairs were not established.

The run ended on `provider_timeout`. Sanitized CodeGG diagnostics again show zero decoded events during the first-event wait (240,001 ms); the verifier ran zero tests and returned `(errors=1)`. This is the second consecutive provider-stream timeout on v29, not a native harness stall. Attempt 20 is the final authorized retry on the same manifest. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-19-summary.json`.

## Attempt 20 and closure

Attempt 20 used v29 and ended normally after 10m07s active. It used 31/750 requests and reported $0.018369. Native event capture was complete (42 starts / 42 terminals, no gaps or drops), maximum activity gap was 190,030 ms, and free disk stayed above the 10 GiB floor (minimum observed 116,355,694,592 bytes). No stall or disk trigger occurred. The exact-ID running-to-terminal job and productive overlap gates were not established; active work was below 30 minutes. The verifier ran zero tests and returned `(errors=1)` after 240,367 ms. Its output remains sealed and uninspected. The task and sustained qualification did not pass.

Attempt 20 exposed a continuation-measurement false positive: the receipt recorded `continuation_confirmed=true` although the public `CONTINUATION.md` said `completed_phases: none`, `next_phase: versioned_records`, and `job_id: pending`. The old check only looked for a productive tool call after any terminal job. Re-evaluating the public document and safe job-status projection with the corrective returns false. The runner now requires a nonempty completed-phase list, `next_phase: none`, a documented job ID whose state is completed and matches a completed native job observation, and productive native work after that observation. The reader is bounded to 64 KiB and rejects symlinks, non-regular files, path escapes, and invalid UTF-8. The preserved attempt 17 complete continuation passes the new projection; attempt 20's incomplete record fails. Focused regression tests pass. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-20-summary.json`; receipt SHA-256 `a5f94dd7049b6093caf94c912191717c2b3fe1e28d3ada4b8a31a22d84e4252f`.

The full 20-run authorization is consumed. S017 remains unqualified: attempt 17 demonstrated a sustained horizon but failed two verifier assertions; attempt 20 did not reach the job or duration gates and its verifier ran zero tests. Further live execution requires fresh explicit authorization.
