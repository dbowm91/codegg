# Compaction Module

The compaction module manages context window overflow by intelligently reducing conversation history while preserving critical information and tool call invariants.

## Overview

**Location**: `src/agent/compaction.rs`

**Purpose**: When the context window approaches capacity, compaction strategies reduce token count while preserving tool call/output pairs and conversation structure.

## Compaction Modes

Three modes control how compaction is performed:

| Mode | Description |
|------|-------------|
| `Programmatic` | Deterministic: builds evidence index, extracts state, retains recent messages. No LLM calls. |
| `Agent` | LLM-driven: runs `semantic_checkpoint` to fill semantic fields (constraints, decisions, etc.). Falls back to programmatic on failure. |
| `Hybrid` (default) | Programmatic extraction + optional semantic enrichment. Uses LLM to fill semantic fields, merges with programmatic frame. Falls back gracefully on LLM failure. |

## Compaction Policies

Policies tune aggressiveness:

| Policy | Max Tool Output Tokens | Keep Recent | Max Summary Tokens |
|--------|----------------------|-------------|-------------------|
| `Conservative` | 2000 | 8 | 1200 |
| `Balanced` (default) | 1000 | 4 | 800 |
| `Cheap` | 500 | 2 | 400 |
| `Emergency` | 200 | 1 | 200 |
| `LosslessDebug` | MAX | 999 | 2000 |

## Configuration

### Config Schema (`src/config/schema.rs`)

```rust
pub struct CompactionConfig {
    pub enabled: Option<bool>,
    pub auto: Option<bool>,
    pub mode: Option<CompactionModeConfig>,       // programmatic | agent | hybrid
    pub policy: Option<CompactionPolicyConfig>,   // conservative | balanced | cheap | emergency | lossless_debug
    pub prune: Option<bool>,
    pub max_tokens: Option<usize>,
    pub threshold: Option<f64>,
    pub reserved: Option<usize>,
    pub model: Option<String>,              // preferred compaction model
    pub summarize_model: Option<String>,    // legacy alias (fallback for model)
    pub max_tool_output_tokens: Option<usize>,
    pub max_summary_tokens: Option<usize>,
    pub max_events: Option<usize>,
    pub keep_recent_messages: Option<usize>,
    pub validate: Option<bool>,
    pub preserve_evidence: Option<bool>,
    pub inject_context_frame: Option<bool>,
}
```

### Resolved Config (`ResolvedCompactionConfig`)

`ResolvedCompactionConfig::from_config()` resolves all `Option` fields to concrete values with defaults, applying policy-based defaults for budgets.

### Model Resolution Order

`compaction.model` -> `summarize_model` -> `active_model`

The first non-None value wins. If none is set, `ResolvedCompactionConfig::default()` uses `None` (compaction_model defaults to `None`).

## Key Types

### CompactionStrategy Enum (Legacy)

```rust
pub enum CompactionStrategy {
    TruncateToolOutputs,
    SummarizeOldTurns,
    DropMiddleMessages,
}
```

### ContextTracker

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

Tracks token usage and determines when compaction is needed. Supports builder pattern for configuration.

### ProgrammaticCompactionState

```rust
pub struct ProgrammaticCompactionState {
    pub frame: ContextFrame,
    pub evidence: Vec<EvidenceRef>,
    pub retained_message_indices: Vec<usize>,
    pub diagnostics: Vec<CompactionDiagnostic>,
}
```

### EvidenceRef

```rust
pub struct EvidenceRef {
    pub id: String,
    pub kind: EvidenceKind,  // UserMessage, AssistantMessage, ToolCall, ToolResult, TestRun, etc.
    pub summary: String,
    pub content_hash: Option<String>,
}
```

### CompactionInput / CompactionOutput

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

## Key Functions

### Primary Entry Point

| Function | Description |
|----------|-------------|
| `compact_with_policy(input, provider)` | Main compaction entry. Builds programmatic state, applies mode (programmatic/agent/hybrid), validates invariants, returns `CompactionOutput`. |

### Programmatic Reducers

| Function | Description |
|----------|-------------|
| `build_programmatic_state(messages, config)` | Builds evidence index, extracts commands/files/test results/errors/constraints, selects retained messages. |
| `build_evidence_index(messages)` | Creates `Vec<EvidenceRef>` with content hashes for all messages. |
| `collect_tool_pairs(messages)` | Maps assistant tool calls to their tool results. |
| `extract_commands(tool_pairs)` | Extracts salient commands (cargo test, git diff, etc.). |
| `extract_file_paths(messages, tool_pairs)` | Extracts file paths from tool args and message text. |
| `extract_test_and_error_state(tool_pairs)` | Extracts test results and error lines. |
| `extract_user_constraints(messages)` | Extracts user constraints from keywords (must, do not, etc.). |
| `select_retained_messages(messages, state, policy, keep_recent)` | Selects message indices to retain, preserving tool call/result pairs. |

### Semantic Checkpoint (Phase 4)

| Function | Description |
|----------|-------------|
| `semantic_checkpoint(reduced, retained, provider, model, max_tokens)` | Asks LLM to fill semantic fields (constraints, decisions, unresolved_errors, next_steps) from reduced programmatic state. |
| `merge_frames(base, semantic)` | Merges semantic frame into programmatic frame. Semantic fields override only when non-empty. |

### Invariant Validation (Phase 2)

| Function | Description |
|----------|-------------|
| `validate_message_invariants(messages)` | Checks for orphan tool results and missing tool results. Returns `CompactionInvariantError`. |
| `emergency_pair_safe_compaction(messages, config)` | Fallback that groups messages into tool-pairs and retains recent pairs. |

### Compilation

| Function | Description |
|----------|-------------|
| `compile_programmatic_messages(original, state, config)` | Builds final message list: system messages + control text + retained messages. |
| `compile_hybrid_messages(original, state, frame, config)` | Same as programmatic but uses merged frame for control text. |

### Legacy Functions

| Function | Description |
|----------|-------------|
| `compact_messages_sync(messages, strategy)` | Apply strategy synchronously (no LLM). |
| `compact_messages_async(messages, strategy, provider, model)` | Apply strategy with LLM summarization. |
| `auto_compact_sync(messages, limit, threshold, prune)` | Auto-select strategy with pruning. |
| `auto_compact_async(messages, limit, threshold, prune, provider, model)` | Auto-select with optional LLM. |
| `prune_tool_outputs(messages, max_tokens)` | Token-based pre-pass truncation. |
| `prune_tool_outputs_rich(messages, max_tokens, policy)` | Rich pruning with head/salient/tail line selection and content hashing. |

## Compaction Invariants

All compaction must maintain:

1. **No orphan `Message::Tool`**: Every tool result must have a matching assistant tool call with same `tool_call_id`
2. **No orphan assistant tool-calls**: Assistant messages with tool calls must have all their tool results present
3. **Order preservation**: Relative order of assistant tool calls and their matching results must be preserved
4. **Tool ID preservation**: Truncation of tool outputs must preserve `tool_call_id`
5. **Multi-tool preservation**: Assistant messages with multiple tool calls must preserve all IDs and order

## Fallback Behavior

1. **Agent mode failure** -> Falls back to programmatic mode
2. **Hybrid semantic checkpoint failure** -> Uses programmatic-only frame (no enrichment)
3. **Invariant validation failure** -> Applies `emergency_pair_safe_compaction`
4. **Emergency fallback failure** -> Preserves original messages unchanged
5. **LLM timeout** -> 60s for semantic checkpoint, 120s for summarization

## Example Configurations

```jsonc
// Default hybrid with balanced policy
{
  "compaction": {
    "mode": "hybrid",
    "policy": "balanced"
  }
}

// Programmatic-only (no LLM calls)
{
  "compaction": {
    "mode": "programmatic",
    "policy": "cheap"
  }
}

// Agent mode with specific model and conservative policy
{
  "compaction": {
    "mode": "agent",
    "policy": "conservative",
    "model": "claude-sonnet-4-20250514",
    "keep_recent_messages": 10
  }
}

// Emergency mode for tight contexts
{
  "compaction": {
    "mode": "hybrid",
    "policy": "emergency",
    "max_tool_output_tokens": 100,
    "max_summary_tokens": 100
  }
}

// Debug mode (preserve everything)
{
  "compaction": {
    "mode": "hybrid",
    "policy": "lossless_debug"
  }
}
```

## Integration

Called from `AgentLoop::compact_if_needed()`. The flow is:
1. `prune_tool_outputs()` runs first (token-based pre-pass, BEFORE hook)
2. `SessionCompacting` hook is dispatched
3. If hook is not blocked, `compact_with_policy()` is called
4. Invariants are validated; emergency fallback if needed

## See Also

- `src/agent/loop.rs` - AgentLoop invoking compaction
- `src/config/schema.rs` - CompactionConfig, CompactionModeConfig, CompactionPolicyConfig
- `src/agent/context_frame.rs` - ContextFrame type
- `tests/compaction.rs` - Module tests
