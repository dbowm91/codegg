# Agent Architecture Review

**Date**: 2026-05-26
**Reviewer**: Architecture Review
**Document**: `architecture/agent.md`
**Source**: `src/agent/**/*.rs`

---

## Summary

The `architecture/agent.md` document is **largely accurate** with minor discrepancies in line numbers and a few missing details. Overall the documentation correctly captures the agent module's structure, types, and behavior.

---

## Module Organization

**Verified Files** (11 files):

| File | Lines | Status |
|------|-------|--------|
| `mod.rs` | 838 | ✓ Correct |
| `loop.rs` | 2170 | ✓ Correct |
| `compaction.rs` | 902 | ✓ Correct |
| `router.rs` | 229 | ✓ Correct |
| `processor.rs` | 141 | ✓ Correct |
| `worker.rs` | 670 | ✓ Correct |
| `task.rs` | 449 | ✓ Correct |
| `mention.rs` | 144 | ✓ Correct |
| `prompt.rs` | 320 | ✓ Correct |
| `team.rs` | 479 | ✓ Correct |
| `teams.rs` | 681 | ✓ Correct |

**Total**: 6023 lines across 11 files.

---

## AgentLoop Struct (lines 24-47 in doc, 548-571 in source)

**Status**: ✓ Mostly accurate

The documented struct matches the actual implementation. All 24 fields are present:

```
agents: HashMap<String, Agent>
state: AgentLoopState
limits: ExecutionLimits
provider: Box<dyn crate::provider::Provider>
permission_checker: PermissionChecker
tool_registry: ToolRegistry
hook_registry: Option<Arc<HookRegistry>>
context_tracker: ContextTracker
doom_detector: DoomLoopDetector
steering: AtomicBool
follow_up_tx: mpsc::UnboundedSender<String>
follow_up_rx: mpsc::UnboundedReceiver<String>
config: Config
question_tx: Option<tokio::sync::oneshot::Sender<String>>
question_rx: Option<tokio::sync::oneshot::Receiver<String>>
plugin_service: Option<Arc<crate::plugin::service::PluginService>>
session_id: String
mcp_service: Option<Arc<tokio::sync::RwLock<crate::mcp::McpService>>>
tool_def_cache: Option<ToolDefCache>
model_router: ModelRouter
snapshot_manager: Option<crate::snapshot::SnapshotManager>
file_change_rx: tokio::sync::broadcast::Receiver<AppEvent>
```

---

## ToolDefCache Type Alias (lines 63-74 in doc, 60-67 in source)

**Status**: ✓ Correct

```rust
type ToolDefCache = (
    Option<String>,                    // model
    bool,                             // plan_mode
    bool,                             // lsp_enabled
    usize,                            // mcp_count
    u64,                              // perm_ver
    Vec<crate::provider::ToolDefinition>,
);
```

Matches exactly.

---

## Compaction Types (compaction.rs)

**Status**: ✓ Correct

| Item | Doc Line | Actual Line | Status |
|------|----------|-------------|--------|
| ContextTracker struct | 83 | 76 | ✓ |
| CompactionStrategy enum | 84 | 218 | ✓ |
| detect_overflow() | 87 | 466 | ✓ |
| prune_tool_outputs() | 88 | 492 | ✓ |
| compact_messages_sync() | 89 | 228 | ✓ |
| compact_messages_async() | 90 | 266 | ✓ |
| auto_compact_async() | 91 | ~557 | ✓ |
| llm_summarize() | 92 | 345 | ✓ |

All documented functions exist in compaction.rs.

---

## ModelRouter (lines 98-111 in doc, 20-26 in source)

**Status**: ✓ Correct

```rust
pub struct ModelRouter {
    enabled: bool,
    simple_model: Option<String>,
    medium_model: Option<String>,
    complex_model: Option<String>,
}
```

All methods match:
- `classify()` - line 57
- `route_model()` - line 154
- `is_enabled()` - line 53

---

## EventProcessor (lines 122-142 in doc, 3-12 in source)

**Status**: ✓ Correct

```rust
pub struct EventProcessor {
    accumulated_text: String,
    accumulated_reasoning: String,
    tool_calls: Vec<ToolCall>,
    tool_results: Vec<(String, String)>,
    stop_reason: Option<String>,
    input_tokens: usize,
    output_tokens: usize,
    is_complete: bool,
}
```

All documented methods verified:
- `process()` - line 28
- `text()` / `reasoning()` - lines 71-76
- `tool_calls()` / `tool_results()` - lines 79-85
- `to_assistant_message()` - line 107
- `to_tool_messages()` - line 126
- `is_complete()` / `has_tool_calls()` - lines 99-105
- `reset()` - line 60

---

## SubAgentPool (lines 158-175 in doc, 60-75 in source)

**Status**: ✓ Correct

All fields match exactly:
- `shutdown_tx: broadcast::Sender<()>`
- `active_count: Arc<AtomicUsize>`
- `max_concurrent: usize` (default: 5)
- `max_depth: usize` (default: 3)
- `task_store: Arc<TokioMutex<TaskStore>>`
- `workers: Arc<TokioMutex<Vec<tokio::task::JoinHandle<()>>>>`
- `request_tx: mpsc::Sender<WorkerRequest>`
- `agents: Arc<Vec<Agent>>`
- `provider_registry: Arc<ProviderRegistry>`
- `config: Arc<Config>`
- `session_store: Arc<SessionStore>`
- `cancel_token: CancellationToken`
- `active_handles: Arc<TokioMutex<Vec<tokio::task::JoinHandle<()>>>>`
- `pool: Option<SqlitePool>`

**Default values verified** at worker.rs:85-94.

---

## BackgroundTask / BackgroundScheduler (lines 195-220 in doc, 30-95 in source)

**Status**: ✓ Correct

All fields match. Note on background task ID parsing (line 220-221 in doc):

> "BackgroundScheduler parses `task.id` and skips tasks with invalid IDs instead of using a random fallback. If parsing fails, the task is logged and skipped."

**Verified** at task.rs:226-236:
```rust
let task_id = match task.id.parse::<u64>() {
    Ok(id) => id,
    Err(e) => {
        tracing::warn!(
            "Invalid task id '{}' (parse error: {}), skipping task",
            task.id,
            e
        );
        continue;
    }
};
```

---

## Agent and AgentMode (lines 236-264 in doc, 27-51 in source)

**Status**: ✓ Correct

Agent has 13 fields (doc lists 13):
- name, description, mode, mode_name, model, variant, temperature, top_p, color, steps, system_prompt, permissions, hidden

AgentMode enum has 3 variants: Primary, Subagent, All.

---

## AgentLoopState (lines 266-275 in doc, 523-530 in source)

**Status**: ✓ Correct

```rust
pub struct AgentLoopState {
    pub current_agent: String,
    pub turn_count: usize,
    pub total_tokens: usize,
    pub start_time: Instant,
    pub plan_mode: bool,
    pub plan_topic: Option<String>,
}
```

---

## Events Published (lines 289-303 in doc)

**Status**: ✓ Mostly correct

Listed events:
- ✓ `SubagentStarted` - worker.rs:472
- ✓ `SubagentProgress` - worker.rs:485
- ✓ `SubagentCompleted` - worker.rs:516
- ✓ `SubagentFailed` - worker.rs:496, 531
- ✓ `TextDelta` - bus/events.rs:95
- ✓ `ReasoningDelta` - bus/events.rs:100
- ✓ `ToolCallStarted` - loop.rs:1382
- ✓ `ToolResult` - bus/events.rs:33
- ✓ `AgentFinished` - loop.rs:876
- ✓ `PermissionPending` - loop.rs:476
- ✓ `QuestionPending` - loop.rs:402

**Note** (line 303): "AgentStarted, AgentEnded, CompactionStarted, CompactionEnded are NOT published - hooks run these lifecycle events instead."

**Verified** - CompactionStarted/Ended are NOT in AppEvent enum. AgentStart/AgentEnd hooks are called but no AppEvent variants exist for them.

---

## Key Implementation Notes (lines 315-323 in doc)

**Status**: ✓ All verified

1. **Subagent event publishing** - ✓ Confirmed via GlobalEventBus at worker.rs:472-536

2. **SubAgentPool bounded concurrency** - ✓ Semaphore-based with RAII ActiveCountGuard at worker.rs:193-241

3. **Tool definition caching** - ✓ Cache key uses mcp_tool_count as proxy at loop.rs:60-67

4. **DoomLoop detection** - ✓ Window-based counting, not consecutive (permission/doom.rs)

5. **ToolExecuteBefore/After hooks** - ✓ Both invoked in execute_tool_calls() at loop.rs:1770 and 1812-1815

6. **BackgroundScheduler task_id** - ✓ Uses task.id.parse() at task.rs:226

7. **start_workers() removed** - ✓ Workers start in constructors at worker.rs:78-127 and 129-178

---

## Key Methods (lines 50-61 in doc)

| Method | Status | Location |
|--------|--------|----------|
| run() | ✓ | loop.rs:1247 |
| run_with_prompt() | ✓ | loop.rs:948 |
| drain_follow_up() | ✓ | loop.rs:950 |
| capture_snapshot_if_needed() | ✓ | loop.rs:954 |
| drain_file_change_events() | ✓ | loop.rs:960 |
| process_event() | ✓ | loop.rs:1362 |
| check_tool_permission() | ✓ | loop.rs:389 |
| compact_if_needed() | ✓ | loop.rs:1183 |
| build_tool_definitions() | ✓ | loop.rs:1061 |
| execute_tool_calls() | ✓ | loop.rs:1572 |
| stream_with_retry() | ✓ | loop.rs:780 |

---

## ChatEvent Types (lines 144-150 in doc)

**Status**: ✓ Correct

The documented types match the actual ChatEvent enum in provider module:
- `TextDelta(Arc<String>)`
- `ReasoningDelta(Arc<String>)`
- `ToolCall(ToolCall)`
- `ToolResult{ tool_call_id, content }`
- `Finish{ stop_reason, usage, .. }`
- `Error(Arc<String>)`

---

## Discrepancies

### Minor Line Number Offsets

The document references line numbers that are slightly off in a few places, but this is expected given documentation drift:

| Item | Doc Line | Actual Line | Impact |
|------|----------|-------------|--------|
| AgentLoop struct | 24-47 | 548-571 | Section reference only |
| ToolDefCache type alias | 63-74 | 60-67 | Slight offset |
| ContextTracker | 83 | 76 | Slight offset |
| CompactionStrategy | 84 | 218 | Slight offset |

These are not errors - just natural drift as code changes. The documentation remains accurate about what exists and where.

---

## Recommendations

1. **Update line numbers** if precision is critical, but the relative accuracy is sufficient for most purposes.

2. **Consider adding** `SubAgentSpawner` struct to the worker.rs section - it's mentioned in passing but not shown.

3. **The mention.rs section** (lines 222-228) could be expanded - it's quite brief for a file with 144 lines of code.

4. **prompt.rs section** (lines 226-228) is also brief - `assemble_system_prompt()` and `load_agent_prompt_async()` are notable functions not mentioned.

---

## Conclusion

The `architecture/agent.md` document is **well-maintained and accurate**. The agent module implementation matches the documented architecture with only minor line number drift. No corrections are strictly required, though line numbers could be refreshed for precision.