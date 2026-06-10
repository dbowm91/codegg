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
    pub id: ContextBlockId,          // Deterministic hash of (kind, source)
    pub kind: ContextBlockKind,     // Determines tier
    pub text: String,               // Block content
    pub content_hash: String,       // SHA-like hash of text for change detection
    pub estimated_tokens: usize,    // Token count estimate
    pub priority: u32,              // Higher = included first within tier
    pub required: bool,             // If true, always included regardless of budget
    pub lossiness: Lossiness,       // How much compression is acceptable
    pub source: String,             // Provenance tag (e.g., "system:claude-3")
}
```

### Lossiness Levels

| Level | Meaning |
|-------|---------|
| `Lossless` | Full text preserved; used for StablePrefix blocks |
| `ProjectedRecoverable` | Can be compressed but full content available via artifact store |
| `SummaryOnly` | Can be reduced to a summary; no recovery needed |

### Block ID Stability

Block IDs are computed from `(kind, source)` via a deterministic hash. This means:
- The same block kind + source always produces the same ID, regardless of text content.
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

### Tool Definitions Hash

`tool_definitions_hash()` in `tool_hash.rs` computes a deterministic hash of the tool palette:
1. Sorts definitions by name.
2. Hashes each definition's name, description, canonicalized parameters (sorted keys), and `defer_loading` flag.
3. Returns a 16-character hex string.

This hash serves as the `source` tag for the `ToolDefinitions` block, making the block ID change when the tool palette changes — which is the correct behavior for cache invalidation.

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

## Agent Loop Integration (`src/agent/loop.rs`)

The packer is invoked at the start of each provider turn, after tool definitions are built and the context frame is assembled:

1. **Build candidates**: `ContextBlockBuilder::build_all()` constructs blocks from the current system prompt, model profile, tool definitions, context frame, goal, memory, todo, and control text.
2. **Compute budget**: `ContextPackBudget` uses configured `max_stable_prefix_tokens` + `max_volatile_tokens` as total, minus reserved output (10K) and emergency margin (4K).
3. **Pack**: `packer::pack(candidates, budget)` sorts and enforces budget.
4. **Observe or act**:
   - **Observe mode** (default): Logs diagnostics only. The original messages are sent unmodified.
   - **Active mode**: Replaces the `Current session context:` section in the system message with the packed session frame content.

## Observation Mode

When `observe_only = true` (the default), the packer runs but does not modify the request. It logs:

```
context-packer: candidates=N, packed=M, stable_prefix_tokens=X, volatile_tokens=Y, omitted=Z
```

Per-block omission details are logged at `debug` level:

```
context-packer: omitted block <id> (N tokens, reason: OverBudget)
```

This allows operators to measure cache efficiency without risking behavioral changes. Enable diagnostics with `log_diagnostics: true` (the default).

## Active Mode

When `observe_only = false`, the packer replaces the session context injection in the system message. It finds the `Current session context:` marker in the system message and replaces everything from that marker onward with the packed session frame block content.

This ensures the packed (ordered, budget-enforced) content is what the model sees, rather than the raw assembled context frame.

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
| `enabled` | `Option<bool>` | `false` | Master toggle; when false, packer is not invoked |
| `observe_only` | `Option<bool>` | `true` | When true, logs diagnostics without modifying requests |
| `stable_prefix` | `Option<bool>` | `true` | When true, stable prefix blocks are prioritized in sort order |
| `max_stable_prefix_tokens` | `Option<usize>` | `32000` | Budget allocated to StablePrefix + SlowChanging tiers |
| `max_volatile_tokens` | `Option<usize>` | `24000` | Budget allocated to Volatile tier |
| `log_diagnostics` | `Option<bool>` | `true` | Log packer metrics at info/debug level |

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

- **Effective-cost compaction**: The packer currently uses a simple token count for budget enforcement. A future enhancement would weight blocks by their cache effectiveness (cached tokens cost less than uncached tokens), allowing the packer to make cost-aware decisions about which volatile blocks to omit.
- **Dynamic tool palettes**: The tool definitions block is always required and always included in full. A future optimization could allow the packer to omit low-usage tools from the definitions when budget is tight, using the tool hash to detect palette changes.
- **Provider-specific pricing**: Cache hit rates vary by provider and model. The packer does not currently adjust its strategy based on provider-specific cache pricing (e.g., Anthropic's 90% discount on cached tokens vs. OpenAI's 50%).
- **Cross-turn block diffing**: The packer currently rebuilds all blocks each turn. A future optimization could diff against the previous turn's blocks and only re-pack changed blocks, further improving cache stability.
