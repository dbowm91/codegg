# S017 — Muse Spark v17 H007 requalification

Status: **authorized; no run started yet**

## Objective

Use `opencode_go/muse-spark-1.3-contributor` on the v17 H007 task to determine whether the frozen-manifest correction resolves task acceptance and whether the additional crash-safe checkpoint work supports a 30-minute substantive horizon. Preserve the independent verifier, exact-ID lifecycle, seeded repairs, continuation, and all H007 acceptance gates.

## Authorization

The user authorized up to 20 sequential live attempts on 2026-11-03. The exact route is `opencode_go/muse-spark-1.3-contributor`; manifest SHA-256 is `402b3c1981a3ba5a53a4331a20825e5f6a7fd8479a5de6cae399aa0e618ba4bc`. Each run is capped at 750 provider requests and 3,600 seconds, with a 10 GiB free-disk floor and 300-second activity-stall cancellation. Cancel on operator request, sensitive exposure, or loss of request-admission observability. Record provider USD cost when available and `unknown` otherwise. No other provider/model is authorized. The append-only ledger is `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-muse-live-authorization-20261103.json`.

## Method

Run one case at a time using the v17 manifest and the existing `opencode_go` adapter. Every started live run consumes one of 20 slots. Record sanitized task/verifier/horizon outcomes, exact job lifecycle, repair count, continuation, request count, event pairing, activity gaps, disk minimum, and cost status. Do not inspect private verifier source or output. If a run fails, attribute only what safe receipts establish; make a narrow offline correction only when evidence supports it, version the prompt, and continue within the same 20-run authorization.

Qualification requires task verifier pass and all unchanged H007 gates, including at least 30 minutes of substantive activity, no gap over five minutes, exact-ID job running/terminal with productive overlap, both repairs, continuation, complete event capture, and no infrastructure failure. Completing the task correctly before 30 minutes is task success but not horizon qualification.

## Stop

Stop on the authorized caps, cancellation triggers, or 20 started runs. Do not claim qualification without a passing independent verifier and all horizon gates. S016's consumed DeepSeek authorization remains separate and is not reopened.
