# Cache Telemetry and Effective-Cost Prep Plan

## Purpose

Finish the bridge between the now-safe cache-aware context packer diagnostics and future effective-cost compaction.

The current repo has a stable context ledger, artifact-backed tool-output projection, and a hardened observation-only cache-aware context packer. The next focused pass should make the diagnostic layer materially useful by wiring real provider usage/cached-token telemetry into `ContextCacheStats`, removing the remaining raw handle helper footgun, and introducing a read-only effective-cost analysis model that can report what a future policy *would* do without changing provider requests.

This pass should not perform active context mutation or compaction decisions. It should make the data reliable enough for the next implementation to safely act on it.

## Current state summary

Relevant files:

- `src/context/artifact.rs`
  - `compute_content_hash()` and `stable_hash_hex()` now use SHA-256.
  - Still exposes raw `build_handle(session_id, turn_index, tool_call_id) -> String`, which string-formats a `ctx://` handle without validation.
- `src/context/block.rs`
  - `ContextBlock` uses stable hashes.
  - `source_handle: Option<String>` is present.
- `src/context/tool_hash.rs`
  - Tool-definition hash uses canonicalized sorted data and stable SHA-256.
- `src/context/block_builder.rs`
  - Tool-definition block now renders deterministic summary text and token estimates are no longer empty.
- `src/context/cache_stats.rs`
  - Tracks input/cached/output tokens and cache hit rate, but needs verified real provider usage wiring.
- `src/agent/loop.rs`
  - `ContextPackObservationPhase` and observation helper exist.
  - Observation helper logs model, candidate/packed/stable/slow/volatile token estimates, omitted blocks, tool hash, and cache hit rate.
  - Active mode is effectively observe-only with a warning.

## Non-goals

Do not enable active context rewriting.

Do not replace existing compaction behavior.

Do not persist cache stats to SQLite yet.

Do not implement vector memory or semantic retrieval.

Do not pack live transcript messages.

Do not alter provider request contents except for existing behavior already present before this pass.

## Phase 1: Remove or check the raw `build_handle()` helper

`src/context/artifact.rs` still exposes:

```rust
pub fn build_handle(session_id: &str, turn_index: usize, tool_call_id: &str) -> String {
    format!("ctx://tool/{session_id}/{turn_index}/{tool_call_id}")
}
```

This bypasses `ContextHandle::build_tool()` validation. The runtime path appears to use the typed builder, but leaving this helper exported is a regression risk.

Preferred implementation:

- Replace it with:

```rust
pub fn build_handle(
    session_id: &str,
    turn_index: usize,
    tool_call_id: &str,
) -> Result<String, ContextHandleError> {
    ContextHandle::build_tool(session_id, turn_index, tool_call_id)
}
```

or remove it entirely if no non-test callers remain.

Requirements:

- Search all usages of `build_handle(`.
- No production code should generate raw `ctx://` strings through unchecked formatting.
- Update `src/context/mod.rs` re-export if needed.
- Update tests to use `ContextHandle::build_tool()` or the checked helper.

Tests:

- Invalid session id returns an error.
- Invalid tool call id returns an error.
- No test relies on unchecked raw formatting.

Acceptance:

- No unchecked handle builder remains in exported context APIs.

## Phase 2: Locate and normalize provider usage telemetry

Find the authoritative provider usage path in `AgentLoop` or provider event processing. It likely already feeds session usage accounting and pricing, including cached tokens when available.

Inventory the current usage model:

- Where input tokens are read.
- Where output tokens are read.
- Where cached input tokens are read, if present.
- Whether cached-token fields differ by provider.
- Where costs are computed.
- Where usage is persisted to `UsageStore`.

Add a small normalization helper if useful:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NormalizedProviderUsage {
    pub input_tokens: usize,
    pub cached_input_tokens: Option<usize>,
    pub output_tokens: usize,
}
```

The helper should safely handle:

- Missing cached-token data.
- Providers that report cached tokens under different names.
- Saturating conversion from provider integer types to `usize`.
- Impossible values, such as cached tokens greater than input tokens, by clamping or logging.

Tests:

- Missing cached-token fields normalize to `None`.
- Cached tokens greater than input tokens are clamped and warned or rejected deterministically.
- Provider samples for at least OpenAI-like and Anthropic-like usage shapes normalize correctly if those shapes exist in code.

Acceptance:

- There is a single normalized usage path suitable for cache stats.

## Phase 3: Wire real usage into `ContextCacheStats`

Once provider usage normalization is identified, record it into `self.context_cache_stats` for each provider call.

Target call:

```rust
self.context_cache_stats.record_usage(
    &request.model,
    usage.input_tokens,
    usage.cached_input_tokens,
    usage.output_tokens,
);
```

Placement requirements:

- It must run after each provider response where usage is available.
- It must not double-count if multiple events report incremental usage for the same provider call.
- If usage arrives only at final response event, record only once at finalization.
- If usage is absent, do not mutate stats or record an explicit zero call unless current usage accounting already treats that as a real call.

Diagnostics:

- Add debug log when cache stats are updated:

```rust
tracing::debug!(
    model = %request.model,
    input_tokens = usage.input_tokens,
    cached_input_tokens = ?usage.cached_input_tokens,
    output_tokens = usage.output_tokens,
    cache_hit_rate = self.context_cache_stats.cache_hit_rate(&request.model),
    "updated context cache stats"
);
```

Tests:

- Synthetic provider usage updates `ContextCacheStats` exactly once.
- Missing usage does not increment `call_count`.
- Missing cached tokens produce cache hit rate `0.0` but still track input/output if usage is otherwise present.
- Multiple models stay independent.
- Existing usage persistence/cost tests continue to pass.

Acceptance:

- Observation logs show nonzero cache stats after provider calls that report cached tokens.

## Phase 4: Make observation phases actually reflect cache stats timing

Current observations run at useful phases, but cache stats will only be meaningful after provider usage is recorded.

Confirm or add observation calls after usage recording:

- `InitialRequest`: before first provider call.
- `BeforeProviderCall`: immediately before each provider request.
- `AfterToolResults`: after projected tool messages are appended.
- `AfterCompaction`: immediately after compaction modifies messages.
- `BeforeFinalization`: after final usage stats are available.

Requirements:

- Observation helper remains pure/read-only.
- Observations after usage recording should include the updated cache hit rate.
- Do not log excessive per-token detail by default; keep omitted-block detail at debug level.

Tests:

- A test or narrow harness proves an observation after synthetic usage sees updated cache hit rate.
- Observation still does not mutate request messages or tools.

Acceptance:

- Cache hit rate in diagnostics reflects the latest recorded usage by `BeforeFinalization` at minimum.

## Phase 5: Add effective-cost analysis types in observation-only mode

Introduce a read-only analysis model that computes effective uncached token burden and a future recommendation, without changing context.

Suggested module:

```text
src/context/effective_cost.rs
```

Suggested types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveCostAction {
    PreserveStablePrefix,
    CompactVolatileTailFirst,
    ReviewToolPalette,
    NoAction,
}

#[derive(Debug, Clone)]
pub struct EffectiveCostAnalysis {
    pub input_tokens: usize,
    pub cached_input_tokens: usize,
    pub uncached_input_tokens: usize,
    pub cache_hit_rate: f64,
    pub stable_prefix_tokens: usize,
    pub slow_changing_tokens: usize,
    pub volatile_tokens: usize,
    pub recommended_action: EffectiveCostAction,
    pub reason: String,
}
```

Initial heuristic, observation-only:

- If cache hit rate is high and stable prefix tokens are large, recommend `PreserveStablePrefix`.
- If volatile tokens are high and cache hit rate is low/unknown, recommend `CompactVolatileTailFirst`.
- If tool-definition/slow-changing tokens dominate and many omitted blocks are tool-related, recommend `ReviewToolPalette`.
- Otherwise `NoAction`.

Do not use provider pricing yet unless pricing data is already easy to inject. This pass should focus on token economics, not dollar economics.

Tests:

- High cached-token ratio + large stable prefix recommends preserve stable prefix.
- Large volatile tokens + low cache hit recommends compact volatile tail first.
- Tool-heavy slow-changing context recommends review tool palette.
- Empty/missing stats recommends no action or compact volatile only if volatile is above threshold.

Acceptance:

- Observation diagnostics can include an effective-cost recommendation string without mutating context.

## Phase 6: Add effective-cost data to diagnostics

Extend `observe_context_pack()` to compute and log the new analysis.

Example log fields:

```rust
recommended_action = ?analysis.recommended_action,
uncached_input_tokens = analysis.uncached_input_tokens,
effective_cache_hit_rate = analysis.cache_hit_rate,
effective_reason = %analysis.reason,
```

Keep high-cardinality details at debug level.

Requirements:

- If no usage stats exist yet, analysis should still run with zeros/unknowns and say so.
- Do not panic when packed result has no blocks.
- Keep the existing token/tier/omission diagnostics.

Tests:

- Observation diagnostics can compute analysis before usage exists.
- Observation diagnostics can compute analysis after usage exists.

Acceptance:

- The log stream can answer: “Is preserving stable prefix likely helping us?” and “What would we compact first later?”

## Phase 7: Document telemetry and effective-cost boundaries

Update docs:

- `architecture/cache-aware-context.md`
- `.opencode/skills/context/SKILL.md`
- `AGENTS.md` if it lists context modules

Document:

- Cache stats are in-memory and per process/session for now.
- Cached-token telemetry only appears when providers report it.
- Effective-cost analysis is diagnostic only.
- No active compaction or request mutation occurs.
- Stable-prefix preservation decisions are future work.
- Raw handle generation has been removed or checked.

Add a small sample diagnostic log and explain the fields.

## Phase 8: Tests and validation

Run:

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features context
cargo test --workspace --all-features
```

If full workspace tests are too expensive or unrelated failures exist, document exact commands and failures.

Minimum targeted tests:

- No unchecked `build_handle()` remains.
- Usage normalization handles present/missing/clamped cached tokens.
- `ContextCacheStats` updates from synthetic provider usage exactly once.
- Observation after usage sees updated cache hit rate.
- Effective-cost analysis recommendations for high-cache, high-volatile, and tool-heavy cases.
- Observation helper remains non-mutating.
- Existing context ledger and packer tests still pass.

## Acceptance criteria

This pass is complete when:

1. Raw unchecked context handle generation is removed or converted to a checked API.
2. Provider usage data has a normalized path for input/cached/output tokens.
3. Real provider usage updates `ContextCacheStats` without double-counting.
4. Observation diagnostics include meaningful cache hit rates after usage is available.
5. Effective-cost analysis exists and is diagnostics-only.
6. Observation logs include recommended future action without mutating context.
7. Docs clearly describe telemetry limitations and no-active-compaction guarantees.
8. Formatting, clippy, and targeted tests pass.

## Suggested stopping point

Stop after diagnostics can reliably answer these questions:

- How many packed tokens are stable, slow-changing, and volatile?
- How much of recent provider input was cached?
- Is stable-prefix preservation paying off?
- Would the future policy compact volatile tail first, preserve stable prefix, or review the tool palette?

The next pass can then implement the first active effective-cost policy behind a strict config gate, likely limited to volatile-tail compaction and phase-scoped tool palette reduction while preserving stable prefixes.
