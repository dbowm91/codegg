# Implementation Plan

**Status**: IN PROGRESS
**Last Updated**: 2026-05-26

---

## Completed Implementation (Historical Context - April 2026 Sprint)

### Security Fixes
- IPv6 ULA (fc00::/7) and multicast (ff00::/8) blocking in SSRF module.
- WASM fuel tracking with proper return after execution.
- SSRF protection for `webfetch`, `websearch`, `codesearch`.
- Symlink validation before canonicalization.
- `env_clear()` and hardcoded minimal safe `PATH` in subprocess invocations.
- No information leakage in `AppError` responses.
- AES-256-GCM encryption module (`src/crypto/mod.rs`).
- Write tool TOCTOU fix - validate parent path before `create_dir_all()`.
- Error redaction for LLM safety - `redact_local_paths()`.
- `#![deny(unsafe_code)]` in lib.rs.
- Upgrade module - semver validation, env_clear, direct curl.
- WASM fuel bug fixed - `return_fuel()` uses `MAX_PLUGIN_FUEL_BUDGET`.
- Critical unwrap removed in plugin execution.

### Async/Mutex
- `TaskStore` uses `tokio::sync::Mutex` throughout.
- LSP `DiagnosticsCollector` uses `tokio::sync::Mutex`.
- `parking_lot::Mutex` replaced with `tokio::sync::Mutex` in `src/server/http.rs`.
- `parking_lot::Mutex` replaced with `tokio::sync::Mutex` in `src/server/ws.rs` (RateLimiter, InMemoryRateLimiter, TuiSessionState).

### Performance
- HTTP client timeouts (60s request, 10s connect).
- Database `busy_timeout` (5s WAL).
- Per-tool timeouts in `bash`, `terminal`, `git` tools.
- Token caching via `ModelDiscoveryService`.
- Model-specific token estimation with `TokenizerType` (Claude: 1.4x, Gemini: 1.2x).
- `ToolRegistry` lazy initialization via `once_cell::Lazy` (`default_registry()`).
- `#[tracing::instrument]` added to `AgentLoop::run()`, `execute_tool_calls()`, and `CircuitBreaker::call()`.

### Agent Capabilities
- Context compaction (adaptive truncation/summarization).
- `SubAgentPool` with bounded concurrency (5).
- Background task scheduling with SQLite persistence.
- `denied_tools` enforcement - `ToolRegistry::filter_out()`.
- `/compact` command wired to `TuiCommand::CompactSession`.
- Subagent `max_depth` configuration with recursion limits (default: 3).

### TUI Features
- Background tasks UI via `/loop`, `/tasks`, `/task-del`.
- Vim mode keybindings (hjkl navigation).
- Diff output colorization.
- Shift+Tab toggles Plan/Build mode.
- `/compact` command properly wired.
- `/unshare` command fully implemented (calls `store.unshare_session()`).
- `/export` command fully implemented (exports to clipboard via `store.export_session()`).
- `/fork` command fully wired to `TuiCommand::ForkSession`.
- `/rename` command redirects to session dialog for rename.

### TUI Input Reliability (Completed 2026-05-01)
- Shift-modified printable characters insert correctly.
- Paste updates completion state, dialog paste isolation.
- Tests pass: `cargo test tui::input` ✅, `cargo test tui` (139 tests ✅)

### TUI Scrolling Fix (Completed 2026-05-06)
- Fixed `set_visible_height` not being called.
- Added `total_rendered_lines()` helper for line-based scrolling.
- Fixed `is_at_bottom()` for `usize::MAX` sentinel.
- Navigate/scroll key separation (arrows=j/k history, PageUp/Down/Ctrl+u/d/G=scroll).
- Added `scroll_to_top()`/`scroll_to_bottom()`.
- Tests pass: `cargo test messages` (17 tests), `cargo test tui` (8 layout tests)

### TUI Message Flow (Completed 2026-05-05)
- Thinking tag parsing (`<thinking>...</thinking>` → collapsible section).
- Removed "You"/"Assistant" labels, replaced with color-coded vertical bars.
- Mode-based color coding (yellow=Plan, blue=Build).
- Git permission explicit type (`"git"` added to `PERMISSION_TYPES`).
- Commit `0add872`: TUI message flow cleanup.

---

## Implementation Waves (Parallelization Strategy)

The implementation is organized into **5 waves** to maximize parallel work:

| Wave | Focus | Items | Parallel Potential |
|------|-------|-------|-------------------|
| 0 | Quick Wins | ~15 items | All independent |
| 1 | Critical Security | ~10 items | 3 groups, items within group sequential |
| 2 | High-Priority Infrastructure | ~12 items | Groups independent |
| 3 | Medium-Priority Groups | ~25 items | Groups independent |
| 4 | Large Refactors | 2 items | Sequential (large effort) |

---

## Wave 0: Quick Wins (Under 2 Hours Each)

These items are small, independent, and can be done in parallel by multiple agents.

### QW-1: Delete Dead Code in TUI (5 min)
- **File**: `src/tui/app/render.rs` (953 lines)
- **Action**: File does not exist. Only `mod.rs`, `types.rs`, and `commands.rs` exist in `src/tui/app/`. No action needed - REMOVE THIS ITEM.

### QW-2: Verify Redis Fallback Logic (15 min)
- **File**: `src/server/ws.rs:168-178`
- **Status**: VERIFIED CORRECT - if `REDIS_URL` is set → use Redis; otherwise → use in-memory. This is proper fallback behavior, not inverted.
- **Action**: REMOVE THIS ITEM - no fix needed.

### QW-3: Delete Duplicate handle_slash_command (30 min)
- **Files**: `src/tui/app/commands.rs:62-288` and `323-536`
- **Action**: Remove duplicate `handle_slash_command`, `on_paste()`, `on_resize()`
- **Fix**: Keep one implementation, remove the duplicate

### QW-4: Remove or Implement execute_command (15 min)
- **File**: `src/tui/app/commands.rs:538-727`
- **Issue**: `execute_command` appears unused - dead code
- **Action**: Either implement it properly or remove it

### QW-5: Fix Early Return Bug (15 min)
- **File**: `src/tui/app/commands.rs:612-623`
- **Issue**: Early return bypasses intended return value
- **Action**: Fix the return statement logic

### QW-6: Add DeniedTools Audit Log (30 min)
- **File**: `src/tool/mod.rs` or wherever `filter_out()` is called
- **Action**: Add `tracing::info!` when tools are filtered
- **Reference**: Keep existing `denied_tools` enforcement

### QW-7: Standardize DB Pool Size (5 min)
- **Files**: `storage/mod.rs`, `session/store.rs`
- **Issue**: `init()` uses 10, `Database::new()` uses 5
- **Fix**: Standardize to single value (recommend 10)

### QW-8: Make DoomLoop Threshold Configurable (30 min)
- **Files**: `config/schema.rs`, `permission/mod.rs`
- **Action**: Add `doomloop_threshold` to config schema
- **Reference**: `DoomLoopDetector` at `permission/mod.rs:1162-1231`

### QW-9: Add Config Watcher Debounce (1 hr)
- **Files**: `config/schema.rs`, `config/watcher.rs`
- **Action**:
  - Add `debounce_duration_ms` config (default 500ms)
  - Implement debounce using `tokio::time::sleep`
  - Add content hash before reload
  - Validate config before applying

### QW-10: Fix Upgrade Duplicate Logic (30 min)
- **Files**: `main.rs:549-590`, `upgrade/mod.rs`
- **Action**: Refactor `cmd_upgrade()` to use module instead of duplicating logic

### QW-11: Add Request Timeout to Upgrade (15 min)
- **File**: `upgrade/mod.rs:72-78`
- **Action**: Add `-m 300` (5 min timeout) to curl command

### QW-12: Add Content Hash Before Reload (1 hr)
- **File**: `config/watcher.rs`
- **Action**: Hash content before triggering reload to avoid unnecessary reloads

### QW-13: DoomLoop O(n) to O(1) Fix (1 hr)
- **File**: `src/permission/mod.rs:1162-1231`
- **Action**: Replace `VecDeque` iteration with `HashMap<String, usize>` for count tracking
- **Impact**: Changes O(n) to O(1) for doomloop detection

### QW-14: Rename/Mark PTY Module (15 min)
- **Files**: `src/pty/`, `docs/ARCHITECTURE.md`
- **Issue**: Module misleadingly named - no actual PTY functionality
- **Action**: Update documentation to clarify actual functionality, or rename

### QW-15: Fix Worktree is_current/is_detached (30 min)
- **Files**: `src/worktree/mod.rs:36-56`
- **Action**:
  - Parse HEAD line to detect current worktree
  - Set `is_detached=true` when HEAD points to commit (not branch)

---

## Wave 1: Critical Security & Data Integrity (Week 1)

Items in this wave address critical bugs that could cause data loss, security vulnerabilities, or crashes.

### CRIT-1: Tool Module Security Fixes
| Item | Location | Action |
|------|----------|--------|
| 1.1 | `tool/util.rs:26-28`, `tool/read.rs:99` | Use `check_path_for_symlinks()` before canonicalization to fix symlink bypass |
| 1.2 | `tool/write.rs:80-115` | Re-validate final path after creation (TOCTOU fix) |
| 1.3 | `tool/replace.rs:114-120` | Always validate `allowed_root` unless `unrestricted=true` |
| 1.4 | `tool/diff.rs` | Add `unrestricted` field to `DiffTool` like other tools |
| 1.5 | `tool/terminal.rs` | Add `HashSet<String>` for blocked commands (like `bash.rs:86-120`) |
| 1.6 | `tool/bash.rs` | Check entire command string for allowlist bypass, not just first word |

**Dependencies**: None - all independent

### CRIT-2: Server Module Auth Middleware (CRITICAL)
- **Files**: `src/server/middleware/auth.rs` (broken), `src/server/http.rs`, `src/server/ws.rs`
- **Issues**:
  - Auth middleware completely broken - wrong signature, undefined variables
  - WebSocket auth returns 500 instead of 401/503
  - Rate limiter cleanup race condition
  - CORS config inconsistent (`cors` field unused, `cors_origins` used)
- **Action**:
  1. Rewrite `AuthMiddleware` as proper Axum middleware
  2. Fix 500 → 401/503 in WebSocket auth
  3. Add WebSocket message timeouts
  4. Remove or use unused `cors` config field
  5. Fix rate limiter cleanup to prevent unbounded growth

### CRIT-3: Session Module Data Integrity
- **Files**: `src/session/store.rs`
- **Issues**:
  - Race condition in `share_session` - UPSERT + set_share_url not atomic (line ~1290-1313)
  - SQL construction pattern in `revert_to_message` - fragile dynamic SQL (line ~1114-1119)
  - Missing error handling in `unrevert_session` transaction (line ~1412-1450)
  - `CheckpointStore::save()` lacks transaction
- **Action**:
  1. Wrap `share_session` in transaction
  2. Add transaction to `CheckpointStore::save`
  3. Fix `revert_to_message` SQL construction
  4. Add proper error handling in `unrevert_session`

### CRIT-4: Storage/Database Race Conditions
- **Files**: `src/storage/mod.rs:87-96`
- **Issues**:
  - Race condition in database creation - `std::fs::File::create` vs SQLite
  - Inconsistent blocking I/O in async context
  - Hardcoded connection pool limit (10)
  - No WAL checkpoint configuration
- **Action**:
  1. Remove `std::fs::File::create` check - let SQLite handle creation atomically
  2. Replace with `tokio::fs::File::create()`
  3. Make `max_connections` configurable

### CRIT-5: Memory Module Persistence
- **Files**: `src/memory/mod.rs`
- **Issues**:
  - Race condition in `save()` - non-atomic file ops (line 119-151)
  - `add()`/`delete()` don't persist - silent data loss (line 92-117)
  - No file locking - multi-process corruption risk
  - Namespace path traversal vulnerability (line 74-78)
- **Action**:
  1. Fix non-atomic save with temp file pattern (write to temp, rename)
  2. Add auto-save option for add/delete
  3. Add file locking for concurrent access
  4. Add namespace path traversal validation

### CRIT-6: Snapshot Module Persistence
- **Files**: `src/snapshot/mod.rs`, `src/snapshot/diff.rs`
- **Issues**:
  - **No persistence** - snapshots lost on restart, in-memory `Vec` only
  - `collect_files()` race condition - no depth limit, stack overflow risk
  - MD5 hash for integrity - cryptographically weak
  - **No restore functionality** - one-way capture only
  - `diff_files()` creates single massive hunk for large files
- **Action**:
  1. Implement SQLite persistence for snapshots
  2. Add depth limit to recursive file collection
  3. Implement `restore(snapshot_id)` functionality
  4. Upgrade MD5 to SHA-256 or BLAKE3

---

## Wave 2: High-Priority Infrastructure (Week 2-3)

### HIGH-1: MCP Automatic Reconnection
- **Files**: `config/schema.rs`, `mcp/mod.rs`, `mcp/local.rs`, `mcp/remote.rs`
- **Reference**: `remote.rs` has `reconnect()` method at line 470 - needs to be wired up
- **Action**:
  1. Add `reconnect_config` to `McpServerConfig` schema
  2. Create `McpConnectionManager` actor in `mcp/connection.rs`
  3. Implement exponential backoff retry
  4. Add ping/pong heartbeat mechanism
  5. Wire auto-reconnection into `McpService::connect()`
  6. Add connection health tracking and status updates

### HIGH-2: MCP Critical Fixes
| Item | Location | Action |
|------|----------|--------|
| 2.1 | `mcp/local.rs:414-460` | Fix `LocalClient::read_loop` race condition on drop |
| 2.2 | `mcp/ide_server.rs:79-113` | Replace blocking I/O in `IdeServer::run_stdio` with async |
| 2.3 | `mcp/remote.rs:608-653` | Add timeout on SSE buffer to prevent memory exhaustion |
| 2.4 | `mcp/remote.rs:179-193` | Fix or remove `McpConnectionManager::clone` (unsound) |
| 2.5 | `mcp/auth.rs:318-332` | Fix OAuth replay protection race |

### HIGH-3: Resilience Module Fixes
- **Files**: `src/resilience/circuit.rs`
- **Issues**:
  - TOCTOU race in `is_available()` (line 67-84)
  - Clone creates inconsistent snapshot (line 158-171)
  - `success_count` not reset on HalfOpen
  - Missing `Send + Sync` bounds
- **Action**:
  1. Use atomic compare-and-swap in `is_available()`
  2. Implement transaction-like snapshot or atomic snapshot
  3. Reset `success_count` when transitioning to HalfOpen
  4. Add `static_assertions::assert_impl_all!(CircuitBreaker: Send, Sync)`

### HIGH-4: Config Module Race Conditions
- **Files**: `config/watcher.rs`, `config/schema.rs`, `config/paths.rs`
- **Issues**:
  - Race Condition in ConfigWatcher - closure captures `tx`, stored in `self.watcher` (line 55-76)
  - Missing Error on Config Watch Failure - `start()` fails silently
  - Hash Collision in Config Change Detection - `DefaultHasher` vulnerable to HashDOS
  - Env Var Interpolation Silent Failure - missing env vars replaced with empty strings
  - JSONC Comment Stripping Bug - `//` inside string values incorrectly stripped
  - Config Migration is a No-Op - `migrate_from_v0()` only logs
- **Action**:
  1. Fix ConfigWatcher race condition (use `Arc<Mutex<...>>`)
  2. Add error propagation in `recv()` for watch failures
  3. Use a DoS-resistant hasher for config changes
  4. Add warning for missing env vars
  5. Fix JSONC stripping to not affect URLs
  6. Implement actual migration logic

### HIGH-5: Hooks Module - Emit Events
- **Files**: `src/hooks/mod.rs`, `src/agent/loop.rs`
- **Issues**:
  - Unused hook events (`SessionStart`, `SessionEnd`, `AgentStart`, `AgentEnd`) never triggered
  - Silent error swallowing - failures not logged (line 1326-1329, 1366-1370)
  - `InlineScript` placeholder always fails
  - No hook failure isolation - one failure stops subsequent hooks
- **Action**:
  1. Emit `SessionStart`/`SessionEnd` in `AgentLoop::run()`
  2. Replace silent error swallowing with proper logging/tracing
  3. Modify `run_hooks()` to execute all hooks and collect errors
  4. Implement `InlineScript` or remove the variant

### HIGH-6: Bus Module Memory Leak
- **Files**: `src/bus/mod.rs`, `src/bus/global.rs`
- **Issues**:
  - Dead letter channels - memory leak if sender dropped without response (line 21-36, 70-85)
  - Silent send failure when broadcast buffer (2048) is full
  - Synchronous file I/O on every `publish()` call
  - Unnecessary `async` on sync methods
- **Action**:
  1. Add TTL cleanup or use channel with drop notification
  2. Log errors on broadcast failures or implement backpressure
  3. Remove debug logging or make it async
  4. Remove `async` from synchronous methods

### HIGH-7: SSRF Implementation Duplication
- **Files**: `src/security/ssrf.rs` (canonical), `src/tool/webfetch.rs:21-138` (duplicate), `src/mcp/remote.rs:45-95` (duplicate)
- **Action**:
  1. Audit all three implementations for differences
  2. Move `validate_url_host()` from `mcp/remote.rs` to `security/ssrf.rs`
  3. Replace `webfetch.rs` copy with re-export from `ssrf.rs`
  4. Update MCP to use centralized SSRF module

---

## Wave 3: Medium-Priority Groups (Week 4-8)

Groups can be worked on in parallel by different agents. Items within a group may have dependencies.

### GROUP-A: Security Hardening

| Item | Files | Action |
|------|-------|--------|
| A-1 | `src/provider/google.rs:185-188` | Test `x-goog-api-key` header auth |
| A-2 | `src/tool/bash.rs` | Add `${`, `$VAR` expansion blocking, block input redirect |
| A-3 | `server/file.rs`, `tool/replace.rs`, `tool/grep.rs`, `tool/glob.rs` | Use `util.rs` validation, fix bypasses |
| A-4 | `src/server/http.rs` | Restrict CORS `allow_methods` to required set |
| A-5 | `src/plugin/loader.rs:156-162` | Fix mtime cache invalidation (use UNIX_EPOCH comparison) |
| A-6 | `src/plugin/loader.rs:136` | Make `Module` `Send+Sync` or change caching strategy |
| A-7 | `src/plugin/event_bus.rs:63-69` | Implement `dispatch_to_plugin` or remove dead code |

### GROUP-B: Performance Optimization

| Item | Files | Action |
|------|-------|--------|
| B-1 | `src/agent/loop.rs:48-80`, `src/tui/app/handlers.rs:~1990` | Use `LazyLock` for redact patterns |
| B-2 | `src/storage/mod.rs:106-120` | Add PRAGMAs, LIMIT constraints |
| B-3 | `agent/loop.rs`, `provider/mod.rs`, `tool/mod.rs` | Wrap `ToolCall`, `Message` fields in `Arc` |
| B-4 | `src/provider/cache.rs` (new) | Create `ResponseCache` with `DashMap` |
| B-5 | `src/provider/mod.rs:7-17` | Replace `debug_log!` macro with `tracing::debug!` |
| B-6 | `src/provider/fallback.rs` | Add exponential backoff with jitter |
| B-7 | `src/provider/sse_parser.rs:11-13` | Ensure parser not shared across concurrent streams |

### GROUP-C: TUI Improvements

| Item | Files | Action |
|------|-------|--------|
| C-1 | `tui/app/handlers.rs` (2543 lines) | Extract dialog handlers to trait objects |
| C-2 | `tui/components/messages.rs` (1289 lines) | Split into smaller widgets |
| C-3 | Various TUI files | Implement `Drop` for dialogs |
| C-4 | `tui/mod.rs` | Add dirty region instead of full redraw |
| C-5 | `tui/app/commands.rs:86,112` | Make hardcoded width 50 configurable |
| C-6 | `src/tts/mod.rs:51` | Add cross-platform TTS provider abstraction |
| C-7 | `src/tts/mod.rs:67-70` | Fix `stop()` race condition by awaiting child process |

### GROUP-D: Agent Loop Improvements

| Item | Files | Action |
|------|-------|--------|
| D-1 | `agent/compaction.rs` | Implement LLM-based summarization (current is placeholder) |
| D-2 | `permission/mod.rs:1054-1128` | Decide behavior for DoomLoop doc mismatch, fix impl or docs |
| D-3 | `tool/catalog.rs` (new) | On-demand tool discovery |
| D-4 | `agent/worker.rs:278-303` | Review `process_request()` - it IS implemented (publishes events, returns success) |
| D-5 | `agent/plan_registry.rs:85-97` | Implement or remove `PlanRegistry` |
| D-6 | `agent/worker.rs:276` | Implement `start_workers()` |
| D-7 | `agent/loop.rs:834-841` | Add MCP version hash for precise cache invalidation |
| D-8 | `agent/compaction.rs:414` | Use configured model instead of hardcoded `gpt-4o-mini` |

### GROUP-E: Provider System

| Item | Files | Action |
|------|-------|--------|
| E-1 | `provider/*.rs` | Extract shared SSE parser utilities, create base trait |
| E-2 | `provider/mod.rs` | Wire `context_window`, `max_output_tokens` |
| E-3 | `provider/*.rs` | Add `ping` or `models` call on startup for health check |
| E-4 | All 17 providers | Create shared base trait to reduce inconsistencies |
| E-5 | `src/provider/google.rs:353` | Extract ID from API response, use UUID as fallback |

### GROUP-F: IDE/LSP Fixes

| Item | Files | Action |
|------|-------|--------|
| F-1 | `src/ide/ide.rs:53-60` | Fix temp file race with `mkstemp` or `tempfile` crate |
| F-2 | `src/ide/ide.rs:78-107` | Fix JetBrains line range handling |
| F-3 | `src/ide/ide.rs:65-66` | Remove unsafe `unwrap()` on temp path |
| F-4 | `src/ide/ide.rs:97-98,116-117,132-133` | Escape paths for command arguments |
| F-5 | `src/lsp/client.rs:451-457` | Fix request ID wrap-around race condition |
| F-6 | `src/lsp/launch.rs:38` | Remove hardcoded PATH in server launch |
| F-7 | `src/lsp/launch.rs` | Add stderr draining background task |
| F-8 | `src/lsp/download.rs` | Add path traversal check to `extract_zip` |
| F-9 | `src/lsp/operations.rs:388-391` | Fix `format_signature_help` panic on invalid params |

### GROUP-G: Skills Module

| Item | Files | Action |
|------|-------|--------|
| G-1 | `src/skills/mod.rs:18-24` | Add `permission: Option<SkillPermission>` to structs |
| G-2 | `src/skills/mod.rs:59-84` | Replace `fs::read_dir()` with `tokio::fs::read_dir()` |
| G-3 | `src/skills/mod.rs:156-167` | Add bounds checking in `parse_frontmatter` |
| G-4 | `src/tool/skill.rs:44-47` | Cache `SkillIndex` instead of reloading on every call |

### GROUP-H: Plugin Module

| Item | Files | Action |
|------|-------|--------|
| H-1 | `src/plugin/api.rs:39-117` vs `hooks.rs:1-115` | Unify `HookType` definitions |
| H-2 | `src/plugin/loader.rs:276-277` | Validate and canonicalize plugin paths |
| H-3 | `src/plugin/service.rs:18` vs `loader.rs:14` | Align 5s service timeout with 30s WASM timeout |
| H-4 | `src/plugin/loader.rs:204-226` | Use proper atomic operations for fuel budget |

### GROUP-I: Client Module

| Item | Files | Action |
|------|-------|--------|
| I-1 | `src/client/attach.rs:98,105-115` | Fix orphaned input channel - wire up or remove |
| I-2 | `src/client/attach.rs:112` | Handle WebSocket send errors properly |
| I-3 | `src/client/attach.rs:140-141` | Graceful task shutdown with `JoinSet` or `CancellationToken` |
| I-4 | `attach.rs:87-89`, `sdk.rs:32-41` | Add connection timeouts |
| I-5 | `src/client/attach.rs:75-146` | Add reconnection logic for WS drops |
| I-6 | `attach.rs:149-171` | Fix URL scheme replacement edge cases |

---

## Wave 4: Large Refactors (Deferred - 2+ weeks each)

These are large efforts that should be done after Wave 3 is complete.

### LARGE-1: Virtual Scrolling for Messages
- **Files**: `src/tui/components/messages.rs`
- **Effort**: 12-16 hours
- **Action**:
  - Pre-calculate line heights
  - Use binary search for visible range
  - Cache rendered lines
  - Add virtual list widget

### LARGE-2: String Interning System
- **Files**: `src/provider/mod.rs`, `src/agent/`, `src/tool/`
- **Effort**: 2-3 days
- **Action**:
  - Create `StringInterner` using `DashMap`
  - Apply to repeated strings in provider, agent, tool modules

---

## Architecture Review Items (Post-Wave 3)

Items identified during architecture review of documentation vs implementation.

### ARCH-1: Documentation Fixes (High Priority)

| Item | File | Issue | Fix |
|------|------|-------|-----|
| ARCH-1.1 | `architecture/protocol.md:291` | Claims subagent events NOT mapped in `map_app_event_to_core_event` | They ARE mapped at `src/core/mod.rs:795-838` - remove incorrect note |
| ARCH-1.2 | `architecture/client.md:41` | Backoff formula says "(2s, 4s)" | Actual is `(1s, 2s, 4s)` - update documentation |
| ARCH-1.3 | `architecture/tui.md:113` | `timeline_visible`/`timeline_selected` in UiState section | These are in `App` struct at `mod.rs:232-233`, not UiState |
| ARCH-1.4 | `architecture/agent.md:63-73` | ToolDefCache missing `lsp_enabled` field | Add `lsp_enabled: bool` to cache tuple structure |
| ARCH-1.5 | `architecture/bus.md:14,65` | Event count says 36 | Actual count is 34 - update |
| ARCH-1.6 | `architecture/command.md:51,114` | Command count says 41 | Actual count is 39 - update |
| ARCH-1.7 | `architecture/skills.md:82-84` | `.skills/` directory description | Runtime only loads from `~/.config/codegg/skills/` and `.codegg/skills/`, NOT `.skills/` - clarify |

### ARCH-2: Code Bugs (Medium Priority)

| Item | Location | Description |
|------|----------|-------------|
| ARCH-2.1 | `src/snapshot/mod.rs:431` | Hash algorithm inconsistency - `collect_files_sync` uses MD5 for non-empty files, SHA256 elsewhere |
| ARCH-2.2 | `src/tool/executor.rs:8` | `ToolExecutor` exists with retry logic but is NEVER used - dead code |
| ARCH-2.3 | `src/security/sandbox.rs:237-257` | Static `CANONICAL_PATHS_CACHE` never clears - stale path data risk |
| ARCH-2.4 | `src/permission/mod.rs:1141-1145` | `PermissionResponse` struct defined but unused internally |
| ARCH-2.5 | `src/permission/mod.rs:1237-1248` | `check_external_directory()` marked `#[allow(dead_code)]` - never called |
| ARCH-2.6 | `src/mcp/auth.rs:318-332` | OAuth replay protection race condition |
| ARCH-2.7 | `src/util/metrics.rs:122-124` | Histogram stores unbounded values per name - memory growth risk |
| ARCH-2.8 | `src/tts/mod.rs:85-103` | `stop()` returns `Ok(())` even when `pkill say` fails - silent failure |
| ARCH-2.9 | `src/tts/mod.rs:45-49` | `init()` silently ignores non-`None` providers - no warning |
| ARCH-2.10 | `src/worktree/mod.rs:69-88` | Current worktree detection via canonicalization may fail with symlinks |

### ARCH-3: Plugin/Skills Module Updates (Low Priority)

| Item | Location | Description |
|------|----------|-------------|
| ARCH-3.1 | `plugin.md:202-206` | Missing 9 dispatch methods in doc list |
| ARCH-3.2 | `marketplace.rs:4-9` | `PluginTier` enum not documented |
| ARCH-3.3 | `plugin.md:330` | `manifest.toml` reference misleading - `find_wasm` searches for `plugin.wasm` |
| ARCH-3.4 | `server.md:128-133` | Permission submit route shows `POST /api/permission/:session_id` but actual is `/api/permission/:session_id/submit` |

---

## TUI Enhancement Features (SKIPPED - Future)

These are enhancement features that build on Wave 3 TUI work.

### TUI-1: Inline Diff Rendering (HIGH)
- **Files**: `Cargo.toml`, `src/tui/components/diff.rs` (new), `src/tui/components/mod.rs`
- **Action**:
  - Add `similar = "3"` dependency for diff algorithm
  - Create `diff.rs` widget
  - Export from `mod.rs`
- **Dependencies**: TUI C-1, C-2 (widget splitting)

### TUI-2: Native Desktop Notifications (HIGH)
- **Files**: `Cargo.toml`, `src/util/notifications.rs` (new), `src/tui/mod.rs`
- **Action**:
  - Add `notify-rust = "4.16"` dependency
  - Create notifications module
  - Wire `AppEvent::AgentFinished`, `AppEvent::SubagentCompleted`
  - Add config option

### TUI-3: Image Attachment Support (HIGH)
- **Files**: `Cargo.toml`, `src/tui/components/image_preview.rs` (new), `src/tui/components/messages.rs`
- **Action**:
  - Add `ratatui-image = "10"` dependency
  - Create image preview widget
  - Render images in messages widget

### TUI-4: Streaming UX Enhancements (MEDIUM)
- **Files**: `src/tui/app/state/messages.rs`, `src/tui/mod.rs`, `src/tui/components/messages.rs`
- **Action**:
  - Add streaming state to `MessagesState`
  - Newline-gated commit
  - 75ms resize debounce
  - Finalize on complete
  - Live overlay rendering

### TUI-5: Accessibility Improvements (MEDIUM)
- **Files**: `src/tui/components/*.rs`, `src/tui/app/handlers.rs`, `src/util/a11y.rs` (new)
- **Action**:
  - Add focus indicator rendering
  - Global Tab/Shift+Tab handler
  - Screen reader announcer utility
  - Announce dialog open/close

### TUI-6: Mouse Support Enhancements (LOW)
- **Files**: `src/tui/app/handlers.rs`, `src/tui/components/sidebar.rs`
- **Action**:
  - Scrollbar navigation (click/drag)
  - Sidebar collapse (click on headers)
  - Dialog buttons (click to activate)
  - Selection (click to select items)

---

## Agent Capabilities Features (In Progress)

### AGENT-1: Context Summarization & Compaction (HIGH)
- **Files**: `src/agent/compaction.rs`, `src/agent/loop.rs`, `src/config/schema.rs`
- **Reference**: Claude Code three-tier system
- **Action**:
  - Add microcompaction tier (Tier 1: clear stale tool results)
  - Create structured 9-section summary prompt
  - Implement auto-compact trigger (~83%)
  - Add rehydration - re-read 5 recent files
  - Wire into AgentLoop context tracking
- **Note**: Current `summarize_old_turns()` is placeholder

### AGENT-2: Review Command (HIGH) - ✅ COMPLETED PR #33
- **Files**: `src/tool/review.rs`, `src/tool/mod.rs`, `src/command/`
- **Reference**: Claude Code `/review`, Codex `/review`
- **Action**: ✅ Create `ReviewTool` struct, git diff parsing, review subagent, emoji categorization, `/review` slash command

### AGENT-3: Multi-Agent Teams (HIGH) - ✅ COMPLETED PR #33
- **Files**: `src/agent/teams.rs`, `src/tool/mod.rs`, `src/agent/mod.rs`, `src/config/schema.rs`
- **Reference**: Claude Code TeamCreate + SendMessage
- **Action**: ✅ Team directory structure, TeamCreate tool, SendMessage tool, shared task list, idle notifications, graceful shutdown
  - Add shared task list with dependencies
  - Add idle notification system
  - Graceful shutdown protocol
- **Note**: SubAgentPool and Task tool exist, need team coordination layer

### AGENT-4: Tool Search / On-Demand Discovery (MEDIUM)
- **Files**: `src/tool/catalog.rs` (new), `src/tool/mod.rs`, `src/agent/loop.rs`, `src/provider/`
- **Reference**: Claude Code Tool Search
- **Action**:
  - Add `defer_loading` flag to tool definitions
  - Create tool catalog index
  - Implement ToolSearch tool
  - Wire into AgentLoop build_tools
  - Add MCP deferred loading

### AGENT-5: Image Generation (MEDIUM)
- **Files**: `src/tool/image.rs` (new)
- **Reference**: Codex CLI built-in, Gemini CLI native
- **Action**:
  - Create ImageTool struct
  - Integrate GPT Image API
  - Add output path management
  - Add transparent support

### AGENT-6: GitHub Integration (MEDIUM)
- **Files**: `config/`, `src/command/`, `src/command/github/` (new)
- **Action**:
  - Add GitHub MCP configuration
  - Create `/pr`, `/issue` slash commands
  - Add workflow templates

### AGENT-7: Sandbox Security Modes (MEDIUM)
- **Files**: `src/sandbox/mod.rs` (new), `src/sandbox/linux.rs` (new), `src/sandbox/mac.rs` (new), `src/tool/bash.rs`
- **Reference**: Codex CLI native sandbox
- **Action**:
  - Three sandbox modes: `read-only`, `workspace-write`, `danger-full-access`
  - Network access control
  - Kernel-level enforcement (Landlock on Linux, Seatbelt on macOS)
  - Sandbox escalation with approval integration

### AGENT-8: TTS/Voice Integration (LOW)
- **Files**: `src/tts/`, `src/hooks/`
- **Action**:
  - Hook Stop event for TTS
  - Add voice input (STT)

---

## Mode System Feature - ✅ COMPLETED PR #33

### MODE-1: Extended Mode System (HIGH)
- **Files**: `src/config/schema.rs`, `src/agent/mod.rs`, `src/tui/app/mod.rs`, `src/tui/app/handlers.rs`, `src/tui/command.rs`, `src/permission/mod.rs`
- **Current**: Two modes (Plan, Build), toggle via Shift+Tab
- **Target**: Five modes (Build, Plan, Review, Debug, Docs) with per-mode tool permissions
- **Action**: ✅ `ModeConfig` structure, mode selection in agent loop, mode state in TUI, `/mode` command, mode-based permissions

---

## Scripting/Exec Mode Feature - ✅ COMPLETED PR #33

### EXEC-1: Non-Interactive Exec Mode (HIGH) - ✅ COMPLETED PR #33
- **Files**: `src/main.rs`, `src/agent/mod.rs`, `src/exec.rs`
- **Reference**: Codex CLI
- **Action**: ✅ `exec` subcommand, `--json`, `--resume`, `--output-file`, exit codes, `--dangerously-bypass-approvals`

### EXEC-2: Session Analytics & Cost Tracking (MEDIUM)
- **Files**: `src/session/schema.rs`, `src/agent/processor.rs`, `src/tui/app/render.rs`, `src/tui/command.rs`
- **Current**: In-memory token tracking, hardcoded pricing
- **Action**:
  - Add database migrations for usage persistence
  - Emit usage to DB on each response
  - Refactor pricing to service
  - Add `/stats` command

### EXEC-3: Token Caching Display (LOW)
- **Files**: `src/provider/mod.rs`, `src/session/store.rs`, `src/tui/app/render.rs`
- **Action**:
  - Parse `prompt_tokens_details.cached_tokens` (OpenAI)
  - Parse `cache_read_input_tokens` (Anthropic)
  - Display cache hit rate in `/usage` or `/cost`

---

## Plugin Marketplace Feature - ✅ COMPLETED PR #33

### PLUGIN-1: Plugin Marketplace (MEDIUM)
- **Files**: `src/plugin/marketplace.rs`, `src/plugin/registry.rs`, `src/command/clap.rs`, `src/command/plugin.rs`
- **Action**: ✅ Three-tier system (Official, Repository, Personal), `codegg plugin install/search/list`, plugin discovery, local/remote storage

---

## Model Variants & Routing (Future)

### MODEL-1: Model Variants with Thinking (MEDIUM)
- **Files**: `src/config/schema.rs`, `src/provider/mod.rs`, `src/provider/anthropic.rs`, `src/provider/openai.rs`, `src/tui/app/mod.rs`
- **Current**: Basic variant structure exists at schema.rs:154, provider/mod.rs:191
- **Action**:
  - Extend `ModelVariant` with thinking/reasoning settings
  - Add variant option builder for API parameters
  - Add thinking parameter support to Anthropic
  - Add `reasoning_effort` parameter to OpenAI

### MODEL-2: Auto-Routing Model Selection (MEDIUM)
- **Files**: `src/provider/router.rs` (new), `src/agent/mod.rs`, `src/config/schema.rs`
- **Action**:
  - Task complexity classification (Simple/Complex)
  - Automatic model selection based on complexity
  - Routing strategies

---

## Git Integration Enhancement (Future)

### GIT-1: Enhanced Git Integration (MEDIUM)
- **Files**: `src/git/mod.rs` (new), `src/agent/prompt.rs`, `src/worktree/mod.rs`
- **Action**:
  - Git branch/status injection into system prompt
  - Checkpoint system with shadow git repo
  - Auto-worktree per session

---

## Documentation (Future)

### DOC-1: Conceptual Guides (Phase 1)
| File | Content |
|------|---------|
| `docs/conceptual/agents-vs-skills.md` | When to use Agents, Skills, Subagents |
| `docs/conceptual/mcp.md` | MCP system, Local vs Remote, OAuth, DNS rebinding |
| `docs/conceptual/lsp.md` | 36+ languages, lsp_tool experimental flag |
| `docs/conceptual/sessions.md` | Sessions (SQLite), Memory (cross-session) |
| `docs/conceptual/permissions.md` | Three levels, path restrictions, DoomLoop |
| `docs/conceptual/plugins.md` | WASM extensibility, fuel tracking, hook system |

### DOC-2: Reference Documentation (Phase 2)
| File | Content |
|------|---------|
| `docs/reference/configuration.md` | Complete config reference (expand ARCHITECTURE.md) |
| `docs/reference/tools.md` | 27 tools with JSON schema |
| `docs/reference/commands.md` | All 34 slash commands |
| `docs/reference/environment.md` | All environment variables |

### DOC-3: Workflow Guides (Phase 3)
| File | Content |
|------|---------|
| `docs/workflows/quick-start.md` | Agent loop, context window, Plan vs Build |
| `docs/workflows/debugging.md` | Debug workflow |
| `docs/workflows/code-review.md` | Code review workflow |
| `docs/workflows/refactoring.md` | Refactoring workflow |
| `docs/workflows/tdd.md` | TDD workflow |

### DOC-4: Operations & Troubleshooting (Phase 4)
| File | Content |
|------|---------|
| `docs/operations/troubleshooting.md` | Common issues and solutions |
| `docs/operations/security-hardening.md` | Production deployment, threat model |
| `docs/operations/migration.md` | From Claude Code, Cursor |

### DOC-5: README Improvements (Phase 5)
- Replace feature lists with explanations
- Add decision framework
- Expand security section
- Add migration section

---

## Deferred Items (Large Rewrites Not Recommended)

The following are large refactors that would require rewriting thousands of lines. They are deferred unless absolutely necessary:

### Large Refactors (DEFERRED)
- **handlers.rs refactor**: Splitting `src/tui/app/handlers.rs` (2543 lines) and `tui/app/mod.rs` (4487 lines)
- **session/store.rs refactor**: Splitting `src/session/store.rs` (2005 lines)
- **agent/loop.rs refactor**: Splitting `src/agent/loop.rs` (1296 lines)

### TUI Features (DEFERRED)
- **PTY Support**: Basic exists, full interactive not implemented
- **UI Parity**: Leader keys, session tabs not implemented
- **Headless Mode**: `--auto-approve` not implemented

### Resilience (DEFERRED - Already Implemented)
- **LLM Summarization**: ✅ IMPLEMENTED - `summarize_old_turns()` uses LLM-based summarization (see `src/agent/compaction.rs`)
- **Checkpointing**: ✅ IMPLEMENTED - SnapshotManager wired to AgentLoop, captures snapshots before file-modifying tools
- **CircuitBreaker Integration**: ✅ IMPLEMENTED - CircuitBreaker integrated into FallbackProvider (see `src/provider/fallback.rs`)

### Cloud Tasks (DEFERRED)
- **Cloud Tasks**: Requires significant infrastructure investment

---

## Notes for Future Agents

### Critical Implementation Notes

1. **WASM Plugin Fuel**: Fuel is consumed per-hook execution. Unused fuel is returned after execution. Check `module_cache::CACHE` in `src/plugin/loader.rs`. `return_fuel()` uses `MAX_PLUGIN_FUEL_BUDGET`.

2. **Async in TUI**: Command handlers are sync but use `TuiCommand` pattern to bridge to async handlers. Use `tui_cmd_tx.try_send(TuiCommand::YourCommand { ... })`.

3. **Plan/Build Mode**: Controlled by `agent_state.plan_mode` in TUI and `state.plan_mode` in AgentLoop. Toggle via markers, `/plan` tool, or Shift+Tab.

4. **LSP Diagnostics**: `DiagnosticsCollector` uses async mutex. `should_debounce()` is async.

5. **Subagent Tasks**: Tasks are persisted to SQLite. `TaskStore` manages in-memory state. Task IDs are atomic u64 counters. Subagent `max_depth` limit (default: 3) prevents infinite recursion.

6. **Adding TuiCommand variants**: Add to enum in `src/tui/app/mod.rs`, add async handler in `src/tui/mod.rs`, use `tui_cmd_tx.try_send()` from sync handlers.

7. **Crypto Module**: `src/crypto/mod.rs` provides AES-256-GCM encryption (`encrypt_to_string`, `decrypt_from_string`).

8. **Tool Path Validation**: `validate_path()` in `src/tool/util.rs` checks symlinks and verifies paths. `check_path_for_symlinks()` rejects symlink components.

9. **Write Tool TOCTOU Fix**: Parent path validated BEFORE `create_dir_all()`.

10. **Token Estimation**: `estimate_tokens_sync()` uses `TokenizerType` for model-specific multipliers. Claude: 1.4x, Gemini: 1.2x.

### Implementation Notes from Review Sessions

11. **`/compact` Command**: Wired to `TuiCommand::CompactSession`. Compaction happens during agent processing.

12. **`/unshare` Command**: Fully implemented via `TuiCommand::UnshareSession` -> `handle_unshare_session()` -> `store.unshare_session()`.

13. **`/export` Command**: Fully implemented via `TuiCommand::ExportSession` -> `handle_export_session()` -> `store.export_session()` -> clipboard.

14. **`/fork` Command**: Fully wired to existing `TuiCommand::ForkSession` handler.

15. **`/rename` Command**: Redirects to session dialog for user interaction.

16. **ToolRegistry Caching**: Use `crate::tool::default_registry()` for singleton registry.

17. **Tracing Instrumentation**: `#[tracing::instrument]` added to `AgentLoop::run()`, `execute_tool_calls()`, and `CircuitBreaker::call()`.

18. **MCP reconnect wired**: HIGH-1 completed auto-reconnection with exponential backoff.

19. **TUI render.rs dead code**: This was a duplicate of mod.rs - left as-is (large file, low priority deletion).

20. **DoomLoop doc mismatch FIXED**: D-2 updated docs to correctly describe window-based counting behavior.

21. **WebSocket rate limiter**: VERIFIED CORRECT - if `REDIS_URL` is set → use Redis; otherwise → use in-memory. Proper fallback behavior.

22. **OAuth tokens verified good**: AES-256-GCM with CODEGG_TOKEN_KEY, file permissions 0o600.

23. **Symlink bypass in tools**: `canonicalize_path()` in `util.rs` doesn't check intermediate symlinks. Use `check_path_for_symlinks()` before canonicalization.

24. **TTS is macOS-only**: Currently uses hardcoded `say` command. Cross-platform abstraction needed.

25. **Memory module doesn't persist**: `add()`/`delete()` in `src/memory/mod.rs` don't actually save to disk.

26. **Commands.rs has duplicate code**: `handle_slash_command` appears twice (lines 62-288 and 323-536).

27. **Agent `process_request()` is implemented**: It publishes `SubagentStarted`/`SubagentCompleted` events and returns `SubAgentResult::success()`. Review if behavior is correct.

28. **PlanRegistry unused**: `wait_for_response()` has send-then-discard bug.

29. **Bus module memory leak**: Dead letter channels if sender dropped without response.

30. **Auth middleware broken**: Wrong signature and undefined variables in `src/server/middleware/auth.rs`.

31. **Session `share_session` race**: UPSERT + set_share_url not atomic.

32. **Snapshot no persistence**: In-memory only, lost on restart.

33. **Storage race condition**: `std::fs::File::create` vs SQLite atomic creation.

34. **IDE temp file race**: Predictable filenames, needs `mkstemp` or `tempfile` crate.

35. **LSP request ID race**: Wrap-around issue in `client.rs:451-457`.

36. **Config watcher race**: Closure captures `tx` before stored in `self.watcher`.

### Architecture Review Findings (2026-05-26)

37. **Subagent events ARE mapped**: Documentation at `protocol.md:291` claims they are NOT, but `src/core/mod.rs:795-838` shows all 4 ARE mapped.

38. **Client backoff formula**: Document says `(2s, 4s)` but actual is `(1s, 2s, 4s)` at `src/client/attach.rs:39`.

39. **TUI timeline fields**: `timeline_visible` and `timeline_selected` are in `App` struct at `mod.rs:232-233`, NOT in UiState.

40. **ToolDefCache fields**: Documentation missing `lsp_enabled: bool` in cache tuple - actual is `(model, plan_mode, lsp_enabled, mcp_count, perm_ver, definitions)`.

41. **Command count**: Architecture doc says 41, actual is 39 built-in commands at `src/tui/command.rs:78-163`.

42. **Event count**: Architecture doc says 36 events, actual is 34 at `src/bus/events.rs:5-147`.

43. **Snapshot hash inconsistency**: `collect_files_sync` uses MD5 at line 431 but SHA256 elsewhere in the module.

44. **ToolExecutor dead code**: `src/tool/executor.rs:8` exists with retry logic but is never used.

45. **PermissionResponse unused**: `src/permission/mod.rs:1141-1145` defined but not used internally.

46. **check_external_directory unused**: `src/permission/mod.rs:1237-1248` marked `#[allow(dead_code)]`, never called.

47. **Static cache never clears**: `CANONICAL_PATHS_CACHE` at `sandbox.rs:237` initialized once and never invalidated.

48. **Histogram unbounded**: `metrics.rs:122-124` - only `pop_front()` at 1000 per name, but no limit on unique names.

49. **TTS stop() silent failure**: `tts/mod.rs:85-103` returns `Ok(())` even when `pkill say` fails.

50. **TTS init() ignores providers**: `tts/mod.rs:45-49` silently accepts non-`None` providers without warning.

51. **Worktree symlink issue**: Current worktree detection via canonicalization may fail with symlinked directories.

52. **OAuth replay protection race**: `mcp/auth.rs:318-332`.

53. **Plugin dispatch methods**: 9 missing from documentation list at `plugin.md:202-206`.

54. **PluginTier undocumented**: `marketplace.rs:4-9` enum not in architecture docs.

55. **Permission route mismatch**: Server docs show `POST /api/permission/:session_id` but actual is `/api/permission/:session_id/submit`.

56. **.skills/ directory misleading**: Runtime only loads from `~/.config/codegg/skills/` and `.codegg/skills/`, NOT `.skills/` at repo root.

### Testing Commands

```bash
# Always run before/after changes
cargo build --all-features
cargo clippy --all-features -- -D warnings
cargo test --all-features

# Specific feature testing
cargo test --all-features -- --test-threads=1  # For integration tests

# TUI tests
cargo test tui::input
cargo test tui
cargo test messages

# Run specific module tests
cargo test --package codegg -- <module>_test_pattern
```

### Security Reminders

- Security-sensitive changes require additional test coverage
- SSRF protection follows RFC 6892
- Command injection follows OWASP Cheat Sheets
- Path traversal follows OWASP File Upload guidance
- Feature gates: Changes to server/plugin modules need `--all-features` testing

---

## Status Summary

| Category | Status |
|----------|--------|
| Historical Completed | ✅ |
| TUI Input Repair (Completed 2026-05-01) | ✅ |
| TUI Scrolling Fix (Completed 2026-05-06) | ✅ |
| TUI Message Flow (Completed 2026-05-05) | ✅ |
| Wave 0: Quick Wins | ⏳ PENDING |
| Wave 1: Critical Security | ⏳ PENDING |
| Wave 2: High-Priority | ⏳ PENDING |
| Wave 3: Medium-Priority | ⏳ PENDING |
| Wave 4: Large Refactors | ⏳ DEFERRED |
| TUI Enhancement Features | ⏳ SKIPPED |
| Agent Capability Features | ✅ PARTIAL (AGENT-2, AGENT-3 done via PR #33) |
| Mode/Exec Features | ✅ COMPLETE (MODE-1, EXEC-1 done via PR #33) |
| Plugin Marketplace | ✅ COMPLETE (PLUGIN-1 done via PR #33) |
| Architecture Review Items | ⏳ NEW |
| Documentation | ⏳ FUTURE |

---

## Implementation Completed (2026-05-06)

### Wave 0: Quick Wins
| PR | Item | Status | Notes |
|----|------|--------|-------|
| #7 | QW-3: Duplicate handle_slash_command | ✅ | Removed duplicate implementations |
| #9 | QW-5: Early return bug | ✅ | Fixed return statement in /goto command |
| #8 | QW-4: DoomLoop threshold configurable | ✅ | Added `doomloop_threshold` to config |
| #13 | QW-9: Config watcher debounce | ✅ | Added debounce and content hash |
| #10 | QW-4: Remove execute_command | ✅ | Removed dead code |
| #15 | QW-10: Upgrade duplicate logic | ✅ | Refactored to use upgrade module |
| #11 | QW-11: Upgrade request timeout | ✅ | Added -m 300 to curl |
| N/A | QW-7: Content hash | ✅ | Already implemented in QW-9 |
| N/A | QW-6: DeniedTools audit log | ✅ | Already existed in tool/mod.rs |
| N/A | QW-7: DB pool size | ✅ | Already standardized to 10 |

### Wave 1: Critical Security
| PR | Item | Status | Notes |
|----|------|--------|-------|
| #21 | CRIT-1: mdns.rs unsafe | ✅ | Verified already using socket2 |
| #20 | CRIT-2: API key encryption config | ✅ | Integrated crypto with config |
| #18 | CRIT-3: SSRF duplication | ✅ | Centralized in ssrf.rs |
| #16 | CRIT-4: Storage race conditions | ✅ | Removed std::fs::File::create, added WAL |
| #19 | CRIT-5: Memory persistence | ✅ | Added atomic saves, file locking |
| #17 | CRIT-6: Snapshot persistence | ✅ | SQLite persistence, restore, SHA-256 |

### Wave 2: High-Priority Infrastructure
| PR | Item | Status | Notes |
|----|------|--------|-------|
| #23 | HIGH-1: MCP auto-reconnect | ✅ | Wired reconnect(), added heartbeat |
| #22 | HIGH-2: WebSocket per-session rate | ✅ | Added session-based rate limiting |
| N/A | HIGH-3: block_on in subagent | ✅ | Not found - already using tokio::spawn |
| #13 | HIGH-4: Config watcher | ✅ | Combined with QW-9 |
| #24 | HIGH-5: Hooks emit events | ✅ | SessionStart/End, error logging |
| #25 | HIGH-6: Bus memory leak | ✅ | TTL cleanup, removed async |

### Wave 3: Medium-Priority Groups
| PR | Item | Status | Notes |
|----|------|--------|-------|
| #28 | GROUP-A: Security hardening | ✅ | A-1 to A-4 all completed |
| #26 | GROUP-D: Agent loop | ✅ | D-1 summarization exists, D-2 doc fixed |
| #29 | GROUP-E: Provider system | ✅ | E-1 to E-4 all completed |
| #27 | GROUP-F: Tool system | ✅ | F-1 (TerminalTool), F-2 (allowlist fix) |
| #31 | GROUP-C: TUI improvements | ✅ | C-1,C-2 documented, C-3,C-4 implemented |
| #30 | GROUP-G: Testing | ✅ | G-1,G-4,G-5 done; G-2,G-3 need CI |

### Diversions from Plan
1. **QW-12 (Content hash)** - Already implemented, merged with QW-9
2. **QW-14 (PTY rename)** - Renamed `src/pty/` to `src/shell/` to clarify purpose
3. **HIGH-3 (block_on)** - Not found in codebase, already using tokio::spawn

---

## Consolidated Statistics

| Metric | Value |
|--------|-------|
| Waves 0-3 Completed | ✅ All (via 25+ PRs) |
| Architecture Review Items | ~55 new items |
| Future Features | ~15 items remaining |
| PRs Created (Waves 0-3 + Features) | 33 |
| Wave 4 (Large Refactors) | ⏳ DEFERRED |
| TUI Enhancement | ⏳ SKIPPED |
| Agent Capabilities | ✅ Partial (2/8 done) |
| Mode/Exec Features | ✅ Complete (MODE-1, EXEC-1) |
| Plugin Marketplace | ✅ Complete (PLUGIN-1) |
| Documentation | ⏳ FUTURE |

---

*(End of file)*