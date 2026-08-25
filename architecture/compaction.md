# Compaction Module

## Purpose

The compaction module manages context window overflow by reducing
conversation history while preserving tool call/output invariants and
session state. It supports a legacy strategy-based path (truncate,
summarize, drop-middle) and a newer hybrid engine with programmatic
evidence extraction, optional LLM-based semantic enrichment, and
invariant validation with emergency fallback.

## Where It Lives

| File | Role |
|------|------|
| `src/agent/compaction.rs` | `ContextTracker`, all compaction strategies, hybrid engine, invariant validation |
| `src/agent/context_frame.rs` | `ContextFrame`, `ContextLedgerState` — post-compaction context snapshot |
| `src/agent/loop.rs:1838` | `compact_if_needed()` — integration point called each turn |
| `src/config/schema.rs` | `CompactionConfig`, `CompactionModeConfig`, `CompactionPolicyConfig` |
| `tests/compaction.rs` | Module-level integration tests |

## How It Works

### Two Paths

1. **Legacy path** (no `compaction.mode` in config): Uses
   `auto_compact_async()` / `auto_compact_sync()` with strategy
   selection (TruncateToolOutputs, SummarizeOldTurns, DropMiddleMessages).
   Entry: `compact_if_needed()` → `detect_overflow()` → `prune_tool_outputs()`
   → `auto_compact_async()`.

2. **New hybrid engine** (`compaction.mode` set in config): Uses
   `compact_with_policy()` as the primary entry. Builds programmatic
   evidence, applies mode (Programmatic/Agent/Hybrid), validates
   invariants, falls back to emergency pair-safe compaction on failure.
   Entry: `compact_if_needed()` → `detect_overflow()` → `prune_tool_outputs()`
   → `compact_with_policy()`.

### Hybrid Engine Flow

```
compact_with_policy(CompactionInput, provider)
  → build_programmatic_state(messages, config)
      → build_evidence_index(messages)         // EvidenceRef per message
      → collect_tool_pairs(messages)           // ToolPair mapping
      → extract_commands(tool_pairs)           // Salient commands
      → extract_file_paths(messages, tool_pairs)
      → extract_test_and_error_state(tool_pairs)
      → extract_user_constraints(messages)
      → select_retained_messages(messages, state, policy, keep_recent)
  → dispatch by CompactionMode:
      Programmatic → compile_programmatic_messages(original, state, config)
      Agent → compact_agent_only(input, provider)
          → build_programmatic_state + semantic_checkpoint + keep recent
          Falls back to programmatic on LLM failure
      Hybrid → build frame + optional semantic_checkpoint + merge_frames
          → compile_hybrid_messages(original, state, frame, config)
  → validate_message_invariants(messages)
      On failure: emergency_pair_safe_compaction → validate again
      On second failure: preserve original messages
  → CompactionOutput { messages, frame, diagnostics, tokens_before/after }
```

### Programmatic Evidence Extraction

`build_evidence_index()` creates `Vec<EvidenceRef>` where each ref has:
- `id`: stable identifier (`msg_0001`, `tool_0042`)
- `kind`: `UserMessage | AssistantMessage | ToolCall | ToolResult | TestRun | FilePath | Command | Diff | SecurityFinding | Todo`
- `summary`: truncated content
- `content_hash`: SHA-256 of content (deterministic)

### Semantic Checkpoint (LLM-based enrichment)

`semantic_checkpoint()` asks the LLM to fill four fields from the reduced
ledger: `constraints`, `decisions`, `unresolved_errors`, `next_steps`.
Returns a `ContextFrame` that gets merged into the programmatic frame.
Timeout: 60s. Falls back to programmatic-only frame on any failure.

`merge_frames()` applies semantic fields only when non-empty, never
overriding `touched_files`, `commands_run`, or `test_results` which are
better extracted deterministically.

### Emergency Fallback

`emergency_pair_safe_compaction()` groups messages into tool-pair units
(System, Single, ToolPair) and retains the most recent groups. Inserts
an emergency marker system message. Preserves tool call/result pairs.

## Key Types & APIs

### ContextTracker (`src/agent/compaction.rs:22`)

```rust
pub struct ContextTracker {
    current_tokens: usize,
    context_limit: usize,
    threshold: f64,
    message_token_counts: Vec<usize>,
    max_messages: Option<usize>,
    max_total_bytes: Option<usize>,
    model: Option<String>,
}
```

Key methods: `add_message()`, `needs_compaction()`, `needs_overflow_protection()`,
`remaining_tokens()`, `estimate_tokens_for_messages()`, `reset()`.

### CompactionStrategy (Legacy) (`src/agent/compaction.rs:197`)

```rust
pub enum CompactionStrategy {
    TruncateToolOutputs,   // Tool outputs > 500 chars → truncated
    SummarizeOldTurns,     // LLM summarization (async only)
    DropMiddleMessages,    // Keep first 2 + last 2 non-system messages
}
```

### CompactionMode (`src/agent/compaction.rs:670`)

```rust
pub enum CompactionMode {
    Programmatic,  // Deterministic: evidence index, state extraction, retained messages
    Agent,         // LLM-driven: semantic checkpoint, falls back to programmatic
    Hybrid,        // Programmatic + optional LLM enrichment (default)
}
```

### CompactionPolicy (`src/agent/compaction.rs:678`)

| Policy | Max Tool Output Tokens | Keep Recent | Max Summary Tokens |
|--------|----------------------|-------------|-------------------|
| `Conservative` | 2000 | 8 | 1200 |
| `Balanced` (default) | 1000 | 4 | 800 |
| `Cheap` | 500 | 2 | 400 |
| `Emergency` | 200 | 1 | 200 |
| `LosslessDebug` | MAX | 999 | 2000 |

### ResolvedCompactionConfig (`src/agent/compaction.rs:718`)

All config fields resolved to concrete values. `from_config()` maps from
`CompactionConfig` schema with policy-based defaults. Model resolution:
`compaction.model` → `summarize_model` → `active_model`.

### CompactionInput / CompactionOutput (`src/agent/compaction.rs:872`)

```rust
pub struct CompactionInput<'a> {
    pub messages: &'a [Message],
    pub config: ResolvedCompactionConfig,
    pub active_model: Option<&'a str>,
}

pub struct CompactionOutput {
    pub messages: Vec<Message>,
    pub frame: Option<ContextFrame>,
    pub diagnostics: Vec<CompactionDiagnostic>,
    pub tokens_before: usize,
    pub tokens_after: usize,
}
```

### ProgrammaticCompactionState (`src/agent/compaction.rs:864`)

```rust
pub struct ProgrammaticCompactionState {
    pub frame: ContextFrame,
    pub evidence: Vec<EvidenceRef>,
    pub retained_message_indices: Vec<usize>,
    pub diagnostics: Vec<CompactionDiagnostic>,
}
```

### EvidenceRef (`src/agent/compaction.rs:842`)

```rust
pub struct EvidenceRef {
    pub id: String,
    pub kind: EvidenceKind,
    pub summary: String,
    pub content_hash: Option<String>,
}
```

### Invariant Validation (`src/agent/compaction.rs:1257`)

`validate_message_invariants()` checks:
1. No orphan `Message::Tool` without matching assistant tool_call
2. No assistant tool_call without all required tool results
3. Returns `CompactionInvariantError::OrphanToolResult` or
   `MissingToolResult` on failure.

## Configuration Surface

| Config Key | Type | Default | Effect |
|------------|------|---------|--------|
| `compaction.enabled` | bool | `true` | Enable/disable compaction |
| `compaction.auto` | bool | `true` | Auto-compact on threshold |
| `compaction.mode` | string | `hybrid` | `programmatic` / `agent` / `hybrid` |
| `compaction.policy` | string | `balanced` | Aggressiveness level |
| `compaction.prune` | bool | `true` | Pre-pass tool output pruning |
| `compaction.max_tokens` | usize | profile-based | Context window limit |
| `compaction.threshold` | f64 | 0.7 | When to trigger (ratio of limit) |
| `compaction.reserved` | usize | 16000 | Tokens reserved for output |
| `compaction.model` | string | `None` | Preferred compaction model |
| `compaction.summarize_model` | string | `None` | Legacy alias (fallback for model) |
| `compaction.max_tool_output_tokens` | usize | policy-based | Max tokens per tool result |
| `compaction.max_summary_tokens` | usize | policy-based | Max tokens for LLM summary |
| `compaction.max_events` | usize | 50 | Max evidence events |
| `compaction.keep_recent_messages` | usize | policy-based | Messages to retain |
| `compaction.validate` | bool | `true` | Validate invariants post-compaction |
| `compaction.preserve_evidence` | bool | `true` | Keep evidence refs in output |
| `compaction.inject_context_frame` | bool | `true` | Inject context frame after compaction |

## Compaction Invariants

All compaction must maintain:

1. **No orphan `Message::Tool`**: Every tool result must have a matching
   assistant tool call with same `tool_call_id`
2. **No orphan assistant tool-calls**: Every tool call must have all its
   tool results present
3. **Order preservation**: Relative order of tool call/result pairs
4. **Tool ID preservation**: `tool_call_id` unchanged through truncation
5. **Multi-tool preservation**: Assistant messages with multiple tool
   calls preserve all IDs and order

## Fallback Behavior

1. **Agent mode failure** → Falls back to programmatic mode
2. **Hybrid semantic checkpoint failure** → Uses programmatic-only frame
3. **Invariant validation failure** → Applies `emergency_pair_safe_compaction`
4. **Emergency fallback failure** → Preserves original messages unchanged
5. **LLM timeout** → 60s for semantic checkpoint, 120s for summarization

## Token Estimation

Delegates to `eggcontext::estimate_tokens_sync()` for token counting.
The model string selects the tokenizer (tiktoken base, model-specific
multipliers handled by the tokenizer crate).

## Integration

Called from `AgentLoop::compact_if_needed()` (`src/agent/loop.rs:1838`).
The flow is:

1. `detect_overflow()` → if over limit, `prune_tool_outputs()` runs first
2. `ContextTracker::needs_compaction()` → check threshold
3. `SessionCompacting` hook dispatched (can block)
4. If new config has `mode` set → `compact_with_policy()` (hybrid engine)
5. Otherwise → `auto_compact_async()` (legacy path)
6. Post-compaction: `build_context_frame()` + `push_control_instruction()`

## Testing

- `tests/compaction.rs` — extensive module tests
- Narrowest run:
  ```bash
  cargo test -p codegg --test compaction
  ```

## Related Docs

- [agent.md](agent.md) — AgentLoop integration, ContextTracker
- [context_frame.rs](../src/agent/context_frame.rs) — ContextFrame type
- [context-ledger.md](context-ledger.md) — artifact storage, context packing
