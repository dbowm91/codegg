# Implementation Plan

**Status**: WAVE 5 - Architecture Documentation & Code Fixes
**Last Updated**: 2026-05-28
**Consolidated from**: All plans/ review files (now removed)

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

## Verified Codebase Facts (used throughout this plan)

These are ground-truth values verified against source code during this review session.

| Item | Value | Source |
|------|-------|--------|
| Tool count (with_defaults) | 27 | `src/tool/mod.rs:90-122` - 27 registrations |
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
| Config merge: provider | field-by-field via ProviderConfig::merge() | `src/config/paths.rs:222-235` |
| Config merge: server | field-by-field via ServerConfig::merge() | `src/config/paths.rs:203-208` |
| Config merge: watcher | field-by-field (manual) | `src/config/paths.rs:209-221` |
| Config merge: agents/mcp/commands/modes | key replacement (insert overwrites) | `src/config/paths.rs:236-281` |
| Config merge: instructions | concatenation | `src/config/paths.rs:266-271` |

---

## Wave 5: Architecture Documentation & Code Fixes

All items below are organized into parallelizable waves. Items within a wave can be done concurrently across files. Items marked [CODE] require code changes; all others are documentation-only.

---

### Wave 5A: High-Priority Documentation Fixes (Parallelizable across files)

Each file is independent and can be worked on by a separate subagent.

#### 5A-1: `architecture/agent.md`
- [ ] Add missing fields to AgentLoop struct listing (total 24 fields): `config`, `question_tx`, `question_rx`, `session_id`, `plugin_service`, `mcp_service`, `tool_def_cache`, `file_change_rx`, `usage_store`, `pricing_service`. Source: `src/agent/loop.rs:559-584`
- [ ] Fix stale instruction file listing: primary sources are `AGENTS.md`, `CLAUDE.md`, `CONTEXT.md` via `INSTRUCTION_FILES` constant at `src/agent/prompt.rs:7`. The current doc lists `.codegg/instructions.md`, `INSTRUCTIONS.md`, `~/.config/codegg/instructions.md` which are secondary/fallback paths from `find_instructions_file()`.

#### 5A-2: `architecture/overview.md`
- [ ] Fix tool count: 27 (not 28 as some docs claim). Verified by counting registrations at `src/tool/mod.rs:90-122`.
- [ ] Fix feature gate name: `plugins` (plural), not `plugin`. Verified in `Cargo.toml:172`.
- [ ] Fix database tables: list all 13 tables from `src/session/schema.rs`: `migration_version`, `project`, `session`, `message`, `part`, `todo`, `permission`, `session_share`, `cached_models`, `task`, `checkpoints`, `snapshot`, `usage`. Current doc lists only 7.
- [ ] Fix provider auto-registration: "Auto-registered: codegg_zen only" is wrong. `register_builtin_with_config()` at `src/provider/mod.rs:390-537` registers 16 providers via env vars. `register_builtin()` (line 279-326) registers 15 providers.
- [ ] Add `exec` and `error` modules to module table (declared in `lib.rs` but missing from overview table).
- [ ] Mark `git/` module as orphaned (not declared in `lib.rs`, no references anywhere).
- [ ] Add `arboard` and `debug-logging` to feature gates table (defined in Cargo.toml but missing from doc).

#### 5A-3: `architecture/provider.md`
- [ ] Fix all stale line number references (15+ references off by 14+ lines). Key locations:
  - Provider trait: should be lines 74-87 (not 60-73)
  - ChatRequest: should be 112-123 (not 97-109)
  - Message: should be 125-142 (not 111-128)
  - ContentPart: should be 144-149 (not 130-135)
  - ChatEvent: should be 156-170 (not 142-156)
  - ModelInfo: should be 236-246 (not 222-232)
  - ModelVariant: should be 226-234 (not 212-220)
  - TokenUsage: should be 189-196 (not 175-182)
  - ToolDefinition: should be 198-223 (not 184-210)
  - ProviderRegistry: should be 248-277 (not 234-263)
  - register_builtin: should be 279-326 (not 265-312)
  - register_builtin_with_config: should be 390-537 (not 376-523)
- [ ] Fix auto-registration description: `register_builtin()` registers 15 providers via env vars. `register_builtin_with_config()` registers 16 (adds codegg_go). Providers are independent - adding one provider via config does NOT disable others. Each provider checks its own config key independently.
- [ ] Fix stale file path: `src/error/mod.rs` should be `src/error.rs` (single file, not directory).

#### 5A-4: `architecture/config.md`
- [ ] Fix `ProviderConfig::merge()` signature: actual is `pub fn merge(&mut self, other: &ProviderConfig)` (returns `()`, at `src/config/schema.rs:207`). Doc shows `&self` (immutable) and returns `ProviderConfig`.
- [ ] Fix merge behavior documentation: `merge_configs()` at `src/config/paths.rs:164-284` uses different strategies per field:
  - **Field-by-field merging**: `provider` (via `ProviderConfig::merge()`), `server` (via `ServerConfig::merge()`), `watcher` (manual field merge)
  - **Key replacement**: `agent`, `mcp`, `commands`, `mode` (insert overwrites)
  - **Concatenation**: `instructions` (appended to list)
  - **Simple override**: all other fields via `merge_option!` macro

#### 5A-5: `architecture/tui.md`
- [ ] Fix typo: `pubruct FocusManager` -> `pub struct FocusManager` (line 313)
- [ ] Fix FocusManager location: defined in `src/tui/components/component/focus.rs:14`, NOT in `types.rs`. Add missing `focus_index: usize` field (actual struct has `stack` + `focus_index`).
- [ ] Add 5 missing Component trait methods: `focus_next`, `focus_prev`, `focusable_count`, `focused_index`, `set_focused`. Total is 13 methods, not 8. Source: `src/tui/components/component.rs:84-113`
- [ ] Fix DialogType: add missing `Stats` variant (actual has 23 variants, doc shows 22). Source: `src/tui/components/component.rs:22-46`
- [ ] Fix vim keybindings: `j` maps to NavigateDown, `k` maps to NavigateUp (standard vim). Doc currently has them swapped.
- [ ] Fix mod.rs line count: 5995 (not 6003).

#### 5A-6: `architecture/tool.md`
- [ ] Fix ImageTool claim: IS registered in `ToolRegistry::with_defaults()` at `src/tool/mod.rs:102`. Remove incorrect "not registered" note.
- [ ] Fix LspTool claim: IS registered at `src/tool/mod.rs:113-115`. Remove incorrect "not a built-in" note.
- [ ] Update `with_defaults()` code block: add ImageTool (line 102), LspTool (lines 113-115); remove BatchTool (not registered). Actual tool count is 27.
- [ ] Remove stale ToolExecutor section (references deleted `src/tool/executor.rs`).
- [ ] Fix stale file path: `src/error/mod.rs` -> `src/error.rs`.

#### 5A-7: `architecture/protocol.md`
- [ ] Fix `TurnFailed.turn_id`: code has `Option<String>`, doc shows required `String`. Source: `src/protocol/core.rs:232-236`
- [ ] Fix `ToolStarted.turn_id` and `ToolCompleted.turn_id` optionality to match code (both are `Option<String>`).
- [ ] Fix TuiMessage "Special (2)" section: EventEnvelope is already listed under Server-to-Client Events, so actual special count is 1, not 2.

#### 5A-8: `architecture/bus.md`
- [ ] Fix TTL: 310 seconds (not 300). Source: `src/bus/mod.rs:59`. Update all 4 locations (lines 126, 334, 338, 345).
- [ ] Fix PermissionChoice timeout example: clarify this is a calling pattern in the agent loop, not registry behavior.

#### 5A-9: `architecture/exec.md`
- [ ] Fix method name: `setup_question_channel_for_exec()` (not `setup_question_channel()`). Source: `src/exec.rs:121`
- [ ] Fix behavior description: exec mode DOES wait for questions (300s timeout). `setup_question_channel_for_exec()` calls `setup_question_channel_impl(true)` which sets `question_rx = Some(rx)`. The "[question not supported]" string is in the `else` branch (non-exec path when `question_rx` is None).
- [ ] Mark `setup_question_channel()` (non-exec version) at `src/agent/loop.rs:784` as dead code - never called.

#### 5A-10: `architecture/permission.md`
- [ ] Fix line number inaccuracies (many off by 15-26 lines). Key corrections:
  - PermissionChecker struct: line 418 (not 392-421)
  - check() method: lines 469-546 (not 443-520)
  - DoomLoopDetector struct: line 1181 (not 1161-1229)
  - is_doom_loop(): lines 1229-1242 (not 1213-1223)
- [ ] Fix TTL: 310 seconds (not 300).
- [ ] Remove `PermissionResponse` type references - this type does not exist in the codebase.
- [ ] Fix DoomLoop key semantics: key is `tool_name:hash(tool_name + arguments)` (per-tool+args, not per-tool). Source: `src/permission/mod.rs:1249-1256`
- [ ] Add missing `mode` field and `SandboxMode` enum to SandboxConfig struct docs.

#### 5A-11: `architecture/security.md`
- [ ] Fix `CANONICAL_PATHS_CACHE`: now has 300s TTL and 100-entry cap (not "never clears"). Source: `src/security/sandbox.rs:259-286`
- [ ] Add `SandboxMode` enum (ReadOnly/WorkspaceWrite/DangerFullAccess) and `access_flags()` method.
- [ ] Note that Landlock access flags use raw bitmasks (1, 3, 7), not named constants.

#### 5A-12: `architecture/core.md`
- [ ] Fix `InprocCoreClient.pool` field type: should be `Option<sqlx::SqlitePool>`, not `Option<Arc<sqlx::SqlitePool>>`. Source: `src/core/mod.rs:27`
- [ ] Fix `map_app_event_to_core_event` line number (off by ~5-8 lines).

#### 5A-13: `architecture/mcp.md`
- [ ] Fix `McpClientType` derive attributes: code only has `#[derive(Clone)]` (not Debug, Clone, Serialize, Deserialize). Source: `src/mcp/mod.rs:77`
- [ ] Fix `RemoteClient` field types: `session_id`, `sse_url`, `oauth_token` use `Arc<Mutex<Option<String>>>` (not bare `Mutex`). `request_id` uses `Arc<AtomicU64>` (not bare `AtomicU64`).
- [ ] Fix `DiagnosticEntry` location: actually in `client.rs`, not `diagnostics.rs`.
- [ ] Fix `McpConnectionManager.max_retries`: `>=` check yields 4 attempts, not 5.
- [ ] Document `McpPrompt`, `McpResource`, `McpResourceContent` structs.

#### 5A-14: `architecture/resilience.md`
- [ ] Remove duplicated state diagram (appears twice in sequence).
- [ ] Fix `call()` line reference: actual is 101-137, not 114-127.

#### 5A-15: `architecture/session.md`
- [ ] Complete SessionStore methods table: verify and add any missing methods (create_from_template, list_all_with_offset, list_deleted, set_tags).

#### 5A-16: `architecture/storage.md`
- [ ] Fix `init()` code example: actual `init()` calls `connect_and_configure()` directly, returns `SqlitePool` (not `Database` struct). Source: `src/storage/mod.rs:85-130`
- [ ] Fix v15 description: creates `usage` table, not "Additional fields".

#### 5A-17: `architecture/crypto.md`
- [ ] Fix `EncryptedData` visibility: IS `pub struct EncryptedData`. Source: `src/crypto/mod.rs:28`. Doc incorrectly says "not pub".

#### 5A-18: `architecture/util.md`
- [ ] Add documentation for `pricing.rs` (ModelPricing, PricingService, calculate_cost). Source: `src/util/pricing.rs`
- [ ] Add documentation for `interner.rs` (StringInterner, tool_interner). Source: `src/util/interner.rs`
- [ ] Document Histogram 1000-entry bound. Source: `src/util/metrics.rs:122-124`

#### 5A-19: `architecture/tts.md`
- [ ] Document TUI integration: auto-stop on AgentFinished, toggle via `/tts` or `/voice` command.

#### 5A-20: `architecture/memory.md`
- [ ] Clarify "Max 20 active memories per namespace" is a soft per-consolidation limit (`.take(20)`), not a hard namespace cap.

#### 5A-21: `architecture/skills.md`
- [ ] Clarify `.skills/` directory purpose (repo-maintained copy vs runtime-loaded).

---

### Wave 5B: Code Fixes (Parallelizable across modules)

Each module is independent. Run `cargo build --all-features && cargo test --all-features` after each fix.

#### 5B-1: [CODE] Remove dead code `setup_question_channel()` in `src/agent/loop.rs:784`
- **Why**: Non-exec version never called (exec uses `setup_question_channel_for_exec()`)
- **Risk**: LOW - removing unused code
- **Verification**: `cargo build --all-features && cargo test --all-features`

#### 5B-2: [CODE] Remove dead code `connect_sse()` in `src/mcp/remote.rs:699-740`
- **Why**: Never called externally
- **Risk**: LOW - removing unused code
- **Verification**: `cargo build --all-features && cargo test --all-features`

#### 5B-3: [CODE] Remove dead code `run_socket()` in `src/mcp/ide_server.rs:121-144`
- **Why**: Unix socket server, never called
- **Risk**: LOW - removing unused code
- **Verification**: `cargo build --all-features && cargo test --all-features`

#### 5B-4: [CODE] Remove dead `check_external_directory()` in `src/permission/mod.rs:1264-1276`
- **Why**: Unused function with `#[allow(dead_code)]` annotation
- **Risk**: LOW - removing unused code
- **Verification**: `cargo build --all-features && cargo test --all-features`

#### 5B-5: [CODE] Remove stale `#[allow(dead_code)]` from `handle_tui` in `src/server/ws.rs:357`
- **Why**: Function is actively used in Axum router at `http.rs:265`
- **Risk**: LOW - removing incorrect annotation
- **Verification**: `cargo build --all-features && cargo test --all-features`

---

### Wave 5C: Improvement Opportunities (Lower Priority, Parallelizable)

These are enhancements, not bugs. Each is independent.

#### 5C-1: Add `CircuitBreakerConfig` struct to `src/resilience/mod.rs`
- Document `max_half_open_duration` as configurable (currently hardcoded 30s at `src/resilience/circuit.rs:66`)
- Add config struct with defaults matching current behavior

#### 5C-2: Document `register_builtin_with_config()` behavior in `architecture/provider.md`
- Each provider is checked independently against config
- Adding one provider via config does NOT suppress other env-var providers
- This is per-provider fallback, not global

#### 5C-3: Add snapshot config validation to `src/snapshot/mod.rs`
- Validate capture/restore parameters at startup

#### 5C-4: Rename `ProviderCache::clear()` to `ProviderCache::evict_expired()` in `src/provider/cache.rs:75-82`
- Current name is misleading (only removes expired entries via `.retain()`)

#### 5C-5: Align PermissionRegistry TTL (310s) with agent loop timeout (300s)
- Currently 10s window where cleanup could remove pending permissions
- Consider making TTL configurable or matching values

#### 5C-6: Add `#[serde(skip_serializing_if)]` to optional `CoreEvent` fields in `src/protocol/core.rs`
- Reduces JSON payload size for variants with optional `turn_id`

---

## Wave Execution Strategy

### Parallelization Plan

**Wave 5A** (Documentation): All 21 files are independent. Launch up to 5 subagents in parallel, each handling 4-5 files:
- **Subagent A**: agent.md, overview.md, exec.md (related to agent loop behavior)
- **Subagent B**: provider.md, config.md, resilience.md (related to provider/config system)
- **Subagent C**: tui.md, tool.md, protocol.md (related to TUI/protocol)
- **Subagent D**: bus.md, permission.md, security.md (related to bus/permissions/security)
- **Subagent E**: core.md, mcp.md, session.md, storage.md, crypto.md, util.md, tts.md, memory.md, skills.md (remaining files)

**Wave 5B** (Code): All 5 items are independent. Can be done in parallel or sequentially. Each is a simple dead-code removal.

**Wave 5C** (Improvements): Lower priority, can be deferred. Each is independent.

### Dependency Order

```
Wave 5A (parallel) ──> Verify all doc changes ──> Wave 5B (parallel) ──> Wave 5C (parallel)
```

No dependencies between items within each wave. Wave 5B should follow 5A to ensure docs match the updated code. Wave 5C can be done anytime.

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
