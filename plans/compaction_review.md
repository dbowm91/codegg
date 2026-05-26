# Compaction Architecture Review

**Source file**: `architecture/compaction.md`
**Code location**: `src/agent/compaction.rs`
**Test file**: `tests/compaction.rs`

## Summary

The architecture document is largely accurate. All major claims verified against source. Minor discrepancy found in one threshold description.

---

## Verified Claims

### Location ✓
- `src/agent/compaction.rs` confirmed at line 1

### CompactionStrategy Enum ✓
- Lines 217-222: Three variants match exactly (TruncateToolOutputs, SummarizeOldTurns, DropMiddleMessages)

### ContextTracker Struct ✓
- Lines 76-84: All 7 fields present (current_tokens, context_limit, threshold, message_token_counts, max_messages, max_total_bytes, model)
- Builder methods confirmed:
  - `with_max_messages()` line 99
  - `with_max_total_bytes()` line 104
  - `with_model()` line 109

### TokenizerType Enum ✓
- Lines 16-22: Four variants with multipliers (Cl100kBase=1.0, Claude=1.4, Gemini=1.2, O200kBase=1.0)

### TruncateToolOutputs Strategy ✓
- 500 character limit confirmed at line 306: `if content.len() > 500`
- `prune_tool_outputs()` is pre-pass, called BEFORE SessionCompacting hook in `compact_if_needed()` (loop.rs:1150-1152, hook dispatched at loop.rs:1198)

### SummarizeOldTurns Strategy ✓
- First 20 non-system messages: line 353 `.take(20)` after filtering System
- Tool content truncated to 300 chars: line 391-392

### DropMiddleMessages Strategy ✓
- keep_each_side = 2 confirmed at line 458
- Returns unchanged if total non-system messages <= 4: line 454-456

### Compaction Invariants ✓
- All 5 invariants documented at lines 5-14 match exactly

### Key Functions Table
| Function | Doc Line | Code Line | Status |
|----------|----------|-----------|--------|
| detect_overflow() | 88 | 466 | ✓ |
| has_long_tool_outputs() | 89 | 531 (private) | ✓ |
| count_non_system_messages() | 90 | 541 (private) | ✓ |
| select_compaction_strategy() | 91 | 579 (private) | ✓ |
| prune_tool_outputs() | 92 | 492 | ✓ |
| truncate_tool_outputs() | 93 | 298 (private) | ✓ |
| compact_messages() | 94 | 224 | ✓ |
| compact_messages_sync() | 95 | 228 | ✓ |
| compact_messages_async() | 96 | 266 | ✓ |
| auto_compact() | 97 | 548 | ✓ |
| auto_compact_sync() | 98 | 592 | ✓ |
| auto_compact_async() | 99 | 623 | ✓ |
| llm_summarize() | 100 | 345 | ✓ |

### CompactionConfig (schema.rs:369-377) ✓
All 7 fields match:
- enabled, auto, prune, max_tokens, threshold, reserved, summarize_model

### Integration with AgentLoop ✓
- `compact_if_needed()` at loop.rs:1130
- prune_tool_outputs runs BEFORE hook (loop.rs:1150-1152 before hook at 1198)

### Tests ✓
- `tests/compaction.rs` exists with 474 lines
- Comprehensive test coverage including invariant preservation tests

---

## Discrepancy Found

### select_compaction_strategy threshold (minor)
**Doc claim (line 91)**: ">6 messages with long tool outputs → TruncateToolOutputs"

**Code (lines 583-584)**:
```rust
if has_long_tools && non_system_count > 6 {
    CompactionStrategy::TruncateToolOutputs
```

**Analysis**: The condition `> 6` means count >= 7. The documentation could be interpreted as count > 6 (same behavior) but phrasing is ambiguous. Recommend clarifying "more than 6" or "7 or more" for precision.

---

## Minor Observations

1. **Function visibility**: Several functions documented in table are `pub` but helper functions like `has_long_tool_outputs`, `count_non_system_messages`, `select_compaction_strategy`, `truncate_tool_outputs` are private (no `pub`). Doc doesn't specify visibility - acceptable.

2. **Builder pattern documentation**: Correctly notes 3 builder methods exist.

---

## Conclusion

The architecture document is accurate and well-maintained. One minor threshold description ambiguity identified. No structural issues or incorrect claims.