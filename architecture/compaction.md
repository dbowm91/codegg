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
}
```

Tracks token usage and determines when compaction is needed.

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

Prunes tool outputs exceeding `max_tokens_per_output` (default 10,000 tokens). Preserves `tool_call_id` field unchanged.

### 2. SummarizeOldTurns

Uses LLM to summarize conversation turns. Falls back to `DropMiddleMessages` with placeholder if summarization unavailable. Only processes first 20 non-system messages, truncates tool content to 300 chars before summarization.

### 3. DropMiddleMessages

Keeps 2 messages per side (start/end of conversation). Returns unchanged if total messages <= 4.

## Compaction Invariants

All strategies must maintain these invariants:

1. **No orphan `Message::Tool`**: Every tool result must have a matching assistant tool call with same `tool_call_id`
2. **Order preservation**: Relative order of assistant tool calls and their matching results must be preserved
3. **Tool ID preservation**: Truncation of tool outputs must preserve `tool_call_id`
4. **Multi-tool preservation**: Assistant messages with multiple tool calls must preserve all IDs and order

## Key Functions

| Function | Description |
|----------|-------------|
| `detect_overflow()` | Returns true if current tokens exceed threshold |
| `prune_tool_outputs()` | Truncates tool outputs to max_tokens_per_output |
| `compact_messages_sync()` | Applies compaction strategies synchronously |
| `compact_messages_async()` | Applies with LLM summarization |
| `auto_compact_async()` | Auto-selects strategy based on content |
| `llm_summarize()` | Uses provider to summarize messages |

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

Called from `AgentLoop::compact_if_needed()` in `src/agent/loop.rs:1130`. Dispatches `SessionCompacting` hook before compaction.

## See Also

- `src/agent/loop.rs` - AgentLoop invoking compaction
- `tests/compaction.rs` - Module tests