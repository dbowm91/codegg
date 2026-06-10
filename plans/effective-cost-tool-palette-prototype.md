# Effective-Cost Tool Palette Prototype Plan

## Purpose

Introduce the first strictly gated active policy that uses cache-aware diagnostics without touching transcript content or rewriting context.

The repo now has the diagnostic prerequisites:

- Artifact-backed tool output projection.
- Stable `ContextBlock` and deterministic packer diagnostics.
- Active context rewriting disabled.
- Provider finish-event usage wired into `ContextCacheStats`.
- `EffectiveCostAnalysis` producing observation-only recommendations.

The next safest active step is not compaction or message rewriting. It is phase-scoped tool palette reduction: reduce the tools sent to the provider when diagnostics indicate tool-definition/slow-changing context is too large, while preserving stable prefixes and avoiding any mutation of conversation history.

This pass should implement a conservative prototype behind explicit config gates, with default behavior unchanged.

## Current state summary

Relevant files:

- `src/agent/loop.rs`
  - Builds tool definitions with `self.build_tool_definitions().await` and filters them through model profile policy.
  - Runs `observe_context_pack()` at key phases.
  - Records provider finish usage into `ContextCacheStats`.
  - Active packer mutation remains disabled.
- `src/context/effective_cost.rs`
  - Emits `EffectiveCostAction::{PreserveStablePrefix, CompactVolatileTailFirst, ReviewToolPalette, NoAction}`.
  - `ReviewToolPalette` is the action this pass should use.
- `src/tool/catalog.rs`
  - Existing deferred tool discovery/search infrastructure.
- `src/tool/mod.rs`
  - Tool registry and tool definition exposure surfaces.
- `src/agent/policy.rs` and model-profile policy code
  - Existing model/profile tool filters.
- `crates/codegg-config/src/schema.rs`
  - Existing `[context_packer]` config.

## Non-goals

Do not rewrite transcript messages.

Do not compact context actively.

Do not remove tools from the actual `ToolRegistry`.

Do not break explicit user-requested tool access.

Do not enable active behavior by default.

Do not implement provider-pricing dollar calculations.

Do not persist cache stats.

Do not introduce semantic/vector tool selection.

## Phase 1: Add active policy config with safe defaults

Add a new config section or extend `[context_packer]` conservatively.

Preferred section:

```toml
[context_policy]
enabled = false
mode = "observe" # observe | warn | tool_palette_reduce
min_cache_observations = 3
review_tool_palette_threshold = true
max_tool_definitions = 24
always_include_tools = ["context_read", "tool_search", "todowrite"]
never_reduce_tools = []
log_policy_decisions = true
```

If keeping config surface small, fields can live under `[context_packer]`:

```toml
[context_packer]
active_policy = false
active_policy_mode = "observe"
max_tool_definitions = 24
min_cache_observations = 3
```

Recommended enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextPolicyMode {
    #[default]
    Observe,
    Warn,
    ToolPaletteReduce,
}
```

Defaults must preserve current behavior:

- `enabled=false`
- `mode=Observe`
- no tool filtering unless explicitly enabled and `mode=ToolPaletteReduce`

Tests:

- Default config does not alter tool definitions.
- Unknown mode fails config parse or falls back safely according to existing config style.
- `enabled=false` wins over mode.

## Phase 2: Add a policy-decision type

Create a small module, preferably:

```text
src/context/policy.rs
```

Suggested types:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextPolicyDecisionKind {
    Noop,
    WarnOnly,
    ReduceToolPalette,
}

#[derive(Debug, Clone)]
pub struct ContextPolicyDecision {
    pub kind: ContextPolicyDecisionKind,
    pub reason: String,
    pub recommended_action: EffectiveCostAction,
    pub original_tool_count: usize,
    pub selected_tool_count: usize,
    pub selected_tools: Vec<String>,
    pub omitted_tools: Vec<String>,
}
```

Policy input should include:

- `EffectiveCostAnalysis`
- current tool definitions
- current model profile / tool policy if needed
- config
- maybe turn count or phase

Core rule for this pass:

- If policy disabled: `Noop`.
- If mode `Observe`: `Noop`, optionally log existing effective-cost diagnostics only.
- If mode `Warn`: `WarnOnly` when `recommended_action == ReviewToolPalette`.
- If mode `ToolPaletteReduce`: reduce only when:
  - `recommended_action == ReviewToolPalette`,
  - cache observations meet `min_cache_observations`,
  - current tool count exceeds `max_tool_definitions`,
  - and the current phase is before provider call.

Tests:

- Disabled policy always noops.
- Warn mode never changes tools.
- Reduce mode only triggers for `ReviewToolPalette` and enough observations.
- Reduce mode noops when already below tool cap.

## Phase 3: Implement deterministic tool selection

The first reducer should be simple and deterministic.

Inputs:

- Current `Vec<ToolDefinition>` after existing model-profile filtering.
- Config `max_tool_definitions`.
- Config `always_include_tools`.
- Existing deferred tool/search tool names.
- Current context/phase if available.

Selection rules:

1. Always include explicitly required tools if present:
   - `context_read`
   - tool search/catalog tool, whatever it is named in current registry
   - `todowrite` / task-state tool if present
   - question/ask-user tool if present
   - any configured `always_include_tools`
2. Include tools preferred by model profile or current agent if existing policy exposes that data.
3. Include recently successful tools from current session if easily available; skip if not already tracked.
4. Fill remaining slots by original deterministic order or existing registry order.
5. Omit the rest.

Important:

- Do not invent semantic ranking in this pass.
- Do not remove the deferred tool-search mechanism; the reduced palette should preserve a way for the model to discover/load tools.
- Never exceed `max_tool_definitions` unless required tools alone exceed the cap; in that case include required tools and log that cap was exceeded by required set.

Suggested helper:

```rust
pub fn reduce_tool_palette(
    tools: &[ToolDefinition],
    config: &ContextPolicyConfig,
    required_tool_names: &[String],
) -> ToolPaletteReduction
```

Tests:

- Required tools are preserved.
- Reduction is deterministic.
- Cap is respected when possible.
- If required tools exceed cap, all required tools are preserved and decision records cap overflow.
- Omitted tool names are reported.

## Phase 4: Integrate before provider call only

Integrate at the point where `request.tools` is set or immediately before `BeforeProviderCall` observation.

Current flow likely does:

```rust
request.tools = Some(filter_tool_definitions_for_profile(
    self.build_tool_definitions().await,
    &model_profile,
));
```

For this pass:

- Keep existing model-profile filtering first.
- Then, if context policy is enabled and mode is active, apply the tool palette reducer to `request.tools`.
- Log a structured decision.
- Then run `observe_context_pack()` so the diagnostic sees the reduced tool palette.

Critical ordering:

1. Build full tool definitions.
2. Apply existing profile/tool policy.
3. Apply new effective-cost tool palette policy only if enabled.
4. Observe packer.
5. Send provider request.

Do not modify `ToolRegistry`; only modify the `request.tools` payload for that provider call.

Tests:

- With policy disabled, request tools are unchanged.
- With warn mode, request tools are unchanged and decision is warn-only.
- With reduce mode and trigger conditions, request tools are reduced before provider call.
- The reduced request still includes `context_read` or equivalent recovery/search tool if present.

## Phase 5: Add policy diagnostics

Add structured logs:

```rust
tracing::info!(
    policy = "context_tool_palette",
    mode = ?mode,
    action = ?decision.kind,
    recommended_action = ?decision.recommended_action,
    original_tool_count = decision.original_tool_count,
    selected_tool_count = decision.selected_tool_count,
    omitted_tool_count = decision.omitted_tools.len(),
    reason = %decision.reason,
    "context policy decision"
);
```

At debug level, log selected and omitted tool names.

If mode is `Warn`, log warning only when action would have reduced:

```rust
tracing::warn!(
    "context policy would reduce tool palette: {} -> {} ({})",
    original,
    selected,
    reason,
);
```

Acceptance:

- Users can inspect logs and see when/why tools were reduced.
- No high-cardinality verbose logs at info level.

## Phase 6: Guard against tool starvation

Add guardrails:

- If model returns text indicating missing/unavailable tools after reduction, log a policy warning and optionally disable reduction for the next turn/session.
- If tool calls fail because tool is absent from advertised definitions but exists in registry, consider this a starvation signal.
- If the policy reduces tools to zero, disable the reduction and send original tool list.

Minimal implementation for this pass:

- Ensure selected tool list is never empty when original was non-empty.
- Ensure tool-search/deferred-loading tool remains available if present.
- Add TODO/comments for dynamic starvation backoff if deeper wiring is too large.

Tests:

- Empty selected set falls back to original tools or required subset.
- Tool-search tool is preserved if present.

## Phase 7: Documentation

Update:

- `architecture/cache-aware-context.md`
- `AGENTS.md`
- `.opencode/skills/context/SKILL.md`
- config example docs if present

Document:

- This is the first active policy, but it only changes the per-call advertised tool palette.
- It does not rewrite transcript, compact messages, or remove tools from registry.
- Defaults are disabled.
- Recommended rollout: observe -> warn -> reduce.
- How to inspect policy decisions in logs.
- How to configure `always_include_tools` and `max_tool_definitions`.

## Phase 8: Validation

Run:

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features context
cargo test --workspace --all-features
```

Minimum targeted tests if full workspace is impractical:

```bash
cargo test --workspace --all-features context_policy
cargo test --workspace --all-features tool_palette
cargo test --workspace --all-features context
```

## Acceptance criteria

This pass is complete when:

1. A config-gated context policy mode exists with defaults preserving current behavior.
2. Effective-cost `ReviewToolPalette` can drive a warn-only or reduce decision.
3. Tool palette reduction is deterministic and preserves required/recovery/search tools.
4. Reduction applies only to `request.tools`, never to `ToolRegistry` or transcript messages.
5. Warn mode never mutates tools.
6. Reduce mode only activates after explicit config and enough cache observations.
7. Policy decisions are logged with original/selected/omitted counts.
8. Guardrails prevent empty/starved tool palettes.
9. Docs describe observe -> warn -> reduce rollout.
10. Formatting, clippy, and targeted tests pass.

## Suggested stopping point

Stop after phase-scoped tool palette reduction is available behind a strict config gate and logs clear policy decisions.

Do not proceed to active context/message compaction yet. The next pass should evaluate empirical behavior from warn/reduce logs and only then consider volatile-tail compaction, still preserving stable prompt-cache prefixes.
