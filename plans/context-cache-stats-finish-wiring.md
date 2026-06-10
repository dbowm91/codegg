# Context Cache Stats Finish-Event Wiring Plan

## Purpose

Complete the missing final piece from the cache telemetry / effective-cost prep pass: record real provider finish-event usage into `ContextCacheStats` exactly once per provider response.

The repo now has the right supporting pieces:

- `EventProcessor` captures `input_tokens`, `output_tokens`, and `cached_tokens` from `ChatEvent::Finish`.
- `NormalizedProviderUsage` exists in `src/context/usage_normalize.rs`.
- `ContextCacheStats` exists and can track cache hit rate per model.
- `EffectiveCostAnalysis` exists and is wired into observation diagnostics.
- `observe_context_pack()` reads cache stats and logs effective-cost recommendations.

The gap is that `AgentLoop` does not appear to call `normalize_from_finish(...)` and `context_cache_stats.record_usage(...)` from the real provider response path. This means effective-cost diagnostics are structurally present but may run with empty/zero cache stats.

This pass should wire the real finish-event usage path and add targeted tests proving it is counted once.

## Current state summary

Relevant files:

- `src/agent/processor.rs`
  - `ChatEvent::Finish { stop_reason, usage, .. }` stores:
    - `input_tokens`
    - `output_tokens`
    - `cached_tokens`
    - `is_complete = true`
  - Exposes getters:
    - `input_tokens()`
    - `output_tokens()`
    - `cached_tokens()`
    - `is_complete()`
- `src/context/usage_normalize.rs`
  - `NormalizedProviderUsage`
  - `normalize_from_finish(input_tokens, output_tokens, cached_tokens)`
  - Clamps cached tokens to input tokens.
- `src/context/cache_stats.rs`
  - `ContextCacheStats::record_usage(model_key, input_tokens, cached_tokens, output_tokens)`
  - `cache_hit_rate(model_key)`
- `src/context/effective_cost.rs`
  - Observation-only `EffectiveCostAnalysis`.
- `src/agent/loop.rs`
  - Processes provider events into `processor` immediately after `stream_with_retry(&request)`.
  - Observes context pack before provider calls, after tool results, after compaction, and before finalization.
  - Needs a real usage-recording call after each provider response.

## Non-goals

Do not change provider request contents.

Do not enable active context rewriting.

Do not change compaction behavior.

Do not persist cache stats to SQLite.

Do not alter cost accounting or existing `UsageStore` behavior except where a shared usage normalization helper can be used safely.

Do not record synthetic zero-usage events when a provider does not return usage.

## Phase 1: Add a narrow `AgentLoop` helper for processor usage recording

Add a private helper to `impl AgentLoop` near the other context-packer helpers:

```rust
fn record_context_cache_stats_from_processor(
    &mut self,
    model: &str,
    processor: &EventProcessor,
) -> Option<crate::context::NormalizedProviderUsage> {
    if !processor.is_complete() {
        return None;
    }

    let input_tokens = processor.input_tokens();
    let output_tokens = processor.output_tokens();

    // Do not record a fake provider call if usage is completely absent.
    if input_tokens == 0 && output_tokens == 0 && processor.cached_tokens().is_none() {
        return None;
    }

    let usage = crate::context::normalize_from_finish(
        input_tokens,
        output_tokens,
        processor.cached_tokens(),
    );

    self.context_cache_stats.record_usage(
        model,
        usage.input_tokens,
        usage.cached_input_tokens,
        usage.output_tokens,
    );

    tracing::debug!(
        model = %model,
        input_tokens = usage.input_tokens,
        cached_input_tokens = ?usage.cached_input_tokens,
        output_tokens = usage.output_tokens,
        cache_hit_rate = self.context_cache_stats.cache_hit_rate(model),
        "updated context cache stats"
    );

    Some(usage)
}
```

Notes:

- Use `&request.model` or a cloned model string at the call site.
- Return the normalized usage for tests and optional debug use.
- If the compiler dislikes borrowing `self` while `request` is borrowed, clone `request.model` before the call.

Acceptance:

- One helper owns the cache-stat update semantics.
- Missing usage does not increment `ContextCacheStats.call_count`.

## Phase 2: Call the helper exactly once per provider response

In `AgentLoop::run()`, after:

```rust
for event in &events {
    processor.process(event.clone());
}
all_events.extend(events);
```

add:

```rust
let model_key = request.model.clone();
let _normalized_usage = self.record_context_cache_stats_from_processor(&model_key, &processor);
```

This is the right point because:

- The stream response has completed.
- `EventProcessor` has consumed any `ChatEvent::Finish` usage.
- The code has not yet reset `processor`.
- It runs once for this provider call iteration.

Important:

- Do not also record again at `BeforeFinalization`.
- Do not record again in `publish_agent_finished()` unless that is the only usage path. Prefer the immediately-after-processing placement.
- If retry logic can produce multiple successful provider calls in one `run()`, each successful call should be recorded once. That is correct: each provider response is a real usage event.

Acceptance:

- Each successful provider response with usage increments `call_count` by one.
- Tool-loop iterations with multiple provider responses record multiple real calls.
- Processor reset paths do not cause double counting.

## Phase 3: Make final observation reflect updated cache stats

The existing observation calls should become meaningful once stats are recorded.

Confirm ordering:

1. `BeforeProviderCall` runs before `stream_with_retry` and may show the previous cache hit rate.
2. Provider events are processed.
3. `record_context_cache_stats_from_processor()` updates stats.
4. Any following observation, especially `BeforeFinalization`, shows updated cache stats.

If a tool-call loop continues after provider response, later `BeforeProviderCall` observations should show the updated hit rate from previous calls.

Do not move the existing observation helper unless necessary.

Acceptance:

- Diagnostics after the first provider call can show nonzero `cache_stats.calls` when provider usage exists.
- `BeforeFinalization` uses up-to-date stats.

## Phase 4: Add targeted tests for exact-once usage recording

Add unit tests where easiest. Options:

1. Test a small helper directly if `AgentLoop` construction is feasible in existing harnesses.
2. Add an isolated test-only helper around `NormalizedProviderUsage` + `ContextCacheStats` update if full `AgentLoop` construction is too heavy.
3. Extend existing `tests/agent_loop_harness.rs` or `tests/provider_mock.rs` if they already run a mocked provider turn.

Minimum test expectations:

### Missing usage is ignored

Given a processor that is not complete, or complete with zero input/output and `cached_tokens=None`, the helper returns `None` and does not increment stats.

### Usage with no cached tokens is recorded

Given `input=1000`, `output=200`, `cached=None`, stats for the model show:

- `call_count = 1`
- `total_input_tokens = 1000`
- `total_cached_tokens = 0`
- `total_output_tokens = 200`
- hit rate `0.0`

### Usage with cached tokens is recorded

Given `input=1000`, `output=200`, `cached=600`, stats show:

- `call_count = 1`
- `total_cached_tokens = 600`
- hit rate `0.6`

### Cached tokens are clamped

Given `input=100`, `output=20`, `cached=500`, stats show:

- `total_cached_tokens = 100`
- hit rate `1.0`

### Repeated provider responses count once each

Two calls to the helper for the same model with real usage should produce `call_count = 2` and summed totals.

### No double count for one response

If a test simulates the real event loop, ensure one finish event in one `events` batch increments call count once.

## Phase 5: Add diagnostics test or smoke test for effective-cost update

Add one test proving effective-cost analysis sees updated stats after recording.

Suggested sequence:

1. Start with empty `ContextCacheStats`.
2. Record usage with high cached ratio.
3. Analyze with stable prefix tokens high enough.
4. Assert `recommended_action == PreserveStablePrefix`.

This can live in `effective_cost.rs` or wherever the new helper test lives.

Acceptance:

- Effective-cost diagnostics are proven to use real cache stats, not only static zero-state behavior.

## Phase 6: Verify no stale docs claim persistence or active policy

Docs should continue to state:

- Cache stats are in-memory only.
- Effective-cost analysis is observation-only.
- Active context rewriting is disabled.
- Future policy work will use these diagnostics.

Update docs only if the previous pass left wording that implies cache stats were already fully wired.

Files to check:

- `architecture/cache-aware-context.md`
- `.opencode/skills/context/SKILL.md`
- `AGENTS.md`

## Phase 7: Validation

Run:

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features context
cargo test --workspace --all-features
```

If full workspace tests are too expensive or unrelated failures remain, document exact commands and failures in the implementation notes.

Minimum targeted commands if full workspace is impractical:

```bash
cargo test --workspace --all-features usage_normalize
cargo test --workspace --all-features cache_stats
cargo test --workspace --all-features effective_cost
cargo test --workspace --all-features context
```

## Acceptance criteria

This pass is complete when:

1. `AgentLoop` records real provider finish usage into `ContextCacheStats`.
2. The recording happens exactly once per successful provider response with usage.
3. Missing usage does not create fake zero-call stats.
4. Cached tokens are normalized/clamped before recording.
5. Observation diagnostics after usage can show updated cache hit rates.
6. Effective-cost analysis can operate on real recorded cache stats.
7. Tests cover missing usage, no-cache usage, cached usage, clamped usage, repeated calls, and no double count.
8. Docs remain accurate: diagnostics-only, in-memory stats, no active context mutation.

## Suggested stopping point

Stop after provider finish usage reliably populates `ContextCacheStats` and effective-cost diagnostics reflect that data.

The next pass can then introduce a strictly gated active policy, likely limited to:

- volatile-tail compaction recommendations becoming warnings/actions,
- phase-scoped tool palette reductions,
- and stable-prefix preservation rules that avoid breaking provider prompt-cache reuse.
