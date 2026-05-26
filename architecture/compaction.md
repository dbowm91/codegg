# Compaction Module

The compaction module manages context window overflow by intelligently reducing conversation history while preserving critical information and tool call invariants.

## Overview

**Location**: `src/agent/compaction.rs`

**Purpose**: When the context window approaches capacity, compaction strategies reduce token count while preserving tool call/output pairs and conversation structure.

## Key Types

### CompactionStrategy Enum

```rust
pub enum CompactionStrategy {
    TruncateToolOutputs,  // Prune long tool outputs to ~10k tokens max
    SummarizeOldTurns,     // Use LLM to summarize older conversation turns
    DropMiddleMessages,   // Drop middle messages keeping start/end
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

Tracks token usage and determines when compaction is needed. Supports builder pattern for configuration:
- `with_max_messages(max)` - Set maximum message count limit
- `with_max_total_bytes(max)` - Set maximum total bytes limit  
- `with_model(model)` - Set model for tokenizer selection

### TokenizerType

```rust
pub enum TokenizerType {
    Cl100kBase,  // GPT-4/3.5 (1.0x multiplier)
    Claude,      // Claude models (1.4x multiplier)
    Gemini,      // Gemini models (1.2x multiplier)
    O200kBase,   // Newer models (1.0x multiplier)
}
```

Model-specific token multipliers for accurate estimation.

## Compaction Strategies

### 1. TruncateToolOutputs

Character-based truncation of tool outputs exceeding 500 characters. Used by `compact_messages_sync()` and `compact_messages_async()` when this strategy is selected.

Note: `prune_tool_outputs()` is a separate pre-pass function (not a strategy) that performs token-based truncation (~10,000 tokens max). It is called BEFORE the `SessionCompacting` hook is dispatched, reducing large tool outputs first to minimize data passed to hooks.

Preserves `tool_call_id` field unchanged.

### 2. SummarizeOldTurns

Uses LLM to summarize conversation turns. Falls back to `DropMiddleMessages` with placeholder if summarization unavailable. Only processes first 20 non-system messages, truncates tool content to 300 chars before summarization.

**Sync Fallback**: When `SummarizeOldTurns` encounters an error (LLM unavailable, timeout, etc.), it silently falls back to `DropMiddleMessages` behavior rather than returning an error. This ensures compaction always succeeds in sync mode.

### 3. DropMiddleMessages

Drops middle messages, keeping `keep_each_side = 2` messages on each end of the conversation (start/end). Returns unchanged if total non-system messages <= 4 (meaning 2 per side is the maximum drop, so nothing to drop).

## Compaction Invariants

All strategies must maintain these invariants:

1. **No orphan `Message::Tool`**: Every tool result must have a matching assistant tool call with same `tool_call_id`
2. **Order preservation**: Relative order of assistant tool calls and their matching results must be preserved
3. **Tool ID preservation**: Truncation of tool outputs must preserve `tool_call_id`
4. **Multi-tool preservation**: Assistant messages with multiple tool calls must preserve all IDs and order

## Key Functions

| Function | Description |
|----------|-------------|
| `detect_overflow()` | Returns true if current tokens exceed context_limit - reserved |
| `has_long_tool_outputs()` | Checks if any tool output exceeds 2000 character threshold |
| `count_non_system_messages()` | Counts messages that are not System messages |
| `select_compaction_strategy()` | Selects strategy based on message count and tool output length |
| `prune_tool_outputs()` | Token-based truncation to max_tokens_per_output (pre-pass) |
| `truncate_tool_outputs()` | Character-based truncation to 500 chars (within strategies) |
| `compact_messages()` | Wrapper around `compact_messages_sync` (sync-only context) |
| `compact_messages_sync()` | Apply strategy to messages (synchronous, no LLM) |
| `compact_messages_async()` | Apply strategy with LLM summarization (async, requires provider) |
| `auto_compact()` | Wrapper around `auto_compact_sync` |
| `auto_compact_sync()` | Auto-select strategy with pruning (sync fallback) |
| `auto_compact_async()` | Auto-select strategy with optional LLM summarization (async) |
| `llm_summarize()` | Uses provider to summarize messages (async) |

## Configuration

Via `CompactionConfig` in `src/config/schema.rs`:

- `enabled`: Enable/disable compaction
- `auto`: Automatic compaction when approaching limit
- `prune`: Use TruncateToolOutputs strategy
- `max_tokens`: Maximum tokens in context
- `threshold`: 0.1-1.0, trigger compaction when tokens exceed threshold
- `reserved`: Reserved token buffer (default 10,000)
- `summarize_model`: Model to use for summarization

## Integration

Called from `AgentLoop::compact_if_needed()`. The flow is:
1. `prune_tool_outputs()` runs first (token-based pre-pass, BEFORE hook)
2. `SessionCompacting` hook is dispatched
3. If hook is not blocked, compaction strategy is selected and applied

## See Also

- `src/agent/loop.rs` - AgentLoop invoking compaction
- `tests/compaction.rs` - Module tests