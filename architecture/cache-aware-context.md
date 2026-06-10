# Cache-Aware Context Packing

## Overview

The cache-aware context packer orders context blocks for stable provider prompt caching. LLM providers (Anthropic, OpenAI) reward requests with stable, repeated prefixes by caching the prefix and billing cached tokens at a fraction of the full rate. The packer ensures that the most cacheable content appears first and consistently, maximizing cache hit rates across turns.

This is distinct from **artifact projection** (`projection.rs`), which compresses individual tool outputs inline. Projection reduces token count per tool result; packing orders *all* context blocks (system prompt, tool definitions, goal context, session frame, messages) for cache stability across the full request.

## Module Structure

```
src/context/
├── mod.rs            # Module root, re-exports
├── block.rs          # ContextBlock, ContextBlockKind, CacheClass, Lossiness
├── block_builder.rs  # ContextBlockBuilder — constructs blocks from runtime state
├── packer.rs         # pack() algorithm — sort by tier/priority, budget enforcement
├── cache_stats.rs    # ContextCacheStats — per-model cache hit rate tracking
├── tool_hash.rs      # tool_definitions_hash — deterministic toolset identity
├── usage_normalize.rs # NormalizedProviderUsage — provider-agnostic token normalization
├── effective_cost.rs  # EffectiveCostAnalysis — diagnostic-only cost recommendations
├── policy.rs          # ContextPolicyConfig, ContextPolicyMode (Observe/Warn/ToolPaletteReduce), decide_policy, reduce_tool_palette — gated first-active policy for tool palette reduction (effective-cost-tool-palette-prototype)
└── (existing files)  # artifact.rs, handle.rs, projection.rs, read_tool.rs
```

## Context Block Tiers

Every `ContextBlockKind` maps to a `CacheClass` tier that determines its position in the packed output:

| Tier | CacheClass | Kinds | Rationale |
|------|-----------|-------|-----------|
| 1 | `StablePrefix` | SystemPrompt, ModelProfile, ProjectInstructions | Identical across turns; the primary cache anchor |
| 2 | `SlowChanging` | ToolDefinitions, GoalContext, MemoryContext | Changes infrequently (tool palette, goal state, memory); cacheable across short sessions |
| 3 | `Volatile` | SessionFrame, ActiveWorkingSet, UserMessage, AssistantMessage, ToolResult, TodoReminder, ArtifactSummary | Changes every turn; never cacheable but ordered after stable content |
| 4 | `NeverCache` | ControlInstruction | Ephemeral directives; explicitly excluded from caching |

### Why Stable Prefix Preservation Matters

Provider prompt caching works by hashing the prefix of the request. If the first N tokens of a request are identical to a previous request, the provider can skip re-processing those tokens. This means:

- The system prompt, model profile, and project instructions must appear first and in a consistent order.
- Tool definitions (which rarely change mid-session) should follow immediately.
- Volatile content (messages, tool results) goes last, since it changes every turn.

The packer's sort order guarantees this: `StablePrefix` blocks always precede `SlowChanging`, which always precede `Volatile`. Within each tier, blocks are sorted by descending priority (higher priority = included first when budget is tight).

## ContextBlock (`block.rs`)

```rust
pub struct ContextBlock {
    pub id: ContextBlockId,          // Deterministic hash of (kind, source) via stable_hash_hex (full SHA-256, 64 lowercase hex)
    pub kind: ContextBlockKind,     // Determines tier
    pub text: String,               // Block content
    pub content_hash: String,       // Stable full SHA-256 (64 hex) of text via stable_hash_hex
    pub estimated_tokens: usize,    // Token count estimate
    pub priority: u32,              // Higher = included first within tier
    pub required: bool,             // If true, always included regardless of budget
    pub lossiness: Lossiness,       // How much compression is acceptable
    pub source: String,             // Stable identity label (e.g., "system:claude-3" or tool hash)
    pub source_handle: Option<String>, // Recoverable context handle (e.g. "ctx://...") or None for non-artifact blocks
}
```

### Lossiness Levels

| Level | Meaning |
|-------|---------|
| `Lossless` | Full text preserved; used for StablePrefix blocks |
| `ProjectedRecoverable` | Can be compressed but full content available via artifact store |
| `SummaryOnly` | Can be reduced to a summary; no recovery needed |

### Block ID Stability

All block IDs, `content_hash`, and `tool_definitions_hash` now use stable full SHA-256 (64 lowercase hex chars) via the shared `stable_hash_hex` helper (no `DefaultHasher` in any context module). Hashes are stable across process restarts and have known test vectors. Tool-definition hashes are order-insensitive.

Block IDs are computed from `(kind, source)`:
- The same block kind + source always produces the same ID (64 hex), regardless of text content.
- Changing the text changes the `content_hash` but not the `id`.
- This allows the packer to track which blocks have changed between turns.

## ContextBlockBuilder (`block_builder.rs`)

Constructs `ContextBlock` instances from runtime state. The builder takes a `session_id` and `model_id` and produces blocks with appropriate tiers, priorities, and lossiness levels.

### Builder Methods

| Method | Kind | Tier | Priority | Required | Lossiness |
|--------|------|------|----------|----------|-----------|
| `build_system_prompt_block` | SystemPrompt | StablePrefix | 100 | yes | Lossless |
| `build_model_profile_block` | ModelProfile | StablePrefix | 90 | yes | Lossless |
| `build_tool_definitions_block` | ToolDefinitions | SlowChanging | 80 | yes | Lossless |
| `build_goal_context_block` | GoalContext | SlowChanging | 70 | no | ProjectedRecoverable |
| `build_memory_context_block` | MemoryContext | SlowChanging | 65 | no | ProjectedRecoverable |
| `build_session_frame_block` | SessionFrame | Volatile | 60 | no | ProjectedRecoverable |
| `build_todo_reminder_block` | TodoReminder | Volatile | 40 | no | SummaryOnly |
| `build_control_instruction_block` | ControlInstruction | NeverCache | 30 | no | SummaryOnly |
| `build_artifact_summary_block` | ArtifactSummary | Volatile | 20 | no | SummaryOnly |

The `build_all` convenience method constructs all blocks from a single call, skipping `None` optionals.

### Tool Definitions Hash and Summary Text

`tool_definitions_hash()` in `tool_hash.rs` computes a deterministic hash of the tool palette using `stable_hash_hex` (full SHA-256, 64 lowercase hex chars; order-insensitive for the set of definitions). No `DefaultHasher` is used in context modules.

`build_tool_definitions_block` now renders deterministic non-empty summary text (hash + per-tool lines with `name | defer=... | schema_hash=... | description`). This makes stable/slow-changing token accounting realistic (previously undercounted because `text=""`). The summary is compact for logs but sufficient for token estimates; full schemas are not inlined.

## Packer Algorithm (`packer.rs`)

### Input

```rust
pub struct ContextPackBudget {
    pub max_tokens: usize,               // Total context window budget
    pub reserved_output_tokens: usize,   // Tokens reserved for model output
    pub emergency_margin_tokens: usize,  // Safety margin
}
```

Available context = `max_tokens - reserved_output_tokens - emergency_margin_tokens`.

### Algorithm

1. **Sort** all candidate blocks by `SortKey`: `(tier ASC, priority DESC, id ASC)`. This guarantees StablePrefix before SlowChanging before Volatile, and higher-priority blocks first within each tier.

2. **Iterate** through sorted blocks:
   - **Required blocks**: Always included, regardless of budget. Token count added to running total.
   - **NeverCache non-required**: Always omitted (reason: `OverBudget`). These are ephemeral directives that should not consume budget.
   - **Volatile low-priority** (priority < 10): Omitted early if budget is tight (reason: `LowerPriority`). This protects budget for higher-priority volatile content.
   - **All other blocks**: Included if they fit within budget; omitted with `OverBudget` reason otherwise.

3. **Output** `ContextPackResult` with included blocks, omitted blocks (with reasons), total estimated tokens, and per-tier token counts.

### Omission Reasons

| Reason | Meaning |
|--------|---------|
| `OverBudget` | Block exceeds remaining budget |
| `LowerPriority` | Volatile block with priority < 10, dropped to preserve budget |
| `VolatileOverflow` | Reserved for future use |
| `ReplacedByHandle` | Reserved for future use (artifact handle replacement) |

### Output

```rust
pub struct ContextPackResult {
    pub blocks: Vec<ContextBlock>,           // Packed blocks in order
    pub estimated_tokens: usize,            // Total tokens of included blocks
    pub omitted_blocks: Vec<OmittedContextBlock>, // What was dropped and why
    pub stable_prefix_tokens: usize,        // Tokens in StablePrefix tier
    pub volatile_tokens: usize,             // Tokens in Volatile tier
}
```

The `stable_prefix_tokens` and `volatile_tokens` fields allow callers to verify the ratio of cacheable to non-cacheable content.

## ContextCacheStats (`cache_stats.rs`)

Tracks per-model cache hit rates across turns:

```rust
pub struct CacheStatsEntry {
    pub last_input_tokens: usize,
    pub last_cached_tokens: usize,
    pub last_output_tokens: usize,
    pub total_input_tokens: usize,
    pub total_cached_tokens: usize,
    pub total_output_tokens: usize,
    pub call_count: usize,
}
```

- `record_usage(model_key, input_tokens, cached_tokens, output_tokens)` — called after each provider response.
- `cache_hit_rate(model_key)` — returns `total_cached / total_input` (0.0–1.0).
- `models()` — lists all tracked model keys.

Stats are session-local and in-memory. They allow the packer (and diagnostics) to measure whether cache-aware ordering is actually improving hit rates.

**Note on cached-token telemetry**: Cached-token values are only reported by providers that support prompt caching (currently OpenAI and Anthropic). Other providers may report zero or omit cached tokens entirely. Cache stats for non-caching providers will show 0% hit rates, which is expected.

## Agent Loop Integration (`src/agent/loop.rs`)

The packer is invoked via `observe_context_pack` (which calls the private `compute_context_pack_result` / `build_packer_candidates`) at these phases during a turn: InitialRequest, AfterToolResults, AfterCompaction, BeforeProviderCall, BeforeFinalization.

1. **Build candidates**: `build_packer_candidates` (via `ContextBlockBuilder::build_all()` + transcript frame) constructs blocks from the current system prompt, model profile, tool definitions, context frame, goal, memory, todo, and control text. (Transcript messages User/Assistant/ToolResult are not yet emitted as live blocks; the packer sorts volatile blocks by priority/id only.)
2. **Compute budget**: `ContextPackBudget` uses configured `max_stable_prefix_tokens` + `max_volatile_tokens` as total, minus reserved output (10K) and emergency margin (4K).
3. **Pack**: `packer::pack(candidates, budget)` sorts and enforces budget (global tier/priority/id sort; no chronological transcript order preservation yet).
4. **Observe only**: Diagnostics are logged (enriched with phase, tool hash, cache hit rate, slow-changing tokens, top omitted). No mutation occurs. The `observe_context_pack` helper never mutates the request.

### Provider Usage Recording

`AgentLoop::record_context_cache_stats_from_processor()` is called exactly once per successful provider response in the main `run()` loop, after `EventProcessor` has consumed the stream events and before the processor is reset. It:

1. Checks `processor.is_complete()` — skips incomplete responses.
2. Skips zero-usage responses (input=0, output=0, no cached_tokens) to avoid synthetic zero-call stats.
3. Calls `normalize_from_finish()` to clamp cached tokens (e.g., cached ≤ input).
4. Records normalized usage into `ContextCacheStats`.
5. Logs updated cache hit rate at debug level.

This replaces the previous inline recording in `stream_once`, ensuring normalization is applied and the single recording point is easy to audit. Each successful provider response with usage increments `call_count` by one.

## Observation Mode (Only Effective Mode)

Active mutation is disabled for this pass. `observe_only` is forced internally; requesting `observe_only=false` emits a warning and runs as observe-only. No code path can replace system prompt content with packed frame text (the "Current session context:" replacement branch is removed).

Observation/diagnostics are the only effective mode. Diagnostics run at multiple phases (InitialRequest, BeforeProviderCall, AfterToolResults, AfterCompaction, BeforeFinalization) via the private `observe_context_pack` helper (which calls `compute_context_pack_result` / `build_packer_candidates` internally). The helper never mutates the request.

Diagnostics include:
- phase,
- model,
- candidate/packed/stable/slow-changing/volatile token estimates,
- omitted count + top omitted (id/kind/reason/tokens),
- tool_definitions_hash,
- cache_hit_rate from `ContextCacheStats` (surfaced from provider `cached_tokens` telemetry via `record_usage`).

Example log (info level when `log_diagnostics` enabled):

```
context-packer[BeforeProviderCall]: model=anthropic/claude-..., candidates=12, packed=9, stable_prefix_tokens=12450, slow_changing_tokens=3870, volatile_tokens=9200, omitted=3, tool_definitions_hash=..., cache_hit_rate=0.6123
```

Enriched diagnostic log showing effective-cost fields:

```
context-packer[BeforeProviderCall]: model=gpt-4, candidates=15000, packed=12000, stable_prefix_tokens=5000, slow_changing_tokens=3000, volatile_tokens=4000, omitted=2, tool_definitions_hash=abc123, cache_hit_rate=0.6500
context-packer[BeforeProviderCall]: recommended_action=preserve_stable_prefix, uncached_input_tokens=3500, effective_cache_hit_rate=0.6500, effective_reason=cache hit rate 0.65 is high and stable prefix is 42% of total; preserving stable prefix maximizes cache reuse
```

Per-block omission details (top omitted) are logged at debug level.

This is an observation/diagnostic layer to inform later effective-cost compaction. It does not change request behavior.

### Effective-Cost Analysis (`effective_cost.rs`)

`EffectiveCostAnalysis` provides diagnostic-only cost recommendations based on cache hit rates and tier composition. It operates on the output of `observe_context_pack` and `ContextCacheStats` to suggest actions that *would* improve cache efficiency, but it does **not** mutate requests, trigger compaction, or alter provider calls.

Key properties:
- **Diagnostic only**: Outputs recommendations (e.g., `preserve_stable_prefix`, `reorder_blocks`) but takes no action.
- **No compaction**: Does not trigger compaction or removal of content.
- **No request mutation**: Does not modify the outgoing provider request in any way.
- **Stable-prefix preservation decisions**: Future work — the analysis can recommend preserving stable prefixes, but this is not yet wired into the packer's behavior.

### Usage Normalization (`usage_normalize.rs`)

`NormalizedProviderUsage` normalizes raw provider token counts (input, output, cached) into a provider-agnostic representation. This allows cache stats and effective-cost analysis to work uniformly across providers that report tokens differently (e.g., some providers count cached tokens as a subset of input, others report them separately).

### Gated Context Policy for Tool-Palette Reduction (policy.rs)

A new top-level `[context_policy]` config section (distinct from `context_packer`) was added under plan name `effective-cost-tool-palette-prototype`. It implements the first *active* (but strictly gated) policy: conservative tool-palette reduction driven by `EffectiveCostAnalysis` + per-tool call counts.

#### ContextPolicyConfig and ContextPolicyMode

```rust
// In src/context/policy.rs (new)
pub struct ContextPolicyConfig {
    pub enabled: bool,                    // default false (safe)
    pub mode: ContextPolicyMode,          // default Observe
    pub min_observations: usize,          // default 3
    pub max_tool_definitions: usize,      // default 24
    pub always_include: Vec<String>,      // defaults include context_read, tool_search, todowrite, ...
    pub never_reduce: Vec<String>,
    pub log_decisions: bool,              // default true
}

pub enum ContextPolicyMode {
    Observe,           // default — no change, full diagnostics
    Warn,              // logs would-reduce decisions at warn level
    ToolPaletteReduce, // first active policy: mutates only per-request request.tools
}
```

All defaults are conservative. `enabled=false` and `mode=Observe` ensure zero behavioral change on upgrade. `always_include` and `never_reduce` are explicit allow/deny lists; `tool_search` is additionally hard-guarded.

#### Policy Decision Flow

Decision is made in `decide_policy` (called from `AgentLoop::apply_tool_palette_policy_if_active`):

1. Review the latest `EffectiveCostAnalysis` (via `ReviewToolPalette` recommended action) for the current model/phase.
2. Check observed `call_count` for the specific tool (from tool-usage sidecar, separate from packer stats) meets or exceeds `min_observations`.
3. Current number of tool definitions in the *per-request* `Vec<ToolDefinition>` exceeds `max_tool_definitions`.
4. Phase is `InitialRequest` or a pre-provider phase in the follow-up drain (before `BeforeProviderCall` observation).

If all gates pass and `mode == ToolPaletteReduce`, emit `ContextPolicyDecision::ReduceToolPalette { recommended, original_count, selected, omitted, reason }`. For `Warn` mode the decision is `WarnOnly`. For `Observe` or any gate failure, decision is `NoAction`.

#### Deterministic Reduction (`reduce_tool_palette`)

Reduction is purely syntactic and order-preserving:

- Collect the *required set* in input order: (a) any tool whose name is in the hardcoded recovery set, (b) `always_include`, (c) `never_reduce`, (d) the tool that originally called the current sub-task (if any).
- If required set size > cap, keep only the required set and set `overflow=true` in the reason.
- Fill remaining slots from the original input list (in arrival order), skipping already-included required tools, up to the cap.
- Guardrails: if the selected set would be empty while input was non-empty, fall back to the first `min(3, input.len())` tools; `tool_search` is forcibly preserved if present in the input.

The function returns a `ToolPaletteReduction` struct containing `selected: Vec<ToolDefinition>`, `omitted_names`, `reason`, and `overflow` flag. It never reorders within the kept prefix and never performs semantic ranking.

#### Integration Point in AgentLoop

`AgentLoop::apply_tool_palette_policy_if_active(&self, phase: ObservationPhase, request: &mut ProviderRequest)` is invoked:

- After the model-profile filter has run on `InitialRequest`.
- Before the `BeforeProviderCall` `observe_context_pack` call (both in the main turn and in the follow-up-drain loop).

It only ever mutates the per-request `request.tools: Vec<ToolDefinition>`. It:
- Never touches the `ToolRegistry`, the transcript, compaction paths, or any packer mutation logic.
- Is a no-op when `config.enabled == false` or `mode == Observe`.
- Is a no-op (with debug log) when the policy decides `NoAction` or `WarnOnly`.

When reduction occurs, the call site logs at info level (structured fields: policy, mode, action, recommended, original, selected, omitted, reason) and at debug level the concrete tool names. Warn-mode decisions log at warn level with the would-reduce message but do not mutate.

#### Logging and Observability

- Info: `context-policy[ToolPaletteReduce]: policy=context_policy mode=ToolPaletteReduce action=reduce recommended=ReviewToolPalette original=37 selected=24 omitted=13 reason="call_count>=3 and defs>24 at InitialRequest; overflow=false"`
- Debug: lists of kept/omitted names.
- Warn (only when mode=Warn): `context-policy[WARN]: would reduce tool palette (37→24) for reason=... (no mutation because mode=Warn)`
- All logs are gated behind the existing `log_decisions` flag (default true) and the master `enabled` flag.

#### Rollout Recommendation

1. Default (`enabled=false`, `mode=Observe`) — zero change, full diagnostics.
2. `enabled=true, mode=Warn` — observe would-reduce decisions in logs without behavioral impact.
3. `enabled=true, mode=ToolPaletteReduce` — first active, conservative reduction.

The policy is deliberately scoped to the tool-definitions list that is about to be sent to the provider on this specific request. It is intentionally *not* a general compaction or context-packer mutation.

#### Non-Goals (Still Hold)

- No transcript rewrite or reordering.
- No compaction triggered.
- No `ToolRegistry` mutation.
- No pricing calculation (uses only call counts + `EffectiveCostAnalysis` recommendation).
- No semantic / usage-based ranking inside the reduction (purely order-of-appearance + explicit allow/deny lists).

## Active Mode (Disabled)

Active mode is disabled for this pass. Requesting it (via config `observe_only: false` when `enabled: true`) produces a warning and forces observe-only execution. There is no code path that mutates provider requests or replaces system prompt content. The previous "Current session context:" replacement logic has been removed. Active mutation is not yet safe; this pass hardened the observation layer only.

## Configuration

In `opencode.json`:

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

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `enabled` | `Option<bool>` | `false` | Master toggle; when false, packer is not invoked. Note: even when enabled, active mutation is disabled for this pass (see Observation Mode). |
| `observe_only` | `Option<bool>` | `true` | Forced true internally for now; requesting false emits a warning and runs observe-only. Active mutation is not yet safe. |
| `stable_prefix` | `Option<bool>` | `true` | Parsed but currently has no effect on sort (sort is always tier-based). Reserved for future ordered-prefix support. |
| `max_stable_prefix_tokens` | `Option<usize>` | `32000` | Budget allocated to StablePrefix + SlowChanging tiers |
| `max_volatile_tokens` | `Option<usize>` | `24000` | Budget allocated to Volatile tier |
| `log_diagnostics` | `Option<bool>` | `true` | Log packer metrics at info/debug level (includes phase, tool hash, cache hit rate, slow-changing, top omitted) |

**Budget calculation**: Total context budget = `max_stable_prefix_tokens + max_volatile_tokens`. Reserved output = 10,000 tokens. Emergency margin = 4,000 tokens. Available for context = total - 14,000.

## Relationship to Artifact Projection

| Concern | Projection (`projection.rs`) | Packing (`packer.rs`) |
|---------|------------------------------|----------------------|
| Scope | Single tool output | Full context window |
| Goal | Reduce token count per tool result | Order all blocks for cache stability |
| When | Per tool result, inline | Per provider turn, before request |
| Mechanism | Truncation, error extraction, summary | Tier-based sort, budget enforcement |
| Provider awareness | None | Optimizes for prompt cache prefix stability |

Both systems are complementary. Projection reduces the size of individual `ToolResult` blocks; packing ensures those (now smaller) blocks are ordered correctly relative to system prompt, tool definitions, and other context.

## What Is Not Implemented Yet

- **Effective-cost compaction (active)**: The diagnostic analysis (`EffectiveCostAnalysis`) is now in place and produces recommendations, but no code path acts on those recommendations. A future enhancement would wire the analysis output into the packer's budget enforcement, allowing cost-aware decisions about which volatile blocks to omit.
- **Stable-prefix preservation decisions**: The analysis can recommend preserving stable prefixes, but the packer does not yet act on this. Future work would allow the packer to adjust block ordering or omission based on cache hit rate feedback.
- **Dynamic tool palettes (packer-level)**: The tool definitions block is always required and always included in full by the packer. A future optimization could allow the packer to omit low-usage tools from the definitions when budget is tight, using the tool hash to detect palette changes.
- **Gated tool-palette reduction (first active policy)**: *This* pass did wire the first active (but strictly gated) policy for conservative tool-palette reduction under the new top-level `[context_policy]` section (plan: effective-cost-tool-palette-prototype). It is implemented in `src/context/policy.rs` (`decide_policy`, `reduce_tool_palette`, `ContextPolicyConfig`, `ContextPolicyMode`, `ContextPolicyDecision`, `ToolPaletteReduction`) and integrated only in `AgentLoop::apply_tool_palette_policy_if_active`. The policy is phase-scoped to pre-provider windows, mutates *only* the per-request `request.tools` Vec, never touches ToolRegistry/transcript/compaction/packer mutation paths, and is behind safe defaults (`enabled=false`, `mode=Observe`). It remains a conservative, order-based reduction (no semantic selection). The broader "dynamic tool palettes" item above refers to deeper packer-level integration that is still future work.
- **Provider-specific pricing**: Cache hit rates vary by provider and model. The packer does not currently adjust its strategy based on provider-specific cache pricing (e.g., Anthropic's 90% discount on cached tokens vs. OpenAI's 50%).
- **Cross-turn block diffing**: The packer currently rebuilds all blocks each turn. A future optimization could diff against the previous turn's blocks and only re-pack changed blocks, further improving cache stability.

**This pass (hardening) added**: multi-phase observation (`observe_context_pack` at InitialRequest/BeforeProviderCall/AfterToolResults/AfterCompaction/BeforeFinalization), `source_handle: Option<String>` on `ContextBlock`, stable full SHA-256 hashes via `stable_hash_hex` (no DefaultHasher in context modules), real non-empty deterministic tool-definition summary text (for realistic token counts), cache-hit-rate wiring from provider `cached_tokens` telemetry into diagnostics, `NormalizedProviderUsage` for provider-agnostic token normalization (`usage_normalize.rs`), `EffectiveCostAnalysis` for diagnostic-only cost recommendations (`effective_cost.rs`), correction of the misleading transcript-order test (packer sorts volatile by priority/id; no live transcript blocks yet; comment added in packer.rs), *and* gated `[context_policy]` top-level config section + `ContextPolicyConfig`/`ContextPolicyMode` (Observe/Warn/ToolPaletteReduce) + `decide_policy`/`reduce_tool_palette` implementation in new `src/context/policy.rs` + `AgentLoop::apply_tool_palette_policy_if_active` integration for phase-scoped per-request `request.tools` reduction (first active but strictly gated policy under plan effective-cost-tool-palette-prototype; only mutates the outgoing tool list, never registry/transcript/compaction/packer; safe defaults; deterministic order-based reduction with explicit guardrails). Active mutation of the packer itself remains disabled; this is still primarily an observation/diagnostic layer, now with one narrow, conservative, opt-in active policy for tool-palette size. No persistence of stats, no transcript packing/reordering, no effective-cost decisions wired into packer behavior, no replacement of compaction.
