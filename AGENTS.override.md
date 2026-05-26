# AGENTS.override.md

## Session Learnings (2026-05-26)

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

## Verified Code Location Notes (2026-05-26)

### CoreRequest Handlers
- `CoreRequest` enum is in `src/protocol/core.rs:50-175`
- InprocCoreClient handler at `src/core/mod.rs:698` handles most variants but has catch-all `Ack`
- **Already implemented**: TurnSubmit, Session* variants, Memory*, Task*, Worktree*, PermissionRespond, QuestionRespond, ModelsRefresh
- **Falls through to Ack** (not truly unimplemented - these are TUI-side): Initialize, Subscribe, Resume, TurnCancel, TurnSteer, AgentSelect, ModelSelect
- The `Ack` response may be intentional for some variants that are handled elsewhere

### Plugin Fuel System
- Fuel reserved at `src/plugin/loader.rs:270` with `reserve_fuel()`
- Fuel returned via `module_cache::CACHE.return_fuel(plugin_id, fuel)`
- **BUG**: Early returns at lines 352, 360-363, 374-377, 390-394 don't return fuel
- Entry point: `execute_wasm_hook()` at loader.rs:230+

### Permission/Question Registry Limitations
- `PermissionRegistry::pending_permission_ids()` returns IDs in format `{tool_call_id}-{tool_name}`
- Session ID is NOT encoded in permission IDs
- `get_pending_permissions_for_session()` at permission.rs:65 ignores session_id parameter (line 70 comment confirms this is by design)
- `get_pending_questions_for_session()` at question.rs:60 has faulty filter comparing IDs directly to session_id

### TUI Event Handling
- `handle_remote_event()` is at `src/tui/app/mod.rs:794`, NOT in client module
- This is important for architecture docs about event flow

### IdeServer Async I/O
- `run_stdio()` uses `tokio::io::stdin()/stdout()` with `AsyncBufReadExt` and `AsyncWriteExt`
- NOT blocking `std::io` as older docs may claim

### Theme Count Bug (Verified)
- `src/tui/theme.rs:8` comment says "42 built-in themes" 
- But only 31 ThemeData entries actually exist in THEMES array (lines 102-630)
- This is the only confirmed doc count error to fix

### TUI Dialog State (Verified)
- `tree_dialog` and `command_palette` are always instantiated (not on-demand as docs say)
- `help_dialog` and `info_dialog` are on-demand (not optional as docs say)
- Corresponds to `src/tui/app/state/dialog.rs`

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

## Findings from 2026-05-26 Plan Review Session

### Verified Bugs (Confirmed by Direct Code Inspection)

1. **BUG-01** (permission.rs:27): Session ID mismatch check is broken - splits on wrong format
2. **BUG-02** (permission.rs:65-90): `get_pending_permissions_for_session()` ignores session_id
3. **BUG-03** (question.rs:63-73): `get_pending_questions_for_session()` filter compares IDs to session_id
4. **BUG-04-08** (core/mod.rs:698): Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect fall through to Ack
5. **BUG-09** (loader.rs:344-354): Fuel not returned when hook function not found
6. **BUG-10** (loader.rs:356-409): Fuel not returned on 4 early error paths

### Claims in Review Files that Were WRONG

Many review files claimed documentation was wrong, but current code was already correct:

1. **Dialog count**: Review said 21 should be 22/23 - but 21 is CORRECT
2. **LSP language count**: Review said 44+ should be 43+ - but actual is 41 and doc says 41+
3. **Tool count**: Review said 33+ should be 27+ - but 26 and doc says 26
4. **Hook types**: Review said 10 should be 13 - but actual is 6 HookEvent variants (Plugin has 13 HookType variants)
5. **Command count**: Review said 36 but doc already says 41
6. **LSP skill count**: Review said 42 but actual is 41 and skill doc says 41

### Key Insight

**The only confirmed documentation count error is the TUI theme count** - comment says 42 but only 31 themes defined.

### Documentation Fix Approach

When implementing documentation fixes:
1. Read the CURRENT architecture doc first to check if fix is needed
2. Then read the actual source code to verify counts/claims
3. Only fix if there's a real discrepancy
4. Don't trust review file claims without verification
