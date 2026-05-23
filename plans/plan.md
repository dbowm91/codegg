# CodeGG Module Review Implementation Plan

**Status**: IN PROGRESS
**Last Updated**: 2026-05-27
**Goal**: Address all bugs and documentation issues identified across 28 module reviews.

---

## Overview

This plan consolidates work from 28 module architecture reviews. Items are organized into waves based on dependencies and parallelization potential.

- **Wave 1**: Documentation-only fixes (no code changes) - Can parallelize across 15 files
- **Wave 2**: Independent code bugs (no interdependencies) - Can parallelize across modules
- **Wave 3**: Bugs requiring coordination (has dependencies)
- **Wave 4**: Already Fixed (Reference only - no action needed)

---

## Wave 1: Documentation Fixes (Parallelizable - No Code Changes)

Multiple agents can work on different documentation files concurrently.

### W1A: Error Module Documentation
**File**: `architecture/error.md`
- ConfigError::Watch HTTP status says 400 but should be 500 (line 198)
- Missing StorageError::Import/Export HTTP mapping (line 200)
- ProviderError::is_retryable() missing Auth variant in docs (line 107-114)
- ToolError::Permission grouping undocumented (line 207)
- McpError::is_retryable() undocumented (line ~115)
- LspError::is_retryable() undocumented (line ~115)

### W1B: Event Bus Documentation
**File**: `.opencode/skills/event-bus/SKILL.md`
- Event count says 38 but actual is 36 (line 71)
- "Other" category count wrong: says 7 but actual is 8 (line 84)

### W1C: Exec Documentation
**File**: `.opencode/skills/exec/SKILL.md`
- Missing PROVIDER_NOT_FOUND error code (line ~40)
- Missing INTERNAL_ERROR code (line 263 in exec.rs)
- Skill doc shows only 11 error codes but implementation has 26 (line 71-86)

### W1D: Hooks Documentation
**File**: `architecture/hooks.md`
- Config example shows deprecated `args` field which doesn't exist in HookConfig::ShellCommand (line 115)
- HookType::as_str() returns dot notation but not documented (line 28-29)
- Plugin hook timeout (5s) and error format not documented

### W1E: Client Documentation
**File**: `.opencode/skills/client/SKILL.md`
- WebSocket retry logic undocumented (3 retries, 2s/4s backoff)
- Task count says 3 but actually 2 (line 57)
- Missing Bearer token auth documentation (line 27-29)
- Line counts outdated: attach.rs is 154 not 118, sdk.rs is 53 not 44

### W1F: Command Documentation
**File**: `architecture/command.md`
- Alias formatting inconsistent: doc shows `/exit | /quit, /q` but aliases stored without leading `/`
- CommandRegistry section uses wrong Command struct (should be `src/tui/command.rs:25-37` not `src/command/mod.rs`)
- Plugin commands undocumented (`src/command/plugin.rs` with PluginCommand enum)

### W1G: Config Documentation
**File**: `architecture/config.md` and `.opencode/skills/config/SKILL.md`
- Config struct missing `schema` field with `#[serde(rename = "$schema")]`
- Agent config merge is full replace, not field-by-field (lines 236-244 in paths.rs)
- Wrong line number references in skill doc (line 248: should be 542 not 508-509)
- Wrong constant name: CRYPTO_V2_PREFIX should be FORMAT_V2_PREFIX (line 168)

### W1H: Crypto Documentation
**File**: `architecture/crypto.md`
- `decrypt_from_string` import typo (duplicate, line 125-126)
- Legacy migration documentation misleading ("automatically" suggests always, but only when `encrypt_provider_keys()` called)
- api_key() method documentation incorrect (should show `prefix: &str` parameter)

### W1I: IDE Documentation
**File**: `.opencode/skills/ide/SKILL.md`
- Exit status formats as `ExitStatus(0)` instead of just `0` (lines 146-147, 233-235)
- Generic fallback description incorrect - IDE-specific handlers ALSO create temp files

### W1J: Memory Documentation
**File**: `architecture/memory.md`
- Storage directory structure: shows `projects/{hash}/conventions/` but actual is `project/{hash}/` (line 69-77)
- Missing `set_auto_save()` method documented
- `/memory-list` behavior when query is empty not documented (shows both user/preferences AND project memories)
- Convention pattern scoring not in table

### W1K: Permission Documentation
**File**: `architecture/permission.md`
- clear_decisions() method undocumented (line 652-654 in mod.rs)
- check_external_directory function undocumented (line 1236-1248)
- PermissionChoice type undocumented (line 128-134)
- PermissionResponse type undocumented (line 1141-1145)
- check_legacy methods undocumented (lines 439-441, 532-538, 637-650)
- Missing `allowed_paths` column in task table schema (v14 migration)

### W1L: Plugin Documentation
**File**: `.opencode/skills/plugin/SKILL.md`
- BuiltinPlugin struct not exported (exists in `builtin/mod.rs:13-16`)
- list_official_plugins() and list_repository_plugins() are TODO stubs (not implemented)

### W1M: Session Documentation
**File**: `architecture/session.md`
- Missing `time_deleted` column in session table schema (v12 migration)
- has_checkpoint() note could be clearer about semantic meaning

### W1N: Tool Documentation
**File**: `architecture/tool.md`
- Documentation lists 33+ tools but only 26 exist in `with_defaults()` (line 11)
- lsp tool documented but NOT registered (requires service injection)
- deferred_tools() and is_deferred() methods undocumented
- ToolSearchTool not documented in External Integrations
- InvalidTool not documented

### W1O: Upgrade Documentation
**File**: `.opencode/skills/upgrade/SKILL.md`
- upgrade() function is dead code - CLI never calls it (main.rs:551-570 only calls check_for_updates)
- upgrade() uses curl script but CLI suggests cargo install - inconsistent

### W1P: TUI Documentation
**File**: `architecture/tui.md`
- UiState struct missing fields: `running`, `remote_status`, `timeline_visible`, `timeline_selected`, `render_panic_count`, `last_render_error`
- SessionState struct missing fields: `history_pos`, `last_edited_file`, `rpm_limit`, `tpm_limit`, `rpm_remaining`, `tpm_remaining`, `permission_pending`, `subagent_count`
- help_overlay.rs and tool_output.rs not in directory structure
- Theme count says 30+ but actual is 42

---

## Wave 2: Independent Code Bugs (Parallelizable - No Dependencies)

Each module's bugs are independent. Multiple agents can fix bugs in different modules concurrently.

### W2A: TTS Module Bugs (5 bugs - can parallelize within module)

**Bug 1**: speak() error path doesn't reset `speaking` flag
- **File**: `src/tts/mod.rs:57`
- **Issue**: When Command fails, `map_err(AppError::Io)?` returns early before lines 58-59 reset the flag
- **Fix**: Restructure to reset flag on all error paths, or use `map_err(|e| { self.speaking.store(false, Ordering::SeqCst); AppError::Io(e) })`

**Bug 2**: Race condition between concurrent speak() and stop()
- **File**: `src/tts/mod.rs`
- **Issue**: `AtomicBool` without synchronization - concurrent access not protected
- **Fix**: Add `Mutex` to serialize access to speaking state

**Bug 3**: stop() silently ignores pkill failure
- **File**: `src/tts/mod.rs:74-77`
- **Issue**: Result from `pkill say` discarded with `let _`
- **Fix**: Log warning when pkill fails, return error

**Bug 4**: speak() accepts empty string
- **File**: `src/tts/mod.rs`
- **Issue**: `say ""` succeeds but produces no audio - no validation
- **Fix**: Return early error for empty strings

**Bug 5**: stop() always returns Ok(())
- **File**: `src/tts/mod.rs:78`
- **Issue**: Function signature misleading - always succeeds even if pkill fails
- **Fix**: Return `Result<(), AppError>` to propagate pkill failures

### W2B: Resilience Module Bugs (2 bugs)

**Bug 1**: last_failure_time never cleared on recovery
- **File**: `src/resilience/circuit.rs:129-132`
- **Issue**: When circuit transitions from HalfOpen to Closed, `failure_count` is reset but `last_failure_time` is not
- **Fix**: Add `*self.inner.last_failure_time.write().await = None;` after line 131

**Bug 2**: No maximum HalfOpen duration
- **File**: `src/resilience/circuit.rs:83`
- **Issue**: HalfOpen state has no timeout - if probe never completes, circuit stays in HalfOpen forever
- **Fix**: Add `max_half_open_duration` field with default (e.g., 30 seconds), force transition back to Open if timeout exceeded

### W2C: Security Module Bugs (2 bugs)

**Bug 1**: validate_path_safety() canonicalizes allowed_paths on every call
- **File**: `src/security/sandbox.rs:247-250`
- **Issue**: Performance issue - `std::fs::canonicalize(allowed)` called for every allowed path on every invocation
- **Fix**: Canonicalize allowed_paths once at initialization or first call, store canonical paths

**Bug 2**: Missing test coverage for ssrf.rs
- **File**: `src/security/ssrf.rs`
- **Issue**: No unit tests for `is_internal_ip`, `validate_host_ip`, `revalidate_dns`, `validate_url_host`
- **Fix**: Add comprehensive tests for all IP ranges (loopback, private, link-local, multicast, IPv4-mapped), URL parsing edge cases, DNS rebinding detection

### W2D: Storage Module Bug (1 bug)

**Bug 1**: Redundant migration execution in init()
- **File**: `src/storage/mod.rs:19-23` and `src/storage/mod.rs:122-124`
- **Issue**: `Database::new()` calls `migrate()` at line 21, then `init()` calls it again at line 124
- **Fix**: Remove duplicate migrate() call - keep only in Database::new()

### W2E: Snapshot Module Bugs - Independent (2 bugs)

**Bug 1**: to_relative_path() silently falls back
- **File**: `src/snapshot/mod.rs:243-250`
- **Issue**: Returns absolute path without checking if path is actually safe when `strip_prefix` fails
- **Fix**: Log warning when fallback occurs, consider returning Err

**Bug 2**: No validation for zero limits in SnapshotOptions
- **File**: `src/snapshot/mod.rs`
- **Issue**: Zero `max_files` or `max_bytes` not validated - `collect_files_sync()` returns empty immediately
- **Fix**: Validate in `new_with_options()` that limits are > 0

### W2F: Provider Module Bugs (4 bugs - can parallelize within module)

**Bug 1**: Google Provider ToolCall ID Ignored
- **File**: `src/provider/google.rs:353`
- **Issue**: Response ID discarded, new UUID generated instead of using `functionCall.id`
- **Fix**: Extract and use the `id` field from functionCall response

**Bug 2**: Anthropic Beta Header Not Configurable
- **File**: `src/provider/anthropic.rs:178`
- **Issue**: Hardcoded `anthropic-beta: prompt-caching-2024-07-31` header may fail for non-beta accounts
- **Fix**: Make beta feature conditional or configurable via provider config

**Bug 3**: OpenAI Tool Arguments Double Serialization
- **File**: `src/provider/openai.rs:178`, `src/provider/openai_compatible.rs:132`
- **Issue**: `tc.arguments.to_string()` may double-serialize if arguments is already a JSON string
- **Fix**: Check if arguments are already a string before calling to_string()

**Bug 4**: register_env_fallback_provider Silent Failure
- **File**: `src/provider/mod.rs:364-368`
- **Issue**: Empty key logs at `debug!` level - may be invisible in production
- **Fix**: Log warning or return Err when env config invalid

### W2G: TUI Module Bugs (5 bugs - can parallelize within module)

**Bug 1**: Hardcoded PATH at lines 5797 and 5817
- **File**: `src/tui/app/mod.rs:5797, 5817` (get_git_branch, check_git_dirty)
- **Issue**: Uses hardcoded "/usr/local/bin:/usr/bin:/bin" instead of user's actual PATH
- **Fix**: Use `std::env::var_os("PATH").unwrap_or_default()` like other modules

**Bug 2**: SelectTreeSession result unused
- **File**: `src/tui/app/mod.rs:1623-1634`
- **Issue**: Spawned task loads session but discards result - dead code
- **Fix**: Remove dead code or implement intended functionality

**Bug 3**: InfoDialog memory leak potential
- **File**: `src/tui/app/mod.rs:3859-3881`
- **Issue**: `info_dialog` not set to None when closed
- **Fix**: Set `info_dialog = None` in close handler

**Bug 4**: render_dialog early return when FocusManager empty but dialog open
- **File**: `src/tui/app/mod.rs:1178-1189`
- **Issue**: State inconsistency - silently returns without logging
- **Fix**: Log error and reset state instead of silent return

**Bug 5**: State inconsistency handler incomplete
- **File**: `src/tui/app/mod.rs:1796-1804`
- **Issue**: Only resets `ui_state.dialog`, doesn't clear pending dialog state
- **Fix**: Also clear pending_delete_session and other dialog state

### W2H: Server Module Bugs - Independent (4 bugs)

**Bug 1**: Health Check Duplicated
- **File**: `src/server/http.rs:111-113` vs `src/server/routes/health.rs:3-4`
- **Issue**: Two different health check implementations - inline one used at http.rs:296, routes/health.rs is dead code
- **Fix**: Remove duplicate in http.rs, use routes/health.rs consistently

**Bug 2**: Auth returns 401 when disabled without token
- **File**: `src/server/middleware/auth.rs:37-41`
- **Issue**: Returns 401 when `CODEGG_SERVER_AUTH_DISABLED` is set AND no token configured
- **Fix**: Return 200 or continue when auth disabled and no token provided

**Bug 3**: TuiSessionState.model Hardcoded
- **File**: `src/server/ws.rs:470`
- **Issue**: Model hardcoded as `"anthropic/claude-sonnet-4-20250514"` instead of using client's configured model
- **Fix**: Read model from TuiSessionState or client config

**Bug 4**: rate_limit_key Format Issue
- **File**: `src/server/ws.rs:541`
- **Issue**: If SessionInfo id is empty, rate_limit_key becomes `"session:"` with trailing colon
- **Fix**: Normalize key format, handle empty id case

---

## Wave 3: Bugs Requiring Coordination (Has Dependencies)

These bugs require more analysis or coordination between modules. They should be addressed after Wave 2.

### W3A: Server Module - Requires Understanding of Intended Design

**Bug**: submit_permission doesn't actually submit
- **File**: `src/server/routes/permission.rs:23-45`
- **Issue**: Function validates but never calls `PermissionRegistry::respond()` to record decision
- **Fix**: Requires understanding of intended permission flow - implement actual submission logic or connect to bus
- **Note**: This is a core design issue - needs analysis of how permissions should flow through the system

### W3B: Server Module - Requires Middleware Coordination

**Bug**: WebSocket Auth Logic Inconsistent
- **File**: `src/server/ws.rs:43` vs `src/server/middleware/auth.rs:12`
- **Issue**: `validate_ws_auth()` checks `CODEGG_SERVER_AUTH_DISABLED` via `is_err()` (auth disabled if env var NOT set), but `auth_middleware` checks via `is_ok()` (auth disabled if env var IS set)
- **Fix**: Unify auth validation logic between HTTP and WebSocket paths
- **Note**: This requires understanding the intended auth behavior

### W3C: Snapshot Module - Error Handling Dependencies

**Bug 1**: error swallowing in restore() and restore_to_path()
- **File**: `src/snapshot/mod.rs:272-273` and `308-309`
- **Issue**: `spawn_blocking` inner closure errors silently discarded - the `?` only propagates join errors, not inner operation errors
- **Fix**: Use oneshot channel to propagate inner errors:
```rust
let (tx, rx) = tokio::sync::oneshot::channel();
spawn_blocking(move || {
    // ... file operations ...
    let _ = tx.send(result);
}).await;
let result = rx.await??; // Properly propagates errors
```

**Bug 2**: TOCTOU race in restore_to_path()
- **File**: `src/snapshot/mod.rs:289-302`
- **Issue**: canonicalize check then write leaves race window between operations
- **Fix**: Use rename operation atomically, or hold lock for duration, or use O_NOFOLLOW in open flags

### W3D: Plugin Module - WASM Path Issue

**Bug**: WASM path construction uses wrong directory
- **File**: `src/plugin/loader.rs:276-278`
- **Issue**: Path constructed as `plugins/{plugin_id}/plugin.wasm` but plugin_id includes `plugin:` prefix, so path becomes `plugins/plugin:my-plugin/plugin.wasm` instead of using `install::plugins_dir()` which is `~/.local/share/codegg/plugins/`
- **Fix**: Use `install::plugins_dir()` and strip `plugin:` prefix from plugin_id, or accept bare plugin name without prefix
- **Impact**: All WASM plugins fail to execute

### W3E: LSP Module - Unused Code

**Bug**: build_env_overrides defined but never used
- **File**: `src/lsp/server.rs:405-411`
- **Issue**: Function defined but never called - `LspService::get_env_overrides` duplicates its logic
- **Fix**: Either integrate properly or remove dead code

---

## Wave 4: Already Fixed (Reference Only)

These items were fixed in previous PRs. Documented for reference only.

| Module | Issue | Fix PR |
|--------|-------|--------|
| LSP | request_id AtomicU64 wrap-around | PR #35 |
| LSP | PATH parsing with std::env::split_paths() | PR #35 |
| LSP | PHP server mapping to php-language-server | PR #35 |
| LSP | Request timeout 30s with LspError::RequestTimeout | PR #35 |
| LSP | close_file/save_file race conditions | PR #35 |
| MCP | McpConnectionManager Clone impl soundness | PR #35 |
| MCP | SSE integration documented as known issue | PR #35 |
| Provider | FallbackProvider exponential backoff | PR #34 |
| Bus | Memory leak and async issues | PR #33 |
| Config | migrate and validate in ConfigWatcher reload | PR #32 |
| MCP | DNS rebinding protection and ensure_connected race | PR #31 |
| Resilience | TOCTOU race in CircuitBreaker::is_available() | PR #30 |
| Security | validate_path_safety() symlink check | PR #29 |
| IDE | Temp file handle bug - drop before invoking IDE | PR #28 |

---

## Implementation Guidance

### Parallelization Strategy

**Wave 1 (Documentation)**: Can be completed by multiple agents working in parallel on different files. No code coordination needed. Each agent can claim 1-2 documentation files and make all fixes for those files.

**Wave 2 (Independent Code Bugs)**: Can be parallelized across modules. Each module's bugs are independent - e.g., TTS bugs don't affect resilience bugs. Agents should claim one module at a time to avoid conflicts.

**Wave 3 (Dependent Bugs)**: Requires more analysis and coordination. Should be addressed after Wave 2 is complete. Some items may need design decisions before implementation.

### Priority Order Recommendation

1. **Wave 2 first** - Quick wins, no dependencies, makes immediate improvements
2. **Wave 1 in parallel** - Documentation can be updated alongside code work
3. **Wave 3 last** - Requires more analysis and coordination

### Testing Recommendations

**TTS Module**:
- Test concurrent speak/stop race condition
- Test speak with empty string returns error
- Test stop() error propagation

**Resilience Module**:
- Test last_failure_time reset on HalfOpen→Closed transition
- Test HalfOpen timeout enforcement (30s default)

**Security Module**:
- Add ssrf.rs unit tests for IPv4/IPv6/localhost variants
- Benchmark validate_path_safety with cached vs uncached paths

**Server Module**:
- Integration test for permission submission flow
- Auth disabled behavior test

### Estimated Effort

- Wave 1: ~3-4 hours (16 documentation files, can parallelize 4x)
- Wave 2: ~5-7 hours (17 independent bugs, can parallelize 4-5x)
- Wave 3: ~4-6 hours (5 bugs requiring design decisions)
- **Total**: ~12-17 hours across multiple engineers

---

## Future Items (Not in Current Scope)

These improvements were identified during review but are not in current scope:

### Tool System Enhancements
- Implement deferred/lazy tool loading (ToolSearch pattern)
- Add `defer_loading` field to ToolDefinition
- Integrate with provider capability detection
- Consider BM25/embeddings-based search upgrade path

### Memory Module Enhancements
- On-demand memory loading optimization
- Git-aware project scoping improvements
- During-session memory commands

### Architecture Documentation
- Team Coordination system (~680 lines) undocumented in agent module
- EventProcessor module undocumented
- Prompt Template System undocumented
- Mention System undocumented

---

## Verification Checklist

When implementing items, verify against the original review files in `plans/*-review.md`:

1. **Documentation fixes**: Check the specific file and line numbers mentioned
2. **Code bugs**: Read the relevant source file to understand the current implementation before making changes
3. **Test additions**: Ensure tests cover the bug scenarios described
4. **Error handling**: Verify error propagation works correctly (use `?` or proper error channels)

---

*This plan was consolidated from 28 module review files. For detailed context on any item, refer to the original review file in `plans/<module>-review.md`.*