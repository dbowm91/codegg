# AGENTS.override.md

## Session-Specific Items (2026-05-28)

Items learned during the 2026-05-28 architecture review sweep and plan consolidation.

### Verified Codebase Facts

| Item | Value | Source |
|------|-------|--------|
| Tool count is 27 (not 28) | Verified | `src/tool/mod.rs:90-122` (27 registrations in with_defaults()) |
| LSP server count is 39 | Verified | `src/lsp/server.rs:27-383` |
| Built-in command count is 46 | Verified | `src/tui/command.rs:79-182` |
| UiState has 26 fields | Verified | `src/tui/app/state/ui.rs:27-76` |
| ImageTool IS registered | Verified | `src/tool/mod.rs:102` |
| LspTool IS registered | Verified | `src/tool/mod.rs:113-115` |
| Dialog::Stats EXISTS in Dialog enum | Verified | `src/tui/app/types.rs:21` |
| Snapshot hash uses SHA256 consistently | Verified | `src/snapshot/mod.rs` |
| DB has 13 tables (not 7) | Verified | `src/session/schema.rs:25-69` |
| PermissionRegistry TTL is 310s | Verified | `src/bus/mod.rs:59` |
| Feature gate is `plugins` (plural) | Verified | `Cargo.toml:169` |
| INSTRUCTION_FILES = AGENTS.md, CLAUDE.md, CONTEXT.md | Verified | `src/agent/prompt.rs:7` |
| DoomLoop key is tool_name:hash(tool_name + args) | Verified | `src/permission/mod.rs:1249-1256` |
| CANONICAL_PATHS_CACHE has 300s TTL | Verified | `src/security/sandbox.rs:262` |
| EncryptedData IS pub | Verified | `src/crypto/mod.rs:28` |
| `src/git/mod.rs` is orphaned (not in lib.rs) | Verified | `src/git/mod.rs` |
| AgentLoop has 24 fields | Verified | `src/agent/loop.rs:559-584` |
| DialogType is in component.rs (not types.rs) | Verified | `src/tui/components/component.rs:22` (23 variants) |
| FocusManager is in component/focus.rs (not types.rs) | Verified | `src/tui/components/component/focus.rs:14` |
| register_builtin_with_config() registers 16 providers | Verified | `src/provider/mod.rs:390-537` (includes codegg_go) |
| OAuthManager load_tokens_sync() now logs warnings | Verified | `src/mcp/auth.rs:118-124` (no longer silently ignores) |
| check_external_directory() is at lines 1264-1276 | Verified | `src/permission/mod.rs:1264-1276` |
| Config merge: provider/server/watcher = field-by-field | Verified | `src/config/paths.rs:203-235` |
| Config merge: agents/mcp/commands/modes = key replacement | Verified | `src/config/paths.rs:236-281` |
| Config merge: instructions = concatenation | Verified | `src/config/paths.rs:266-271` |
| register_builtin_with_config() is per-provider, not global | Verified | `src/provider/mod.rs:328-388` |

### Architecture Documentation Issues (from 2026-05-28 review)

| Priority | Count | Description |
|----------|-------|-------------|
| HIGH | 5 | tool code blocks stale, exec.md wrong behavior, AgentLoop missing fields, DB table count wrong |
| MEDIUM | 21 | TTL values, line refs, config merge docs, type annotations |
| LOW | 26 | Line number drift, typos, missing enum variants |

**Full details**: See `plans/plan.md` (Wave 5) - consolidated plan with all fixes

### Known Code Issues - Completed ✅

| Issue | Resolution | Date |
|-------|------------|------|
| ToolExecutor deprecated | **REMOVED** | 2026-05-27 |
| CANONICAL_PATHS_CACHE never clears | **FIXED** - TTL + LRU | 2026-05-27 |
| OAuth replay protection TOCTOU | **FIXED** - atomic entry() | 2026-05-27 |
| PermissionResponse struct unused | **REMOVED** | 2026-05-27 |
| OAuthManager sync error ignore | **FIXED** - now logs warnings | 2026-05-27 |

### Known Code Issues - Remaining (Planned for Wave 5B)

| Issue | Status | Plan Item |
|-------|--------|-----------|
| TTS init() ignores providers | **LEAVE** | Known issue - macOS say adequate |
| Worktree symlink detection | **LEAVE** | Known issue - low priority |
| check_external_directory unused | **PLANNED** | 5B-4: Remove dead code |
| setup_question_channel() dead code | **PLANNED** | 5B-1: Remove dead code |
| MCP connect_sse() dead code | **PLANNED** | 5B-2: Remove dead code |
| MCP run_socket() dead code | **PLANNED** | 5B-3: Remove dead code |
| handle_tui stale dead_code annotation | **PLANNED** | 5B-5: Remove annotation |

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

**Config Merge Behavior** (`src/config/paths.rs:164-284`):
- `merge_option!` macro for simple scalar fields (override with latest non-None)
- `provider`: field-by-field via `ProviderConfig::merge()` (non-None from override replaces base)
- `server`: field-by-field via `ServerConfig::merge()`
- `watcher`: field-by-field (manual if-some checks)
- `agent`, `mcp`, `commands`, `mode`: key replacement (insert overwrites existing keys)
- `instructions`: concatenation (appended to list)

**Provider Auto-Registration** (`src/provider/mod.rs:328-537`):
- `register_builtin()` (line 279-326): registers 15 providers via env var checks
- `register_builtin_with_config()` (line 390-537): per-provider, checks config key first, falls back to env var
- Adding one provider via config does NOT suppress other env-var providers
- Each provider is independent

### Key Verification Commands

```bash
# Count tools in default registry (should be 27)
grep -c "registry.register" src/tool/mod.rs

# Count DB tables (should be 13)
grep -c "CREATE TABLE" src/session/schema.rs

# Count LSP servers (should be 39)
grep -c "id:" src/lsp/server.rs

# Count commands (should be 46)
grep -c "Command::new" src/tui/command.rs

# Count UiState fields (should be 26)
grep -c ":" src/tui/app/state/ui.rs

# Verify ImageTool registration
grep "ImageTool" src/tool/mod.rs

# Verify AgentLoop field count (should be 24)
grep -c "pub " src/agent/loop.rs | head -1
```

*(End of file)*
