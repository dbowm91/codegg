# Compaction & Config & Core Architecture Review

## Verified Claims

### Compaction Module (`architecture/compaction.md`)

| Claim | Status | Location |
|-------|--------|----------|
| `CompactionStrategy` enum with 3 variants | VERIFIED | `src/agent/compaction.rs:217-222` |
| `ContextTracker` struct with all 7 fields | VERIFIED | `src/agent/compaction.rs:76-84` |
| `TokenizerType` enum with 4 variants | VERIFIED | `src/agent/compaction.rs:17-22` |
| `select_compaction_strategy()` logic: 7+ non-system msgs with long tool outputs → TruncateToolOutputs; >8 non-system → SummarizeOldTurns | **INCORRECT** | `src/agent/compaction.rs:581-591` |

**Correct `select_compaction_strategy` logic (code):**
```rust
fn select_compaction_strategy(messages: &[Message]) -> CompactionStrategy {
    let non_system_count = count_non_system_messages(messages);
    let has_long_tools = has_long_tool_outputs(messages, 2000);

    if has_long_tools && non_system_count > 6 {  // >6 (NOT >=7)
        CompactionStrategy::TruncateToolOutputs
    } else if non_system_count > 8 {  // >8 (NOT >=9)
        CompactionStrategy::SummarizeOldTurns
    } else {
        CompactionStrategy::DropMiddleMessages
    }
}
```

**Doc says:** "7 or more messages with long tool outputs → TruncateToolOutputs; more than 8 messages → SummarizeOldTurns"

**Code says:** "> 6" (= 7 or more) for TruncateToolOutputs, "> 8" (= 9 or more) for SummarizeOldTurns

The logic is effectively the same, but the documentation is technically incorrect in its inequality representation.

| Claim | Status | Location |
|-------|--------|----------|
| `prune_tool_outputs()` called BEFORE `SessionCompacting` hook | VERIFIED | `src/agent/loop.rs:1214-1219` |
| `SessionCompacting` hook dispatched | VERIFIED | `src/agent/loop.rs:1265` |
| `truncate_tool_outputs()` character-based truncation to 500 chars | VERIFIED | `src/agent/compaction.rs:306` |
| `drop_middle_messages()` keeps 2 messages per side | VERIFIED | `src/agent/compaction.rs:460` |
| `drop_middle_messages()` returns unchanged if total non-system <= 4 | VERIFIED | `src/agent/compaction.rs:456-458` |
| `SummarizeOldTurns` truncates tool content to 300 chars before summarization | VERIFIED | `src/agent/compaction.rs:391-396` |
| `llm_summarize()` processes first 20 non-system messages | VERIFIED | `src/agent/compaction.rs:353` |

### Config Module (`architecture/config.md`)

| Claim | Status | Location |
|-------|--------|----------|
| `Config` struct with all 35+ fields | VERIFIED | `src/config/schema.rs:22-64` |
| `ProviderConfig` struct with all listed fields | VERIFIED | `src/config/schema.rs:167-180` |
| `ProviderConfig::api_key()` method checks env vars first | VERIFIED | `src/config/schema.rs:183-205` |
| `ServerConfig::merge()` method | VERIFIED | `src/config/schema.rs:133-163` |
| `ProviderConfig::merge()` method | VERIFIED | `src/config/schema.rs:207-244` |
| Discovery order: CODEGG_TUI_CONFIG, system, global, project | VERIFIED | `src/config/paths.rs:12-38` |
| `ConfigWatcher` struct with all fields | VERIFIED | `src/config/watcher.rs:12-21` |
| Master key lookup order: CODEGG_MASTER_KEY, CODEGG_ENCRYPTION_KEY, OPENCODE_ENCRYPTION_KEY | VERIFIED | `src/config/encryption.rs:5-10` |
| `decrypt_provider_keys()` called in `Config::load()` | VERIFIED | `src/config/schema.rs:545` |
| `decrypt_provider_keys()` called in `ConfigWatcher::reload_config()` | VERIFIED | `src/config/watcher.rs:163` |
| `medium_model` validation added | VERIFIED | `src/config/schema.rs:631-638` |
| `tool_timeout_seconds` validation: 0=invalid, >3600=invalid | VERIFIED | `src/config/schema.rs:704-710` |
| `max_parallel_tools` validation: 0=invalid, >100=invalid | VERIFIED | `src/config/schema.rs:712-718` |
| `compaction.threshold` validation: 0.1-1.0 | VERIFIED | `src/config/schema.rs:723-729` |
| `compaction.max_tokens` validation: at least 1000 | VERIFIED | `src/config/schema.rs:731-735` |

### Core Module (`architecture/core.md`)

| Claim | Status | Location |
|-------|--------|----------|
| `CoreClient` trait with `request()` and `subscribe()` | VERIFIED | `src/core/mod.rs:13-20` |
| `InprocCoreClient` has 4 fields wrapped in `Option<Arc<T>>` | VERIFIED | `src/core/mod.rs:22-28` |
| `StdioCoreClient` spawns `codegg core-stdio` | VERIFIED | `src/core/transport/stdio.rs:18-47` |
| `SocketCoreClient` connects to Unix socket | VERIFIED | `src/core/transport/socket.rs:18-27` |
| `subscribe()` returns empty receiver for stdio/socket clients | VERIFIED | `src/core/transport/stdio.rs:88-91`, `src/core/transport/socket.rs:126-129` |
| `InprocCoreClient::subscribe()` reads from GlobalEventBus | VERIFIED | `src/core/mod.rs:705-728` |
| Protocol version is 1 | VERIFIED | `src/protocol/core.rs:3` |
| TurnCancel, TurnSteer, AgentSelect, ModelSelect fall through to `Ack` | VERIFIED | `src/core/mod.rs:701` |
| Snapshot events not mapped via `map_app_event_to_core_event` | VERIFIED | `src/core/mod.rs:845` (catch-all `_ => None`) |

## Incorrect/Stale Claims

### Compaction

1. **Line 91**: "7 or more messages" should be "more than 6 messages" to match code `> 6`
2. **Line 91**: "more than 8 messages" should be "more than 8 messages" (correct)

### Core

1. **Line 37**: "Contains 4 fields" - actually `InprocCoreClient` has exactly 4 fields as documented, but the specific field names are not listed in the doc (subagent_pool, memory_store, bg_scheduler, pool).

## Bugs Found

### Core - SubagentPool field access issue

In `src/core/mod.rs:607-618`, the code clones `pool` from `self.subagent_pool` but then tries to access `pool.task_store()` and `pool.spawner()`. However, `self.subagent_pool` is `Option<Arc<SubAgentPool>>`, so `pool` would be `SubAgentPool` not a reference to the pool that has these methods. This appears to be a bug:

```rust
if let Some(pool) = self.subagent_pool.clone() {
    let task_tool = crate::tool::task::TaskTool::new(
        pool.task_store(),    // pool is SubAgentPool, should work
        Some(pool.spawner()), // pool is SubAgentPool, should work
        ...
    );
```

Wait, actually `pool` is `SubAgentPool` after cloning the `Arc`, and `SubAgentPool` has `task_store()` and `spawner()` methods. So this is likely correct.

Actually, let me reconsider. The code does:
```rust
if let Some(pool) = self.subagent_pool.clone() {
    let task_tool = crate::tool::task::TaskTool::new(
        pool.task_store(),
        Some(pool.spawner()),
```

`self.subagent_pool` is `Option<Arc<SubAgentPool>>`. If we clone it, we get `Arc<SubAgentPool>`. Then `pool` is `Arc<SubAgentPool>`, and `Arc<SubAgentPool>` has `task_store()` and `spawner()` methods (inherited from `SubAgentPool`). So this is correct.

### Compaction - TurnCancel/TurnSteer/AgentSelect/ModelSelect handling

The documentation says these fall through to `Ack`. However, looking at the code at `src/core/mod.rs:701`, the catch-all `_ => Ok(CoreResponse::Ack)` handles all unmatched variants including these. This is correct.

## Stale References

None found - the documentation is generally up to date.

## Improvements Identified

### Compaction Documentation

1. The table on lines 85-100 could be more precise about async variants. The documentation lists both `compact_messages()` (wrapper) and `compact_messages_sync()`, but in the source code `compact_messages()` simply delegates to `compact_messages_sync()`. Consider clarifying.

2. Line 59: "Character-based truncation of tool outputs exceeding 500 characters" - while this is correct, it doesn't mention that this is within `truncate_tool_outputs()` which is used by `compact_messages_sync()` when `TruncateToolOutputs` strategy is selected.

3. The documentation doesn't mention that `auto_compact()` (line 550) and `auto_compact_sync()` (line 594) are nearly identical - both exist but `auto_compact_sync` is the actual implementation. `auto_compact` just wraps `auto_compact_sync`. This seems like unnecessary duplication.

### Config Documentation

1. The "Dead tui_config code removed" section (lines 247-249) is stale. The issue was fixed in 2026-05-22, but the document should be updated to reflect this is historical, not an active issue.

2. The document lists `merge_configs()` under paths.rs key functions but doesn't fully explain the HashMap field behavior. For providers, agents, mcp, commands, and modes, the documentation says "full replace for agents/mcp/commands/modes" but the actual behavior is more nuanced - providers use field-level merging (via `ProviderConfig::merge()`), while agents/mcp/commands/modes use full replace.

3. The `merge_configs()` function at `src/config/paths.rs:164-284` has a special case: `provider` HashMap entries are merged field-by-field using `ProviderConfig::merge()`, not replaced entirely. This is correctly implemented but not documented.

### Core Documentation

1. Line 37: "Contains 4 fields" could list the actual field names for clarity.

2. Line 197: "The in-process client subscribes to the GlobalEventBus" is correct, but could mention that the subscribe loop runs in a spawned task.

3. The documentation states snapshot events are "not published" via `map_app_event_to_core_event`, but doesn't explain why or what the implications are.

## Recommendations

### High Priority

1. **Fix compaction docs inequality representation**: Change "7 or more" to "more than 6" in line 91 of `architecture/compaction.md` to accurately represent `> 6`.

### Medium Priority

1. **Document ProviderConfig::merge behavior in config.md**: The actual merge behavior for providers is field-level merging, not full replace. Update the merge_configs description.

2. **Document auto_compact duplication**: Either remove `auto_compact()` wrapper or document why both exist.

3. **Fix "Dead tui_config code removed" section**: Mark as historical (2026-05-22) rather than active issue.

### Low Priority

1. Consider adding actual field names to "InprocCoreClient" description in core.md line 37.

2. Consider explaining why snapshot events are not mapped in the documentation.
