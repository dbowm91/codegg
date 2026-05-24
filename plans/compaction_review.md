# Compaction Module Review

## Summary

Reviewed `architecture/compaction.md` against actual implementation at `src/agent/compaction.rs` and skill at `.opencode/skills/compaction/SKILL.md`. Overall the documentation is fairly accurate but has several discrepancies that need correction.

## Verified Items (Correct)

| Item | Status |
|------|--------|
| CompactionStrategy enum variants (TruncateToolOutputs, SummarizeOldTurns, DropMiddleMessages) | Accurate |
| TokenizerType enum with correct variants | Accurate |
| ContextTracker struct fields (current_tokens, context_limit, threshold, message_token_counts) | Accurate |
| Compaction invariants (no orphan tools, order preservation, tool ID preservation) | Accurate |
| Configuration fields (enabled, auto, prune, max_tokens, threshold, reserved, summarize_model) | Accurate |
| Integration via `AgentLoop::compact_if_needed()` in `src/agent/loop.rs:1130` | Accurate |
| HookType::SessionCompacting dispatch | Accurate |

## Discrepancies Found

### 1. TruncateToolOutputs Strategy - Different Thresholds (Bug)

**Documentation says**: "Prunes tool outputs exceeding `max_tokens_per_output` (default 10,000 tokens)" (arch doc line 17, skill line 23)

**Actual implementation**: `truncate_tool_outputs()` at `compaction.rs:306` uses `content.len() > 500` as threshold, NOT tokens. The function truncates at 500 characters, not 10,000 tokens.

The token-based pruning (10,000 tokens) is used only by `prune_tool_outputs()` at `compaction.rs:492-529`, which is a separate function used in the first-pass pruning in `compact_if_needed()`.

**Impact**: The sync `compact_messages_sync()` uses `truncate_tool_outputs()` which truncates at 500 chars, not tokens. This is a significant behavioral difference from what documentation describes.

### 2. DropMiddleMessages - Keep Count Discrepancy

**Documentation says**: "Keeps 2 messages per side (start/end of conversation)" (arch doc line 61)

**Actual implementation**: `drop_middle_messages()` at `compaction.rs:453-464`:
```rust
let keep_each_side = 2;
```

This is **correct** - keeps 2 messages per side.

### 3. SummarizeOldTurns - Processing Limit Discrepancy

**Documentation says**: "Only processes first 20 non-system messages" (skill line 63, arch doc line 57)

**Actual implementation**: `llm_summarize()` at `compaction.rs:350-355`:
```rust
let messages_to_summarize: Vec<Message> = messages
    .iter()
    .filter(|m| !matches!(m, Message::System { .. }))
    .take(20)
    .cloned()
    .collect();
```

This is **correct** - takes first 20 non-system messages.

**However**: The skill documentation also says "truncates tool content to 300 chars before summarization" which is **accurate** at `compaction.rs:391-396`.

### 4. select_compaction_strategy() - Documented vs Implementation

**Skill says** (lines 94-106):
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

**Actual implementation** at `compaction.rs:579-590`: Matches exactly. This is **correct**.

### 5. Two-Phase Pruning Not Documented

The documentation does not explain the difference between:
- `truncate_tool_outputs()` (500-char truncation, used in compaction strategies)
- `prune_tool_outputs()` (token-based, used as pre-pass in compact_if_needed)

This causes confusion about what "10,000 tokens" refers to.

### 6. Key Functions Table - Missing Functions

| Function | Status |
|----------|--------|
| `detect_overflow()` | Correct - exists at `compaction.rs:466` |
| `prune_tool_outputs()` | Correct - exists at `compaction.rs:492` |
| `compact_messages_sync()` | Correct - exists at `compaction.rs:228` |
| `compact_messages_async()` | Correct - exists at `compaction.rs:266` |
| `auto_compact_async()` | Correct - exists at `compaction.rs:623` |
| `llm_summarize()` | Correct - exists at `compaction.rs:345` |

The table is missing:
- `auto_compact()` - exists at `compaction.rs:548`
- `auto_compact_sync()` - exists at `compaction.rs:592`
- `compact_messages()` - exists at `compaction.rs:224` (wrapper for sync)

## Bugs Found in Code

### Bug 1: Inconsistent System Message in SummarizeOldTurns sync fallback

**Location**: `compaction.rs:247-256`

When `SummarizeOldTurns` is called in sync context, it adds a system message "[Previous conversation summarized for context efficiency]" BEFORE calling `drop_middle_messages()`. But `drop_middle_messages()` doesn't add such a message.

```rust
CompactionStrategy::SummarizeOldTurns => {
    tracing::warn!("SummarizeOldTurns requested in sync context, using fallback");
    let mut result = vec![Message::System {
        content: "[Previous conversation summarized for context efficiency]"
            .to_string()
            .into(),
    }];
    result.extend(drop_middle_messages(non_system));
    return result;
}
```

If messages are <= 6, `summarize_old_turns()` returns early without adding a system message, but `compact_messages_sync()` does add the placeholder. This is inconsistent.

### Bug 2: Unused `keep_count` variable in summarize_old_turns

**Location**: `compaction.rs:326`

```rust
let keep_count = 4;
```

This variable is only used at line 341:
```rust
result.extend(messages.into_iter().rev().take(keep_count).rev());
```

The naming is confusing since the documentation says "2 messages per side" for `DropMiddleMessages` but this function keeps 4.

## Recommendations

### For Documentation (architecture/compaction.md)

1. Add note explaining two-phase pruning:
   - `prune_tool_outputs()` - token-based (10k default), called first
   - `truncate_tool_outputs()` - character-based (500 chars), called within strategies

2. Update functions table to include:
   - `auto_compact()` 
   - `auto_compact_sync()`
   - `compact_messages()` wrapper

3. Clarify that `TruncateToolOutputs` strategy uses 500-char truncation in both sync and async paths

4. Add note about `SummarizeOldTurns` sync fallback behavior

### For Skill (.opencode/skills/compaction/SKILL.md)

1. Same two-phase pruning clarification needed
2. Add missing functions to table
3. The selection criteria are accurate - keep as-is

### For Code (src/agent/compaction.rs)

1. Consider adding documentation to `truncate_tool_outputs()` explaining it truncates to 500 characters
2. The sync fallback for `SummarizeOldTurns` could be made more consistent

## Files Reviewed

| File | Lines | Notes |
|------|-------|-------|
| `architecture/compaction.md` | 102 | Main arch doc |
| `src/agent/compaction.rs` | 902 | Full implementation |
| `.opencode/skills/compaction/SKILL.md` | 185 | Skill documentation |
| `src/agent/loop.rs` | 1130-1245 | Integration point |
| `src/config/schema.rs` | 369-377 | Config struct |
| `tests/compaction.rs` | 474 | Test file |

## Conclusion

The compaction module is mostly correctly documented. The main issues are:
1. Documentation of `TruncateToolOutputs` strategy doesn't distinguish between the token-based `prune_tool_outputs()` (first pass) and character-based `truncate_tool_outputs()` (second pass within strategy)
2. Missing functions in the documented table
3. Minor inconsistency in sync fallback for `SummarizeOldTurns`

No critical bugs that would cause data loss or crashes. The tool call invariants are properly maintained throughout.
