# AGENTS.override.md

## Session-Specific Items (2026-05-28)

Items learned during the 2026-05-28 architecture review sweep useful for future agents.

### Verified Codebase Facts

| Item | Value | Source |
|------|-------|--------|
| Tool count is 28 (not 27) | Verified | `src/tool/mod.rs:90-122` (includes tool_search) |
| LSP server count is 39 | Verified | `src/lsp/server.rs:27-383` |
| Built-in command count is 46 | Verified | `src/tui/command.rs:79-182` |
| UiState has 26 fields | Verified | `src/tui/app/state/ui.rs:27-76` |
| ImageTool IS registered | Verified | `src/tool/mod.rs:102` |
| Dialog::Stats EXISTS in Dialog enum | Verified | `src/tui/app/types.rs:21` |
| Snapshot hash uses SHA256 consistently | Verified | `src/snapshot/mod.rs` |
| DB has 13 tables (not 7) | Verified | `src/session/schema.rs:25-69` |
| PermissionRegistry TTL is 310s | Verified | `src/bus/mod.rs:59` |
| Feature gate is `plugins` (plural) | Verified | `Cargo.toml:169` |
| INSTRUCTION_FILES = AGENTS.md, CLAUDE.md, CONTEXT.md | Verified | `src/agent/prompt.rs:7` |
| DoomLoop key is tool_name:hash(args) | Verified | `src/permission/mod.rs:1249` |
| CANONICAL_PATHS_CACHE has 300s TTL | Verified | `src/security/sandbox.rs:262` |
| EncryptedData IS pub | Verified | `src/crypto/mod.rs:28` |
| `src/git/mod.rs` is orphaned (not in lib.rs) | Verified | `src/git/mod.rs` |

### Architecture Documentation Issues (from 2026-05-28 review)

| Priority | Count | Description |
|----------|-------|-------------|
| HIGH | 5 | tool code blocks stale, exec.md wrong behavior, AgentLoop missing fields, DB table count wrong |
| MEDIUM | 21 | TTL values, line refs, config merge docs, type annotations |
| LOW | 26 | Line number drift, typos, missing enum variants |

**Full details**: See `plans/review_consolidated.md`

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
| setup_question_channel() dead code | **LEAVE** | Non-exec version never called |
| MCP connect_sse() dead code | **LEAVE** | Never called externally |
| MCP run_socket() dead code | **LEAVE** | Unix socket server never wired up |

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

# Count tools in default registry
grep -c "register(" src/tool/mod.rs

# Check DB table count
grep -c "CREATE TABLE" src/session/schema.rs
```

*(End of file)*