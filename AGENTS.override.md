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

## Notes from Architecture Review Session (2026-05-26)

### General Lessons
1. **Always verify documentation claims against actual code** before accepting them as truth
2. **Counts in documentation drift** - verify tool counts, command counts, event counts against source
3. **Use subagents for batch review work** with 4-5 files per subagent to avoid context compaction
4. **Client backoff is (1s, 2s, 4s)** not (2s, 4s) as some docs claim based on `2u64.saturating_pow((attempt - 1) as u32)`
5. **.skills/ directory is documentation-only** - runtime only loads from `~/.config/codegg/skills/` and `.codegg/skills/`
6. **Permission route is /submit** not /:session_id despite what some docs show

### Critical Code Findings
- ToolDefCache at `src/agent/loop.rs:60-67` is `(Option<String>, bool, bool, usize, u64, Vec<ToolDefinition>)` - contains `lsp_enabled` field
- 39 built-in commands (not 41) at `src/tui/command.rs:79-161`
- 34 AppEvent variants (not 36) at `src/bus/events.rs:5-147`
- timeline_visible and timeline_selected are in `App` struct, NOT `UiState` at `src/tui/app/mod.rs:232-233`
- Snapshot hash inconsistency confirmed - MD5 used at mod.rs:431, SHA256 elsewhere

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

The following issues were identified during architecture review (2026-05-26):

### High Priority - Code Bugs
| Bug | Location | Description |
|-----|----------|-------------|
| Snapshot hash inconsistency | `src/snapshot/mod.rs:431` | Uses MD5 for non-empty files, SHA256 elsewhere |
| ToolExecutor unused | `src/tool/executor.rs:8` | Exists with retry logic but never integrated |
| Static cache never clears | `src/security/sandbox.rs:237` | `CANONICAL_PATHS_CACHE` never invalidates |

### Medium Priority - Design Issues
| Bug | Location | Description |
|-----|----------|-------------|
| TTS stop() silent failure | `src/tts/mod.rs:85-103` | Returns `Ok(())` even when `pkill` fails |
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | Silently accepts non-None providers |
| Histogram unbounded | `src/util/metrics.rs:122-124` | Only limits per name, not unique names count |
| Worktree symlink issue | `src/worktree/mod.rs:69-88` | Canonicalization may fail with symlinks |

### Low Priority - Dead Code
| Bug | Location | Description |
|-----|----------|-------------|
| PermissionResponse unused | `src/permission/mod.rs:1141-1145` | Struct defined but never used internally |
| check_external_directory unused | `src/permission/mod.rs:1237-1248` | Marked `#[allow(dead_code)]` |

### Documentation Staleness (Non-Bugs)
These are documentation issues, not code bugs:
- Event count in bus.md says 36, actual is 34
- Command count in command.md says 41, actual is 39
- Backoff formula in client.md says (2s, 4s), actual is (1s, 2s, 4s)
- Subagent events documentation claims NOT mapped, but they ARE mapped
- .skills/ directory description implies runtime loading, but it doesn't load from there

*(End of file)*
