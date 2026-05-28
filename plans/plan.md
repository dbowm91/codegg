# Implementation Plan

**Status**: COMPLETED (all waves finished 2026-05-28)
**Last Updated**: 2026-05-28

---

## Completed Work Summary

All waves (R0-R5) completed. Key achievements:
- 21 architecture documentation files corrected
- 5 dead code items removed
- 4 improvements implemented (SnapshotOptions clamping, ProviderCache rename, serde skip_serializing_if, provider registration docs)
- Pre-existing `doom_loop.rs` test fixed
- Virtual scrolling, string interning, accessibility (FocusManager) implemented

---

## Known Issues Remaining

| Issue | Location | Priority | Status |
|-------|----------|----------|--------|
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | LOW | **LEAVE** - macOS say adequate |
| Worktree symlink detection | `src/worktree/mod.rs:69-88` | LOW | **FIXED** - now uses `paths_match()` with proper symlink resolution |

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
