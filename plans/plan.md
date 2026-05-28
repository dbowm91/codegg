# Implementation Plan

**Status**: WAVE 5 - COMPLETED
**Last Updated**: 2026-05-28
**Consolidated from**: All plans/ review files (now removed)

---

## Historical Completed Items

### Wave 5 (2026-05-28)
- **Wave 5A**: COMPLETED - 21 architecture documentation files fixed (all subagents A-E)
- **Wave 5B**: COMPLETED - 5 dead code items removed
- **Wave 5C**: COMPLETED - 4 improvement items implemented, 2 skipped (design decisions)
- **Bonus**: Fixed pre-existing `doom_loop.rs` test compilation error (missing `arguments` parameter)

### Wave 4 (2026-05-27)
- **TUI-5**: COMPLETED - Accessibility (FocusManager + Component trait)
- **LARGE-1**: COMPLETED - Virtual scrolling (MessageLayoutCache with binary search)
- **LARGE-2**: COMPLETED - String interning (StringInterner with DashMap)
- **Quick fixes**: 4 minor issues resolved (OAuth TOCTOU, CANONICAL_PATHS_CACHE, ToolExecutor removed, PermissionResponse removed)

### Previous Waves (R0-R3)
- **R0**: 38 documentation-only fixes
- **R1**: 4 code fixes (low risk)
- **R2**: 4 code fixes (medium risk)
- **R3**: 4 incomplete implementations documented

---

## Wave 5 Completion Summary

### Wave 5A: Documentation Fixes (21 files)

| File | Status | Notes |
|------|--------|-------|
| `architecture/agent.md` | ✅ DONE | AgentLoop fields expanded to 24, instruction files corrected |
| `architecture/overview.md` | ✅ DONE | Tool count=27, feature gate=plugins, 13 DB tables, 16 providers, modules updated |
| `architecture/provider.md` | ✅ DONE | 13 line refs fixed, auto-registration corrected, error path fixed |
| `architecture/config.md` | ✅ DONE | ProviderConfig::merge() signature fixed, merge strategies documented |
| `architecture/tui.md` | ✅ DONE | FocusManager typo/location fixed, 13 Component methods, Stats variant, vim keys |
| `architecture/tool.md` | ✅ DONE | ImageTool/LspTool registration fixed, ToolExecutor removed, error path fixed |
| `architecture/protocol.md` | ✅ DONE | TurnFailed/ToolStarted/ToolCompleted optionality fixed, Special count fixed |
| `architecture/bus.md` | ✅ DONE | TTL 310s fixed in all locations, timeout clarification |
| `architecture/exec.md` | ✅ DONE | setup_question_channel_for_exec() fixed, exec question behavior corrected |
| `architecture/permission.md` | ✅ DONE | Line numbers fixed, TTL=310s, PermissionResponse removed, DoomLoop key fixed, SandboxMode added |
| `architecture/security.md` | ✅ DONE | CANONICAL_PATHS_CACHE TTL/cap, SandboxMode enum, Landlock bitmasks |
| `architecture/core.md` | ✅ DONE | InprocCoreClient.pool type fixed, map_app_event line ref fixed |
| `architecture/mcp.md` | ✅ DONE | McpClientType derive, RemoteClient field types, max_retries fixed |
| `architecture/resilience.md` | ✅ DONE | Duplicate state diagram removed, call() line ref fixed |
| `architecture/session.md` | ✅ DONE | Missing SessionStore methods added |
| `architecture/storage.md` | ✅ DONE | init() code example fixed, v15 description fixed |
| `architecture/crypto.md` | ✅ DONE | EncryptedData visibility fixed |
| `architecture/util.md` | ✅ DONE | pricing.rs, interner.rs, Histogram documented |
| `architecture/tts.md` | ✅ DONE | TUI integration documented |
| `architecture/memory.md` | ✅ DONE | Soft limit clarified |
| `architecture/skills.md` | ✅ DONE | .skills/ directory purpose clarified |

### Wave 5B: Code Fixes (5 items)

| Item | Status | Notes |
|------|--------|-------|
| 5B-1: Remove `setup_question_channel()` | ✅ DONE | Dead code removed from `src/agent/loop.rs` |
| 5B-2: Remove `connect_sse()` | ✅ DONE | Dead code removed from `src/mcp/remote.rs` |
| 5B-3: Remove `run_socket()` | ✅ DONE | Dead code removed from `src/mcp/ide_server.rs` |
| 5B-4: Remove `check_external_directory()` | ✅ DONE | Dead code removed from `src/permission/mod.rs` |
| 5B-5: Remove stale `#[allow(dead_code)]` | ✅ DONE | Annotation removed from `handle_tui` in `src/server/ws.rs` |

### Wave 5C: Improvements (4 done, 2 skipped)

| Item | Status | Notes |
|------|--------|-------|
| 5C-1: CircuitBreakerConfig | ⏭️ SKIPPED | Would change public API; hardcoded 30s is fine as-is |
| 5C-2: Provider registration docs | ✅ DONE | Per-provider independence clarified in provider.md |
| 5C-3: Snapshot config validation | ✅ DONE | Clamping zero values to 1 in `new_with_options()` |
| 5C-4: ProviderCache rename | ✅ DONE | `clear()` → `evict_expired()` in cache.rs |
| 5C-5: Align TTLs | ⏭️ SKIPPED | 10s gap is intentional (permissions survive agent timeout) |
| 5C-6: serde skip_serializing_if | ✅ DONE | 6 optional fields annotated in CoreEvent |

### Bonus: Test Fix

| Item | Status | Notes |
|------|--------|-------|
| `doom_loop.rs` test | ✅ FIXED | Pre-existing compilation error: `record_tool_call` needs 2 args |

---

## Known Issues Remaining

| Issue | Location | Priority | Status |
|-------|----------|----------|--------|
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | LOW | **LEAVE** - macOS say adequate |
| Worktree symlink detection | `src/worktree/mod.rs:69-88` | LOW | **LEAVE** |

---

## Verified Codebase Facts

| Item | Value | Source |
|------|-------|--------|
| Tool count (with_defaults) | 27 | `src/tool/mod.rs:90-122` |
| LSP servers | 39 | `src/lsp/server.rs:27-384` |
| UiState fields | 26 | `src/tui/app/state/ui.rs:27-76` |
| AppEvent variants | 36 | `src/bus/events.rs:5-150` |
| Built-in commands | 46 | `src/tui/command.rs:83-182` |
| DB tables | 13 | `src/session/schema.rs` |
| Feature gate | `plugins` (plural) | `Cargo.toml:172` |
| PermissionRegistry TTL | 310s | `src/bus/mod.rs:59` |
| Agent loop timeout | 300s | `src/agent/loop.rs:494` |
| INSTRUCTION_FILES | AGENTS.md, CLAUDE.md, CONTEXT.md | `src/agent/prompt.rs:7` |
| AgentLoop fields | 24 | `src/agent/loop.rs:559-584` |
| Component trait methods | 13 | `src/tui/components/component.rs:84-113` |
| DialogType variants | 23 (includes Stats) | `src/tui/components/component.rs:22-46` |
| DoomLoop key | `tool_name:hash(tool_name + args)` | `src/permission/mod.rs:1249-1256` |
| EncryptedData | IS pub | `src/crypto/mod.rs:28` |
| CANONICAL_PATHS_CACHE | 300s TTL, 100-entry cap | `src/security/sandbox.rs:259-286` |
| register_builtin() | 15 providers | `src/provider/mod.rs:279-326` |
| register_builtin_with_config() | 16 providers | `src/provider/mod.rs:390-537` |
| Config merge: provider | field-by-field | `src/config/paths.rs:222-235` |
| Config merge: server | field-by-field | `src/config/paths.rs:203-208` |
| Config merge: watcher | field-by-field | `src/config/paths.rs:209-221` |
| Config merge: agents/mcp/commands/modes | key replacement | `src/config/paths.rs:236-281` |
| Config merge: instructions | concatenation | `src/config/paths.rs:266-271` |

---

## Notes for Future Agents

### Implementation Patterns

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT use `await` when calling these.
- **Registration-before-publish pattern**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event.
- **Subprocess PATH**: All tools use `std::env::var_os("PATH")` instead of hardcoded paths.
- **ToolCatalog::register() takes `&dyn Tool`**: Not `Box<dyn Tool>`.
- **Dialog::Info doesn't exist**: Despite `src/tui/components/dialogs/info.rs` existing, `Dialog::Info` is NOT in the Dialog enum.
- **exec mode question behavior**: `setup_question_channel_for_exec()` at `src/exec.rs:121` DOES set `question_rx`. Exec waits up to 300s before timing out.
- **Feature gate is `plugins` (plural)**: Not `plugin` as some docs claim.
- **Tool count is 27** (not 28): verified by counting `with_defaults()` registrations.
- **AgentLoop has 24 fields**: The struct at `src/agent/loop.rs:559-584` has 24 fields; docs list only 15.
- **DialogType is in `component.rs`**: Not in `types.rs`. FocusManager is in `component/focus.rs`.
- **Config merge is heterogeneous**: provider/server/watcher merge field-by-field; agents/mcp/commands/modes use key replacement; instructions concatenates.

### Testing Commands

```bash
cargo build --all-features
cargo clippy --all-features -- -D warnings
cargo test --all-features
cargo test --all-features -- --test-threads=1
cargo test tui::input
cargo test tui
cargo test messages
```
