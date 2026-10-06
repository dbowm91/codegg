---
name: context
description: Artifact storage, tool-output projection, context_read tool, cache-aware packing observation layer, effective-cost analysis, volatile-tail compaction
version: 1.1.0
process: any
tags:
  - context
  - artifacts
  - projection
  - packer
  - compaction
---

# Context Module

The context module manages artifact storage, tool-output projection, the `context_read` tool, and cache-aware context packing for stable provider prompt-cache prefixes.

Authoritative docs: `architecture/context-ledger.md` (artifacts, projection,
handles, `[context]` config) and `architecture/compaction.md` (compaction
budgets, continuation/rollover/epochs, volatile tail).

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module root, re-exports |
| `block.rs` | `ContextBlock`, `ContextBlockKind`, `CacheClass`, `Lossiness` |
| `block_builder.rs` | `ContextBlockBuilder` — constructs blocks from runtime state |
| `packer.rs` | `pack()` algorithm — sort by tier/priority, budget enforcement |
| `cache_stats.rs` | `ContextCacheStats` — per-model cache hit rate tracking (in-memory, per-turn; owned by `AgentLoopServices`) |
| `tool_hash.rs` | `tool_definitions_hash` — deterministic toolset identity |
| `usage_normalize.rs` | `NormalizedProviderUsage` — provider-agnostic token normalization |
| `effective_cost.rs` | `EffectiveCostAnalysis` — diagnostic-only cost recommendations |
| `policy.rs` | Gated active context policy (first: `ContextPolicyMode` Observe|Warn|ToolPaletteReduce, `decide_policy`, deterministic `reduce_tool_palette`); strictly disabled by default |
| `volatile_tail.rs` | Gated late-context-only compaction of old tool-result messages with recovery handles; observe/warn/compact rollout |
| `artifact.rs` | Artifact storage |
| `handle.rs` | `ContextHandle::build_tool()` (checked) — only builder; raw `build_handle()` removed |
| `projection.rs` | Tool-output projection/compression |
| `read_tool.rs` | `context_read` tool registration |
| `plan.rs` | `ContextPlan`, `ContextPlanDiagnostics`, `ContextPlanMode`, `PlannedMessage` |
| `compaction.rs` | Single-owner compaction budgets/triggers/strategy (see `architecture/context-compaction-ownership.md`; `agent::compaction` is a compat re-export, `eggcontext` is the tokenizer primitive) |
| `continuation.rs` | Authoritative continuation snapshot assembler |
| `evidence.rs` | Bounded exact context recovery references |
| `epoch.rs` | Fresh provider context-epoch reconstruction for long horizons |
| `rollover.rs` | Transactional context rollover sequencing |

## Key Facts

- **`build_handle()` removed**: Only `ContextHandle::build_tool(session_id, turn_index, tool_call_id)` is available. It validates segments for unsafe characters.
- **Cache stats are in-memory and per-turn**: `ContextCacheStats` lives on `AgentLoopServices`, and `AgentLoop` is constructed per turn (`build_agent_loop` in `DefaultTurnRuntime::run_turn`). `min_cache_observations` therefore counts provider calls **within one turn**, never across turns. No persistence.
- **Cached-token telemetry**: Only appears when providers report it. `EventProcessor::cached_tokens()` is `Option`, and `normalize_from_finish` clamps `cached_tokens > input_tokens` down to `input_tokens` with a warn.
- **Effective-cost analysis is diagnostic-only**: `EffectiveCostAnalysis` produces recommendations but takes no action. No compaction or request mutation occurs.
- **Stable-prefix preservation is recommendation-only**: `EffectiveCostAction::PreserveStablePrefix` is computed and logged, but nothing acts on it — the packer does not yet honor it.
- **The packer is observation-only**: `observe_context_pack` only logs; `observe_only` is forced internally.
- **Tool Palette Policy Hardening (2026)**: base_request_tools (full profile-filtered palette captured once per run after model-profile filter) + ContextPolicyRuntimeState (backoff `reduction_disabled_until_turn`, consecutive_reductions, last_* counters/names) in AgentLoop. Reductions are non-cumulative and always derived from the unreduced base (noop or backoff can restore full base palette on subsequent call). `request.tools=None` respected and never re-enabled. Starvation detection after tool_calls parse (main loop + drain_follow_up): if name in base but not last_selected (only base-present tools), set backoff + warn. Starvation detection is implemented via `detect_palette_starvation()` (pure helper in `src/context/policy.rs`, testable without AgentLoop) and `AgentLoop::observe_tool_palette_starvation()`. Starvation never blocks the tool call — it only disables reduction for the next provider call. Backoff triggers (empty selected fallback, starvation) logged with `policy_backoff_active`/`reduction_disabled_until_turn`. Warn mode performs dry-run `reduce_tool_palette` (when base passed to decide_policy) and populates `would_selected_tool_count` / `would_omitted_tool_count` (logs include would_select/would_omit). `review_tool_palette_threshold=false` gates ReviewToolPalette trigger in decide_policy. Diagnostics (info when log_policy_decisions): base_tool_count/selected_tool_count/omitted_tool_count/cap_exceeded_by_required/policy_backoff_active/reduction_disabled_until_turn (+ debug names/overflow). Wired only to per-request tools before provider observes. Defaults remain disabled/observe. Active mutation of the packer itself remains disabled.
- **Volatile-tail compaction (gated, observe-only by default)**: `volatile_tail.rs` compacts old volatile tool-result messages with `ctx://` recovery handles. Configured via `[context_policy]` section: `volatile_tail_compaction` (bool), `volatile_tail_mode` (observe|warn|compact), `min_volatile_tokens_for_compaction` (12000), `preserve_recent_messages` (12), `max_compacted_tail_tokens` (8000), `require_effective_cost_signal` (true), `compact_tool_results_only_first` (true). Tombstone format preserves original token count and recovery handle for `context_read`. Idempotent — already-compacted messages are skipped. Preserves the stable/system prefix, user-authored messages, assistant messages with tool calls, and recent messages; machine-generated control instructions are eligible only when `compact_tool_results_only_first` is `false`. Rollout: observe → warn → compact (all disabled by default).

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

- Only compacts old volatile tool-result messages whose content carries a
  recovery handle (`has_recovery_handle` checks the message content for
  `ctx://`).
- Skips messages within the `preserve_recent_messages` window.
- Never compacts system messages (the stable prefix is cut before analysis),
  user-authored messages, or assistant messages carrying tool calls.
- Machine-generated control instructions (single-part user messages starting
  `[[`, containing `SYSTEM:`, or starting `<control`) **are** eligible when
  `compact_tool_results_only` is `false`; under the default `true` they are
  skipped.
- Assistant narration is never compacted in the first pass.
- Already-compacted messages are detected by tombstone format and skipped (idempotent).

### Tombstone Format

```
[compacted volatile tool result]
original_estimated_tokens=N
reason=older volatile tail compacted by context policy
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

Provider usage is recorded into `ContextCacheStats` exactly once per successful provider response via `record_context_cache_stats_from_processor()` (in `src/agent/context_runtime.rs`, called from the AgentLoop), which normalizes cached tokens (clamping to input) before recording. Missing or zero usage is skipped to avoid synthetic stats.

## Configuration

`context_packer` gates the observation layer. Defaults shown are the
`unwrap_or` fallbacks in `src/agent/context_runtime.rs:64-113`; all are inert
unless `enabled` is `true`.

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

`stable_prefix` is declared in `ContextPackerConfig` (`schema.rs:645`) but is
**not read anywhere** — the stable prefix is always computed from block cache
class, not gated on this flag.

## Tool-Palette Reduction Configuration

The `[context_policy]` section (`ContextPolicyConfig` in `crates/codegg-config/src/schema.rs`) carries BOTH the gated tool-palette reduction policy and volatile-tail compaction. Tool-palette fields:

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `enabled` | `bool` | `false` | Master toggle for the whole policy layer (false = decisions are Noop regardless of mode) |
| `mode` | `observe\|warn\|tool_palette_reduce` | `observe` | observe logs only; warn dry-runs reductions; `tool_palette_reduce` applies reductions to `request.tools` only |
| `min_cache_observations` | `usize` | `3` | Cache observations required before active reduction decisions |
| `review_tool_palette_threshold` | `bool` | `true` | Consider ReviewToolPalette recommendations from EffectiveCostAnalysis as a trigger |
| `max_tool_definitions` | `usize` | `24` | Hard cap on tool definitions when reduction is active (overflow logged, not truncated) |
| `always_include_tools` | `Vec<String>` | `[context_read, tool_search, todowrite]` | Tools always kept if present in the filtered palette |
| `never_reduce_tools` | `Vec<String>` | `[]` | Tools never removed by the reducer |
| `log_policy_decisions` | `bool` | `true` | Structured policy decision logs (info decisions, debug names) |

Reductions are always derived from `base_request_tools` (the full profile-filtered palette captured once per run), never cumulatively from an already-reduced palette.

## Testing

```bash
cargo test -p codegg context::                     # src/context/ unit tests
cargo test -p codegg --test context_continuity_m004
cargo test -p codegg --test context_projection_adversarial
```

## Source verification

Verified 2026-10-06 against `src/context/` (20 modules) and
`crates/codegg-config/src/schema.rs`. Corrected the module table, which was
missing `continuation.rs`, `evidence.rs`, `epoch.rs`, and `rollover.rs`.
Claims without a traceable source were removed rather than guessed.

Second pass (2026-10-06) against all 20 `src/context/*.rs` files,
`src/agent/context_runtime.rs`, `crates/codegg-config/src/schema.rs`, and
`src/agent/loop.rs`/`turn_runtime.rs`:
- Module count `21` → `20` (`ls src/context/*.rs`).
- Tombstone format was invented: it claimed `reason=volatile_tail_compaction`.
  The real literals are `reason=older volatile tail compacted by context
  policy` and a `(no recovery handle)` variant (`volatile_tail.rs:251-265`).
- Behavior section claimed compaction only touches tool results "with
  `source_handle` containing `ctx://`". `source_handle` is a `ContextBlock`
  field, not a message field; the actual gate is `has_recovery_handle`
  matching `ctx://` in message content (`volatile_tail.rs:72-78`), and
  machine-generated control instructions are eligible when
  `compact_tool_results_only=false` (`volatile_tail.rs:165-176`).
- "Cache stats are session-local" was wrong: `ContextCacheStats` is owned by
  `AgentLoopServices` (`coordinator.rs:77`) and `AgentLoop` is built per turn
  (`turn_runtime.rs:725`), so observations never span turns.
- "OpenAI and Anthropic do" report cached tokens was unsourced; replaced with
  the verifiable `Option` + clamp contract (`usage_normalize.rs:26-42`).
- "Observation mode only" was ambiguous next to the tool-palette mutation
  path; scoped it to `observe_context_pack`.
- Added the `context_packer.stable_prefix` dead-flag fact: declared at
  `schema.rs:645`, zero readers repo-wide.
- Verified accurate: all 20 module paths, the `ContextHandle` builder set,
  `build_tool`/`build_evidence` signatures, all 8 `[context_policy]`
  tool-palette defaults and all 7 volatile-tail defaults
  (`schema.rs:715-768`), `always_include_tools` default
  (`[context_read, tool_search, todowrite]`), the 5 `ContextPackObservationPhase`
  variants, all 8 `project_tool_output` call sites (`loop.rs:1819`,
  `follow_up.rs:341`), `read_tool` defaults (`offset` 0, `max_bytes` 20000,
  `read_tool.rs:45-75`), the `bounded_artifact_handles` 32 cap
  (`context_frame.rs:7`), and the diagnostic log field names at
  `context_runtime.rs:162`.
