# Implementation Plan - Documentation Corrections (Phase 2)

**Status**: Completed
**Created**: 2026-05-25
**Last Updated**: 2026-05-25 (consolidated from 33 module review files)

---

## Summary

This plan consolidates remaining items from Phase 1 review that need attention. Items have been verified against the actual codebase. Wave 1 contains a code bug fix; Wave 2 contains documentation corrections that can be done in parallel.

### Items Status

| Category | Count | Status |
|----------|-------|--------|
| Code Bugs | 1 | Completed (2026-05-25) |
| Documentation Corrections | 7 | Completed (2026-05-25) |

### Completion Notes

- **1.1**: Fixed `find_command_files()` to use `filter_map(|r| r.ok())` instead of panicking on errors
- **2.1**: Updated overview.md to show 13 components and 20 dialogs
- **2.2**: Added `heartbeat_token` and `heartbeat_cancellation` fields to McpConnectionManager doc
- **2.3**: Added explicit CoreRequest variants enumeration to core.md
- **2.4**: Updated LSP server count from 44 to 39 in lsp.md
- **2.5**: Fixed config.md line number from 157-158 to 163
- **2.6**: Updated command count from 36 to 41 in command.md and SKILL.md

---

## Wave 1: Code Bug Fix

### 1.1 Command Module Panic on Error
**File**: `src/command/mod.rs:20-25`
**Severity**: High
**Issue**: `find_command_files()` panics with `panic!("expected")` on error instead of gracefully skipping failed commands
**Context**: The sync version `find_command_files_sync()` returns `Vec<Result<Command, String>>` - each element is either `Ok(Command)` or `Err(String)`. The async wrapper should filter out errors, not panic.
**Current Code**:
```rust
pub async fn find_command_files(base: &Path) -> Vec<Command> {
    find_command_files_sync(base).into_iter().map(|r| r.unwrap_or_else(|e| {
        warn!("Failed to load command: {}", e);
        panic!("expected")
    })).collect()
}
```

**Fix**: Change to use `filter_map(|r| r.ok())` pattern:
```rust
pub async fn find_command_files(base: &Path) -> Vec<Command> {
    find_command_files_sync(base)
        .into_iter()
        .filter_map(|r| r.ok())
        .collect()
}
```

**Verification**: After fix, run `cargo test` to ensure no regressions. The test module at lines 187-268 covers command template execution but does not have direct tests for `find_command_files` - consider adding a test to prevent future regressions.

---

## Wave 2: Documentation Corrections (Parallel)

### 2.1 Overview Architecture - Component/Dialog Counts
**File**: `architecture/overview.md`
**Issue**: Inconsistent component/dialog counts within the same document
**Details**:
- Line 25 (ASCII diagram): Shows "Components (14)" and "Dialogs (20)"
- Lines 285-286 (TUI Module table): Claims "17 reusable widgets" and "21 modal dialogs"

**Actual counts** (verified via `src/tui/components/` and `src/tui/components/dialogs/`):
- Components: **13 files** (toast, sidebar, scroll, tool_output, spinner, messages, diff, footer, notification, help_overlay, image, prompt, completion_overlay)
- Dialogs: **20 files** (tree, mcp, model, template, share, plan, theme, permission, question, session, diff, agent, help, info, keybind, confirm, import, goto, connect, command)

**Fix**: Update lines 285-286 to say "13 reusable widgets" and "20 modal dialogs" to match actual counts

### 2.2 MCP Architecture - Missing Struct Fields
**File**: `architecture/mcp.md:109-119`
**Issue**: The `McpConnectionManager` struct in the architecture doc is missing 2 fields that exist in the actual implementation at `src/mcp/remote.rs:29-41`
**Actual struct fields**:
```rust
pub struct McpConnectionManager {
    client: RemoteClient,
    state: Arc<Mutex<ConnectionState>>,
    retry_count: Arc<AtomicU64>,
    max_retries: u64,
    base_delay: Duration,
    max_delay: Duration,
    heartbeat_interval: Duration,
    heartbeat_token: CancellationToken,                      // MISSING from doc
    heartbeat_cancellation: Arc<Mutex<Option<CancellationToken>>>, // MISSING from doc
    shutdown: Arc<Notify>,
    reconnect_needed: Arc<Notify>,
}
```

**Fix**: Add the two missing fields to the architecture documentation between `heartbeat_interval` and `shutdown`

### 2.3 Core Architecture - Explicit CoreRequest Variants
**File**: `architecture/core.md:56-60`
**Issue**: The "Request Families" section embeds variant names in descriptive text rather than explicitly listing them. This makes it harder to discover all variants.
**Current text**:
```
### Request Families

- Session lifecycle: list, create, load, attach, fork, delete, archive, restore, share, unshare, rename, export, import, create-from-template, initialize, subscribe, resume
- Turn lifecycle: submit, cancel, steer, agent select, model select
...
```

**Fix**: Add explicit enumeration after the descriptive text:
```rust
### Request Families

- Session lifecycle: list, create, load, attach, fork, delete, archive, restore, share, unshare, rename, export, import, create-from-template, initialize, subscribe, resume
- Turn lifecycle: submit, cancel, steer, agent select, model select

#### Explicit CoreRequest Variants

The `CoreRequest` enum (in `src/protocol/core.rs`) contains these variants:
- `Initialize` - Initialize session
- `Subscribe { session_id }` - Subscribe to session events
- `Resume { session_id, from_event_seq }` - Resume from event sequence
- `TurnCancel { session_id, turn_id }` - Cancel a turn
- `TurnSteer { session_id, turn_id, text }` - Steer with text
- `AgentSelect { session_id, agent_name }` - Select agent
- `ModelSelect { session_id, model }` - Select model
- (Plus additional variants for list, create, load, etc.)

See `src/protocol/core.rs` for complete enum definition.
```

### 2.4 LSP Architecture - Server Count
**File**: `architecture/lsp.md:229`
**Issue**: Says "44 servers" but actual count is **39**
**Actual servers** (from `src/lsp/server.rs:29-383`):
1. rust-analyzer, 2. gopls, 3. pyright, 4. typescript-language-server, 5. jdtls, 6. clangd, 7. omnisharp, 8. kotlin-language-server, 9. lua-language-server, 10. haskell-language-server, 11. metals, 12. elixir-ls, 13. clojure-lsp, 14. vue-language-server, 15. svelte-language-server, 16. yaml-language-server, 17. taplo, 18. bash-language-server, 19. terraform-ls, 20. zls, 21. marksman, 22. dockerfile-language-server, 23. sql-language-server, 24. ruby-lsp, 25. php-language-server, 26. swift-sourcekit, 27. dart-analysis-server, 28. erlang-ls, 29. html-language-server, 30. css-language-server, 31. json-language-server, 32. solidity-language-server, 33. perl-language-server, 34. powershell-editor-services, 35. graphql-language-server, 36. buf-language-server, 37. r-languageserver, 38. nimlsp, 39. vls

**Fix**: Update line 229 to say "Supported Languages (39 servers)"

### 2.5 Config Architecture - Line Number Reference
**File**: `architecture/config.md:221`
**Issue**: Mentions `watcher.rs:157-158` but actual line is **163**
**Verification**: Confirmed `decrypt_provider_keys(&mut config)` is at `src/config/watcher.rs:163` in the `reload_config()` function
**Fix**: Update line 221 to say `watcher.rs:163` (line 225 about `schema.rs:542` is correct)

### 2.6 Command Architecture - Built-in Command Count
**Files**: `architecture/command.md:52, 115`, `.opencode/skills/command/SKILL.md:68, 175`
**Issue**: Documents 36 built-in commands but actual count is **41**
**Actual commands** (from `src/tui/command.rs:78-163`):
1. /connect, 2. /exit, 3. /status, 4. /themes, 5. /help, 6. /sessions, 7. /new, 8. /share, 9. /unshare, 10. /rename, 11. /compact, 12. /timeline, 13. /fork, 14. /undo, 15. /redo, 16. /export, 17. /import, 18. /timestamps, 19. /thinking, 20. /models, 21. /models-refresh, 22. /variants, 23. /agents, 24. /mcps, 25. /workspaces, 26. /tree, 27. /editor, 28. /keybinds, 29. /context, 30. /cost, 31. /usage, 32. /tui, 33. /loop, 34. /tasks, 35. /task-del, 36. /memory, 37. /memory-search, 38. /memory-list, 39. /memory-remember, 40. /memory-forget, 41. /memory-consolidate

**Fix**: Update all occurrences of "36" to "41" in both files

---

## Implementation Guidance

### Parallelization Strategy

**Wave 1** (Code Bug - must be done first):
- Agent 1: Fix command/mod.rs panic bug

**Wave 2** (Documentation - all items independent, can run in parallel):
- Agent 2: Overview architecture counts (section 2.1)
- Agent 3: MCP architecture missing fields (section 2.2)
- Agent 4: Core architecture explicit variants (section 2.3)
- Agent 5: LSP architecture server count (section 2.4)
- Agent 6: Config architecture line number (section 2.5)
- Agent 7: Command architecture count (section 2.6)

### Dependencies

- Wave 1 has no prerequisites
- Wave 2 documentation items have no dependencies on each other
- Wave 2 can start immediately and run in parallel with Wave 1

### Verification Commands

After each change:
```bash
cargo check  # Should pass
```

After all changes:
```bash
cargo test   # All tests should pass
```

---

## Notes for Future Agents

1. **Always verify documentation claims against actual code** - the original review files contained some inaccuracies that were discovered during verification
2. **TUI component/dialog counts** may change as the codebase evolves - use the actual file listings rather than relying on documentation
3. **MCP Heartbeat fields**: The `heartbeat_token` and `heartbeat_cancellation` fields manage the heartbeat task lifecycle via `CancellationToken`
4. **Command module has no direct tests for `find_command_files`** - the existing tests cover template execution but not the file discovery function

---

*Plan consolidated from 33 module review files (2026-05-25)*