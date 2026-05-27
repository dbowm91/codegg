# AGENTS.override.md

## Session-Specific Items (2026-05-27)

Items learned during the 2026-05-27 architecture review session that are useful for future agents working on this codebase.

### Verified Claims from Batch Reviews

The following items were verified during architecture review and should be trusted:

| Item | Value | Source |
|------|-------|--------|
| Tool count is 27 (includes ImageTool) | Verified | `src/tool/mod.rs:89-119` |
| LSP server count is 39 | Verified | `src/lsp/server.rs:27-383` |
| Built-in command count is 46 | Verified | `src/tui/command.rs:79-182` |
| UiState has 26 fields | Verified | `src/tui/app/state/ui.rs:27-76` |
| ImageTool IS registered | Verified | `src/tool/mod.rs:102` |
| Dialog::Stats EXISTS in Dialog enum | Verified | `src/tui/app/types.rs:21` |
| redact_for_export uses "terminal" (correct) | Verified | `src/session/import.rs:133` |
| Server route files DO exist | Verified | `src/server/routes/workspace.rs`, `src/server/routes/project.rs` |

### WebSocket Auth Inconsistency (BUG)

**Issue**: HTTP auth middleware (`src/server/middleware/auth.rs:37-39`) allows requests when no token is configured, but WebSocket auth (`src/server/ws.rs:103-106`) returns 500 when no token is configured.

**Recommendation**: Make behavior consistent - either both allow or both reject when no token is configured.

### StatsDialog Missing (POTENTIAL BUG)

**Issue**: `/stats` command at `src/tui/command.rs:147` uses `Dialog::Stats`, and the Dialog variant EXISTS at `src/tui/app/types.rs:21`, but there is NO `StatsDialog` implementation in `src/tui/components/dialogs/`.

**Recommendation**: Either implement `StatsDialog` or remove the `/stats` command.

### Snapshot Module Issues

1. **restore() missing atomic write**: `src/snapshot/mod.rs:292` does not use temp file + rename pattern like `restore_to_path()` does.

2. **Hash algorithm inconsistency**: `collect_files_sync()` uses MD5 at line 431, but `capture_incremental()` uses SHA256 at line 143.

### MCP Dead Code (Not Bugs - Known Issues)

1. **connect_sse()**: Defined at `src/mcp/remote.rs:698-740` but never called externally
2. **run_socket()**: Defined at `src/mcp/ide_server.rs:121-144` but returns Ok(()) without actual socket handling
3. **McpCli Debug command**: Stub at `src/mcp/cli.rs:309-318` that only prints, doesn't test connections
4. **OAuthManager sync methods**: `load_tokens_sync()` and `load_used_codes_sync()` are marked `#[allow(dead_code)]` but are actually called in `OAuthManager::new()` at `src/mcp/auth.rs:119` with errors silently ignored via `let _`

### Session Planning Notes

When implementing Wave R0 items:
- R0-DOCS items are PURE documentation fixes - safe to parallelize
- R1-CODE items are isolated low-risk code fixes - safe to parallelize
- R2-CODE items involve actual code changes - review carefully
- R3-IMPL items may need design discussion before implementation

### Key Verification Commands

```bash
# Count LSP servers
grep -c "id:" src/lsp/server.rs

# Count commands
grep -c "Command::new" src/tui/command.rs

# Count UiState fields
grep -c ":" src/tui/app/state/ui.rs

# Verify ImageTool registration
grep "ImageTool" src/tool/mod.rs
```

### Architecture Review Batch Files

The following batch review files were consolidated into plan.md - DO NOT reference them:
- batch1_compaction_config_core_review.md (REMOVED)
- batch1_hooks_ide_lsp_review.md (REMOVED)
- batch1_agent_bus_review.md (REMOVED)
- batch1_permission_plugin_protocol_review.md (REMOVED)
- batch1_client_command_shell_review.md (REMOVED)
- batch1_mcp_memory_overview_review.md (REMOVED)
- batch1_provider_resilience_security_review.md (REMOVED)
- batch1_crypto_error_exec_review.md (REMOVED)
- batch2_server_session_skills_review.md (REMOVED)
- batch2_snapshot_storage_tool_review.md (REMOVED)
- batch2_tts_tui_upgrade_review.md (REMOVED)
- batch2_util_worktree_review.md (REMOVED)
- consolidated_plan_draft.md (REMOVED)