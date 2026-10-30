# Harness Lab Self-Improvement S007 — Closure

Status: **closed — request-level external proposer admission qualified against local fixtures; live route remains blocked pending fresh authorization**.

## Implementation

`dev/harness_lab/proposer_gateway.py` provides a bounded loopback-only HTTP gate for the OpenCode CLI provider endpoints. It validates explicit upstream host/scheme before any live-capable forwarding, disables ambient proxy forwarding, bounds request/response size and request time, refuses redirects, and counts each admitted request under a lock before forwarding. Request N+1 receives HTTP 429 before upstream/fixture dispatch. Receipts contain endpoint class, method/path class, body digest, counts, status and timing; they omit body, prompt and headers.

`run_opencode` now requires an explicitly pinned route and hard request ceiling, supplies a temporary provider-only OpenCode config, disables plugins, denies unlisted tools, and routes provider traffic to the gate. A refusal or timeout stops the CLI and retains the complete sanitized receipt. `run_optimizer` persists a refusal receipt in its hash-linked checkpoint without retrying the iteration. Invocation count is no longer used as a substitute for provider HTTP request admission.

## Qualification

- The installed OpenCode CLI was 1.19.3. A real CLI invocation against the local Responses API fixture issued more than one HTTP request. With ceiling 1, exactly one request was admitted and fixture-forwarded; each later request was refused before fixture forwarding. The refusal stopped the child and the receipt was available to the caller.
- Concurrent request admission, unsupported paths, response-size bounds, explicit HTTPS/host validation, synthetic credentials, sanitized receipt fields and provider-only tool permissions are covered by `dev.harness_lab.tests.test_proposer_gateway`.
- `dev.harness_lab.tests.test_improve.ProposerRouteAndBudgetTests` verifies pinned-route requirements, zero-budget refusal before CLI launch and cumulative SpendGuard enforcement.
- `OptimizerCampaignSequenceTests.test_proposer_n_plus_one_receipt_is_persisted_without_iteration_retry` injects N+1 at the optimizer boundary and verifies the receipt is persisted, no iteration result is published, and no retry is made.
- No live provider request occurred. USD cost is `unknown`; no amount was spent.

The focused gateway, route/budget and campaign-checkpoint tests passed. The repeated S008 qualification ran the affected test set twice: 162 tests passed per run, with one optional test skipped each time. Hosted exact-head CI remains pending for the pushed PR.

## Limits and handoff

Offline fixtures prove the CLI-to-loopback route and request cap, not live OpenCode Go credential resolution, endpoint compatibility, provider-side accounting or a paid model response. The live route stays blocked until the operator separately authorizes a pinned endpoint and bounded run. No H007 result was reused as a current defect.
