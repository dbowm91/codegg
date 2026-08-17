# Context

Context budgeting, packing, and projection for LLM conversation history.

## Purpose

The context module manages how much conversation history fits within a model's token budget. It decides what to include, what to omit, and what to compact, ensuring the agent stays within context limits while preserving the most useful information.

## Module Structure

```
src/context/
├── mod.rs              # Module root, re-exports
├── artifact.rs         # Artifact storage (file/in-memory) for tool outputs
├── block.rs            # ContextBlock primitives (13 kinds, 4 cache classes)
├── block_builder.rs    # Builder pattern for constructing blocks
├── cache_stats.rs      # Cache hit/miss tracking per block
├── effective_cost.rs   # Cost analysis for tool palette reduction
├── handle.rs           # ContextHandle — compact references to large artifacts
├── packer.rs           # Token-budget greedy packing algorithm
├── plan.rs             # ContextPlan — pre-pack message assembly
├── policy.rs           # Policy decisions (noop/warn/reduce tool palette)
├── projection.rs       # Tool output projection (truncate/summarize)
├── read_tool.rs        # context_read tool implementation
├── tool_hash.rs        # Deterministic tool-definition hashing
├── usage_normalize.rs  # Cross-provider token usage normalization
└── volatile_tail.rs    # Volatile-tail compaction for recent messages
```

## Key Types

### ContextBlock

The fundamental unit of context. Each block has a kind, cache class, priority, and token estimate.

| Field | Type | Description |
|-------|------|-------------|
| `id` | `ContextBlockId` | SHA-256 of `"{kind}:{source}"` |
| `kind` | `ContextBlockKind` | 13 variants (SystemPrompt, ToolDefinitions, UserMessage, etc.) |
| `text` | `String` | The block's text content |
| `estimated_tokens` | `usize` | Token count estimate |
| `priority` | `u32` | Higher = more important |
| `required` | `bool` | Always included regardless of budget |
| `lossiness` | `Lossiness` | Lossless / ProjectedRecoverable / SummaryOnly |

### ContextBlockKind (13 variants)

Each maps to a `CacheClass` tier:

| Kind | Cache Class | Description |
|------|-------------|-------------|
| `SystemPrompt` | StablePrefix | System instructions |
| `ModelProfile` | StablePrefix | Model behavioral profile |
| `ToolDefinitions` | StablePrefix | Tool schemas |
| `ProjectInstructions` | SlowChanging | Project-specific context |
| `SessionFrame` | SlowChanging | Session metadata |
| `GoalContext` | SlowChanging | Active goals |
| `MemoryContext` | SlowChanging | Persistent memories |
| `ActiveWorkingSet` | SlowChanging | Currently relevant files |
| `UserMessage` | Volatile | User input |
| `AssistantMessage` | Volatile | Agent responses |
| `ToolResult` | Volatile | Tool output |
| `ControlInstruction` | NeverCache | Internal control signals |
| `TodoReminder` | Volatile | Todo state injection |
| `ArtifactSummary` | SlowChanging | Summarized artifacts |

### CacheClass (4 tiers)

Used for packing priority — lower tiers are evicted first:

1. `StablePrefix` — Always included
2. `SlowChanging` — Included when budget allows
3. `Volatile` — Evicted on overflow (low-priority first)
4. `NeverCache` — Always omitted from cache, re-fetched

## Packing Algorithm

`packer::pack()` implements a greedy token-budget packing:

1. Sort blocks by `CacheClass` tier → reverse priority → id
2. `required` blocks always included
3. Greedily add blocks until budget exhausted
4. `NeverCache` non-required blocks always omitted
5. `Volatile` low-priority blocks evicted first on overflow

Returns `ContextPackResult` with included blocks, omitted blocks, and token accounting.

## Policy Decisions

`policy::decide_policy()` evaluates context state against config thresholds:

| Decision | Meaning |
|----------|---------|
| `Noop` | Within budget, no action needed |
| `WarnOnly` | Over threshold but only logging (dry-run) |
| `ReduceToolPalette` | Actually omit low-priority tools from definitions |

## Volatile-Tail Compaction

`volatile_tail.rs` handles recent-message compaction when context overflows:

- Analyzes recent messages for compaction candidates
- Applies tombstone formatting for compacted messages
- Extracts recovery handles for later retrieval
- Disabled by default (`observe` mode in config)

## Tool Output Projection

`projection.rs` truncates or summarizes large tool outputs before they enter context:

- `project_tool_output()` applies size limits and summarization
- Configurable per-tool projection rules
- Works with the preflight system for validation

## Public API

| Function | Purpose |
|----------|---------|
| `pack(blocks, budget) -> ContextPackResult` | Greedy token-budget packing |
| `decide_policy(analysis, ...) -> ContextPolicyDecision` | Evaluate context policy |
| `project_tool_output(config, output) -> ToolOutputProjection` | Project/truncate tool output |
| `analyze_volatile_tail(messages) -> VolatileTailAnalysis` | Identify compaction candidates |
| `apply_volatile_tail_compaction(plan) -> Vec<Message>` | Apply compaction |

## See Also

- [Agent](agent.md) — AgentLoop uses context packing for conversation management
- [Cache-Aware Context](cache-aware-context.md) — Cache-aware packing optimization
- [Context Ledger](context-ledger.md) — Token counting utilities
- [Compaction](compaction.md) — Context window overflow management
- [Tool](tool.md) — Tool output feeds into context blocks
