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
- Fuel returned via `module_cache::CACHE.return_fuel(plugin_id, fuel)` on clean exit paths
- All early error returns now correctly return fuel (fixed dead code at line 407)

### CoreEvent Mapping
- `map_app_event_to_core_event()` at `src/core/mod.rs` properly maps all events
- SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed now correctly mapped to CoreEvent variants
- Events mapped: TextDelta, ReasoningDelta, ToolCallStarted, ToolResult, PermissionPending, QuestionPending, AgentFinished, Error, and all Subagent events

### Permission/Question Registry Limitations
- `PermissionRegistry::pending_permission_ids()` returns IDs in format `{tool_call_id}-{tool_name}`
- Session ID is NOT encoded in permission IDs
- `get_pending_permissions_for_session()` ignores session_id parameter (returns empty - filtering not supported without extending registry)
- `get_pending_questions_for_session()` ignores session_id (returns empty - filtering not supported)

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

### TTS Keybindings
- Ctrl+Y: Toggle TTS playback
- Ctrl+Shift+Y: Stop TTS playback
- (Verified from tts.md review - may need verification against actual TUI keybinding implementation)

### Helpful Patterns for Future Agents

#### Batch Review Pattern
```
Parent Agent:
  1. Launch subagent batch 1 (4-5 plan files) → temp_consolidated_1.md
  2. Launch subagent batch 2 (4-5 plan files) → temp_consolidated_2.md
  3. Continue batches as needed
  4. Read all temp files
  5. Consolidate into final plan.md
  6. Clean up temp files
```

#### Parallel Implementation Pattern
```
Wave 1 (Parallel - 2 agents):
  - W1-1: Fix plugin fuel leaks (loader.rs:259,270,285,403)
  - W1-2: Fix CoreEvent mapping (core/mod.rs:728-797)

Wave 2 (Parallel - Documentation):
  - Each agent takes one documentation fix
  - Run 7 agents in parallel
  - Verify each compiles/builds
```

#### SessionCompacting Hook Verification
- DON'T trust claims that "dispatch_session_compacting not found in loop.rs"
- The hook IS dispatched via `dispatch_hook(ctx)` with `HookType::SessionCompacting` at loop.rs:1197-1201
- The convenience wrapper `dispatch_session_compacting()` exists but AgentLoop uses the generic method

#### Provider Auto-Registration
- Only `codegg_go` is auto-registered via `register_builtin()`
- SAP AI Core, Zenmux, Kilo, Vercel AI Gateway are config-only, NOT auto-registered
- Check `src/provider/mod.rs:register_builtin_with_config()` for details

---

## Current Active Bugs

No known active bugs. Previous issues have been resolved:
- Plugin fuel leaks: Fixed (dead code removal at loader.rs:407)
- CoreEvent mapping: Fixed (subagent events now properly mapped)
- Hash algorithm: Fixed (snapshot now uses SHA256 like checkpoint)

*(End of file)*
