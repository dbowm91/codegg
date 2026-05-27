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

### Known Code Issues - Completed ✅

| Issue | Resolution | Date |
|-------|------------|------|
| ToolExecutor deprecated | **REMOVED** | 2026-05-27 |
| CANONICAL_PATHS_CACHE never clears | **FIXED** - TTL + LRU | 2026-05-27 |
| OAuth replay protection TOCTOU | **FIXED** - atomic entry() | 2026-05-27 |
| PermissionResponse struct unused | **REMOVED** | 2026-05-27 |

### Known Code Issues - Remaining

| Issue | Status | Notes |
|-------|--------|-------|
| TTS init() ignores providers | **LEAVE** | macOS say works, would need AVFoundation |
| Worktree symlink detection | **LEAVE** | Low priority |
| check_external_directory unused | **LEAVE** | Low priority |

### Wave 4 Implementation Status ✅

| Item | Complexity | Status |
|------|------------|--------|
| TUI-5: Accessibility | MEDIUM | ✅ COMPLETED |
| LARGE-1: Virtual Scrolling | HIGH | ✅ COMPLETED |
| LARGE-2: String Interning | LOW | ✅ COMPLETED |
| OAuth TOCTOU Fix | LOW | ✅ COMPLETED |
| CANONICAL_PATHS_CACHE Fix | MEDIUM | ✅ COMPLETED |
| ToolExecutor Removal | LOW | ✅ COMPLETED |
| PermissionResponse Removal | LOW | ✅ COMPLETED |

### Key Implementation Details

**String Interning** (`src/util/interner.rs`):
- Uses DashMap for thread-safe storage
- Global static TOOL_STRING_INTERNER
- tool_interner() function for access

**Virtual Scrolling** (`src/tui/components/messages/layout.rs`):
- MessageLayoutCache with binary search
- O(log n) vs O(n) for visible range lookup
- Cache invalidation in 9 mutation methods

**Accessibility** (`src/tui/components/component.rs`, `focus.rs`):
- Component trait extended with 5 new focus methods
- FocusManager intercepts Tab before component
- ConfirmDialog implements focusable navigation

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

*(End of file)*