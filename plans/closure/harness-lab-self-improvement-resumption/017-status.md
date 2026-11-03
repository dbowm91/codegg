# S017 status — Muse Spark v17 H007 requalification

Status: **authorized; 1 of 20 runs started**

The user authorized up to 20 sequential runs on `opencode_go/muse-spark-1.3-contributor`. Per-run limits: 750 requests, 3,600 seconds, 10 GiB free disk, and cancellation at a 300-second activity stall. USD cost is recorded when available and `unknown` otherwise. Initial v17 SHA-256: `402b3c1981a3ba5a53a4331a20825e5f6a7fd8479a5de6cae399aa0e618ba4bc`; v18 SHA-256: `2558b3d03f0adc9b272462c2aae27e07bae4d4e292a8091996fb8646e0485589`. Authorization ledger: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-muse-live-authorization-20261103.json`.

## Attempt 1

Attempt 1 used v17 and ended normally after 26m36s active with 233/750 requests. The task verifier failed one of five tests; horizon outcome was `not_qualified` because active work was 1,596,475 ms (<1,800,000 ms). Exact-ID job lifecycle, productive overlap, continuation, 18 repair cycles, and complete event capture passed. Maximum activity gap was 59,759 ms; minimum free disk was 220,531,945,472 bytes. No provider, cap, stall, event-gap, or disk failure occurred. USD cost was reported as $0.106883.

The lane fixture's README was byte-identical to the stale runtime staging source and differed from the corrected frozen starter. This is the likely cause of the single verifier failure, but the private output remains uninspected. The actual runtime fixture README is synchronized and both packaged-fixture and frozen-hash tests pass. Attempt 2 is prepared on v18; it remains within the same authorization. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-01-summary.json`.

## Attempt 2

Attempt 2 used v18 and passed the independent verifier (5/5). It completed normally after 18m46s active, so horizon outcome was `not_observed` (`task_completed_before_threshold`). Exact-ID job lifecycle, productive overlap, continuation, nine repairs, and complete event capture passed. Maximum activity gap was 57,140 ms; free disk remained at least 221,271,543,808 bytes. It used 231/750 requests, with $0.090256 reported USD and no provider, cap, stall, event-gap, or disk failure. The runtime README matched the corrected frozen input. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-02-summary.json`.

Prompt v19 adds reverse-impact analysis and digest-bound read-only policy evaluation as dependent product work. Attempt 3 will use its versioned manifest under the existing authorization.
