# AGENTS.override.md

## Session Learnings (2026-05-25)

### Review Process Improvements

1. **Batch Processing for Plan Reviews**
   - When reviewing multiple plan files, process in batches of 4-5 to avoid subagent context compaction
   - Consolidate each batch into a temporary file, then consolidate those files
   - This prevents losing context during long review sessions

2. **Verification Before Assumption**
   - Initial review files may contain incorrect claims about bugs
   - Always verify claims against actual code before marking as "bug"
   - Many "bugs" turn out to be correct implementation after direct inspection
   - Example: Memory superseding threshold was correctly `>` not `>=`

3. **Documentation vs Implementation**
   - Documentation often lags behind code changes
   - When a review says "X is wrong", check if it's been fixed since the review
   - Architecture docs can become stale even while code is correct

### Plan Organization

1. **Wave-based Parallelization**
   - Group independent items into waves for parallel execution
   - Wave 1 items (code bugs) should be done first
   - Wave 2+ items (documentation) can be done in parallel by different agents
   - Mark dependencies explicitly

2. **Accurate Status Tracking**
   - Many items initially flagged as "pending" were actually already fixed
   - Plan should accurately reflect current state, not historical claims
   - Use "PASS" verification before including items in plan

### Command Module Bug (COMPLETED 2026-05-25)

```rust
// Fixed - src/command/mod.rs:20-25
pub async fn find_command_files(base: &Path) -> Vec<Command> {
    find_command_files_sync(base)
        .into_iter()
        .filter_map(|r| r.ok())
        .collect()
}
```

### Documentation Corrections (COMPLETED 2026-05-25)

1. **Overview architecture**: Fixed to 13 components, 20 dialogs
2. **MCP architecture**: Added `heartbeat_token` and `heartbeat_cancellation` fields to McpConnectionManager
3. **Core architecture**: Added explicit CoreRequest variants enumeration
4. **LSP architecture**: Server count fixed to 39 (was 44)
5. **Config architecture**: `decrypt_provider_keys()` line fixed to 163
6. **Command architecture**: Built-in command count fixed to 41 (was 36)

### Subagent Context Preservation

1. **Context window limits**: Subagents undergo compaction after ~2000 lines of context
2. **Batch size**: 4-5 plan files per subagent is optimal
3. **Consolidation pattern**:
   - Subagent reads batch → writes consolidated temp file
   - Parent agent reads all temp files → creates final plan
   - This preserves subagent context for accurate summarization