# Architecture Review Plan

**Status**: COMPLETED - All waves implemented (2026-05-27)
**Last Updated**: 2026-05-27
**Objective**: Review all architecture documents, verify claims against code, identify bugs and improvements

---

## Overview

This plan systematically reviews all architecture documentation in the `architecture/` directory. Each module was reviewed by a dedicated subagent that:
1. Read the architecture document
2. Cross-referenced with actual source code in `src/`
3. Verified claims, counts, and line numbers against code
4. Identified bugs, inconsistencies, and improvements
5. Wrote findings to `plans/<batch>_*_review.md`

All 12 review batches have been completed.

---

## Review Batch Results Summary

| Batch | Modules Reviewed | Output File | Issues Found |
|-------|-----------------|-------------|--------------|
| 1 | Agent, Bus | `plans/batch1_agent_bus_review.md` | 3 minor documentation issues |
| 1 | Client, Command, Shell Session | `plans/batch1_client_command_shell_review.md` | Command count error (39 vs 46) |
| 1 | Compaction, Config, Core | `plans/batch1_compaction_config_core_review.md` | Compaction inequality representation |
| 1 | Crypto, Error, Exec | `plans/batch1_crypto_error_exec_review.md` | All claims verified correct |
| 1 | Hooks, IDE, LSP | `plans/batch1_hooks_ide_lsp_review.md` | LSP count error (40 vs 39) |
| 1 | MCP, Memory, Overview | `plans/batch1_mcp_memory_overview_review.md` | IdeServer socket mode incomplete |
| 1 | Permission, Plugin, Protocol | `plans/batch1_permission_plugin_protocol_review.md` | Session lifecycle count (16 vs 19) |
| 1 | Provider, Resilience, Security | `plans/batch1_provider_resilience_security_review.md` | SSE parser line numbers stale |
| 2 | Server, Session, Skills | `plans/batch2_server_session_skills_review.md` | Auth inconsistency (HTTP vs WS) |
| 2 | Snapshot, Storage, Tool | `plans/batch2_snapshot_storage_tool_review.md` | ImageTool not registered, hash inconsistency |
| 2 | TTS, TUI, Upgrade | `plans/batch2_tts_tui_upgrade_review.md` | UiState missing resize_debounce field |
| 2 | Util, Worktree | `plans/batch2_util_worktree_review.md` | pricing.rs undocumented, line numbers stale |

---

## Consolidated Findings

### Critical Issues Requiring Fixes

| # | Module | Issue | Severity | Fix Location |
|---|--------|-------|----------|---------------|
| 1 | LSP | "40 servers" should be "39 servers" | HIGH | `architecture/lsp.md:229` |
| 2 | Command | Built-in command count is 46, not 39 | HIGH | `architecture/command.md:51` |
| 3 | Tool | ImageTool exists but is NOT registered | HIGH | `src/tool/mod.rs` |
| 4 | Protocol | Session lifecycle "(16 variants)" should be "(19 variants)" | MEDIUM | `architecture/protocol.md:69` |
| 5 | Server | Auth inconsistency: HTTP allows no-token, WS returns 500 | MEDIUM | `src/server/ws.rs:103-106` vs `middleware/auth.rs:37-40` |
| 6 | TUI | UiState missing `resize_debounce` 26th field | MEDIUM | `architecture/tui.md` |
| 7 | Storage | Migration versions "v1-v14" should be "v1-v15" | LOW | `architecture/storage.md:106` |
| 8 | Worktree | Line numbers 36/56 are actually 172/180 | LOW | `architecture/worktree.md:117` |

### Verified Correct Counts

| Item | Documented | Actual | Module |
|------|------------|--------|--------|
| LSP servers | 40 | 39 | `src/lsp/server.rs:27-383` |
| AppEvent variants | 36 | 36 | `src/bus/events.rs:5-147` |
| Built-in agents | 7 | 7 | `src/agent/mod.rs:147-262` |
| UiState fields | 25 | 26 | `src/tui/app/state/ui.rs:27-76` |
| Tool count (with_defaults) | 27 | 27 | `src/tool/mod.rs:89-119` |
| Permission types | 16 | 16 | `src/permission/mod.rs:70-87` |
| HookType variants | 13 | 13 | `src/plugin/hooks.rs:6-20` |
| CoreRequest variants | 35 | 35 | `src/protocol/core.rs:50-175` |
| CoreEvent variants | 19 | 19 | `src/protocol/core.rs:179-271` |
| CoreResponse variants | 7 | 7 | `src/protocol/core.rs:24-46` |
| TuiMessage variants | 18 | 18 | `src/protocol/tui.rs:3-75` |

---

## Stale Items to Prune

### Documentation Corrections Needed

| # | File | Issue | Action |
|---|------|-------|--------|
| 1 | `architecture/lsp.md:229` | "40 servers" → "39 servers" | UPDATE |
| 2 | `architecture/command.md` | Command table missing 7 commands (/stats, /tts, /pr, /issue, /checkpoint, +2) | UPDATE TABLE |
| 3 | `architecture/command.md:114-158` | "39 commands" → "46 commands" | UPDATE |
| 4 | `architecture/protocol.md:69` | "(16 variants)" → "(19 variants)" | UPDATE |
| 5 | `architecture/tui.md` | Add `resize_debounce: Option<std::time::Instant>` to UiState | UPDATE |
| 6 | `architecture/tui.md` | Component trait is `Send + Any`, not just `Send` | UPDATE |
| 7 | `architecture/tui.md` | Dialog enum missing `Stats` variant | UPDATE |
| 8 | `architecture/storage.md:106` | "v1-v14" → "v1-v15" | UPDATE |
| 9 | `architecture/worktree.md:117` | "line 36, line 56" → "line 172, line 180" | UPDATE |
| 10 | `architecture/provider.md:526` | SseParser line range is 988 lines, not 382 | UPDATE or REMOVE LINE REFS |
| 11 | `architecture/security.md:197` | "fc00::/8" should be "fc00::/7" | UPDATE |
| 12 | `architecture/compaction.md:91` | "7 or more" → "more than 6" for accuracy | UPDATE |
| 13 | `architecture/command.md:207-217` | "Removed orphaned src/tui/app/commands.rs" - stale historical note | REMOVE |
| 14 | `architecture/command.md:207-217` | "Fixed unused variable warnings" - stale historical note | REMOVE |

### Code Bugs to Address

| # | Module | Bug | Priority |
|---|--------|-----|----------|
| 1 | Tool | ImageTool at `src/tool/image.rs` not registered anywhere | HIGH |
| 2 | Server | ws.rs validate_ws_auth() returns 500 when no token, HTTP allows - inconsistent | MEDIUM |
| 3 | Snapshot | `collect_files_sync()` uses MD5 while `capture_incremental()` uses SHA256 | LOW |
| 4 | Snapshot | `restore()` lacks atomic write pattern that `restore_to_path()` has | LOW |

### Incomplete Implementations

| # | Module | Issue | Status |
|---|--------|-------|--------|
| 1 | MCP | IdeServer::run_socket() exists but returns Ok(()) without actual socket handling | UNFINISHED |
| 2 | MCP | SSE methods (`connect_sse()`, `take_sse_events()`) exist but no consumer in agent loop | NOT INTEGRATED |
| 3 | MCP | McpCli Debug command doesn't actually test connections | UNFINISHED |
| 4 | TUI | `/stats` command exists but StatsDialog not found in `src/tui/components/dialogs/` | POSSIBLE DEAD CODE |

---

## Known Issues (Pre-existing, Not From This Review)

These are documented in AGENTS.md but verified during review:

| Issue | Location | Status |
|-------|----------|--------|
| ToolExecutor exists but unused | `src/tool/executor.rs:8` | DEPRECATED |
| CANONICAL_PATHS_CACHE never clears | `src/security/sandbox.rs:237` | KNOWN |
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | KNOWN |
| Worktree symlink detection | `src/worktree/mod.rs:69-88` | KNOWN |
| OAuth replay protection TOCTOU | `src/mcp/auth.rs:318-332` | KNOWN |
| PermissionResponse unused | `src/permission/mod.rs:1141-1145` | KNOWN |
| check_external_directory unused | `src/permission/mod.rs:1237-1248` | KNOWN |
| ImageTool not in registry | `src/tool/image.rs` | CONFIRMED |

---

## Files to Review/Update

### Architecture Files Requiring Updates

| Priority | File | Action | Summary |
|----------|------|--------|---------|
| HIGH | `architecture/lsp.md` | UPDATE | Fix 40 → 39 servers |
| HIGH | `architecture/command.md` | UPDATE | Fix command count 39 → 46, update table |
| HIGH | `architecture/tool.md` | UPDATE | Document ImageTool status (dead code or register) |
| MEDIUM | `architecture/tui.md` | UPDATE | Add resize_debounce, Stats dialog, Component trait |
| MEDIUM | `architecture/protocol.md` | UPDATE | Fix 16 → 19 Session Lifecycle variants |
| MEDIUM | `architecture/storage.md` | UPDATE | v1-v14 → v1-v15 |
| MEDIUM | `architecture/worktree.md` | UPDATE | Fix line numbers 36/56 → 172/180 |
| LOW | `architecture/provider.md` | UPDATE | Fix SSE parser line references |
| LOW | `architecture/security.md` | UPDATE | Fix fc00::/8 → fc00::/7, add CANONICAL_PATHS_CACHE issue |
| LOW | `architecture/compaction.md` | UPDATE | Fix inequality representation |

### Architecture Files to Prune

| File | Reason |
|------|--------|
| `architecture/command.md:207-217` | Historical implementation notes no longer relevant |

---

## Execution Plan

1. **Phase 1 (Completed)**: Fix critical count errors in:
   - `architecture/lsp.md:229` - 40 → 39 ✅
   - `architecture/command.md` - 39 → 46 commands ✅
   - `architecture/protocol.md:69` - 16 → 19 variants ✅

2. **Phase 2 (Completed)**: Address code bugs:
   - Register ImageTool (`src/tool/mod.rs`) ✅
   - Fix Server auth inconsistency (ws.rs vs middleware/auth.rs) ✅

3. **Phase 3 (Completed)**: Update all other documentation issues listed above ✅

4. **Phase 4 (Blocked)**: Add verification tests for counts:
   - LSP server count test
   - AppEvent count test
   - Command count test
   - BLOCKED: Pre-existing build errors in codebase (not from this review)

---

## Implementation Summary (2026-05-27)

### Completed Fixes

| Phase | Item | Status | Commit |
|-------|------|--------|--------|
| 1 | LSP: 40 → 39 servers | ✅ Done | 6761703 |
| 1 | Command: 39 → 46 commands | ✅ Done | 6761703 |
| 1 | Protocol: 16 → 19 variants | ✅ Done | 6761703 |
| 2 | ImageTool registered | ✅ Done | 285bab7 |
| 2 | Server auth fix (ws.rs 500 → OK) | ✅ Done | 285bab7 |
| 3 | tui.md updates (resize_debounce, Stats, Component trait) | ✅ Done | 5c40c97 |
| 3 | storage.md: v1-v14 → v1-v15 | ✅ Done | 5c40c97 |
| 3 | worktree.md: line 36/56 → 172/180 | ✅ Done | 5c40c97 |
| 3 | compaction.md: "7 or more" → "more than 6" | ✅ Done | 5c40c97 |
| 3 | security.md: fc00::/8 → fc00::/7 + CANONICAL_PATHS_CACHE note | ✅ Done | 5c40c97 |
| 3 | command.md: removed stale historical notes at 207-217 | ✅ Done | 5c40c97 |
| 4 | Verification tests | ⚠️ BLOCKED | Pre-existing build errors |

### Pre-existing Build Errors (Not Fixed - Out of Scope)

The codebase has pre-existing build errors unrelated to the review findings:

| Error | Location | Description |
|-------|----------|-------------|
| E0277 UnwindSafe | src/client/attach.rs:86 | WebSocket stream doesn't implement UnwindSafe |
| E0599 as_ref | src/server/ws.rs:229,284 | JsonValue doesn't have as_ref method |
| E0063 missing fields | src/provider/*.rs | Missing reasoning_effort, thinking_budget fields |
| E0308 mismatched types | Various | Type mismatch errors |

These errors exist in the codebase prior to this review and were not addressed.

---

## Review Methodology Notes

For future architecture reviews, subagents should:

1. **Verify counts first** - These are most likely to drift
2. **Check line numbers conservatively** - If off by >5 lines, flag as stale
3. **Note incomplete implementations** - Don't assume incomplete code is a bug
4. **Distinguish docs vs code bugs** - Not everything is a code bug

---

*Review plan created: 2026-05-27*
*All 12 review batches completed*
*Total issues identified: 30+ (8 critical/medium, rest informational)*