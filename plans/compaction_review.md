# Compaction Architecture Review

## Summary
The compaction architecture document is largely accurate with minor discrepancies in line number references and some stale documentation about internal behavior.

## Verified Correct
- Location: `src/agent/compaction.rs` - matches doc at line 7
- CompactionStrategy enum at lines 217-222 matches doc's listed variants (TruncateToolOutputs, SummarizeOldTurns, DropMiddleMessages)
- ContextTracker struct at lines 76-84 matches doc's field listing (current_tokens, context_limit, threshold, message_token_counts, max_messages, max_total_bytes, model)
- TokenizerType enum at lines 17-22 with multipliers (Cl100kBase=1.0, Claude=1.4, Gemini=1.2, O200kBase=1.0) matches doc
- `select_compaction_strategy()` at line 579 correctly uses thresholds: long tools && count > 6 → TruncateToolOutputs, count > 8 → SummarizeOldTurns, else DropMiddleMessages
- `prune_tool_outputs()` at line 492 uses 10_000 tokens max, protected 40_000 tokens (line 493), character hint formula max_chars = max_tokens * 4
- `detect_overflow()` at line 466 returns true when `total > context_limit.saturating_sub(reserved)` - matches doc description
- `compact_if_needed()` at `src/agent/loop.rs:1130` confirmed by grep results

## Discrepancies Found
- **Stale line reference**: Doc at line 116 says "Called from AgentLoop::compact_if_needed() in src/agent/loop.rs:1130" - this is correct, but should note the actual flow involves multiple steps in loop.rs not just the single function call
- **Hook dispatch location**: Doc at line 119 says "SessionCompacting hook is dispatched" but doesn't specify where. The hook dispatch actually happens in `src/agent/loop.rs` (not in compaction.rs), which is not explicitly documented

## Bugs Identified
- No actual bugs found - the implementation correctly follows the documented behavior

## Improvement Suggestions
- **Add hook location to docs**: The SessionCompacting hook dispatch happens in `src/agent/loop.rs` not in compaction.rs. Document the complete flow with the actual file location for hook dispatch
- **Clarify prune vs truncate**: The doc mentions `prune_tool_outputs()` (token-based ~10k) is separate from `truncate_tool_outputs()` (character-based 500). Consider adding a note that prune happens as a pre-pass before strategy selection
- **Missing `has_long_tool_outputs` threshold in doc**: The function takes a threshold parameter (2000 in practice) but doc doesn't specify what threshold is used in practice

## Stale Items in Architecture Doc
- The architecture doc is generally up-to-date with one minor note: the section on "Sync Fallback" behavior for SummarizeOldTurns at lines 68-69 could be more explicit about the placeholder message format change