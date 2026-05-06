---
name: compaction
description: Context compaction strategies and safety tests for AgentLoop
tags: [agent, compaction, context, memory]
---

# Compaction System Guide

This skill covers the compaction system in opencode-rs, which manages context window limits by reducing message history.

## Compaction Strategies

Defined in `src/agent/compaction.rs`:

```rust
#[derive(Debug, PartialEq)]
pub enum CompactionStrategy {
    TruncateToolOutputs,  // Truncates tool result content to ~500 chars
    SummarizeOldTurns,    // Uses LLM to summarize old turns (async)
    DropMiddleMessages,   // Removes middle messages, keeping first and last pairs
}
```

### Strategy Selection (Adaptive)

The `select_compaction_strategy()` function picks the best strategy:

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

## Key Functions

### compaction.rs Functions

```rust
// Sync version - uses placeholder summary
pub fn compact_messages_sync(
    messages: Vec<Message>,
    strategy: CompactionStrategy,
) -> Vec<Message>

// Async version - uses LLM summarization
pub async fn compact_messages_async(
    messages: &[Message],
    strategy: CompactionStrategy,
    provider: &Arc<dyn Provider>,
) -> Vec<Message>

// Auto-compact with adaptive strategy selection (sync)
pub fn auto_compact(
    messages: &[Message],
    context_limit: usize,
    threshold: f64,
    prune: bool,
) -> Vec<Message>

// Auto-compact with adaptive strategy selection (async)
pub async fn auto_compact_async(
    messages: &[Message],
    context_limit: usize,
    threshold: f64,
    prune: bool,
    provider: Option<&Arc<dyn Provider>>,
) -> Vec<Message>

// Prune long tool outputs
pub fn prune_tool_outputs(messages: &[Message], max_len: usize) -> Vec<Message>
```

### Integration with AgentLoop

The `compact_if_needed()` function in `src/agent/loop.rs`:
- Checks if context needs compaction
- Calls appropriate compaction function
- Dispatches hook events via `plugin_service.dispatch_hook()`

## ContextTracker

Tracks token usage and triggers compaction when needed:

```rust
pub struct ContextTracker {
    context_limit: usize,
    threshold: f64,
    current_tokens: AtomicUsize,
}

impl ContextTracker {
    pub fn new(context_limit: usize, threshold: f64) -> Self;
    pub fn add_message(&mut self, msg: &Message);
    pub fn needs_compaction(&self) -> bool;
    pub fn needs_overflow_protection(&self, overflow_buffer: usize) -> bool;
}
```

## Compaction Safety Tests (Packet 8)

Located in `tests/compaction.rs`:

### Key Test Patterns

**Truncating preserves IDs** (Packet 8):
```rust
#[test]
fn test_truncate_tool_output_preserves_id() {
    let messages = vec![
        Message::Tool { tool_call_id: Arc::new("test_tool_call_1".to_string()), ... },
    ];
    let result = compact_messages(messages, CompactionStrategy::TruncateToolOutputs);
    // Verify tool_call_id is preserved after truncation
}
```

**Drop-middle preserves tool pairs** (Packet 8):
```rust
#[test]
fn test_drop_middle_preserves_tool_pairs() {
    let result = compact_messages(messages, CompactionStrategy::DropMiddleMessages);
    // Check no orphan tools: every tool has matching assistant
    for m in &result {
        if let Message::Tool { tool_call_id, .. } = m {
            assert!(assistant_tool_ids.contains(&tool_call_id.as_ref().to_string()));
        }
    }
}
```

**Summarization no orphan results** (Packet 8):
```rust
#[test]
fn test_summarization_fallback_no_orphans() {
    let result = compact_messages(messages, CompactionStrategy::SummarizeOldTurns);
    // Falls back to placeholder if no provider
    // Verify no orphan tool results
}
```

**Multiple tool calls preserve order** (Packet 8):
```rust
#[test]
fn test_multiple_tool_calls_preserve_ids_order() {
    let truncated = compact_messages(messages, CompactionStrategy::TruncateToolOutputs);
    // Check IDs preserved in order: ["tc1", "tc2"]
}
```

## Compaction Invariants

1. **Assistant tool calls must have matching Tool results** in subsequent messages
2. **Tool result `tool_call_id` must match** a prior assistant `tool_calls[].id`
3. **Assistant message must come before** its corresponding Tool result
4. **No orphan tool results** - every Tool message needs a prior Assistant with matching tool_call

## Related Skills

- See `.opencode/skills/agent-loop/SKILL.md` for AgentLoop integration
- See `.opencode/skills/provider/SKILL.md` for provider transcript tests with compaction
