# Compaction Architecture Review (2026-05-25)

## Verified Correct Items

- **Location**: `src/agent/compaction.rs` confirmed
- **CompactionStrategy enum**: `TruncateToolOutputs`, `SummarizeOldTurns`, `DropMiddleMessages` all exist and match
- **TokenizerType**: All 4 variants (Cl100kBase, Claude, Gemini, O200kBase) with correct multipliers (1.0, 1.4, 1.2, 1.0)
- **TokenizerType::for_model()**: Correct logic for detecting model type from string
- **estimate_tokens_sync()**: Uses tiktoken correctly with multiplier application
- **ContextTracker::new()**, `needs_compaction()`, `remaining_tokens()`, `current_tokens()`, `context_limit()`: All accurate
- **detect_overflow()**: Correct implementation using saturating_sub
- **prune_tool_outputs()**: Token-based truncation with 40,000 protected tokens constant
- **truncate_tool_outputs()**: Character-based truncation at 500 chars
- **llm_summarize()**: Takes first 20 non-system messages, truncates tool content to 300 chars before summarization
- **select_compaction_strategy()**: Correct thresholds (has_long_tools && non_system_count > 6 → Truncate; non_system_count > 8 → Summarize; else DropMiddle)
- **auto_compact_sync()**: Full flow accurate (prune pre-pass then strategy selection)
- **CompactionConfig in schema.rs**: All 6 fields match (`enabled`, `auto`, `prune`, `max_tokens`, `threshold`, `reserved`, `summarize_model`)
- **Integration at loop.rs:1130**: `compact_if_needed` method exists and is called from loop

## Incorrect/Stale Items

### 1. TruncateToolOutputs description conflates two distinct functions
**Doc line 17**: `"Prune long tool outputs to ~10k tokens max"`  
**Actual**: Two separate functions with different behavior:
- `prune_tool_outputs()` (line 492): Token-based, default 10,000 tokens, includes PROTECTED_TOKENS=40,000 hint
- `truncate_tool_outputs()` (line 298): Character-based at 500 chars, used within strategies

**Fix**: Update description to clarify two-phase approach.

### 2. ContextTracker struct incomplete
**Doc line 26-31** shows 4 fields but actual has 6:
- `max_messages: Option<usize>` (line 81) - MISSING
- `max_total_bytes: Option<usize>` (line 82) - MISSING
- `model: Option<String>` (line 83) - MISSING

**Fix**: Update struct definition in doc to match all 6 fields.

### 3. SummarizeOldTurns sync fallback not "silent"
**Doc line 65**: `"silently falls back to DropMiddleMessages behavior"`  
**Actual** (compaction.rs:247-256): When SummarizeOldTurns encounters sync context, it adds a system message:
```rust
let mut result = vec![Message::System {
    content: "[Previous conversation summarized for context efficiency]"
        .to_string()
        .into(),
}];
result.extend(drop_middle_messages(non_system));
return result;
```

**Fix**: Update to clarify fallback adds placeholder system message.

### 4. DropMiddleMessages "returns unchanged if total messages <= 4"
**Doc line 69**: `"Returns unchanged if total messages <= 4"`  
**Actual** (compaction.rs:454): Returns unchanged if `messages.len() <= 4` (applies to non_system messages since System messages are filtered out before this function)

**Fix**: Clarify this applies to non-system messages after filtering.

### 5. Missing functions in Key Functions table
**Doc line 82-91**: Table lists 8 functions but missing:
- `prune_tool_outputs()` - public, token-based pre-pass (line 492)
- `auto_compact_async()` - async version with provider parameter (line 623)
- `compact_messages_async()` - async LLM-based version (line 266)
- `detect_overflow()` - standalone detection function (line 466)

**Fix**: Add missing functions to table.

### 6. Hook dispatch timing slightly misleading
**Doc line 107**: `"Dispatches SessionCompacting hook before compaction"`  
**Actual** (loop.rs:1150-1155): `prune_tool_outputs()` pre-pass runs BEFORE hook dispatch (lines 1157-1204). Hook fires after pre-pass but before strategy application.

**Fix**: Clarify hook dispatches after initial prune but before strategy application.

## Bugs Found in Related Code

None - implementation appears correct.

## Line-Specific Updates Needed

| Location | Change |
|----------|--------|
| Doc line 17 | Rewrite TruncateToolOutputs description to explain two-phase approach |
| Doc line 26-31 | Add `max_messages`, `max_total_bytes`, `model` fields to ContextTracker |
| Doc line 53-59 | Clarify two-phase pruning with distinct function names |
| Doc line 65 | Update sync fallback description to mention placeholder system message |
| Doc line 69 | Clarify message count applies to non-system messages |
| Doc line 82-91 | Add `prune_tool_outputs()`, `auto_compact_async()`, `compact_messages_async()`, `detect_overflow()` |
| Doc line 107 | Revise hook timing description |