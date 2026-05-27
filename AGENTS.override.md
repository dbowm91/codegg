# AGENTS.override.md

## Session-Specific Items (2026-05-27)

Items learned during the 2026-05-27 architecture review session useful for future agents.

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

| Issue | Location | Complexity | Action |
|-------|----------|------------|--------|
| ToolExecutor deprecated | `src/tool/executor.rs:8` | LOW | **REMOVE** - safe to delete |
| CANONICAL_PATHS_CACHE never clears | `src/security/sandbox.rs:237` | MEDIUM | **FIX** - add TTL or LRU |
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | HIGH | **LEAVE** - macOS say works |
| Worktree symlink detection | `src/worktree/mod.rs:69-88` | LOW | **LEAVE** |
| OAuth replay protection TOCTOU | `src/mcp/auth.rs:318-332` | MEDIUM | **FIX** - security issue |
| PermissionResponse struct unused | `src/permission/mod.rs:1141-1145` | LOW | **REMOVE** - no consumers |
| check_external_directory unused | `src/permission/mod.rs:1237-1248` | LOW | **LEAVE** or **REMOVE** |

### Wave 4 Active Items (Detailed)

**TUI-5: Accessibility Improvements**
- FocusManager is modal-only, Tab consumed in handle_dialog_key()
- Implementation: Add focus_index tracking, intercept Tab before component
- See `plans/plan.md` for detailed steps

**LARGE-1: Virtual Scrolling for Messages**
- O(n) linear scan on every render (4-5 passes through messages)
- Implementation: MessageLayoutCache with binary search
- See `plans/plan.md` for detailed steps

**LARGE-2: String Interning System**
- DashMap available, Hot spot: ToolRegistry::definitions()
- Implementation: Global StringInterner static
- See `plans/plan.md` for detailed steps

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

Waves R0-R3 completed (54 items, 25+ PRs). Wave 4 now ACTIVE:
- TUI-5: MEDIUM complexity
- LARGE-1: HIGH complexity  
- LARGE-2: LOW complexity
- Quick fixes (FIX-1 to FIX-4) also available

*(End of file)*