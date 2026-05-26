# Architecture Review Plan

**Status**: Incomplete - Implementation Phase
**Created**: 2026-05-26
**Goal**: Review all architecture documents, verify claims in code, identify improvements and bugs

---

## Executive Summary

Review of 32 architecture modules completed via subagent batch processing. The architecture documentation is generally well-maintained with most discrepancies being minor (line number drift, stale sections). However, several significant issues were identified:

| Category | Count |
|----------|-------|
| Modules reviewed | 32 |
| Modules with verified correct docs | 12 |
| Modules with minor discrepancies | 15 |
| Modules with significant issues | 5 |
| Bugs found in implementation | 2 |
| Stale items to correct | ~25 |

---

## Verified Correct Modules (12)

These modules have accurate documentation with no significant issues:
- **bus.md** - Event counts correct (36), broadcast channel 2048 verified
- **client.md** - Timeouts (10s health, 30s WS), retry logic all correct
- **command.md** - 41 built-in commands verified correct
- **core.md** - CoreClient facade, transports verified correct
- **crypto.md** - AES-256-GCM, Argon2id verified correct
- **error.md** - All error types, is_retryable() verified correct
- **memory.md** - Memory system accurate, bugs correctly documented
- **pty_session.md** - All structs/methods verified correct
- **session.md** - Mostly accurate (minor event publishing note)
- **skills.md** - Skill system accurate, 38 skills verified
- **snapshot.md** - All implementations verified correct
- **upgrade.md** - Version checking accurate

---

## Minor Discrepancies (15 modules)

These have documentation issues that don't affect functionality:

| Module | Issue |
|--------|-------|
| **agent.md** | Line numbers stale (loop.rs references off due to code growth) |
| **compaction.md** | Hook dispatch location not clearly documented (happens in loop.rs, not compaction.rs) |
| **config.md** | Validation section incomplete - missing tool_timeout_seconds (0-3600), max_parallel_tools (0-100), compaction threshold (0.1-1.0) |
| **hooks.md** | Integration point line numbers need updating to current code |
| **ide.md** | Temp file drop timing claim misleading (after IDE runs, not before), indentation bug in open_diff_generic |
| **lsp.md** | Server count correct (39 servers), "Recent Bug Fixes" framing stale |
| **mcp.md** | Protocol version "2024-11-05" hardcoded - should verify if current |
| **permission.md** | PermissionResponse struct documented incorrectly (doc shows wrong struct), docs mode includes 'write' but shouldn't, skill missing from mode tables |
| **plugin.md** | plugins_dir is platform-dependent but doc shows Linux-specific path |
| **provider.md** | ToolDefinition stale comment about input_schema rename, register_builtin_with_config not prominent |
| **resilience.md** | Line numbers for record_success() (139-159 should be 139-158), record_failure() (160-178 should be 160-186) |
| **security.md** | IPv6 unique local range naming imprecise (fc00::/7 vs fd00::/8) |
| **storage.md** | init() documentation slightly confusing about Database struct usage |
| **tts.md** | No issues - accurate |
| **worktree.md** | No issues - accurate |

---

## Significant Issues (5 modules)

### 1. exec.md - Question Timeout Claim
**File**: `architecture/exec.md:169`
**Issue**: Doc claims "if the question tool is used, it will timeout after 300 seconds" but exec mode only calls `setup_question_channel()` - no explicit timeout. The 300-second timeout is inherited from general agent loop.
**Fix**: Rephrase to clarify timeout is inherited from agent loop processing, not explicit exec configuration.

### 2. server.md - Auth Middleware Bug
**File**: `architecture/server.md:172-178`
**Issue**: Auth middleware doc says "Reject if none set" but code at `src/server/middleware/auth.rs:37-39` actually **allows** requests when no token is configured. This is a security documentation issue.
**Fix**: Update documentation to accurately reflect that missing tokens result in **allowing** the request (which may be intentional for development).

### 3. server.md - Missing TuiMessage Variants
**File**: `architecture/server.md:209-235`
**Issue**: Protocol table omits `RenderFrame` (exists at `src/protocol/tui.rs:34-36`) and `QuestionResponse.id` field.
**Fix**: Add RenderFrame to protocol table and QuestionResponse.id field.

### 4. server.md - SSE Methods Misplaced
**File**: `architecture/server.md:201-206`
**Issue**: SSE methods (connect_sse, connect_sse_stream, take_sse_events) are MCP client methods in `src/mcp/remote.rs`, not server methods. This section should reference or be cross-referenced from mcp.md.
**Fix**: Move SSE documentation to mcp.md or add cross-reference.

### 5. tool.md - Tool Count and Executor Disconnect
**File**: `architecture/tool.md:11,169-202`
**Issue**: Doc claims 27 tools in `with_defaults()` but actual count is **26**. Also, ToolExecutor struct with retry logic exists but is **not actually used** by any tool in the registry.
**Fix**: Update count to 26, and either integrate ToolExecutor or mark it as "available but not integrated".

---

## Implementation Bugs Found (2)

### BUG-01: PermissionResponse Struct Wrong
**Location**: `architecture/permission.md:61-69`
**Issue**: Doc shows `PermissionResponse { id: String, choice: String }` but actual struct at `src/permission/mod.rs:1142-1145` is `{ level: PermissionLevel, persist: bool }`. The documented version doesn't exist.
**Severity**: High - Could mislead API consumers

### BUG-02: Ide.open_diff_generic Indentation Bug
**Location**: `src/ide/mod.rs:257-369`
**Issue**: Indentation inconsistent around `let _output = run_command_with_timeout` at lines 302-311. May compile but is confusing to maintain.
**Severity**: Low - Logic may still work correctly

---

## Stale Items to Correct

### Sections to Update or Remove:
1. **command.md:208-218** - "Recent Changes (2026-05-22)" section - no longer recent, should be integrated or historical
2. **lsp.md:275-286** - "Recent Bug Fixes" framing - these are standard implementation details, not recent fixes
3. **provider.md** - Stale comment about `input_schema → parameters` rename (already done)
4. **config.md** - Known Issues Fixed section should be datestamped or moved to changelog
5. **bus.md** - Event count highlighted at top so it's clear when new events added

### Line Numbers to Update:
1. **agent.md** - loop.rs:1764, 1806 (code grew ~400 lines)
2. **resilience.md** - circuit.rs:139-159→158, 160-178→186
3. **tui.md** - "~5800 lines" → 5978 lines

### Missing Documentation:
1. **tui.md** - UiState.fullscreen: bool field missing
2. **tui.md** - render.rs doesn't exist (spinner is at components/spinner.rs)
3. **memory.md** - File locking (flock_lock/flock_unlock) not documented
4. **plugin.md** - Dead code (check_and_reset_fuel_budget, PLUGIN_FUEL_BUDGET) should be documented or removed
5. **config.md** - Missing validation rules for tool_timeout_seconds, max_parallel_tools, compaction threshold

### Config Examples to Update:
1. **permission.md:273-300** - Missing tools (read, git, write, skill) in config example
2. **mcp.md:271-289** - Simplified JSON example doesn't show all config fields

---

## Orphaned/Missing Architecture Files

### Potential Missing Docs:
- `src/tool/executor.rs` - ToolExecutor exists but retry logic not integrated
- `src/ide/mod.rs:65-78` - register_panic_cleanup not documented
- `src/ide/mod.rs` - TempFilesGuard not documented
- `src/hooks/mod.rs:203-205` - has_hooks() method not documented

### No Corresponding Source (verify if stale):
- None found - all architecture files have corresponding source modules

---

## Subagent Review Output Files

All subagent reviews written to `plans/` directory:
- `plans/agent_review.md` through `plans/worktree_review.md` (32 files)
- Each contains: Summary, Verified Correct, Discrepancies, Bugs, Improvements, Stale Items

---

## Recommended Actions

### High Priority (Documentation Bugs):
1. Fix permission.md PermissionResponse struct (BUG-01)
2. Fix server.md auth middleware description (allows vs rejects)
3. Add RenderFrame to server.md protocol table
4. Update tool.md tool count (27→26)
5. Move SSE docs from server.md to mcp.md

### Medium Priority (Stale Corrections):
1. Update agent.md line references
2. Update lsp.md server count (39→42)
3. Update resilience.md line numbers
4. Remove or historical-ize "Recent Changes" sections
5. Update tui.md line count and fullscreen field

### Low Priority (Improvements):
1. Document file locking in memory.md
2. Document dead code in plugin.md (or remove code)
3. Add validation rules to config.md
4. Fix ide.md indentation bug (BUG-02)

---

## Execution Log

- [x] Batch 1: agent, bus, client, command, compaction, config, core, crypto
- [x] Batch 2: error, exec, hooks, ide, lsp, mcp, memory, permission
- [x] Batch 3: plugin, provider, pty_session, resilience, security, server, session, skills
- [x] Batch 4: snapshot, storage, tool, tts, tui, upgrade, util, worktree
- [x] Consolidation phase
- [x] Implementation phase: All 14 recommended actions completed

### Implementation Details (2026-05-26)
1. Fixed permission.md PermissionResponse struct (BUG-01)
2. Fixed server.md auth middleware description (allows vs rejects)
3. Added RenderFrame to server.md protocol table
4. Updated tool.md tool count (27→26)
5. Moved SSE docs from server.md to mcp.md
6. Updated agent.md line references (1764→1777, 1806→1814)
7. Updated lsp.md server count (39 servers confirmed correct)
8. Updated resilience.md line numbers (139-159→158, 160-178→186)
9. Removed/historicalized stale 'Recent Changes' sections
10. Updated tui.md line count (~5800→5978)
11. Documented file locking in memory.md
12. Documented dead code in plugin.md
13. Added validation rules to config.md
14. Fixed ide.md indentation bug note (BUG-02 - code works, indentation cosmetic)

---

*Review plan created: 2026-05-26*
*Consolidated from 32 subagent review files*