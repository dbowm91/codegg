# Agent Module Architecture Review

**Date**: 2026-05-24
**Reviewed Files**: `architecture/agent.md`, `src/agent/` (loop.rs, mod.rs, worker.rs, compaction.rs, router.rs, task.rs, processor.rs, team.rs, teams.rs, prompt.rs, mention.rs), `.opencode/skills/agent-loop/SKILL.md`

## Summary

The architecture document at `architecture/agent.md` is **largely accurate** and reflects the actual implementation. Most types, structs, and methods match their implementations. However, there are some minor discrepancies and one significant bug found.

---

## Verified Correct Items

### AgentLoop Struct (loop.rs:548-571)
- All documented fields match the actual implementation
- Field types are correct: `agents: HashMap<String, Agent>`, `provider: Box<dyn crate::provider::Provider>`, etc.
- `file_change_rx: tokio::sync::broadcast::Receiver<AppEvent>` field present (documented at line 46)

### AgentLoopState (loop.rs:523-530)
- All documented fields exist: `current_agent`, `turn_count`, `total_tokens`, `start_time`, `plan_mode`, `plan_topic`

### ExecutionLimits (loop.rs:532-546)
- Struct matches documentation with default values of max_turns=100, max_tokens=1_000_000, timeout=600s

### Compaction Types (compaction.rs)
- `CompactionStrategy` enum with all three variants: `TruncateToolOutputs`, `SummarizeOldTurns`, `DropMiddleMessages`
- `ContextTracker` struct with all documented fields and methods
- `detect_overflow()`, `prune_tool_outputs()`, `compact_messages_sync()`, `compact_messages_async()`, `auto_compact_async()`, `llm_summarize()` all exist

### Router Types (router.rs)
- `ModelRouter` struct with documented fields
- `TaskComplexity` enum with `Simple`, `Medium`, `Complex` variants
- `classify()`, `route_model()`, `is_enabled()` methods exist

### Worker Types (worker.rs)
- `SubAgentPool` struct fields match documentation (including `active_handles: Arc<TokioMutex<Vec<tokio::task::JoinHandle<()>>>>`)
- `SubAgentRequest` and `SubAgentResult` match documentation
- `SubAgentSpawner` exists with `send()` and `send_async()` methods

### Task Types (task.rs)
- `BackgroundTask` struct with documented fields
- `BackgroundScheduler` struct matches documentation
- Methods `add()`, `tick()`, `spawn_loop()`, `load_tasks()`, `save_task()` exist

### Processor (processor.rs)
- `EventProcessor` struct exists with all documented fields
- Methods `process()`, `reset()`, `text()`, `tool_calls()`, `stop_reason()` exist

### Team Coordination (team.rs, teams.rs)
- `Team`, `TeamMessage`, `AgentRole`, `MessageStatus` types match documentation
- File-based inbox communication implemented

### Events Published (loop.rs)
- `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` events published via `GlobalEventBus::publish()` (worker.rs:472, 485, 516, 531)
- `TextDelta`, `ReasoningDelta`, `ToolCallStarted`, `ToolResult`, `AgentFinished` events published in `stream_once()` (loop.rs:854-879)

---

## Discrepancies Found

### 1. `run_with_prompt()` Missing from Documentation

**Status**: The architecture doc mentions `run_with_prompt()` as a convenience method (line 52) but does not list it in the Key Methods section.

**Actual Implementation**: `run_with_prompt()` exists at loop.rs:2072-2103.

**Recommendation**: Add `run_with_prompt()` to the Key Methods list in the documentation.

---

### 2. `run()` Method Signature Inconsistency

**Architecture Doc** (line 39): `pub async fn run(&mut self, request: ChatRequest) -> Result<Vec<ChatEvent>, AppError>`

**Actual Implementation** (loop.rs:1248): `pub async fn run(&mut self, mut request: ChatRequest) -> Result<Vec<ChatEvent>, AppError>`

The `mut` keyword on `request` is an implementation detail not shown in the docs. This is minor and acceptable.

---

### 3. BackgroundScheduler `spawn_loop()` Uses `task.id.parse()` Not `rand::random()`

**Documentation** (agent.md line 177): States "BackgroundScheduler now uses `task.id` for `task_id` in SubAgentRequest (was using `rand::random()` before)"

**Actual Implementation** (task.rs:228):
```rust
task_id: task.id.parse().unwrap_or_else(|_| rand::random::<u64>()),
```

**Verification**: This is correctly documented. The fallback to `rand::random()` is reasonable for malformed IDs.

---

### 4. `start_workers()` Removed - Correctly Documented

**Documentation** (agent.md line 146): States "Note: `start_workers()` method was removed (was a dead no-op method)"

**Actual Verification**: No `start_workers()` method exists in `SubAgentPool`. Workers are started in constructors (`new()` and `new_with_store()`) via `start_worker_loop()` at worker.rs:124 and 175.

Correctly documented.

---

## Bugs Found

### Bug 1: `spawn_loop()` Uses `rand::random()` as Fallback (task.rs:228)

**Location**: `src/agent/task.rs:228`

**Issue**: When `task.id.parse()` fails, the code falls back to `rand::random::<u64>()` instead of propagating the error. This could cause task ID mismatch issues when the background scheduler retrieves results from the task store.

**Code**:
```rust
task_id: task.id.parse().unwrap_or_else(|_| rand::random::<u64>()),
```

**Impact**: If a task has a malformed ID string that fails parsing, it will get a random task_id, which won't match the original task's ID in the task store. This could lead to orphaned tasks or failed results not being recorded.

**Recommendation**: Either:
1. Log a warning and skip the task if ID parsing fails
2. Return an error from `spawn_loop()` if task ID is invalid

---

## Documentation Improvements Needed

### 1. Missing `drain_follow_up()` Method

The method `drain_follow_up()` at loop.rs:1916-2070 is not documented in the architecture. It handles queued follow-up prompts and is critical to the follow-up contract documented in the skill file.

**Recommendation**: Add documentation for `drain_follow_up()` and its non-blocking behavior.

---

### 2. Missing Snapshot Methods

The methods `capture_snapshot_if_needed()` and `capture_incremental_snapshot_if_needed()` at loop.rs:1559-1624 are not documented.

**Recommendation**: Add documentation for snapshot capture functionality.

---

### 3. Missing `drain_file_change_events()` Method

This method at loop.rs:1578-1594 drains file change events from the broadcast channel. Not documented.

**Recommendation**: Add to architecture doc.

---

### 4. `tool_def_cache` Type Documentation

The `ToolDefCache` type alias at loop.rs:60-67 is not explicitly documented in the architecture. It should be documented as part of the tool definition caching system.

---

## Verified Correct Implementation Notes

The following items from the "Known Implementation Notes" section (architecture/agent.md lines 272-280) were verified:

1. **Subagent event publishing** - Events properly published via `GlobalEventBus`
2. **`SubAgentPool` bounded concurrency** - Semaphore with default of 5, RAII guard pattern
3. **Tool definition caching** - Cache key uses `mcp_tool_count` and `permission_version`
4. **DoomLoop detection** - Window-based counting, code at loop.rs:411-412
5. **ToolExecuteBefore/After hooks** - Both invoked at loop.rs:1764 and 1806
6. **BackgroundScheduler task_id** - Uses `task.id.parse()` (with fallback)
7. **`start_workers()` removed** - Confirmed not present

---

## Skill File Review (.opencode/skills/agent-loop/SKILL.md)

The skill file is comprehensive and largely accurate. Verified:
- AgentLoop struct fields match implementation
- Provider trait documentation accurate
- Message types with Arc<String> correctly documented
- GlobalEventBus pattern correct
- Permission flow diagram accurate
- QuestionRegistry pattern correct
- Follow-up contract correctly documented

**Minor issue**: The skill file shows `Arc<String>` usage but doesn't mention that when creating messages, you should use `.into()` to convert. This is mentioned in the skill but could be more prominent.

---

## Overall Assessment

| Category | Status |
|----------|--------|
| AgentLoop struct | Accurate |
| AgentLoopState | Accurate |
| ExecutionLimits | Accurate |
| Compaction types | Accurate |
| Router types | Accurate |
| Worker/SubAgent types | Accurate |
| Task/Background types | Accurate |
| Processor | Accurate |
| Team coordination | Accurate |
| Events published | Accurate |
| Configuration section | Accurate |

**Verdict**: The architecture document is **largely accurate** with only minor discrepancies. One bug was found in `task.rs:228` where `rand::random()` is used as a fallback for task ID parsing.

---

## Recommendations

### High Priority
1. **Fix bug in `task.rs:228`**: Add proper error handling for invalid task IDs instead of using random fallback

### Medium Priority
2. **Add `drain_follow_up()` to documentation**
3. **Add snapshot methods to documentation**
4. **Add `run_with_prompt()` to Key Methods list**

### Low Priority
5. **Rename `Message` in `team.rs`** to avoid confusion with provider's `Message`
6. **Add doc comments to helper functions** in loop.rs
