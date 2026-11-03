# S017 status — Muse Spark v17 H007 requalification

Status: **authorized; 1 of 20 runs started**

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
