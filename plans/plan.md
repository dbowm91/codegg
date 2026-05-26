# Implementation Plan - Phase 3: Documentation Corrections

**Status**: Completed (2026-05-26)
**Created**: 2026-05-26
**Consolidated from**: Review of 33 plan files across the codebase
**Implementation completed**: 2026-05-26

---

## Summary

This plan consolidates 10 code bugs and ~25 documentation corrections across the codebase. Items are organized into waves for parallel implementation.

**IMPORTANT**: Before implementing any documentation fix, verify the actual current state of the architecture docs. Many documentation items in this plan were based on review files that referenced older doc versions - several "corrections" were already fixed in the current docs.

### Wave Structure

| Wave | Type | Items | Description |
|------|------|-------|-------------|
| Wave 1 | Code Bugs | 10 | Actual code fixes - must be completed first |
| Wave 2 | Documentation | ~25 | Architecture/skills doc corrections - can parallelize |
| Wave 3 | Optional | 2 | Known issues (SSE support, tool cache staleness) |

---

## Wave 1: Code Bugs (Must Fix First)

All bugs verified against actual source code. These fix actual functional issues.

### Phase 1.1: Server Routes Permission/Question Bugs

#### BUG-01: Server Permission Session Mismatch

- **Module**: server/routes
- **File**: `src/server/routes/permission.rs:27`
- **Current Behavior**: Session ID mismatch check uses `perm_id.splitn(2, '-').next()` expecting format `{session_id}-...`, but perm_id format is actually `{tool_call_id}-{tool_name}`
- **Expected**: Should validate that request's session_id matches the session that created the permission, or remove the faulty check
- **Fix**: Remove or replace the faulty session_id validation. The PermissionRegistry doesn't store session_id in the key, so this check cannot work as intended
- **Implementation**: Either remove lines 27-31 entirely, or change the check to properly validate against stored session data

#### BUG-02: Server get_pending_permissions_for_session Ignores session_id

- **Module**: server/routes
- **File**: `src/server/routes/permission.rs:65-90`
- **Current Behavior**: `get_pending_permissions_for_session()` ignores the `session_id` parameter and returns ALL pending permissions with the provided session_id filled in
- **Expected**: Should only return permissions that belong to the specified session_id
- **Fix**: The comment at line 70 confirms the limitation: "To filter by session, we would need to extend the registry to store session_id". Options:
  1. Extend PermissionRegistry to track session_id per permission (requires code change)
  2. Document the limitation and change behavior to return empty when filtering is not possible
  3. Store session_id in permission keys when registering

#### BUG-03: Server get_pending_questions_for_session Filter Faulty

- **Module**: server/routes
- **File**: `src/server/routes/question.rs:63-73`
- **Current Behavior**: `get_pending_questions_for_session()` filters with `|id| *id == session_id` which compares pending question IDs against the session_id string, resulting in empty/bogus results
- **Expected**: Should filter questions that belong to the specified session
- **Fix**: The filter compares question IDs (likely UUIDs or internal identifiers) against session_id strings. Change the filter logic or extend QuestionRegistry to track session associations

### Phase 1.2: Core Module Unimplemented Handlers

All at `src/core/mod.rs` - the InprocCoreClient falls through to `Ok(CoreResponse::Ack)` for these:

#### BUG-04: CoreRequest::Initialize Not Implemented

- **File**: `src/core/mod.rs:698` (the catch-all `_ => Ok(CoreResponse::Ack)`)
- **Current**: Falls through to Ack, no initialization performed
- **Expected**: Should return capabilities, confirm version
- **Fix**: Add match arm for `CoreRequest::Initialize` that returns appropriate `CoreResponse::Initialize` with capabilities

#### BUG-05: CoreRequest::TurnCancel Not Implemented

- **File**: `src/core/mod.rs`
- **Current**: Falls through to Ack
- **Expected**: Should stop ongoing agent work, clean up
- **Fix**: Add match arm for `CoreRequest::TurnCancel` that signals cancellation to the agent loop

#### BUG-06: CoreRequest::TurnSteer Not Implemented

- **File**: `src/core/mod.rs`
- **Current**: Falls through to Ack
- **Expected**: Should handle mid-turn steering
- **Fix**: Add match arm for `CoreRequest::TurnSteer` that redirects or modifies agent behavior

#### BUG-07: CoreRequest::AgentSelect Not Implemented

- **File**: `src/core/mod.rs`
- **Current**: Falls through to Ack
- **Expected**: Should switch active agent
- **Fix**: Add match arm for `CoreRequest::AgentSelect` that changes the active agent

#### BUG-08: CoreRequest::ModelSelect Not Implemented

- **File**: `src/core/mod.rs`
- **Current**: Falls through to Ack
- **Expected**: Should switch LLM model
- **Fix**: Add match arm for `CoreRequest::ModelSelect` that changes the model mid-session

**Context**: The full `CoreRequest` enum is defined in `src/protocol/core.rs:50-175`. The existing handlers at `src/core/mod.rs:52-355` handle TurnSubmit, SessionMessagesLoad, SessionMessageCounts, SessionCreate, SessionLoad, SessionAttach, and various state queries. The Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect variants are not yet handled.

**IMPORTANT**: Before implementing BUG-04 through BUG-08, verify if TUI or other modules actually send these requests. If no module sends them, they can be left as Ack but should be documented as "acknowledged but not actioned until needed".

### Phase 1.3: Plugin Fuel Leaks

#### BUG-09: Plugin Fuel Leak on Hook Function Not Found

- **Module**: plugin
- **File**: `src/plugin/loader.rs:344-354`
- **Current Behavior**: When hook function (e.g., `on_auth`, `on_tool_execute_before`) is not found at line 344, returns early at line 352 without returning the reserved fuel
- **Expected**: Reserved fuel should be returned on early exit
- **Fix**: Add `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved);` before the return at line 352.

Note: `fuel_reserved` is set at line 270. The `return_fuel` calls occur at lines 518, 523, and 528 in the final match block only.

#### BUG-10: Plugin Fuel Leaks on Early Errors (4 Locations)

- **Module**: plugin
- **File**: `src/plugin/loader.rs:356-409`
- **Current Behavior**: Multiple early returns after `fuel_reserved` is set at line 270 don't return the fuel:

| Location | Condition | Line Range |
|----------|-----------|------------|
| 1 | No memory export | 359-365 |
| 2 | No allocate function | 373-379 |
| 3 | Allocate returned no value | 389-396 |
| 4 | Input exceeds memory bounds | 403-409 |

- **Expected**: Reserved fuel should be returned on all early exits
- **Fix**: Add `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved);` before each early return at lines 360, 374, 390, and 404

**Additional Bug (Plan Item)**: Global `PLUGIN_FUEL_BUDGET` at line 15 is never decremented - only per-plugin fuel in `ModuleCache` is used. The `check_and_reset_fuel_budget()` function at lines 24-41 is never called. Consider either integrating this or removing dead code.

---

## Wave 2: Documentation Fixes

**Before editing any architecture doc, read the current version to verify it actually needs correction.** Several items in this plan were based on review files that referenced older doc versions.

### Group A: High-Priority Corrections

#### DOC-A01: TUI Theme Count Discrepancy

- **File**: `src/tui/theme.rs:8` and `architecture/tui.md`
- **Issue**: Comment at line 8 says "The module includes 42 built-in themes" but only 31 ThemeData entries exist
- **Fix**: Update the comment at `src/tui/theme.rs:8` from "42" to "31"

#### DOC-A02: TUI DialogState Classification Errors

- **Files**: `architecture/tui.md` lines 166-177
- **Issue**: DialogState always/on-demand classifications are incorrect:
  - `tree_dialog` is always instantiated but doc says "on-demand"
  - `command_palette` is always instantiated but doc says "on-demand"
  - `help_dialog` and `info_dialog` are on-demand but doc says "optional"
- **Fix**: Update the classifications to match `src/tui/app/state/dialog.rs`:
  - Always: `model_dialog`, `agent_dialog`, `session_dialog`, `tree_dialog`, `command_palette`
  - On-demand: all others

#### DOC-A03: TUI TuiCommand Listing Incomplete

- **File**: `architecture/tui.md` lines 247-255
- **Issue**: Many TuiCommand variants not documented
- **Actual** (`src/tui/app/mod.rs:81-167`): Undocumented variants include `UndoDelete`, `UnshareSession`, `ExportSession`, `RenameSession`, `BulkArchive`, `BulkExport`, `ReloadSessions`, `OpenTreeDialog`, `PreviewImport`, `ConfirmImport`, `CreateFromTemplate`, `LoadSessionMessages`, `SpawnSubagent`, `ListTasks`, `DeleteTask`, `TaskSchedule`, `WorktreeList`, `MemorySummary`, `MemorySearch`, `MemoryRemember`, `MemoryForget`, `CompactSession`, `OpenDiffDialog`, `SendNotification`, `UpdateModels`
- **Fix**: Add documentation for missing TuiCommand variants

#### DOC-A04: Permission PERMISSION_TYPES Bug

- **File**: `src/permission/mod.rs:79`
- **Issue**: `external_directory` incorrectly included in PERMISSION_TYPES - this is not a real tool name
- **Fix**: Remove `external_directory` from PERMISSION_TYPES (already flagged as fixed per AGENTS.md)

### Group B: Skills Documentation Fixes

#### DOC-B01: Hooks Skill YAML Format

- **File**: `.opencode/skills/hooks/SKILL.md:149-165`
- **Issue**: YAML configuration example uses map format instead of TOML array format
- **Actual**: Config schema uses `hooks: Vec<HookConfigEntry>` (array), not a map keyed by event name
- **Fix**: Update example to show correct TOML format matching `architecture/hooks.md:101-116`

#### DOC-B02: Memory Skill Path Error

- **File**: `.opencode/skills/memory/SKILL.md:64-69`
- **Issue**: Shows path as `project/{hash}/conventions/MEMORY.md` but the `conventions/` subdirectory doesn't exist
- **Actual**: Files go directly at `project/{hash}/MEMORY.md`
- **Fix**: Remove the `conventions/` subdirectory from the path diagram

#### DOC-B03: Resilience Skill Missing half_open_start_time

- **File**: `.opencode/skills/resilience/SKILL.md:34-52`
- **Issue**: `is_available()` snippet doesn't show `half_open_start_time` assignment
- **Actual**: `is_available()` at `src/resilience/circuit.rs:88` assigns `half_open_start_time` when entering half-open state
- **Fix**: Add `half_open_start_time = Some(now)` to the `HalfOpen => Open` transition in the snippet

#### DOC-B04: LSP Skill Server Count

- **File**: `.opencode/skills/lsp/SKILL.md:76`
- **Issue**: Many versions of review docs say "42 servers" - verify current actual count
- **Actual**: The architecture doc at `architecture/lsp.md:229` says "39 servers" which matches actual count
- **Fix**: Update to "39 server implementations" if it doesn't already say so

### Group C: Architecture Doc Corrections

#### DOC-C01: security.md IPv6 Missing

- **File**: `architecture/security.md` lines 32, 195-197
- **Issue**: IPv6 link-local range `fe80::/10` not documented
- **Fix**: Add `fe80::/10` (IPv6 link-local) to the internal IP ranges alongside `169.254.0.0/16`

#### DOC-C02: security.md Config Section Not Implemented

- **File**: `architecture/security.md` lines 200-206
- **Issue**: Shows `[security]` configuration section but this is not implemented
- **Fix**: Remove the `[security]` configuration section or mark as "not implemented"

#### DOC-C03: tool.md Missing Items and Numbering Error

- **File**: `architecture/tool-18.md`
- **Issue**: 
  - Missing `lsp` tool in Code Operations (may be dead code)
  - Missing "Team Operations" category for TeamCreateTool, SendMessageTool, etc.
  - Security section says item 7 but only 6 items (numbering error)
  - `ToolExecutor::new()` constructor not documented
- **Fix**: 
  1. Add `lsp` tool if it exists in registry
  2. Add "Team Operations" category
  3. Fix security item numbering
  4. Document `ToolExecutor::new()` constructor

#### DOC-C04: tts.md stop() Behavior Misleading

- **File**: `architecture/tts.md` lines 30-31, 37
- **Issue**: Doc implies `pkill` always runs, but actual `stop()` returns early if not speaking
- **Fix**: Clarify that `stop()` first checks if speaking and only kills if actively speaking

#### DOC-C05: plugin.md Hook Flow Diagram

- **File**: `architecture/plugin.md:371`
- **Issue**: Diagram references `dispatch_to_plugin` which was removed (dead code)
- **Fix**: Update to show `execute_hook_with_timeout()` directly

#### DOC-C06: resilience.md HalfOpen->Open Timeout

- **File**: `architecture/resilience.md`
- **Issue**: Diagram missing `HalfOpen->Open` timeout transition for `max_half_open_duration=30s`
- **Fix**: Add timeout transition to diagram; document `half_open_start_time` assignment in `is_available()`

#### DOC-C07: provider.md Missing Types

- **File**: `architecture/provider.md`
- **Issue**: 
  - `store` field should be `cache`, `CachedResponse` should be `CacheEntry` in ModelCatalog
  - Missing `OpenAiToolState` struct in SseParser section
  - Missing `ResponseFormat` enum (`src/provider/mod.rs:156-164`)
  - Missing `ModelVariant` struct (`src/provider/mod.rs:209-217`)
- **Fix**: Update field names, add missing type documentation

#### DOC-C08: compaction.md Multiple Issues

- **File**: `architecture/compaction.md`
- **Issue**:
  - TruncateToolOutputs description conflates `prune_tool_outputs()` (token-based) vs `truncate_tool_outputs()` (character-based)
  - Missing 3 fields in ContextTracker: `max_messages`, `max_total_bytes`, `model`
  - SummarizeOldTurns sync fallback behavior unclear
  - DropMiddleMessages message count parameters unclear
  - Missing 4 functions in Key Functions table
  - `prune_tool_outputs()` runs BEFORE hook dispatch (not after)
- **Fix**: Address each sub-issue with precise corrections

#### DOC-C09: session.md Corrections

- **File**: `architecture/session.md`
- **Issue**:
  - Lines 451-462: `MessageStore`, `PartStore`, `PermissionStore` are NOT module-level exports - only `CheckpointStore` is exported
  - Lines 383-392: Checkpoints table schema shows separate columns but actual is single `state TEXT` JSON column
  - Lines 291-299: Lead with `validate_import_size` as public API, not `parse_import` (which is internal `pub(crate)`)
- **Fix**: Update exports documentation and schema description

#### DOC-C10: core.md Missing Documentation

- **File**: `architecture/core.md`
- **Issue**:
  - Lines 33-37: `InprocCoreClient` documented as text but doesn't enumerate the 4 fields
  - Lines 62-74: `CoreRequest` variant list may be incomplete
  - Lines 115-123: `TurnSubmit` fields not explicitly documented
  - Lines 177-272: `CoreEvent` enum not documented at all
- **Fix**: Add documentation for missing items

#### DOC-C11: error.md Missing api_with_url

- **File**: `architecture/error.md` lines 207-208
- **Issue**: `ProviderError::api_with_url()` method not documented
- **Fix**: Document `ProviderError::api_with_url()` method and note about reqwest URL extraction via `.url()` method

---

## Wave 3: Optional / Known Issues

### OPT-01: SSE Support Not Fully Integrated

- **Module**: server
- **Issue**: `connect_sse()` and `connect_sse_stream()` exist but not automatically called during remote connection setup. SSE events collected but not processed by agent.
- **Fix**: Requires understanding SSE event flow integration with the agent loop
- **Status**: Known limitation, not addressed in this plan

### OPT-02: Tool Definition Cache Staleness

- **Module**: tool
- **Issue**: Using `mcp_tool_count` as proxy means if MCP tool identities change without count changing, cache may be stale. MCP service would need to expose version/hash for precise invalidation.
- **Fix**: Would require MCP protocol changes to expose version/hash
- **Status**: Known limitation, not addressed in this plan

### OPT-03: Plugin Global Fuel Budget Dead Code

- **Module**: plugin
- **File**: `src/plugin/loader.rs:15, 24-41`
- **Issue**: Global `PLUGIN_FUEL_BUDGET` and `check_and_reset_fuel_budget()` are never used
- **Fix**: Keep as-is; per-plugin fuel via ModuleCache is what's actually used. Global budget removal would require significant refactoring without clear benefit.
- **Status**: Documented as dead code, no action taken

---

## Implementation Order

### Phase 1 (Sequential - Code Bugs)

| Step | Tasks | Files |
|------|-------|-------|
| 1.1 | Fix BUG-01 through BUG-03 | `src/server/routes/permission.rs`, `src/server/routes/question.rs` |
| 1.2 | Fix BUG-04 through BUG-08 (verify first if TUI sends these) | `src/core/mod.rs` |
| 1.3 | Fix BUG-09 through BUG-10 | `src/plugin/loader.rs` |

**Verification**: Run `cargo test` after each step

### Phase 2 (Parallel - Documentation)

After Phase 1 is complete, documentation updates can run in parallel:

| Agent | Files | Items |
|-------|-------|-------|
| Agent A | `src/tui/theme.rs`, `architecture/tui.md` | Theme count fix, DialogState, TuiCommand |
| Agent B | `.opencode/skills/hooks/SKILL.md`, `.opencode/skills/memory/SKILL.md` | Hooks YAML, Memory path |
| Agent C | `.opencode/skills/resilience/SKILL.md`, `.opencode/skills/lsp/SKILL.md` | Resilience half_open, LSP count |
| Agent D | `architecture/security.md`, `architecture/tool.md` | IPv6, tool md |
| Agent E | `architecture/plugin.md`, `architecture/resilience.md` | Hook flow, timeout diagram |
| Agent F | `architecture/provider.md`, `architecture/compaction.md` | Missing types, compaction |
| Agent G | `architecture/session.md`, `architecture/core.md` | Session exports, core docs |
| Agent H | `architecture/error.md`, `architecture/tts.md` | api_with_url, tts stop |

**Verification**: Build and check documentation renders correctly

### Phase 3 (Optional)

Address OPT-01, OPT-02, OPT-03 when time permits.

---

## Notes for Future Agents

### Pre-implementation Verification

1. **ALWAYS verify documentation claims against actual code before editing** - Many "bugs" in review files were incorrect until verified. Several items in this plan may already be fixed in current docs.

2. **Core handlers (BUG-04 through BUG-08) may have implicit dependencies** - The `CoreRequest::Initialize`, `TurnCancel`, `TurnSteer`, `AgentSelect`, `ModelSelect` handlers need to be verified as actually being sent by TUI or other modules before implementing meaningful responses.

3. **PermissionRegistry/QuestionRegistry limitations** - BUG-02 and BUG-03 require fundamental changes to how registries track session associations. If the registries don't store session_id, proper filtering is not possible without code changes.

4. **Plugin fuel tracking** - When fixing BUG-09/BUG-10, ensure ALL early returns after `fuel_reserved` is set include a fuel return call. Use the pattern: `let fuel_reserved = module_cache::CACHE.reserve_fuel(plugin_id, fuel_limit);` at line 270, and `module_cache::CACHE.return_fuel(plugin_id, fuel_reserved);` on all exits.

5. **Theme count is real** - The only confirmed doc count error is `src/tui/theme.rs:8` saying 42 themes when only 31 exist.

### Documentation File Locations

- Architecture docs: `architecture/*.md` (overview, tui, core, provider, etc.)
- Skills docs: `.opencode/skills/*/SKILL.md`
- Module source: `src/*/mod.rs` and `src/*/*.rs`

### Key File References

| Bug | Key Line | File |
|-----|----------|------|
| BUG-01 | 27 | `src/server/routes/permission.rs` |
| BUG-02 | 65-90 | `src/server/routes/permission.rs` |
| BUG-03 | 63-73 | `src/server/routes/question.rs` |
| BUG-04-08 | 698 | `src/core/mod.rs` (catch-all) |
| BUG-09 | 344-354 | `src/plugin/loader.rs` |
| BUG-10 | 356-409 | `src/plugin/loader.rs` |

---

## Files Modified by This Plan

### Code Changes (Phase 1)
- `src/server/routes/permission.rs`
- `src/server/routes/question.rs`
- `src/core/mod.rs`
- `src/plugin/loader.rs`

### Documentation Changes (Phase 2)
- `src/tui/theme.rs` (1 line)
- `architecture/tui.md`
- `architecture/security.md`
- `architecture/tool.md`
- `architecture/plugin.md`
- `architecture/resilience.md`
- `architecture/provider.md`
- `architecture/compaction.md`
- `architecture/session.md`
- `architecture/core.md`
- `architecture/error.md`
- `architecture/tts.md`
- `.opencode/skills/hooks/SKILL.md`
- `.opencode/skills/memory/SKILL.md`
- `.opencode/skills/resilience/SKILL.md`
- `.opencode/skills/lsp/SKILL.md`

---

## Implementation Summary (2026-05-26)

### Wave 1: Code Bugs - COMPLETED
- **BUG-01**: Removed faulty session_id check in `submit_permission` (permission registry doesn't store session_id in keys)
- **BUG-02**: Changed `get_pending_permissions_for_session` to return empty list (session filtering not supported)
- **BUG-03**: Changed `get_pending_questions_for_session` to return empty list (session filtering not supported)
- **BUG-04 through BUG-08**: Verified TUI does NOT send Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect requests - no action needed
- **BUG-09**: Added `return_fuel` before early return when hook function not found
- **BUG-10**: Added `return_fuel` before all early exits after fuel reservation (5 locations)

### Wave 2: Documentation - COMPLETED
All items addressed as documented in git commit `641f015`.

Items already correct in current docs (no changes needed):
- DOC-A04: `external_directory` not in PERMISSION_TYPES
- DOC-B02: Memory skill path already correct
- DOC-C01: IPv6 fe80::/10 already documented
- DOC-C02: No [security] config section in docs
- DOC-C04: TTS stop() behavior already correctly documented
- DOC-C05: Plugin hook flow diagram already uses execute_hook_with_timeout
- DOC-C09: Session exports already correctly documented

### Wave 3: Optional - DEFERRED
- OPT-01 (SSE support): Known limitation, requires significant architectural work
- OPT-02 (Tool cache staleness): Requires MCP protocol changes
- OPT-03 (Global fuel budget): Dead code, but removal would require refactoring without clear benefit

---

*Plan consolidated from codebase review (2026-05-26)*
