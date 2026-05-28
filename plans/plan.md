# Implementation Plan

**Status**: WAVE 5 - Architecture Documentation & Code Fixes
**Last Updated**: 2026-05-28

---

## Historical Completed Items

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

## Known Issues Remaining (from prior waves)

| Issue | Location | Priority | Status |
|-------|----------|----------|--------|
| TTS init() ignores providers | `src/tts/mod.rs:45-49` | LOW | **LEAVE** - macOS say adequate |
| Worktree symlink detection | `src/worktree/mod.rs:69-88` | LOW | **LEAVE** |
| check_external_directory unused | `src/permission/mod.rs:1264-1276` | LOW | **LEAVE** or remove if desired |

---

## Wave 5: Architecture Documentation & Code Fixes

All items below are organized into parallelizable waves. Items within a wave can be done concurrently across files. Items marked [CODE] require code changes; all others are documentation-only.

---

### Wave 5A: High-Priority Documentation Fixes (Parallelizable across files)

These fix incorrect/missing information in architecture docs. Each file is independent.

#### 5A-1: `architecture/agent.md`
- [ ] Add 9 missing fields to AgentLoop struct: `config`, `question_tx`, `question_rx`, `session_id`, `plugin_service`, `mcp_service`, `tool_def_cache`, `file_change_rx`, `usage_store`, `pricing_service` (total 24 fields)
- [ ] Fix stale instruction file listing: primary sources are `AGENTS.md`, `CLAUDE.md`, `CONTEXT.md` via `INSTRUCTION_FILES` constant in `src/agent/prompt.rs:7`
- [ ] Document `INSTRUCTION_FILES` constant location and behavior

#### 5A-2: `architecture/overview.md`
- [ ] Fix tool count: 27 (not 28 as AGENTS.md claims) - verified by counting `with_defaults()` registrations in `src/tool/mod.rs:90-122`
- [ ] Fix feature gate name: `plugins` (plural), not `plugin` - verified in `Cargo.toml:169`
- [ ] Fix database tables count: 13 (not 7) - list all tables from `src/session/schema.rs`: `migration_version`, `project`, `session`, `message`, `part`, `todo`, `permission`, `session_share`, `cached_models`, `task`, `checkpoints`, `snapshot`, `usage`
- [ ] Fix provider auto-registration: `register_builtin_with_config()` at `src/provider/mod.rs:390-536` registers 16 providers (not 15) - includes all env-var providers plus `codegg_go`
- [ ] Add `exec` and `error` modules to module table (they exist in `lib.rs` but missing from overview table)
- [ ] Remove or mark `git/` module as orphaned (not declared in `lib.rs`)
- [ ] Fix line number references (verified accurate within 3 lines)

#### 5A-3: `architecture/provider.md`
- [ ] Fix all stale line number references (items shifted by ~14 lines due to prior edits)
- [ ] Fix provider auto-registration claims: 3 locations incorrectly state only `codegg_go` is auto-registered. Actually, `register_builtin_with_config()` registers all 16 env-var providers when registry is empty
- [ ] Document `register_builtin_with_config()` behavior: adding ANY provider via config disables all env-var auto-registration (Medium priority code insight)
- [ ] Fix stale file path references: `src/error/mod.rs` should be `src/error.rs` (single file)

#### 5A-4: `architecture/config.md`
- [ ] Fix `ProviderConfig::merge()` signature: actual is `pub fn merge(&mut self, other: &ProviderConfig)` (at `src/config/schema.rs:207`), not returning `Self`
- [ ] Fix merge behavior documentation: only `provider` merges field-by-field; `agents`/`mcp`/`commands`/`modes` use key replacement

#### 5A-5: `architecture/tui.md`
- [ ] Fix typo: `pubruct FocusManager` -> `pub struct FocusManager`
- [ ] Fix FocusManager location: defined in `src/tui/components/component/focus.rs:14`, NOT in `types.rs`
- [ ] Add missing `focus_index: usize` field to FocusManager struct
- [ ] Add 5 missing Component trait methods: `focus_next`, `focus_prev`, `focusable_count`, `focused_index`, `set_focused`
- [ ] Fix DialogType location: defined in `src/tui/components/component.rs:22`, NOT in `types.rs` - verify Stats variant is present (23 variants confirmed)
- [ ] Fix vim keybindings: `j`/`k` are swapped (NavigateUp should be `k`, NavigateDown should be `j`)
- [ ] Fix mod.rs line count: 5995 (not 6003)

#### 5A-6: `architecture/tool.md`
- [ ] Fix ImageTool claim: IS registered in `ToolRegistry::with_defaults()` at `src/tool/mod.rs:102` - remove incorrect "not registered" note
- [ ] Fix LspTool claim: IS registered at `src/tool/mod.rs:113-115` - remove incorrect "not a built-in" note
- [ ] Update `with_defaults()` code block to include ImageTool, LspTool; remove BatchTool (not registered)
- [ ] Remove or rewrite stale ToolExecutor section (references deleted `src/tool/executor.rs`)
- [ ] Fix stale file path: `src/error/mod.rs` -> `src/error.rs`

#### 5A-7: `architecture/protocol.md`
- [ ] Fix `TurnFailed.turn_id` documentation: code has `Option<String>`, doc shows required
- [ ] Fix `ToolStarted` and `ToolCompleted` `turn_id` optionality to match code
- [ ] Fix TuiMessage "Special (2)" section count: actual special count is 1
- [ ] Add `#[serde(skip_serializing_if)]` suggestion for optional CoreEvent fields

#### 5A-8: `architecture/bus.md`
- [ ] Fix TTL documentation: 310 seconds (not 300) - at `src/bus/mod.rs:59`
- [ ] Fix PermissionChoice timeout example: clarify this is calling pattern, not registry behavior
- [ ] Fix TTL values in 4 locations (126, 334, 338, 345): all should be 310 seconds

#### 5A-9: `architecture/exec.md`
- [ ] Fix method name: `setup_question_channel_for_exec()` (not `setup_question_channel()`)
- [ ] Fix behavior description: exec mode DOES wait for questions (300s timeout) - `setup_question_channel_for_exec()` at `src/exec.rs:121` DOES set `question_rx`
- [ ] Remove stale content about exec mode not supporting questions

#### 5A-10: `architecture/permission.md`
- [ ] Fix numerous line number inaccuracies (many off by 15-26 lines)
- [ ] Fix TTL: 310 seconds (not 300)
- [ ] Remove `PermissionResponse` type references (type does not exist in codebase)
- [ ] Fix DoomLoop key semantics: key is `tool_name:hash(arguments)` (per-tool+args), not per-tool as documented
- [ ] Add missing `mode` field to SandboxConfig struct docs
- [ ] Add missing `SandboxMode` enum to docs

#### 5A-11: `architecture/security.md`
- [ ] Fix `CANONICAL_PATHS_CACHE`: now has 300s TTL and 100-entry cap (not "never clears") - at `src/security/sandbox.rs:262`
- [ ] Create `architecture/sandbox.md` if needed (currently missing)

#### 5A-12: `architecture/core.md`
- [ ] Fix `InprocCoreClient.pool` field type: should be `Option<sqlx::SqlitePool>`, not `Option<Arc<sqlx::SqlitePool>>`
- [ ] Fix `map_app_event_to_core_event` line number (off by ~5-8 lines)

#### 5A-13: `architecture/mcp.md`
- [ ] Fix `McpClientType` derive attributes: code only has `Clone` (not Debug, Clone, Serialize, Deserialize) - at `src/mcp/mod.rs:77`
- [ ] Fix `RemoteClient.session_id` and `request_id` field types: use `Arc<Mutex<...>>` and `Arc<AtomicU64>` wrappers
- [ ] Fix `DiagnosticEntry` location: actually in `client.rs`, not `diagnostics.rs`
- [ ] Fix `McpConnectionManager.max_retries`: `>=` check yields 4 attempts, not 5
- [ ] Document `McpPrompt`, `McpResource`, `McpResourceContent` structs

#### 5A-14: `architecture/resilience.md`
- [ ] Remove duplicated state diagram (lines 76-78)
- [ ] Fix `call()` line reference: actual is 101-137, not 114-127

#### 5A-15: `architecture/session.md`
- [ ] Complete SessionStore methods table: add `create_from_template`, `list_all_with_offset`, `list_deleted`, `set_tags`

#### 5A-16: `architecture/storage.md`
- [ ] Fix `init()` code example: actual `init()` calls `connect_and_configure()` directly, not `Database::new()` then `migrate()` separately
- [ ] Fix v15 description: creates `usage` table, not "Additional fields"

#### 5A-17: `architecture/crypto.md`
- [ ] Fix `EncryptedData` visibility: IS `pub struct EncryptedData` (not "not pub")

#### 5A-18: `architecture/util.md`
- [ ] Add documentation for `pricing.rs` (ModelPricing, PricingService, calculate_cost)
- [ ] Add documentation for `interner.rs` (StringInterner, tool_interner)
- [ ] Document Histogram 1000-entry bound

#### 5A-19: `architecture/tts.md`
- [ ] Document TUI integration: auto-stop on AgentFinished, toggle via `/tts` or `/voice` command

#### 5A-20: `architecture/memory.md`
- [ ] Clarify "Max 20 active memories per namespace" is a soft per-consolidation limit, not a hard namespace cap

#### 5A-21: `architecture/skills.md`
- [ ] Clarify `.skills/` directory purpose (repo-maintained copy vs runtime)

---

### Wave 5B: Code Fixes (Parallelizable across modules)

These require actual code changes. Each module is independent.

#### 5B-1: [CODE] Remove dead code `setup_question_channel()` in `src/agent/loop.rs:784`
- **Why**: Non-exec version never called (exec uses `setup_question_channel_for_exec()`)
- **Risk**: LOW - removing unused code
- **Verification**: `cargo build --all-features && cargo test --all-features`

#### 5B-2: [CODE] Remove dead code `connect_sse()` in `src/mcp/remote.rs:698-740`
- **Why**: Never called externally
- **Risk**: LOW - removing unused code
- **Verification**: `cargo build --all-features && cargo test --all-features`

#### 5B-3: [CODE] Remove dead code `run_socket()` in `src/mcp/ide_server.rs:121-144`
- **Why**: Unix socket server, never called
- **Risk**: LOW - removing unused code
- **Verification**: `cargo build --all-features && cargo test --all-features`

#### 5B-4: [CODE] Remove dead `check_external_directory()` in `src/permission/mod.rs:1264-1276`
- **Why**: Unused function with `#[allow(dead_code)]` annotation (line numbers verified)
- **Risk**: LOW - removing unused code
- **Verification**: `cargo build --all-features && cargo test --all-features`

---

### Wave 5C: Improvement Opportunities (Lower Priority, Parallelizable)

These are enhancements, not bugs. Each is independent.

#### 5C-1: Add `CircuitBreakerConfig` struct to `src/resilience/mod.rs`
- Document `max_half_open_duration` as configurable (currently hardcoded 30s)
- Add config struct with defaults matching current behavior

#### 5C-2: Document `register_builtin_with_config()` behavior in `architecture/provider.md`
- Adding ANY provider via config disables all env-var auto-registration
- This is intentional design, not a bug

#### 5C-3: Add snapshot config validation to `src/snapshot/mod.rs`
- Validate capture/restore parameters at startup

#### 5C-4: Rename `ProviderCache::clear()` to `ProviderCache::evict_expired()` in `src/provider/mod.rs`
- Current name is misleading (only removes expired entries)

#### 5C-5: Align PermissionRegistry TTL (310s) with agent loop timeout (300s)
- Currently 10s window where cleanup could remove pending permissions
- Consider making TTL configurable or matching values

#### 5C-6: Add `#[serde(skip_serializing_if)]` to optional `CoreEvent` fields in `src/protocol/core.rs`
- Reduces JSON payload size for variants with optional `turn_id`

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
- **Tool count is 27** (not 28 as some docs claim): verified by counting `with_defaults()` registrations.
- **AgentLoop has 24 fields**: The struct at `src/agent/loop.rs:559-584` has 24 fields; docs list only 15.
- **DialogType is in `component.rs`**: Not in `types.rs`. FocusManager is in `component/focus.rs`.

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
