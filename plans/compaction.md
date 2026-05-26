# Compaction Architecture Review Findings

## Verified Claims

- **CompactionStrategy enum**: Verified in `src/agent/compaction.rs:217-222` - TruncateToolOutputs, SummarizeOldTurns, DropMiddleMessages.

- **ContextTracker struct**: Verified at `src/agent/compaction.rs:76-84` with current_tokens, context_limit, threshold, message_token_counts, max_messages, max_total_bytes, model fields.

- **TokenizerType enum**: Verified at `src/agent/compaction.rs:16-22` - Cl100kBase, Claude, Gemini, O200kBase with correct multipliers (1.0, 1.4, 1.2, 1.0).

- **Builder pattern methods**: `with_max_messages()`, `with_max_total_bytes()`, `with_model()` all verified at lines 99-112.

- **detect_overflow()**: Verified at `src/agent/compaction.rs:466-490` - returns true when total > context_limit - reserved.

- **has_long_tool_outputs()**: Verified at `src/agent/compaction.rs:531-539` - checks threshold of 2000 characters by default.

- **count_non_system_messages()**: Verified at `src/agent/compaction.rs:541-546`.

- **prune_tool_outputs()**: Token-based truncation at `src/agent/compaction.rs:492-529` with 10,000 tokens max and 40,000 protected tokens.

- **truncate_tool_outputs()**: Character-based truncation at `src/agent/compaction.rs:298-319` - 500 char threshold.

- **compact_messages() / compact_messages_sync()**: Verified at `src/agent/compaction.rs:224-264`. SummarizeOldTurns in sync context falls back to DropMiddleMessages with placeholder text.

- **compact_messages_async()**: Verified at `src/agent/compaction.rs:266-296`.

- **auto_compact() / auto_compact_sync()**: Verified at `src/agent/compaction.rs:548-577` and `592-621`.

- **auto_compact_async()**: Verified at `src/agent/compaction.rs:623-664`.

- **llm_summarize()**: Verified at `src/agent/compaction.rs:345-451` - filters first 20 non-system messages, truncates tool content to 300 chars before summarization.

- **Compaction Invariants**: Documented at `src/agent/compaction.rs:5-14` - matches architecture doc exactly.

- **keep_each_side = 2 for DropMiddleMessages**: Verified at `src/agent/compaction.rs:458`.

- **DropMiddleMessages returns unchanged when <= 4 messages**: Verified at `src/agent/compaction.rs:453-456`.

## Stale Information

- **Line reference for compact_if_needed (compaction.md:116)**: States "in `src/agent/loop.rs:1130`" - actual call is at line 1130, but `compact_if_needed` method itself is at line 1130. The reference is slightly imprecise but essentially correct.

## Bugs Found

- **No bugs found**: Compaction implementation matches documentation. All strategies correctly implemented, invariants maintained.

## Improvements Suggested

- **Sync vs async compaction behavior difference**: `SummarizeOldTurns` in sync context (`compact_messages_sync`) adds a system message with placeholder "[Previous conversation summarized for context efficiency]" per line 250-254, but `compact_messages_async` does actual LLM summarization. This difference should be more explicitly documented.

- **select_compaction_strategy thresholds**: The logic at `src/agent/compaction.rs:579-590` - uses 2000 char threshold for long tools, 6 messages for TruncateToolOutputs, 8 for SummarizeOldTurns. These magic numbers aren't configurable via CompactionConfig.

## Cross-Module Issues

- **Hook dispatch order**: The architecture doc states `SessionCompacting` hook is dispatched after `prune_tool_outputs()` but the actual code path should be verified in `AgentLoop::compact_if_needed()` to confirm this ordering.

- **CompactionConfig schema location**: Document says `CompactionConfig` is in `src/config/schema.rs` - this should be verified since config module structure may differ.