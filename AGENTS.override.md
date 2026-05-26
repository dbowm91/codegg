# AGENTS.override.md

## Implementation Patterns

### Working with Git Worktrees for Parallel Changes

When implementing code changes in parallel using worktrees:
1. Create worktree with `git worktree add <path> -b <branch-name>`
2. Work in the worktree for compilation safety
3. Commit changes in the worktree
4. Switch to main and reset/rebase to incorporate changes
5. Use `git rm --cached <worktree-path>` to avoid embedded repo issues
6. Clean up worktree directory manually after pruning

**Critical**: Git worktrees have nested `.git` directories that can cause "embedded git repository" warnings and issues with `git add -A`. Always clean up properly.

---

## Session Learnings

### Verification Before Assumption
- Initial review files may contain incorrect claims about bugs
- Always verify claims against actual code before marking as "bug"
- Many "bugs" turn out to be correct implementation after direct inspection
- Example: Memory superseding threshold was correctly `>` not `>=`

### Documentation vs Implementation
- Documentation often lags behind code changes
- When a review says "X is wrong", check if it's been fixed since the review
- Architecture docs can become stale even while code is correct

---

## Verified Code Location Notes

### CoreRequest Handlers
- `CoreRequest` enum is in `src/protocol/core.rs:50-175`
- InprocCoreClient handlers at `src/core/mod.rs:52-355` handle: TurnSubmit, SessionMessagesLoad, SessionMessageCounts, SessionCreate, SessionLoad, SessionAttach, etc.
- Variants falling through to `Ack`: Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect - TUI does not send these requests, so `Ack` is intentional

### Plugin Fuel System
- Fuel reserved at `src/plugin/loader.rs:245` with `reserve_fuel()`
- Fuel returned via `module_cache::CACHE.return_fuel(plugin_id, fuel)` on CLEAN exit paths
- **BUG**: Early error returns at lines 259, 270, 285 do NOT call `return_fuel()` - causes fuel leaks
- Entry point: `execute_wasm_hook()` at loader.rs:230+

### CoreEvent Mapping
- `map_app_event_to_core_event()` at `src/core/mod.rs:728-797` drops most events via `_ => None`
- **BUG**: SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed NOT mapped
- Events that ARE mapped: TextDelta, ReasoningDelta, ToolCallStarted, ToolResult, PermissionPending, QuestionPending, AgentFinished, Error
- CoreEvent has SubagentStarted (core.rs:244) and SubagentCompleted (core.rs:256) variants but they never receive data

### Permission/Question Registry Limitations
- `PermissionRegistry::pending_permission_ids()` returns IDs in format `{tool_call_id}-{tool_name}`
- Session ID is NOT encoded in permission IDs
- `get_pending_permissions_for_session()` at permission.rs:65 ignores session_id parameter (returns empty - filtering not supported without extending registry)
- `get_pending_questions_for_session()` at question.rs:60 ignores session_id (returns empty - filtering not supported)

### TUI Event Handling
- `handle_remote_event()` is at `src/tui/app/mod.rs:794`, NOT in client module
- This is important for architecture docs about event flow

### IdeServer Async I/O
- `run_stdio()` uses `tokio::io::stdin()/stdout()` with `AsyncBufReadExt` and `AsyncWriteExt`
- NOT blocking `std::io`

### TUI Theme Count
- `src/tui/theme.rs:8` comment correctly says "31 built-in themes" (verified 2026-05-26)

### TUI Dialog State
- Always instantiated: `model_dialog`, `agent_dialog`, `session_dialog`, `tree_dialog`, `command_palette`
- On-demand (Option<T>): all others including `theme_picker`, `question_dialog`, `permission_dialog`, `keybind_dialog`, `mcp_dialog`

---

## Helpful Patterns for Future Agents

### Batch Review Pattern
```
Parent Agent:
  1. Launch subagent batch 1 (4-5 plan files) → temp_consolidated_1.md
  2. Launch subagent batch 2 (4-5 plan files) → temp_consolidated_2.md
  3. Continue batches as needed
  4. Read all temp files
  5. Consolidate into final plan.md
  6. Clean up temp files
```

### Parallel Implementation Pattern
```
Phase 1 (Sequential - Code Bugs):
  - Fix plugin fuel leaks (loader.rs:259,270,285)
  - Fix CoreEvent mapping (core/mod.rs:728-797)

Phase 2 (Parallel - Documentation):
  - Each agent takes 5-6 modules
  - Run 6 agents in parallel
  - Verify each module compiles/builds
```

---

## Key Insights from Review Sessions

1. **Only confirmed documentation count error was TUI theme count** - comment said 42 but only 31 themes defined

2. **Many review claims were WRONG** - current code was already correct:
   - Dialog count: 21 is CORRECT (not 22/23)
   - LSP language count: 41 is correct (not 44+)
   - Tool count: 26 is CORRECT (not 33+)
   - Hook types: 6 HookEvent variants (not 13)
   - Command count: doc already says 41 (not 36)
   - UiState: All documented fields ARE present (tts, tts_enabled, fullscreen, dirty_regions, render_panic_count, last_render_error)

3. **Documentation Fix Approach**
   - Read the CURRENT architecture doc first to check need before fixing
   - Then read the actual source code to verify counts/claims
   - Only fix if there's a real discrepancy
   - Don't trust review file claims without verification

---

## Current Active Bugs

| Bug | Location | Description |
|-----|----------|-------------|
| Plugin fuel leaks | `src/plugin/loader.rs:255-285` | `module_cache::CACHE.return_fuel()` not called on early exits at lines 259, 270, 285 |
| CoreEvent mapping | `src/core/mod.rs:728-797` | Subagent* events dropped via `_ => None` in `map_app_event_to_core_event` |

*(End of file)*
