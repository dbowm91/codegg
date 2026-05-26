# Implementation Plan

**Status**: IN PROGRESS
**Last Updated**: 2026-05-26

---

## Overview

This plan consolidates findings from 31 module review files in the `plans/` directory. The primary goal is to fix documentation discrepancies between architecture docs and actual code implementation. **No critical bugs found** - mostly documentation improvements and minor code enhancements.

**Key Finding**: Many "bugs" in review files were actually correctly implemented - always verify claims against code before implementing.

---

## Verification Before Implementation

⚠️ **CRITICAL**: Before implementing ANY item, verify the claim against actual code. Many items marked as "bugs" in review files were already fixed or are correctly implemented.

Verification commands:
```bash
# Line counts
wc -l src/tui/app/mod.rs

# Tool count (should be 26)
rg "pub struct \w+Tool" src/tool/mod.rs | wc -l

# LSP server count (should be 42)
rg "LspServerDef" src/lsp/server.rs | wc -l

# Check specific locations
rg "fullscreen.*bool" src/tui/app/state/
rg "PermissionResponse" src/permission/mod.rs
rg "check_and_reset_fuel_budget" src/plugin/loader.rs
```

---

## HIGH Priority Items

### H-1: TUI Architecture Documentation Fixes
**Source**: `plans/tui_review.md`

| Item | Current | Should Be | Action |
|------|---------|-----------|--------|
| app/mod.rs line count | "~5800 lines" | "5978 lines" | Update architecture/tui.md |
| UiState.fullscreen field | Missing | `fullscreen: bool` at `src/tui/app/state/ui.rs:70-71` | Add to architecture doc |
| SpinnerWidget reference | `src/tui/app/render.rs:461` | `src/tui/components/spinner.rs` | Fix stale file reference |

**Status**: Documentation only - no code changes

### H-2: Tool Count Documentation
**Source**: `plans/tool_review.md`

- **Issue**: Architecture doc says "27 tools" but actual count is **26 tools** in `with_defaults()` at `src/tool/mod.rs:89-119`
- **Action**: Update architecture doc line 11 to say "26 tools"

**Status**: Documentation only

### H-3: LSP Server Count Documentation
**Source**: `plans/lsp_review.md`

- **Issue**: Architecture doc says "39 servers" at `lsp.md:229` which is CORRECT - actual count is 39 servers at `src/lsp/server.rs:27-385`
- **Issue**: AGENTS.md incorrectly says "42 servers" - should be 39
- **Action**: Update AGENTS.md to say 39 servers (doc was already correct)

**Status**: Documentation only

### H-4: PermissionResponse Documentation
**Source**: `plans/permission_review.md`

- **Issue**: Doc shows `{id, choice}` but actual `PermissionResponse` at `src/permission/mod.rs:1142-1145` is `{level: PermissionLevel, persist: bool}` (no `reason` field)
- **Issue**: docs mode table shows `write` allowed but `BuiltinModes::docs()` at `modes.rs:161-172` does NOT include `write`
- **Issue**: PERMISSION_TYPES missing `git` (at `mod.rs:77`)
- **Issue**: `skill` tool missing from mode tables

**Action**:
1. Correct `PermissionResponse` struct documentation
2. Remove `write` from docs mode table OR add to PERMISSION_TYPES
3. Add `git` to PERMISSION_TYPES documentation
4. Add `skill` to mode tables (restricted/allowed tools)

### H-5: Agent Hook Invocation Clarification
**Source**: `plans/agent_review.md`

- **Issue**: Architecture doc claims "PreToolExecute/PostToolExecute hooks invoked at loop.rs:1764 and 1806" but these lines show `dispatch_tool_execute_before/after` (plugin service), NOT HookRegistry hooks
- **Reality**: BOTH plugin hooks (`dispatch_tool_execute_before/after`) AND HookRegistry hooks (`PreToolExecute`/`PostToolExecute`) ARE invoked for each tool execution, but at DIFFERENT locations
- **Action**: Clarify that both hook systems are invoked; update or remove specific line numbers (they're fragile)

**Status**: Documentation clarification

### H-6: Plugin Dead Code Removal
**Source**: `plans/plugin_review.md`

- **Issue**: `check_and_reset_fuel_budget()` at `loader.rs:24-41` is never called anywhere in codebase
- **Issue**: `PLUGIN_FUEL_BUDGET` and `PLUGIN_FUEL_LAST_RESET` statics are unreachable
- **Note**: Per-agent fuel tracking via `ModuleCache` is the actual mechanism in use
- **Action**:
  1. Remove dead code: `check_and_reset_fuel_budget()`, `PLUGIN_FUEL_BUDGET`, `PLUGIN_FUEL_LAST_RESET`
  2. Update architecture doc to reflect that only per-plugin fuel tracking is used

**Verification**: Fuel tracking at `loader.rs:262-266` is CORRECT - condition `current_plugin_fuel >= MAX_PLUGIN_FUEL_BUDGET` properly returns early when fuel exhausted

### H-7: Provider ToolDefinition Documentation
**Source**: `plans/provider_review.md`

- **Issue**: Architecture doc has stale comment "input_schema renamed to parameters" - rename already happened
- **Issue**: `register_builtin_with_config` is primary public API but doc shows `register_builtin`
- **Action**:
  1. Remove stale "input_schema renamed to parameters" comment
  2. Document `register_builtin_with_config` as primary entry point

### H-8: Session Event Publishing Clarification
**Source**: `plans/session_review.md`

- **Issue**: Doc line 485 says "SessionSelected, SessionDeleted, SessionRenamed are listed but not currently published" but doesn't clarify which events ARE published
- **Reality**: `SessionCreated` and `MessageAdded` ARE published at `src/bus/events.rs:7,21`
- **Action**: Explicitly state which events ARE published vs NOT published

### H-9: MCP Config Example Update
**Source**: `plans/mcp_review.md`

- **Issue**: Config example shows simplified JSON; actual config uses `McpEntry` with more fields (`server_type`, `env`, `url`, `headers`, `transport`, `timeout`, `oauth`, `reconnect`)
- **Action**: Update config example in architecture doc or reference actual schema

---

## MEDIUM Priority Items

### M-1: Config Validation Documentation ✅ COMPLETED
**Source**: `plans/config_review.md`

Missing documentation for validations that exist in code:
- `tool_timeout_seconds`: cannot be 0 or exceed 3600
- `max_parallel_tools`: cannot be 0 or exceed 100
- Compaction threshold: 0.1-1.0
- Max tokens: at least 1000

**Action**: Document these validations in architecture/config.md

### M-2: Memory Module Documentation ✅
**Source**: `plans/memory_review.md`

1. **frequency_bonus formula**: Added formula `(count - 1) * 2.0` to documentation
2. **File locking mechanism**: Already documented at `mod.rs:497-526`
3. **Namespace format**: Already correct (`project/{hash}` in table)

**Status**: COMPLETE - committed as `bf4decd`

### M-3: IDE Module Documentation
**Source**: `plans/ide_review.md`

1. **Temp file timing**: Doc says temp files dropped BEFORE IDE invocation but actually dropped AFTER (at `mod.rs:168-169,253`)
2. **register_panic_cleanup**: Not documented at `src/ide/mod.rs:65-78`
3. **TempFilesGuard**: Not documented - implements Drop to clean up temp files

**Action**: Fix temp file timing documentation, add missing function documentation

### M-4: Provider Error Retry Status
**Source**: `plans/provider_review.md`

- **Issue**: Need to verify `ProviderError::Auth(_)` `is_retryable()` implementation against `src/error/mod.rs`
- **Action**: Verify and update documentation if needed

### M-5: Security "Used By" Verification
**Source**: `plans/security_review.md`

- **Issue**: "Used by" list (webfetch, websearch, codesearch, mcp/remote) may be incomplete
- **Action**: Verify against actual tool implementations

### M-6: LSP Completion Fallback Behavior
**Source**: `plans/lsp_review.md`

- **Issue**: `operations.rs:282-285` has fallback to `Vec<CompletionItem>`, but `client.rs:412-413` only does `CompletionList` without fallback
- **Action**: Clarify which module handles completion fallback behavior

### M-7: Command normalize_name() Documentation
**Source**: `plans/command_review.md`

- **Issue**: Doc claims "Improved duplicate detection" but doesn't specify mechanism
- **Reality**: Uses `normalize_name()` which lowercases and strips leading `/`
- **Action**: Document this behavioral detail

### M-8: Plugin plugins_dir Cross-Platform
**Source**: `plans/plugin_review.md`

- **Issue**: Doc shows `~/.local/share/codegg/plugins/` (Linux) but actual uses `dirs::data_local_dir()/codegg/plugins` (cross-platform)
- **Action**: Update documentation to reflect cross-platform path

### M-9: Hook InlineScript Handling
**Source**: `plans/hooks_review.md`

- **Issue**: `InlineScript` at `src/hooks/mod.rs:181-184` is deprecated dead code with `#[allow(deprecated)]` but undocumented
- **Action**: Either document it as deprecated or remove the dead code

### M-10: IDE open_diff_generic Indentation
**Source**: `plans/ide_review.md`

- **Issue**: Lines 302-311 have questionable indentation around guard drop placement
- **Action**: Review and fix if needed

### M-11: Exec Timeout Documentation
**Source**: `plans/exec_review.md`

- **Issue**: Doc line 169 says "question tool timeout after 300 seconds" but actual code at `src/exec.rs:121` only calls `setup_question_channel()` without timeout handling
- **Action**: Clarify that timeout is inherited from AgentLoop's general processing

### M-12: Server Auth Middleware Security Review
**Source**: `plans/server_review.md`

- **Issue**: Auth middleware documentation order incorrect - doc claims "reject if none set" but code at `middleware/auth.rs:37-39` actually **allows** requests when no token is configured
- **Action**: Determine if this is intentional security design or bug; update accordingly

### M-13: Core Type Precision
**Source**: `plans/core_review.md`

1. Add `Option<Arc<...>>` wrapper type details to InprocCoreClient field descriptions
2. `Subscribe { session_id }` should specify `session_id: Option<String>`
3. `Resume { session_id, from_event_seq }` should specify `session_id: Option<String>`

### M-14: ToolExecutor Usage Documentation
**Source**: `plans/tool_review.md`

- **Issue**: Documentation may not reflect that `ToolExecutor::execute_with_retry()` IS used by bash, read, and glob tools at `src/tool/executor.rs:72,92,112`
- **Action**: Update documentation to reflect actual usage OR expand retry logic to other tools

### M-15: Worktree force Parameter
**Source**: `plans/worktree_review.md`

- **Issue**: `remove_worktree()` doesn't support `--force` flag
- **Action**: Add `force` parameter for `git worktree remove --force` support
- **Status**: ✅ COMPLETED (Wave 2, Group N)

---

## LOW Priority Items

### L-1: Documentation Formatting Improvements

| Item | Source | Action |
|------|--------|--------|
| Rename `stat_core.rs` to `metrics.rs` | util_review | Consider renaming (would require updating all references) |
| Fix IPv6 unique local range doc | security_review | Clarify code covers fc00::/7 AND fd00::/8 |
| Align Landlock access flags naming | security_review | Use `LANDLOCK_ACCESS_FS_*` constants in doc |
| Add test location reference in PTY doc | pty_session_review | Specify `session.rs` for unit tests |
| Convert Error Categories to Rust code blocks | error_review | Use `#[derive(Error, Debug)]` format |
| Update record_success/record_failure line refs | resilience_review | Update to 139-158 and 160-186 |
| Add ToolCallStarted explicitly to event list | bus_review | Document 36 events clearly |
| Rename "Recent Bug Fixes" to "Design Notes" | lsp_review | Less alarming title |

### L-2: Line Number References

Remove specific line number references throughout architecture docs (they are fragile and become stale). Use function names instead:
- Agent: loop.rs:1764, 1806
- LSP: Various line references
- Core: Various line references

### L-3: Minor Code Improvements

| Item | Location | Description |
|------|----------|-------------|
| Add `has_long_tool_outputs` threshold (2000) to docs | compaction docs | Document this parameter |
| Update Sync Fallback section | compaction docs | More explicit about placeholder message format |
| Document `dispatch_provider()` method | plugin docs | Missing from Hook Flow section |
| Add line reference for CommandRegistry | command docs | At `src/tui/command.rs:72` |
| Fix off-by-one line references | command docs | TUI Command (25-37 not 26-37), Command (8-18 not 9-18) |
| Document pool type as `sqlx::SqlitePool` | server docs | In ServerState |
| Add `/health` endpoint to docs | server docs | Optional route |

### L-4: Namespace and Type Clarifications

| Item | Source | Description |
|------|--------|-------------|
| Clarify session_id type inconsistency | bus_review | Some events use `Arc<str>`, others use `String` |
| Clarify "empty receiver" behavior | core_review | Both stdio and socket subscribe() return channel where receiver is dropped |
| Add 36-event count summary at doc top | bus_review | Make total count prominent |
| Document empty turn_id handling | core_review | Behavior when turn_id is empty string |

---

## Already Completed / Verified Correct

The following items were reviewed and confirmed correct - no action needed:

| Module | Status | Notes |
|--------|--------|-------|
| Upgrade | ✅ Accurate | Architecture doc matches implementation |
| TTS | ✅ Accurate | macOS-only, hardcoded `say` command |
| Storage | ✅ Accurate | SQL pragma values verified |
| Snapshot | ✅ Accurate | All structs match implementation |
| Worktree | ✅ Accurate | All function signatures verified |
| Skills | ✅ Accurate | 38 skill subdirectories verified |
| Security | ✅ Mostly | Functions correct, "Used by" needs verification |
| PTY Session | ✅ Accurate | All 11 unit tests present |
| Resilience | ✅ Accurate | All circuit breaker logic verified |
| Hooks | ✅ Mostly | InlineScript issue noted |
| Core | ✅ Mostly | Type precision suggestions |
| Crypto | ✅ Accurate | Well-documented |
| Config | ✅ Mostly | Missing validation docs |
| Compaction | ✅ Accurate | Implementation correct |
| Bus | ✅ Accurate | 36 events verified correct |
| Command | ✅ Accurate | 41 commands verified |
| Client | ✅ Accurate | Implementation sound |
| IDE | ✅ Mostly | Temp file timing doc bug |
| Memory | ✅ Accurate | All bugs correctly implemented |
| LSP | ✅ Mostly | Server count and completion docs need updates |
| Permission | ⚠️ Issues | PermissionResponse and mode tables need fixes |
| Provider | ✅ Mostly | ToolDefinition comment and config example need updates |
| Session | ✅ Mostly | Event publishing needs clarification |
| MCP | ✅ Mostly | Config example needs update |
| Agent | ⚠️ Issues | Hook invocation and line numbers need fixes |
| Exec | ✅ Accurate | Error handling correct |
| Error | ✅ Accurate | Well-documented |

---

## Implementation Waves (Parallelization Strategy)

Items are organized into waves that can be executed in parallel by different agents. Each item includes specific file locations and verification steps so future agents can implement without heavy research.

### Wave 1: HIGH Priority Documentation Fixes (All Independent - 9 Agents)

| Agent | Items | Module | Description | Files to Modify |
|-------|-------|--------|-------------|-----------------|
| 1 | H-1 | TUI | Line count, fullscreen field, spinner | `architecture/tui.md` |
| 2 | H-2 | Tool | Tool count correction | `architecture/tool.md` |
| 3 | H-3 | LSP | Server count correction | `architecture/lsp.md` |
| 4 | H-4 | Permission | PermissionResponse, mode tables | `architecture/permission.md` |
| 5 | H-5 | Agent | Hook invocation clarification | `architecture/agent-loop.md` |
| 6 | H-6 | Plugin | Dead code removal | `src/plugin/loader.rs`, `architecture/plugin.md` |
| 7 | H-7 | Provider | ToolDefinition comment, API | `architecture/provider.md` |
| 8 | H-8 | Session | Event publishing clarification | `architecture/session.md` |
| 9 | H-9 | MCP | Config example update | `architecture/mcp.md` |

**All 9 items are independent and can run in parallel.**

**H-6 (Plugin Dead Code Removal)** involves actual code changes:
```bash
# Verify dead code location
rg "PLUGIN_FUEL_BUDGET|check_and_reset_fuel_budget" src/plugin/loader.rs

# Files to edit: src/plugin/loader.rs
# Remove lines 15-21 (PLUGIN_FUEL_BUDGET, PLUGIN_FUEL_LAST_RESET statics)
# Remove lines 24-41 (check_and_reset_fuel_budget function)
```

---

### Wave 2: MEDIUM Priority Improvements (14 Groups - Independent)

Each group is independent; agents should pick one group at a time:

| Group | Items | Module | Description | Files to Modify |
|-------|-------|--------|-------------|-----------------|
| A | M-1 ✅ | Config | COMPLETE - validation docs added | `architecture/config.md` |
| B | M-2 ✅ | Memory | COMPLETE - frequency_bonus formula added | `architecture/memory.md` |
| C | M-3, M-10 | IDE | Temp file timing, indentation | `architecture/ide.md`, `src/ide/mod.rs` |
| D | M-4 | Provider | Already verified - no action needed | - |
| E | M-5 | Security | "Used by" list verification | `architecture/security.md` |
| F | M-6 | LSP | Completion fallback clarification | `architecture/lsp.md` |
| G | M-7 | Command | normalize_name() documentation | `architecture/command.md` |
| H | M-8 | Plugin | plugins_dir cross-platform | `architecture/plugin.md` |
| I | M-9 | Hooks | InlineScript deprecation handling | `src/hooks/mod.rs` OR `architecture/hooks.md` |
| J | M-11 | Exec | Timeout documentation | `architecture/exec.md` |
| K | M-12 | Server | Auth middleware security | `architecture/server.md` |
| L | M-13 ✅ | Core | Type precision improvements | `architecture/core.md` |
| M | M-14 ✅ | Tool | ToolExecutor usage docs - CORRECTED | `architecture/tool.md` |
| N | M-15 | Worktree | force parameter consideration | `architecture/worktree.md` |

**M-4 (Provider Error Retry Status) is already VERIFIED - no action needed.**

---

### Wave 3: LOW Priority Polish

| Category | Items | Description | Files to Modify |
|----------|-------|-------------|-----------------|
| Formatting | L-1 | Rename files, fix naming | Various architecture docs |
| Line Numbers | L-2 | Remove fragile refs | All architecture docs |
| Minor Code | L-3 | has_long_tool_outputs, dispatch_provider | `architecture/compaction.md`, `architecture/plugin.md` |
| Clarifications | L-4 | session_id types, empty receiver | `architecture/bus.md`, `architecture/core.md` |

---

## Verified Items (Pre-Implementation Checklist)

Before implementing any item, verify against this list:

| Item | Verified Value | Location |
|------|----------------|----------|
| Tool count = 26 | ✅ | `src/tool/mod.rs:89-119` |
| LSP server count = 39 | ✅ | `src/lsp/server.rs:27-385` |
| PermissionResponse = {level, persist} | ✅ | `src/permission/mod.rs:1142-1145`, no `reason` field |
| Auth middleware allows without token | ✅ | `src/server/middleware/auth.rs:37-39` - intentional dev mode |
| Plugin fuel tracking logic | ✅ CORRECT | Condition NOT inverted - properly returns early when exhausted |
| UiState.fullscreen exists | ✅ | `src/tui/app/state/ui.rs:71` |
| ToolExecutor NOT used by tools | ✅ | architecture/tool.md:205 correctly states "not currently integrated" |
| ProviderError::Auth retryable | ✅ | `src/error.rs:169` |
| Memory frequency_bonus formula | ✅ | `(count - 1) * 2.0` at `patterns.rs:232` |
| SessionCreated, MessageAdded published | ✅ | `src/bus/events.rs:7,21` |
| InlineScript deprecated | ✅ | `#[allow(deprecated)]` at `mod.rs:180` |
| InprocCoreClient uses Option<Arc<>> | ✅ | `src/core/mod.rs:22-28` |
| CommandRegistry at line 72 | ✅ | `src/tui/command.rs:72` |
| register_panic_cleanup exists | ✅ | `src/ide/mod.rs:65-78` |
| Plugin dead code | ✅ | `check_and_reset_fuel_budget()` at `loader.rs:24-41` - never called |
| AppEvent count = 36 | ✅ | `src/bus/events.rs:5-190` |

---

## Items Needing Further Research

Before implementing, investigate these:

1. **Auth middleware security** (`src/server/middleware/auth.rs:37-39`): Is allowing requests without token intentional? This is a security design decision - currently appears to be intentional for development mode.

2. **IDE open_diff_generic** (`src/ide/mod.rs:302-311`): Review guard drop placement for correctness.

3. **Security "Used by" list**: Verify all actual consumers of `ssrf.rs` functions.

---

## Testing Commands

After any changes, run:

```bash
# Build verification
cargo build --all-features

# Lint
cargo clippy --all-features -- -D warnings

# Test
cargo test --all-features

# TUI tests
cargo test tui::input
cargo test tui

# Specific module tests
cargo test --package codegg -- <module>_test_pattern
```

---

## Implementation Notes for Future Agents

1. **Batch processing**: Process 4-5 review files per subagent to avoid context compaction (~2000 line limit)
2. **Plan consolidation pattern**: Subagent reads batch → writes consolidated temp file → parent reads all temp files → creates final plan
3. **Subagent context limits**: Subagents undergo compaction after ~2000 lines
4. **Accurate status tracking**: Many items flagged as "pending" were already fixed - verify before implementing
5. **Line numbers fragile**: Always use code search to find exact locations, never trust line numbers in docs
6. **Verification before assumption**: Many "bugs" in review files turned out to be correctly implemented after direct inspection
7. **Implementation approach**: When implementing, read the current architecture doc first, then verify against actual source code, then make changes only if there's a real discrepancy

---

## Consolidated From

This plan was consolidated from 31 individual module review files. The original review files have been removed; this consolidated plan contains all actionable items.

*(End of file)*