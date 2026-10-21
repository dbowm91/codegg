# Harness Lab Post-Pilot Corrective 001 — Verification Evidence

Branch: `plans/harness-lab-post-pilot-correctives`
Baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`
Verified: 2026-10-21

## 1. Rust — native failure classification and usage provenance

```
$ cargo test -p codegg --lib acp
running 15 tests
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 7272 filtered out; finished in 0.03s

$ cargo test -p codegg --lib failure_classification
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 7284 filtered out; finished in 0.01s

$ cargo test -p codegg-protocol
test result: ok. 292 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

$ cargo test -p codegg-core
test result: ok. 884 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 33.07s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.91s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.77s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.57s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.46s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.15s
test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.75s
test result: ok. 34 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.72s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.91s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.52s

$ cargo test -p codegg --test agent_loop_harness
test result: ok. 71 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 1.67s
```

### The five terminal distinctions required by §8

| Distinction | Test | Result |
|---|---|---|
| Successful terminal with usage | `acp::tests::acp_harness_extensions_expose_turn_usage_and_argument_fingerprint` | ok |
| Provider failure after ≥1 metered attempt | `failed_turn_after_a_metered_attempt_reports_the_aggregate_usage` | ok |
| Failure before any provider response | `M001MidStreamToolFailProvider` harness test, extended to assert `not_observed` | ok |
| Failure with no provider usage support | `failed_turn_without_provider_usage_support_reports_not_provided` | ok |
| Projection disconnection | `acp::tests::acp_projection_turn_failure_without_usage_reports_lost_at_projection` | ok |

Redaction and category assertions:
`acp_turn_failed_error_carries_the_redacted_category_and_usage`,
`acp_native_turn_failure_without_usage_reports_unspecified`,
`acp_projection_turn_failure_keeps_a_reported_category_and_usage`,
`failure_classification_tests::provider_error_classes_map_to_their_own_categories`,
`failure_classification_tests::non_provider_failures_map_to_their_own_categories`.

## 2. Rust — the `-32010` data payload

Printed from the built code by the implementation, one case per required
distinction:

```json
{"kind":"turn_failed","sessionId":"session-1","turnId":"turn-1",
 "codeggFailureCategory":"provider_transport",
 "codeggUsage":{"inputTokens":120,"outputTokens":35,"cachedTokens":20,"reasoningTokens":5},
 "codeggUsageReason":"reported"}

{"kind":"turn_failed","sessionId":"session-1","turnId":"turn-1",
 "codeggFailureCategory":"provider_transport",
 "codeggUsage":null,"codeggUsageReason":"not_observed"}

{"kind":"turn_failed","sessionId":"session-1","turnId":"turn-1",
 "codeggFailureCategory":"provider_transport",
 "codeggUsage":null,"codeggUsageReason":"not_provided"}

{"kind":"turn_failed","sessionId":"session-1","turnId":"turn-1",
 "codeggFailureCategory":"native_failure_unspecified",
 "codeggUsage":null,"codeggUsageReason":"lost_at_projection"}
```

The success path is byte-identical to the previous v1 shape:
`{"turnId":"turn-1","inputTokens":120,"outputTokens":35,"cachedTokens":20,"reasoningTokens":8,"contextLimit":8192}`.

## 3. Python — budget admission preflight

```
$ python3 -m dev.harness_lab budget-check --budget <admissible>.json
{"detail": {"invalid_caps": [], "missing_sources": [], "reason": ""},
 "enforced_caps": ["cost_limit_usd", "token_limit"],
 "missing_source": null, "status": "admitted"}
exit=0

$ python3 -m dev.harness_lab budget-check --budget <cost-without-enforcement>.json
{"detail": {"invalid_caps": [], "missing_sources": ["cost_limit_usd", "token_limit",
 "max_provider_requests", "max_wallclock_seconds", "provider_quota"],
 "reason": "cost_limit_usd: cost enforcement is not stated; controller_enforced or
 provider_enforced is required"},
 "enforced_caps": [], "missing_source": "cost_limit_usd",
 "status": "budget_unverifiable"}
exit=2

$ python3 -m dev.harness_lab budget-check --budget <empty>.json
{"detail": {"invalid_caps": [], "missing_sources": ["token_limit", "cost_limit_usd",
 "max_provider_requests", "max_wallclock_seconds", "provider_quota"],
 "reason": "no enforceable hard cap is declared (missing token_limit, cost_limit_usd,
 max_provider_requests, max_wallclock_seconds, provider_quota)"},
 "enforced_caps": [], "missing_source": "token_limit",
 "status": "budget_unverifiable"}
exit=2
```

The gate refuses before anything is spawned:

```
$ python3 -m dev.harness_lab tool-trial --binary /nonexistent --profile workspace-write --live-provider
{"error": "a live trial requires --budget pointing at a budget policy document with at
 least one enforceable hard cap", "ok": false}
exit=2
```

## 4. Python — full harness suite

```
$ python3 -m unittest discover -s dev/harness_lab/tests
................................................................................
----------------------------------------------------------------------
Ran 112 tests in 3.304s

OK
```

## 5. Workspace guards

`scripts/verify.sh quick` exits 0, including: the agent schema, core/client/
desktop boundaries, sandbox, execution-ownership, TUI authority,
http-route-disposition, audit-coverage, queue-delivery-callsites,
queue-write-transaction-shape, scheduler-bypass, config-merge-coverage,
no-hardcoded-models, the provider wire/catalog/resilience guards, the OpenAI
endpoint guard, the Eggwork target-routing guard, `cargo fmt --check`, and
`cargo check --workspace --all-targets --locked`.

```
==> CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 55.54s
==> Quick verification passed.
```
