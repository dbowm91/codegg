---
name: context
description: Artifact storage, tool-output projection, context_read tool, cache-aware packing observation layer, effective-cost analysis, volatile-tail compaction
version: 1.0.0
process: any
---

# Context Module

The context module manages artifact storage, tool-output projection, the `context_read` tool, and cache-aware context packing for stable provider prompt-cache prefixes.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module root, re-exports |
| `block.rs` | `ContextBlock`, `ContextBlockKind`, `CacheClass`, `Lossiness` |
| `block_builder.rs` | `ContextBlockBuilder` — constructs blocks from runtime state |
| `packer.rs` | `pack()` algorithm — sort by tier/priority, budget enforcement |
| `cache_stats.rs` | `ContextCacheStats` — per-model cache hit rate tracking (in-memory, per-session) |
| `tool_hash.rs` | `tool_definitions_hash` — deterministic toolset identity |
| `usage_normalize.rs` | `NormalizedProviderUsage` — provider-agnostic token normalization |
| `effective_cost.rs` | `EffectiveCostAnalysis` — diagnostic-only cost recommendations |
| `policy.rs` | Gated active context policy (first: `ContextPolicyMode` Observe|Warn|ToolPaletteReduce, `decide_policy`, deterministic `reduce_tool_palette`); strictly disabled by default |
| `volatile_tail.rs` | Gated late-context-only compaction of old tool-result messages with recovery handles; observe/warn/compact rollout |
| `artifact.rs` | Artifact storage |
| `handle.rs` | `ContextHandle::build_tool()` (checked) — only builder; raw `build_handle()` removed |
| `projection.rs` | Tool-output projection/compression |
| `read_tool.rs` | `context_read` tool registration |

## Key Facts

- **`build_handle()` removed**: Only `ContextHandle::build_tool(session_id, turn_index, tool_call_id)` is available. It validates segments for unsafe characters.
- **Cache stats are in-memory**: `ContextCacheStats` is session-local and per-process. No persistence.
- **Cached-token telemetry**: Only appears when providers report it (OpenAI and Anthropic do; others may not).
- **Effective-cost analysis is diagnostic-only**: `EffectiveCostAnalysis` produces recommendations but takes no action. No compaction or request mutation occurs.
- **Stable-prefix preservation**: Analysis can recommend preserving stable prefixes, but this is future work — the packer does not yet act on it.
- **Observation mode only**: Active mutation is disabled. `observe_only` is forced internally.
- **Tool Palette Policy Hardening (2026)**: base_request_tools (full profile-filtered palette captured once per run after model-profile filter) + ContextPolicyRuntimeState (backoff `reduction_disabled_until_turn`, consecutive_reductions, last_* counters/names) in AgentLoop. Reductions are non-cumulative and always derived from the unreduced base (noop or backoff can restore full base palette on subsequent call). `request.tools=None` respected and never re-enabled. Starvation detection after tool_calls parse (main loop + drain_follow_up): if name in base but not last_selected (only base-present tools), set backoff + warn. Starvation detection is implemented via `detect_palette_starvation()` (pure helper in `src/context/policy.rs`, testable without AgentLoop) and `AgentLoop::observe_tool_palette_starvation()`. Starvation never blocks the tool call — it only disables reduction for the next provider call. Backoff triggers (empty selected fallback, starvation) logged with `policy_backoff_active`/`reduction_disabled_until_turn`. Warn mode performs dry-run `reduce_tool_palette` (when base passed to decide_policy) and populates `would_selected_tool_count` / `would_omitted_tool_count` (logs include would_select/would_omit). `review_tool_palette_threshold=false` gates ReviewToolPalette trigger in decide_policy. Diagnostics (info when log_policy_decisions): base_tool_count/selected_tool_count/omitted_tool_count/cap_exceeded_by_required/policy_backoff_active/reduction_disabled_until_turn (+ debug names/overflow). Wired only to per-request tools before provider observes. Defaults remain disabled/observe. Active mutation of the packer itself remains disabled.
- **Volatile-tail compaction (gated, observe-only by default)**: `volatile_tail.rs` compacts old volatile tool-result messages with `ctx://` recovery handles. Configured via `[context_policy]` section: `volatile_tail_compaction` (bool), `volatile_tail_mode` (observe|warn|compact), `min_volatile_tokens_for_compaction` (12000), `preserve_recent_messages` (12), `max_compacted_tail_tokens` (8000), `require_effective_cost_signal` (true), `compact_tool_results_only_first` (true). Tombstone format preserves original token count and recovery handle for `context_read`. Idempotent — already-compacted messages are skipped. Preserves stable prefix, system prompts, user messages, assistant messages with tool calls, and recent messages. Rollout: observe → warn → compact (all disabled by default).

## Usage Normalization

`NormalizedProviderUsage` (in `usage_normalize.rs`) normalizes raw provider token counts into a provider-agnostic form. This allows cache stats and effective-cost analysis to work uniformly across providers that report tokens differently.

## Effective-Cost Analysis

`EffectiveCostAnalysis` (in `effective_cost.rs`) operates on the output of `observe_context_pack` and `ContextCacheStats`. It recommends actions that would improve cache efficiency but does not mutate requests or trigger compaction.

Diagnostic log lines include:
- `recommended_action`: e.g., `preserve_stable_prefix`
- `uncached_input_tokens`: tokens not served from cache
- `effective_cache_hit_rate`: computed from normalized usage
- `effective_reason`: human-readable explanation

## Volatile-Tail Compaction

`volatile_tail.rs` implements a gated, late-context-only compaction policy that reduces token consumption by compacting old volatile tool-result messages with recovery handles.

### Configuration

Configured via `[context_policy]` section:

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `volatile_tail_compaction` | `bool` | `false` | Master toggle |
| `volatile_tail_mode` | `observe\|warn\|compact` | `observe` | Rollout mode |
| `min_volatile_tokens_for_compaction` | `usize` | `12000` | Minimum volatile tokens before considering compaction |
| `preserve_recent_messages` | `usize` | `12` | Recent messages to always preserve |
| `max_compacted_tail_tokens` | `usize` | `8000` | Target token count for compacted tail |
| `require_effective_cost_signal` | `bool` | `true` | Require `EffectiveCostAnalysis` recommendation |
| `compact_tool_results_only_first` | `bool` | `true` | Only compact first eligible messages |

### Behavior

- Only compacts old volatile tool-result messages with `source_handle` containing `ctx://`.
- Skips messages within the `preserve_recent_messages` window.
- Never compacts system prompts, user messages, or assistant messages with tool calls.
- Already-compacted messages are detected by tombstone format and skipped (idempotent).

### Tombstone Format

```
[compacted volatile tool result]
original_estimated_tokens=N
reason=volatile_tail_compaction
recovery_handle=ctx://...
Use context_read with the recovery_handle if full output is needed.
```

### Rollout

1. **observe** (default): No-op diagnostics showing what would be compacted.
2. **warn**: Dry-run with would-compact logs but no mutation.
3. **compact**: Active compaction of eligible messages.

All disabled by default (`volatile_tail_compaction: false`).

## Agent Loop Integration

The packer is invoked via `observe_context_pack` at multiple phases: InitialRequest, AfterToolResults, AfterCompaction, BeforeProviderCall, BeforeFinalization. The helper never mutates the request.

Provider usage is recorded into `ContextCacheStats` exactly once per successful provider response via `record_context_cache_stats_from_processor()`, which normalizes cached tokens (clamping to input) before recording. Missing or zero usage is skipped to avoid synthetic stats.

## Configuration

```json
{
  "context_packer": {
    "enabled": false,
    "observe_only": true,
    "stable_prefix": true,
    "max_stable_prefix_tokens": 32000,
    "max_volatile_tokens": 24000,
    "log_diagnostics": true
  }
}
```
