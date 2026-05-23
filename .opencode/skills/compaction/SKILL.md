---
name: compaction
description: Context window overflow management through intelligent compaction strategies
version: 1.0.0
tags:
  - agent
  - context
  - memory
  - tokenization
  - compaction
---

# Compaction Module Guide

The compaction module manages context window overflow by intelligently reducing conversation history while preserving critical information and tool call invariants.

## Key Types

### CompactionStrategy Enum (`src/agent/compaction.rs`)

```rust
pub enum CompactionStrategy {
    TruncateToolOutputs,  // Prune long tool outputs to ~10k tokens max
    SummarizeOldTurns,    // Use LLM to summarize older conversation turns
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

## Adaptive Strategy Selection

The `select_compaction_strategy()` function picks the best strategy based on message characteristics:

```rust
fn select_compaction_strategy(messages: &[Message]) -> CompactionStrategy {
    let non_system_count = count_non_system_messages(messages);
    let has_long_tools = has_long_tool_outputs(messages, 2000);

    if has_long_tools && non_system_count > 6 {
        CompactionStrategy::TruncateToolOutputs
    } else if non_system_count > 8 {
        CompactionStrategy::SummarizeOldTurns
    } else {
        CompactionStrategy::DropMiddleMessages
    }
}
```

**Selection criteria:**
- **TruncateToolOutputs**: When messages have long tool outputs (>2000 chars) AND there are many messages (>6)
- **SummarizeOldTurns**: When there are many messages (>8 non-system) without long tool outputs
- **DropMiddleMessages**: Default for smaller conversations

## Async LLM Summarization

The `SummarizeOldTurns` strategy uses `llm_summarize()` to generate contextual summaries:

```rust
pub async fn llm_summarize(
    messages: &[Message],
    provider: &Arc<dyn Provider>,
) -> Result<String, AppError>
```

The `compact_messages_async()` and `auto_compact_async()` functions use this strategy with proper fallback:

```rust
pub async fn auto_compact_async(
    messages: &[Message],
    context_limit: usize,
    threshold: f64,
    prune: bool,
    provider: Option<&Arc<dyn Provider>>,
) -> Vec<Message> {
    // Falls back to DropMiddleMessages if provider unavailable
    if strategy == CompactionStrategy::SummarizeOldTurns {
        if let Some(p) = provider {
            result = compact_messages_async(result, strategy, p).await;
        } else {
            result = compact_messages_sync(result, CompactionStrategy::DropMiddleMessages);
        }
    }
}
```

## Sync Version for Compatibility

For contexts where async is not available, use `auto_compact_sync()` or `compact_messages_sync()`:

```rust
pub fn compact_messages_sync(
    messages: Vec<Message>,
    strategy: CompactionStrategy,
) -> Vec<Message>
```

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

## Testing

Compaction safety tests are located in `tests/compaction.rs`. These verify:

- Message ordering is preserved after compaction
- Tool call IDs remain consistent
- No orphan tool results after compaction
- Adaptive strategy selection works correctly

## See Also

- `src/agent/loop.rs` - AgentLoop invoking compaction
- `tests/compaction.rs` - Module tests
- `.opencode/skills/agent-loop/SKILL.md` - AgentLoop integration