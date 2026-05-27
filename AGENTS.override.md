# AGENTS.override.md

## Session-Specific Items (2026-05-27)

Items learned during the 2026-05-27 architecture review session that are useful for future agents working on this codebase.

### Verified Codebase Facts

| Item | Value | Source |
|------|-------|--------|
| Tool count is 27 | Verified | `src/tool/mod.rs:89-119` |
| LSP server count is 39 | Verified | `src/lsp/server.rs:27-383` |
| Built-in command count is 46 | Verified | `src/tui/command.rs:79-182` |
| UiState has 26 fields | Verified | `src/tui/app/state/ui.rs:27-76` |
| ImageTool IS registered | Verified | `src/tool/mod.rs:102` |
| Dialog::Stats EXISTS in Dialog enum | Verified | `src/tui/app/types.rs:21` |
| Snapshot hash uses SHA256 consistently | Verified | `src/snapshot/mod.rs` |
| MCP SSE connect_sse() is dead code | Verified | `src/mcp/remote.rs:698-740` |
| MCP run_socket() is dead code | Verified | `src/mcp/ide_server.rs:121-144` |

### Known Code Issues

| Issue | Location | Priority |
|-------|----------|----------|
| ToolExecutor deprecated | `src/tool/executor.rs:8` | MEDIUM |
| CANONICAL_PATHS_CACHE never clears | `src/security/sandbox.rs:237` | MEDIUM |
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | LOW |
| Worktree symlink detection | `src/worktree/mod.rs:69-88` | LOW |
| OAuth replay protection TOCTOU | `src/mcp/auth.rs:318-332` | MEDIUM |

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

### Architecture Review Notes

Waves R0-R3 completed (54 items, 25+ PRs). Wave 4 deferred (TUI-5, LARGE-1, LARGE-2).

*(End of file)*