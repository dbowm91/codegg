# Compaction Architecture Review

## Architecture Document
- Path: **DOES NOT EXIST** - `architecture/compaction.md` is missing
- The compaction module is briefly documented in `architecture/agent.md` (lines 60-74)
- Reference to `.opencode/skills/compaction/SKILL.md` exists but file is missing

## Source Code Location
- Primary: `src/agent/compaction.rs` (902 lines)
- Config schema: `src/config/schema.rs` (CompactionConfig at line 369)
- Integration: `src/agent/loop.rs` (`compact_if_needed()` at line 1130)
- Tests: `tests/compaction.rs` (474 lines)

## Verification Summary
**Partial** - No architecture document exists to verify. The source code is well-implemented with tests, but documentation is incomplete.

## Verified Claims

| Claim | Status | Notes |
|-------|--------|-------|
| `CompactionStrategy` enum with 3 variants | **Pass** | TruncateToolOutputs, SummarizeOldTurns, DropMiddleMessages all present |
| `ContextTracker` for token tracking | **Pass** | Tracks current_tokens, context_limit, threshold, message_token_counts |
| `detect_overflow()` function | **Pass** | Returns bool based on context_limit - reserved |
| `prune_tool_outputs()` function | **Pass** | Prunes tool outputs to max_tokens_per_output (default 10k) |
| `compact_messages_sync()` function | **Pass** | Sync compaction with 3 strategies |
| `compact_messages_async()` function | **Pass** | Async compaction with LLM summarization |
| `auto_compact_async()` function | **Pass** | Auto-compact with adaptive strategy selection |
| `llm_summarize()` function | **Pass** | LLM-based summarization via provider.stream() |
| `TokenizerType` with model detection | **Pass** | Claude (1.4x), Gemini (1.2x), Cl100kBase (1.0x), O200kBase (1.0x) multipliers |
| `CompactionConfig` with fields | **Pass** | enabled, auto, prune, max_tokens, threshold, reserved, summarize_model |
| System messages preserved during compaction | **Pass** | `compact_messages_*` always keeps system messages at start |
| Tool call IDs preserved in truncation | **Pass** | `truncate_tool_outputs` only modifies content, preserves tool_call_id |
| Hook integration (SessionCompacting) | **Pass** | `loop.rs:1158-1204` dispatches hook before compaction |
| Tests exist | **Pass** | 18 tests in compaction.rs, 13 tests in src/agent/compaction.rs |

## Issues Found

### Missing Documentation
1. **No architecture/compaction.md** - The dedicated architecture document does not exist
2. **No .opencode/skills/compaction/SKILL.md** - The skill documentation referenced in agent.md does not exist
3. **Compaction invariants not documented** - The "Compaction Invariant" comment at lines 5-14 in compaction.rs is implementation guidance but should be in docs

### Inconsistencies
1. **`drop_middle_messages` behavior** - Keeps 2 messages per side (`keep_each_side = 2`), but no sliding window of recent context. With small message counts (<=4), returns unchanged.
2. **`truncate_tool_outputs` threshold** - Hardcoded 500 character truncation limit (`if content.len() > 500`), while `prune_tool_outputs` uses 10,000 token limit
3. **Sync vs Async summarization** - `SummarizeOldTurns` in sync context falls back to `DropMiddleMessages` with a placeholder system message, but this fallback is not well documented
4. **`llm_summarize` limits** - Only processes first 20 non-system messages, truncates tool content to 300 chars before summarization
5. **Strategy selection thresholds** - `select_compaction_strategy` uses `has_long_tools && non_system_count > 6` for TruncateToolOutputs, `non_system_count > 8` for SummarizeOldTurns, else DropMiddleMessages - these magic numbers are undocumented

### Improvement Opportunities
1. **Configuration validation** - `CompactionConfig` validation only checks `threshold` (0.1-1.0) and `max_tokens` (>=1000). `reserved` should have validation (currently defaults to 10_000 but has no min check)
2. **Hardcoded values should be configurable**:
   - `PROTECTED_TOKENS` (40,000) in `prune_tool_outputs`
   - Truncation limits (500 for truncate_tool_outputs, 300 for llm_summarize, 10,000 for prune)
   - Strategy selection thresholds (6, 8 messages)
3. **Missing `compact_if_needed` documentation** - The main entry point in loop.rs is not documented
4. **No mention of `time_compacting` session field** - `SessionState` has `time_compacting` but its purpose is unclear

## Recommendations

1. **Create architecture/compaction.md** - Document the compaction system thoroughly including:
   - Overview of all compaction strategies
   - Token estimation and ContextTracker behavior
   - Flow diagram showing when/how compaction triggers
   - Configuration options
   - Invariants that must be maintained

2. **Create .opencode/skills/compaction/SKILL.md** - Create skill documentation matching other modules

3. **Document strategy selection** - Explain the logic in `select_compaction_strategy` or make it configurable

4. **Add validation for `reserved`** - Currently no min value check; 0 would cause divide-by-zero in some calculations

5. **Consider making truncation limits configurable** - The hardcoded values (500, 300, 10,000, 40,000) should be documented or configurable

6. **Document the async compaction flow** - When `SummarizeOldTurns` is used with a provider, explain the full flow including model selection from `summarize_model` config

## Summary

The compaction implementation is solid with good test coverage, but documentation is severely lacking. There is no `architecture/compaction.md` document, and the referenced skill file `.opencode/skills/compaction/SKILL.md` does not exist. The code itself is correct and well-structured with proper invariants documented in comments, but external documentation is missing entirely.
