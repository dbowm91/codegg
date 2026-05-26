# Implementation Plan - Phase 3: Documentation Corrections

**Status**: Draft - Awaiting Implementation
**Created**: 2026-05-26
**Consolidated from**: 33 review files in plans/review/

---

## Summary

This plan consolidates 10 code bugs and ~50 documentation corrections across the codebase. Items are organized into waves for parallel implementation.

### Wave Structure

| Wave | Type | Items | Description |
|------|------|-------|-------------|
| Wave 1 | Code Bugs | 10 | Actual code fixes - must be completed first |
| Wave 2 | Documentation | ~50 | Architecture/skills doc corrections - can parallelize |
| Wave 3 | Optional | 2 | Known issues (SSE support, tool cache staleness) |

---

## Wave 1: Code Bugs (Must Fix First)

All bugs verified against actual source code. These fix actual functional issues.

### BUG-01: Server Permission Registry Session Mismatch

- **Module**: server/routes
- **File**: `src/server/routes/permission.rs:27`
- **Current Behavior**: Session ID mismatch check uses `perm_id.splitn(2, '-').next()` expecting format `{session_id}-...`, but perm_id format is actually `{tool_call_id}-{tool_name}`
- **Expected**: Should validate that request's session_id matches the session that created the permission, or remove the faulty check
- **Fix**: Remove or replace the faulty session_id validation. The PermissionRegistry doesn't store session_id in the key, so this check cannot work as intended

### BUG-02: Server get_pending_permissions_for_session Ignores session_id

- **Module**: server/routes
- **File**: `src/server/routes/permission.rs:65-90`
- **Current Behavior**: `get_pending_permissions_for_session()` ignores the `session_id` parameter and returns ALL pending permissions with the provided session_id filled in
- **Expected**: Should only return permissions that belong to the specified session_id
- **Fix**: The PermissionRegistry would need to track session_id per permission, or the filtering logic needs to be updated. Currently line 70 confirms this limitation with comment "To filter by session, we would need to extend the registry to store session_id"

### BUG-03: Server get_pending_questions_for_session Filter Faulty

- **Module**: server/routes
- **File**: `src/server/routes/question.rs:63-73`
- **Current Behavior**: `get_pending_questions_for_session()` filters with `|id| *id == session_id` which compares pending question IDs against the session_id string, resulting in empty/bogus results
- **Expected**: Should filter questions that belong to the specified session
- **Fix**: The filter compares question IDs (likely UUIDs or internal identifiers) directly against session_id strings, which will never match. Need to either store session association in registry or use a different filtering approach

### BUG-04 to BUG-08: Core Module Unimplemented Handlers

All at `src/core/mod.rs:698` (the catch-all `_ => Ok(CoreResponse::Ack)`):

| ID | Handler | Current | Expected |
|----|---------|---------|----------|
| BUG-04 | `CoreRequest::Initialize` | Falls through to Ack, no init | Should return capabilities, confirm version |
| BUG-05 | `CoreRequest::TurnCancel` | Falls through to Ack | Should stop ongoing agent work, clean up |
| BUG-06 | `CoreRequest::TurnSteer` | Falls through to Ack | Should handle mid-turn steering |
| BUG-07 | `CoreRequest::AgentSelect` | Falls through to Ack | Should switch active agent |
| BUG-08 | `CoreRequest::ModelSelect` | Falls through to Ack | Should switch LLM model |

**Context**: The full `CoreRequest` enum is defined in `src/protocol/core.rs:50-175` and includes all these variants. The InprocCoreClient at `src/core/mod.rs:698` doesn't handle them.

### BUG-09: Plugin Fuel Leak on Hook Function Not Found

- **Module**: plugin
- **File**: `src/plugin/loader.rs:344-354`
- **Current Behavior**: When hook function (e.g., `on_auth`, `on_tool_execute_before`) is not found at line 344, returns early at line 352 without returning the reserved fuel
- **Expected**: Reserved fuel should be returned on early exit
- **Fix**: Add `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved);` before the return at line 352

### BUG-10: Plugin Fuel Leaks on Early Errors

- **Module**: plugin
- **File**: `src/plugin/loader.rs:356-409`
- **Current Behavior**: Multiple early returns after `fuel_reserved` is set at line 270 don't return the fuel:
  - Line 360-363: No memory export
  - Line 374-377: No allocate function
  - Line 390-394: Allocate returned no value
  - (Line 405-408 was also flagged but may have been fixed - verify)
- **Expected**: Reserved fuel should be returned on all early exits
- **Fix**: Add `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved);` before each early return

---

## Wave 2: Documentation Fixes

### Group A: Architecture Docs (Overview through Core)

#### DOC-A01: architecture/01_overview.md
- **Lines 25, 95**: Dialog count is 21 but should be 22 or 23 (count actual dialogs in `src/tui/app/dialogs/`)
- **Line 54**: LSP language count is 44+ but should be 43+ (count actual in `src/lsp/language.rs`)
- **Line 70**: Tool count is 33+ but should be 27+ (count actual built-in tools in `src/tool/`)
- **Line 111**: Hook types is 10 but should be 13 (count actual in `src/hooks/mod.rs`)

#### DOC-A02: architecture/02_tui.md
- **Line 9**: Says "42 built-in themes" but line 82 says "31 themes" - verify actual count in `src/tui/theme.rs` and reconcile
- **DialogState classification**: `tree_dialog` and `command_palette` are always open but documented as on-demand; `help_dialog` and `info_dialog` are on-demand but listed as optional
- **TuiCommand listing incomplete**: Many variants not documented (UndoDelete, UnshareSession, ExportSession, RenameSession, BulkArchive, BulkExport, etc.)

#### DOC-A03: architecture/03_snapshot.md
- **Lines 155-180**: "Integration with AgentLoop" section has wrong line references. Actual locations in `src/agent/loop.rs`:
  - `capture_snapshot_if_needed`: lines 1559-1576
  - `capture_incremental_snapshot_if_needed`: lines 1596-1620
  - `drain_file_change_events()`: lines 1578-1594

#### DOC-A04: architecture/04_server.md
- **Lines 52-68**: Missing `FromRef` implementations for `SqlitePool`, `Arc<RwLock<McpService>>`, `Config`
- **Lines 63-67**: `check_rate_limit` returns `bool` not `(bool, usize)`
- **Line 127**: Missing rate limit headers (`X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset`)
- **Line 131**: Permission route description misleading - `get_pending_permissions_for_session()` returns ALL pending, ignoring session_id

#### DOC-A05: architecture/05_mcp.md
- **Lines 107-121**: Add Clone impl documentation for McpConnectionManager (manual impl due to `CancellationToken` being `!Clone`)
- **Line 297**: Clarify `validate_url_host` location is `src/security/ssrf.rs`
- **Lines 205-208**: Document IdeServer async I/O using `tokio::io::stdin()/stdout()`

#### DOC-A06: architecture/07_permission.md
- **Lines 108-119**: Add `check_with_args` method documentation (internal, low priority)
- **Lines 196-202**: `write` tool in docs mode `allowed_tools` but missing from `PERMISSION_TYPES` constant

#### DOC-A07: architecture/08_lsp.md
- **Line ~102**: Add `code_lens()` to LspOperations documentation (exists at `operations.rs:333-358`)
- **Line ~84**: Add `send_initialized()` to Key operations (exists at `client.rs:181-184`)
- **Skill `.opencode/skills/lsp/SKILL.md`**: Says "42 server implementations" but should be "39"

#### DOC-A08: architecture/09_config.md
- **Line ~23**: Add `schema: Option<String>` field to Config struct (exists at `schema.rs:24-25`)
- **Lines 229, 233**: Line number references in "Known Issues Fixed" section are off
- **Line ~84**: `ProviderConfig::api_key()` signature incomplete - document `prefix: &str` parameter and env var lookup

#### DOC-A09: architecture/10_core.md
- **Lines 33-37**: `InprocCoreClient` documented as `{}` but has 4 specific optional fields
- **Lines 62-74**: `CoreRequest` variant list incomplete - missing Session*, Turn*, Memory*, Task*, Worktree variants from `protocol/core.rs:50-175`
- **Lines 115-123**: `TurnSubmit` fields not explicitly documented
- **Lines 177-272**: `CoreEvent` enum in `protocol/core.rs:177-272` not documented at all

#### DOC-A10: architecture/11_agent.md
- **Line 87**: Missing `ToolResult{ tool_call_id, content }` variant in ChatEvent types
- **Line 89**: Header says "team.rs / teams.rs" should be "team.rs and teams.rs"
- **team.rs:103-108**: Add documentation for `PartData::ToolCall` variant

### Group B: Architecture Docs (Bus through TTS)

#### DOC-B01: architecture/12_bus.md
- **Status**: All verified correct - no action needed

#### DOC-B02: architecture/13_command.md
- **Line 115**: Command count says "36 total" but should be "41 total"
- **Lines 199-206**: Clarify async behavior: `load_command_from_file()` is truly async, `find_command_files()` is sync wrapper

#### DOC-B03: architecture/14_hooks.md
- **Line 38**: Remove stale note about `PreAgentRun`/`PostAgentRun` - these events don't exist
- **`.opencode/skills/hooks/SKILL.md:149-165`**: YAML config example uses map format, not TOML array format

#### DOC-B04: architecture/15_skills.md
- Add `list(&self) -> &[Skill]` and `get(&self, name: &str) -> Option<&Skill>` method signatures
- Document `find_matching` behavior (searches name, description, tags)
- Document `list_skill_resources` function (`src/tool/skill.rs:67-98`)
- Add `Default` trait and `SkillFrontmatter` documentation

#### DOC-B05: architecture/16_client.md
- **`handle_remote_event()` location**: Incorrectly attributed to client module - actual location is `src/tui/app/mod.rs:794`

#### DOC-B06: architecture/17_security.md
- **Lines 32, 195-197**: Missing IPv6 link-local range `fe80::/10`
- **Lines 200-206**: Remove `[security]` configuration section - not implemented

#### DOC-B07: architecture/18_tool.md
- Missing `lsp` tool in Code Operations (not in default registry - appears dead code)
- Missing "Team Operations" category for TeamCreateTool, SendMessageTool, etc.
- **Security section**: Says item 7 but only 6 items (numbering error)
- `ToolExecutor::new()` constructor not documented

#### DOC-B08: architecture/19_tts.md
- **Lines 30-31, 37**: `stop()` behavior misleading - doc implies `pkill` always runs, but actual returns early if not speaking

#### DOC-B09: architecture/20_plugin.md
- **Fuel leak diagram**: "dispatch_to_plugin" was removed - update to "execute_hook_with_timeout()"

#### DOC-B10: architecture/21_resilience.md
- Add `HalfOpen->Open` timeout transition to diagram for `max_half_open_duration=30s`
- Document `half_open_start_time` assignment in `is_available()` (line 88)
- Update `.opencode/skills/resilience/SKILL.md:34-52` with `half_open_start_time` assignment

#### DOC-B11: architecture/22_provider.md
- `store` field should be `cache`, `CachedResponse` should be `CacheEntry` in ModelCatalog
- Missing `OpenAiToolState` struct in SseParser section
- Missing `ResponseFormat` enum (`src/provider/mod.rs:156-164`)
- Missing `ModelVariant` struct (`src/provider/mod.rs:209-217`)

#### DOC-B12: architecture/23_compaction.md
- Rewrite TruncateToolOutputs description - conflates `prune_tool_outputs()` (token-based) vs `truncate_tool_outputs()` (character-based)
- Add 3 missing fields to ContextTracker: `max_messages`, `max_total_bytes`, `model`
- Clarify SummarizeOldTurns sync fallback behavior
- Clarify DropMiddleMessages message count parameters
- Add 4 missing functions to Key Functions table
- Clarify hook dispatch timing - `prune_tool_outputs()` runs BEFORE hook dispatch

#### DOC-B13: architecture/24_worktree.md
- **Status**: Fully accurate - no corrections needed

#### DOC-B14: architecture/25_crypto.md
- **Status**: All correct - no action needed

#### DOC-B15: architecture/26_ide.md
- **Lines 78-95**: VS Code integration example outdated (uses `TempFilesGuard` now)
- **Line 109**: Generic fallback description unclear - all handlers use temp files after slicing
- **Lines 119-125**: `IdeServer::run_stdio()` example outdated (uses tokio async I/O now)
- **Lines 131-143**: `IdeServer::run_socket()` example outdated
- **Lines 46-56**: `open_diff()` parameter names unclear (`_original`/`_modified` are file paths)

#### DOC-B16: architecture/27_exec.md
- **Status**: Accurate - no corrections needed

#### DOC-B17: architecture/28_session.md
- **Lines 451-462**: `MessageStore`, `PartStore`, `PermissionStore` are NOT module-level exports - only `CheckpointStore` is exported
- **Lines 383-392**: Checkpoints table schema shows separate columns but actual is single `state TEXT` JSON column
- **Lines 291-299**: Lead with `validate_import_size` as public API, not `parse_import` (which is internal `pub(crate)`)

#### DOC-B18: architecture/29_memory.md
- **`.opencode/skills/memory/SKILL.md:64-69`**: Path is `project/{hash}/conventions/MEMORY.md` but should be `project/{hash}/MEMORY.md`

#### DOC-B19: architecture/30_error.md
- **Lines 207-208**: Document `ProviderError::api_with_url()` method
- **Line 206**: Add note about reqwest URL extraction via `.url()` method

#### DOC-B20: architecture/31_storage.md
- **Status**: Fully accurate - no corrections needed

#### DOC-B21: architecture/32_util.md
- **Line 51**: `truncate_lines` description ambiguous - clarify it keeps `max_lines/2` lines from **each** end (total max_lines), not max_lines/2 total

#### DOC-B22: architecture/33_upgrade.md
- **Optional**: Add test file reference (`tests/upgrade.rs`) after line 123, add error variant documentation

### Group C: Skills Documentation

#### DOC-C01: Skills to update for count corrections
- `lsp/SKILL.md`: 42 servers → 39 servers
- `hooks/SKILL.md:149-165`: Fix YAML config example format
- `memory/SKILL.md:64-69`: Fix path `project/{hash}/conventions/MEMORY.md` → `project/{hash}/MEMORY.md`
- `resilience/SKILL.md:34-52`: Add `half_open_start_time` assignment to is_available() snippet

---

## Wave 3: Optional / Known Issues

### OPT-01: SSE Support Not Fully Integrated
- **Module**: server
- **Issue**: `connect_sse()` and `connect_sse_stream()` exist but not automatically called during remote connection setup. SSE events collected but not processed by agent.
- **Priority**: Low

### OPT-02: Tool Definition Cache Staleness
- **Module**: tool
- **Issue**: Using `mcp_tool_count` as proxy means if MCP tool identities change without count changing, cache may be stale. MCP service would need to expose version/hash for precise invalidation.
- **Priority**: Low

---

## Implementation Order

### Phase 1 (Sequential - Code Bugs)
1. Fix BUG-01 through BUG-03 (server/routes - permission.rs, question.rs)
2. Fix BUG-04 through BUG-08 (core/mod.rs - unimplemented handlers)
3. Fix BUG-09 through BUG-10 (plugin/loader.rs - fuel leaks)

**Verification**: Run tests after each module's fixes

### Phase 2 (Parallel - Documentation)
After Wave 1 is complete, documentation updates can run in parallel:

| Agent | Module | Files |
|-------|--------|-------|
| Agent A | 01-05 | overview, tui, snapshot, server, mcp |
| Agent B | 06-10 | permission, lsp, config, core, agent |
| Agent C | 11-16 | bus, command, hooks, skills, client, security |
| Agent D | 17-22 | tool, tts, plugin, resilience, provider, compaction |
| Agent E | 23-29 | worktree, crypto, ide, exec, session, memory, error |
| Agent F | 30-33, skills | storage, util, upgrade + skills sync |

**Verification**: Build and check documentation renders correctly

### Phase 3 (Optional)
Address OPT-01 and OPT-02 when time permits.

---

## Notes for Future Agents

1. **Always verify documentation claims against actual code** - many review items were incorrect until verified against source
2. **Core handlers may have implicit dependencies** - BUG-04 through BUG-08 implement handlers for messages defined in `protocol/core.rs`. Check if TUI or other modules send these requests before implementing
3. **PermissionRegistry/QuestionRegistry limitations** - BUG-02 and BUG-03 require changes to the registry to track session associations. Currently they don't store session_id
4. **Fuel tracking in plugin system** - When fixing BUG-09/BUG-10, ensure all early returns after `fuel_reserved` is set include a fuel return call

---

## Files Modified by This Plan

### Code Changes (Wave 1)
- `src/server/routes/permission.rs`
- `src/server/routes/question.rs`
- `src/core/mod.rs`
- `src/plugin/loader.rs`

### Documentation Changes (Wave 2)
- `architecture/01_overview.md` through `architecture/33_upgrade.md`
- `.opencode/skills/*/SKILL.md` (multiple files)

---

*Plan consolidated from 33 review files (2026-05-26)*