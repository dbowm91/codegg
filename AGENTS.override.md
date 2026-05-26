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

### Plan Review Process

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

### Subagent Context Preservation

1. **Context window limits**: Subagents undergo compaction after ~2000 lines of context
2. **Batch size**: 4-5 plan files per subagent is optimal
3. **Consolidation pattern**:
   - Subagent reads batch → writes consolidated temp file
   - Parent agent reads all temp files → creates final plan
   - This preserves subagent context for accurate summarization

---

## Verified Code Location Notes

### CoreRequest Handlers
- `CoreRequest` enum is in `src/protocol/core.rs:50-175`
- InprocCoreClient handlers at `src/core/mod.rs:52-355` handle: TurnSubmit, SessionMessagesLoad, SessionMessageCounts, SessionCreate, SessionLoad, SessionAttach, etc.
- Variants falling through to `Ack`: Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect - TUI does not send these requests, so `Ack` is intentional

### Plugin Fuel System
- Fuel reserved at `src/plugin/loader.rs:270` with `reserve_fuel()`
- Fuel returned via `module_cache::CACHE.return_fuel(plugin_id, fuel)` on ALL exit paths
- Entry point: `execute_wasm_hook()` at loader.rs:230+

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
  - Fix server bugs (permission.rs, question.rs)
  - Fix core handlers (may need investigation for dependencies)
  - Fix plugin fuel leaks

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
   - Tool count: 26 is correct (not 33+)
   - Hook types: 6 HookEvent variants (not 13)
   - Command count: doc already says 41 (not 36)
   - LSP skill count: 41 (not 42)

3. **Documentation Fix Approach**
   - Read the CURRENT architecture doc first to check if fix is needed
   - Then read the actual source code to verify counts/claims
   - Only fix if there's a real discrepancy
   - Don't trust review file claims without verification

---

*Updated 2026-05-26: Consolidated plan review learnings, added verified codebase facts from 31 review files*

---

## New Verified Codebase Facts (2026-05-26 Review Session)

| Item | Value | Location |
|------|-------|----------|
| Tool count | 26 | `src/tool/mod.rs:89-119` |
| LSP server count | 42 | `src/lsp/server.rs:27-385` |
| PermissionResponse | `{level: PermissionLevel, persist: bool}` | `src/permission/mod.rs:1142-1145` |
| InprocCoreClient fields | All wrapped in `Option<Arc<...>>` | `src/core/mod.rs:22-28` |
| ToolExecutor usage | bash, read, glob tools | `src/tool/executor.rs:72,92,112` |
| Plugin fuel logic | CORRECT - returns early when exhausted | `src/plugin/loader.rs:262-266` |
| InlineScript | Deprecated, non-functional | `src/hooks/mod.rs:180-184` |
| CommandRegistry location | Line 72 | `src/tui/command.rs:72` |
| register_panic_cleanup | Private function for temp file cleanup | `src/ide/mod.rs:65-78` |
| ProviderError::Auth | is_retryable = true | `src/error.rs:169` |
| Memory frequency_bonus | `(count - 1) * 2.0` | `src/memory/patterns.rs:232` |
| Session events published | SessionCreated, MessageAdded | `src/bus/events.rs:7,21` |
| Auth middleware | Allows requests when no token set | `src/server/middleware/auth.rs:37-39` |
| AppEvent count | 36 variants | `src/bus/events.rs:5-190` |
| Plugin dead code | `check_and_reset_fuel_budget()` never called | `src/plugin/loader.rs:24-41` |

---

## Plan Consolidation Notes (2026-05-26)

### Consolidated Plan Structure
The implementation plan at `plans/plan.md` consolidates findings from 31 module review files. Key structure:

1. **Verification Section**: Critical commands to verify claims before implementing
2. **HIGH Priority Items** (9 items): Documentation fixes and one dead code removal
3. **MEDIUM Priority Items** (15 items): Various improvements grouped by module
4. **LOW Priority Items** (4 categories): Polish work
5. **Implementation Waves**: Parallelization strategy with 9 parallel agents for Wave 1

### Key Verification Findings
Many "bugs" in review files were actually correctly implemented. Verified:
- PermissionResponse shape is `{level, persist}` not `{id, choice}`
- Auth middleware intentionally allows requests when no token configured (dev mode)
- Plugin fuel tracking logic is CORRECT (not inverted as some reviews suggested)
- LSP server count is 42 (not 39 as doc claimed)

### Wave-Based Implementation
- **Wave 1**: 9 independent agents can work in parallel (HIGH priority)
- **Wave 2**: 14 groups, each independent (MEDIUM priority)
- **Wave 3**: LOW priority polish

### Files Modified by Each Agent
Each implementation item includes `Files to Modify` column for clarity.

---

*Updated 2026-05-26: Consolidated plan review learnings, added verified codebase facts from 31 review files*