# Execution Reliability, Approval, and Autonomy M001 — Provider Retry Attempt Safety and Error Taxonomy

Status: implemented

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md` (reliability sibling boundary; approval behavior is out of scope here)

Primary class: invariant / reliability

## 1. Objective

Make provider streaming retries attempt-safe and correctly classify transient versus permanent failures. CodeGG must not silently replay a provider turn after externally visible output without identifying/superseding the abandoned attempt, and it must not retry permanent bad authentication more aggressively than transient transport/server failures.

## 2. Why this milestone is ready

- `ProviderTurnAdapter` is already the canonical agent-facing streaming/retry boundary.
- providers normalize into `ProviderError`/`ChatEvent`.
- FallbackProvider/circuit breaker already owns multi-provider acquisition health.
- event/log infrastructure can carry attempt identity additively.
- no storage or authorization redesign is required.

## 3. Current implementation evidence

At the baseline:

- `src/agent/provider_turn.rs::stream_with_retry()` makes three whole-request attempts with exponential delay.
- `stream_once()` publishes `TextDelta`, `ReasoningDelta`, and `ToolCallStarted` immediately, then returns an error if the stream later fails. The outer function can then replay the same request, leaving previously emitted output/tool-start events visible.
- stream setup is bounded by 120s and event idle by 90s.
- `ProviderError::is_retryable()` considers RateLimit, Timeout, Stream, CircuitOpen, and Auth retryable.
- `From<eggfetch_core::Error>` maps Timeout distinctly but maps other transport failures to `Api { code: "request_error" }`; generic Api is not retryable.
- `FallbackProvider` applies circuit breaker/fallback around `provider.stream(request)` acquisition, not the full consumption lifetime.
- fallback status defaults are 429/500/502/503/504.

## 4. Invariants that must not regress

- Authentication failure is not automatically retried unless an explicit credential refresh operation changed credentials.
- Invalid request/model/policy errors are permanent for the unchanged request.
- 429/temporary server/transport/timeout conditions may be retryable within bounds.
- Once provider output becomes externally visible, retry behavior is explicit and attributable; two attempts cannot masquerade as one authoritative stream.
- Tool execution is not triggered twice through retry bookkeeping.
- Provider credentials/URLs remain redacted from public diagnostics.
- Cancellation stops backoff/retry promptly.
- Session-selected provider/model is not silently changed by retry.

## 5. Scope

### In scope

- `ProviderAttemptId` or equivalent per logical turn attempt;
- event metadata/lifecycle for attempt started/failed/superseded/retrying;
- visible-output tracking;
- retry classification taxonomy;
- Retry-After/provider retry hint handling where available;
- exponential backoff with full jitter/cap;
- full-stream provider health/circuit outcome accounting;
- tests/diagnostics/docs.

### Explicitly out of scope

- global nested retry budget (M002);
- arbitrary provider failover that changes selected connection;
- tool-side retries;
- approval/sandbox work;
- provider API-specific stream resume unless already natively supported.

## 6. Required production changes

### Core/domain

Introduce an internal typed disposition, for example:

```text
Permanent: auth, invalid_request, model_not_found, policy
Transient: rate_limit(retry_after), timeout, connect, dns, tls/io, 5xx, stream_interrupted
Conditional: circuit_open / auth_refreshable only when an explicit recovery changes state
```

Preserve safe public `ProviderError` compatibility where possible; an internal classifier may map current variants plus HTTP/transport metadata rather than exploding the public enum.

Add attempt identity to provider-turn lifecycle. Attempt ID must be stable only for one replay and nested under one logical turn/request.

### Provider/transport normalization

Improve `eggfetch_core` mapping so secret-safe transport kind survives far enough to classify connect/DNS/TLS/IO transient errors. Do not include secret-bearing URL/query content.

Map HTTP status/provider error metadata to disposition explicitly. `Auth` becomes permanent by default. Rate limits preserve bounded Retry-After if the adapter can extract it.

### Streaming lifecycle

Track whether any externally visible event has been emitted for the attempt.

Policy:

- before visible output: transparent bounded retry may occur;
- after visible output: do not simply replay and append as if same stream. Either stop with a typed interrupted-attempt result or emit explicit attempt supersession/retry lifecycle so the frontend/projection replaces/marks abandoned provisional output. The simpler safe implementation is acceptable: no automatic replay after visible output.

Do not emit final authoritative tool-call/result processing for an abandoned incomplete attempt.

### Fallback/circuit health

Ensure circuit/health accounting observes terminal stream success/failure, not only stream-object acquisition. Implementation may wrap the stream with a completion observer or report final outcome from ProviderTurnAdapter through an explicit provider-health seam. Avoid duplicate circuit owners.

### Backoff

Use bounded exponential backoff plus full jitter. Respect server/provider retry hints within configured cap/deadline. Backoff sleep is cancellation-aware.

### Protocol/frontends

Add bounded attempt ID/retry/supersession fields/events where necessary. Existing clients ignoring new fields remain functional.

### Security

Retain current secret-safe error normalization. Attempt diagnostics include provider/model logical identifiers and error classes, never credentials or unredacted request URLs.

### Documentation/static guards

Document retry taxonomy and stream-attempt semantics. Add regression tests rather than broad lint rules unless a source-level guard is clearly useful.

## 7. Ordered work packages

### Work package A — Retry classification

Map provider/http/eggfetch errors to permanent/transient/conditional classes; add Retry-After metadata; correct Auth behavior.

Acceptance evidence: table-driven classification tests for 429, 5xx, timeout, DNS/connect/TLS, bad auth, invalid request, missing model, circuit.

### Work package B — Attempt identity and visible-output policy

Add attempt IDs/lifecycle and ensure mid-stream failure cannot silently append a second generation.

Acceptance evidence: scripted stream emits deltas then fails; no mixed authoritative output occurs and retry/supersession is explicit.

### Work package C — Full-stream provider health

Connect terminal stream failure/success to existing circuit/fallback health semantics without a second health service.

### Work package D — Backoff/cancellation/diagnostics

Add jitter/retry hints and bounded events/logging.

## 8. Failure, cancellation, restart, and contention semantics

- If retry classification itself lacks metadata, default conservatively rather than assume safe retry.
- Cancellation during stream/backoff returns interrupted/cancelled state and no further attempt.
- Concurrent turns have independent attempt IDs/counters.
- Daemon restart does not replay an unfinished provider turn automatically unless existing turn-recovery contract explicitly requests it; this milestone does not invent durable provider-call replay.
- Provider health updates are idempotent by attempt ID where necessary.

## 9. Compatibility and migration

No DB migration should be necessary unless attempt lifecycle is durably projected by an existing event table. Protocol additions are additive. Existing ProviderError display strings remain compatible where practical, while retry policy moves to explicit classification.

## 10. Required tests

### Focused unit tests

- retry taxonomy table;
- Retry-After cap/parsing;
- jitter bounds;
- visible-output flag/attempt ID.

### Integration tests

- failure before first event retries and succeeds;
- failure after text delta does not silently merge replay;
- failure after ToolCallStarted is explicit and tool is not executed from abandoned attempt;
- 429 and 503 retry; auth/400/model missing do not;
- transport connect/DNS/TLS fixture classifies transient.

### Restart and recovery tests

Only if durable attempt events are added; ensure incomplete attempt isn't replayed as completed.

### Contention and cancellation tests

- cancel during backoff;
- two simultaneous turns keep independent attempts;
- circuit half-open remains race-safe.

### Security and negative tests

- secret-bearing URLs/API keys absent from errors/events;
- invalid-request loop cannot retry to exhaustion.

### Migration and compatibility tests

Protocol older-client decoding where required.

## 11. Required verification commands

```bash
cargo test -p codegg-providers
cargo test --test agent_loop_harness -- retry
cargo test --test agent_loop_harness -- stream
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Use current focused target names and record them in closure.

## 12. Documentation updates

- provider/retry architecture documentation;
- `architecture/agent.md` provider-turn lifecycle section;
- protocol docs if attempt events are added.

## 13. Acceptance criteria

- retryable failure before visible output can recover automatically;
- mid-stream visible failure cannot silently merge generations;
- Auth/invalid request/model missing are not blindly retried;
- transient transport/429/5xx/timeout are correctly eligible;
- Retry-After/jitter/cancellation work within bounds;
- circuit/health sees terminal stream outcome;
- no selected-provider/model failover occurs implicitly.

## 14. Stop conditions

Stop if a safe solution would require provider-specific resume tokens as the universal contract, silent connection/model switching, credential refresh semantics not owned by existing auth infrastructure, or durable replay of provider requests.

## 15. Closure evidence required

- error taxonomy matrix;
- pre-stream/mid-stream scripted traces showing attempt IDs;
- retry/backoff/cancellation test results;
- secret-redaction evidence;
- full-stream health/circuit evidence;
- actual verification commands and residual limitations.

## 16. Handoff notes

Prefer the simplest safe policy after visible output. Explicitly terminating the turn with recoverable interruption is better than a clever replay protocol that can merge or duplicate streamed state.
