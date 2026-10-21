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

## 4. Python — the admitted caps are enforced

```
$ python3 -m unittest dev.harness_lab.tests.test_budget.SpendGuardTests
Ran 9 tests in 0.004s

OK
```

| Requirement | Test |
|---|---|
| The provider-request cap refuses work **before** it starts | `test_the_provider_request_cap_is_refused_before_the_work_starts` |
| A projected request is reserved until it is observed | `test_an_admitted_request_is_reserved_until_it_is_observed` |
| An observed count replaces the reservation | `test_an_observed_count_replaces_the_reservation` |
| The projected request is counted against the cap | `test_the_provider_request_cap_counts_the_projected_request` |
| The token cap stops further work after it is crossed | `test_the_token_cap_stops_further_work_after_it_is_crossed` |
| An unobserved cost is not reported as zero | `test_an_unobserved_cost_is_not_reported_as_zero` |
| A controller-enforced cost cap needs an observable cost | `test_a_controller_enforced_cost_cap_needs_an_observable_cost` |
| A provider-enforced cost cap needs no observable cost | `test_a_provider_enforced_cost_cap_needs_no_observable_cost` |
| The observed spend is JSON-serializable | `test_the_observed_spend_is_json_serializable` |

## 5. Python — full harness suite

```
$ python3 -m unittest discover -s dev/harness_lab/tests
..............................................................................................
----------------------------------------------------------------------
Ran 126 tests in 5.570s

OK
```

## 6. Workspace guards

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
