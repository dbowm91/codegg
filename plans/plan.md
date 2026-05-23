# CodeGG Module Review Implementation Plan

**Status**: IN PROGRESS  
**Last Updated**: 2026-05-23  
**Goal**: Address all bugs and documentation issues identified across 28 module reviews.

---

## Overview

- Total Documentation Fixes: ~35 items across 15 documentation files
- Total Code Bugs: ~27 items across 8 modules
- Status: Much work remains (plan.md previously marked COMPLETED incorrectly)

---

## Wave 1: Documentation Fixes (Can Parallelize - No Code Changes)

Documentation-only fixes that require no code changes. Multiple files can be updated concurrently.

### Error Module Documentation
- **File**: `architecture/error.md`
- ConfigError::Watch HTTP status says 400 but should be 500
- Missing StorageError::Import/Export HTTP mapping
- ProviderError::is_retryable() missing Auth variant in docs
- ToolError::Permission grouping undocumented

### Event Bus Documentation
- **File**: `.opencode/skills/event-bus/SKILL.md`
- Skill file incorrect event count (says 38, has 36)

### Exec Documentation
- **File**: `.opencode/skills/exec/SKILL.md`
- Missing PROVIDER_NOT_FOUND error code
- Skill doc outdated error code list (11 vs 26)
- INTERNAL_ERROR code undocumented

### Hooks Documentation
- **File**: `.opencode/skills/hooks/SKILL.md`
- Config example shows deprecated args field
- HookType::as_str() dot notation not documented

### Client Documentation
- **File**: `.opencode/skills/client/SKILL.md`
- WebSocket retry logic undocumented (3 retries, 2s/4s backoff)
- Task count inconsistency (says 3 tasks, actually 2)
- Missing Bearer token auth documentation

### Command Documentation
- **File**: `.opencode/skills/command/SKILL.md`
- Alias formatting inconsistent
- CommandRegistry documentation uses wrong struct
- Plugin commands not documented

### Config Documentation
- **File**: `.opencode/skills/config/SKILL.md`
- Config struct missing schema field
- Agent config merge is full replace, not field-by-field
- Wrong constant name references

### Crypto Documentation
- **File**: `architecture/crypto.md` / `.opencode/skills/crypto/SKILL.md`
- api_key() method documentation incorrect
- Legacy migration documentation misleading

### IDE Documentation
- **File**: `.opencode/skills/ide/SKILL.md`
- Exit Status Display formats as ExitStatus(0) instead of just 0

### Memory Documentation
- **File**: `.opencode/skills/memory/SKILL.md`
- Storage directory structure mismatch (projects vs project)
- Missing set_auto_save() method documented
- /memory-list behavior when query is empty

### Permission Documentation
- **File**: `.opencode/skills/permission/SKILL.md`
- clear_decisions() undocumented
- check_external_directory undocumented
- PermissionChoice type undocumented
- check_legacy methods undocumented

### Plugin Documentation
- **File**: `.opencode/skills/plugin/SKILL.md`
- BuiltinPlugin struct not exported

### Session Documentation
- **File**: `.opencode/skills/session/SKILL.md`
- Missing time_deleted column in schema doc
- has_checkpoint() note inaccurate

### Tool Documentation
- **File**: `.opencode/skills/tool/SKILL.md`
- Documentation lists 33+ tools but only 26 exist
- LSP tool documented but NOT registered
- ToolSearchTool not documented in External Integrations
- InvalidTool not documented
- terminal tool description vague

### Upgrade Documentation
- **File**: `.opencode/skills/upgrade/SKILL.md`
- upgrade() function dead code (CLI never calls it)
- Inconsistent upgrade mechanisms

### TUI Documentation
- **File**: `architecture/tui.md`
- UiState struct missing 6 fields in docs
- SessionState struct missing 8+ fields in docs
- help_overlay.rs and tool_output.rs not in directory structure

---

## Wave 2: Independent Code Bugs (Can Parallelize - No Dependencies)

Code bugs that have no interdependencies and can be fixed concurrently across different modules.

### TTS Module (5 bugs)

**Bug 1**: speak() error path doesn't reset speaking flag
- **File**: `src/tts/mod.rs:57`
- **Description**: When Command fails, `map_err(AppError::Io)?` returns early before lines 58-59 reset the flag
- **Fix**: Restructure to reset flag in a finally block or use Drop guard

**Bug 2**: Race condition between concurrent speak() and stop()
- **File**: `src/tts/mod.rs`
- **Description**: AtomicBool without synchronization - concurrent access not protected
- **Fix**: Add Mutex or use atomic fence operations for synchronization

**Bug 3**: stop() silently ignores pkill failure
- **File**: `src/tts/mod.rs`
- **Description**: `result` from pkill discarded without logging or handling
- **Fix**: Log pkill failures at debug/warn level

**Bug 4**: speak() accepts empty string
- **File**: `src/tts/mod.rs`
- **Description**: `say ""` succeeds but produces no audio - no validation
- **Fix**: Return early error for empty strings

**Bug 5**: stop() always returns Ok(())
- **File**: `src/tts/mod.rs`
- **Description**: Function signature misleading - always succeeds even if pkill fails
- **Fix**: Return Result<(), AppError> to propagate pkill failures

### Resilience Module (2 bugs)

**Bug 1**: last_failure_time never cleared on recovery
- **File**: `src/resilience/circuit.rs:129-132`
- **Description**: When circuit recovers, last_failure_time is not reset, causing incorrect failure count calculation
- **Fix**: Reset last_failure_time when transitioning from Open to HalfOpen

**Bug 2**: No maximum HalfOpen duration
- **File**: `src/resilience/circuit.rs:83`
- **Description**: HalfOpen state has no timeout - if probe never completes, circuit stays in HalfOpen forever
- **Fix**: Add max_half_open_duration field with default (e.g., 30 seconds)

### Security Module (2 bugs)

**Bug 1**: validate_path_safety() canonicalizes allowed_paths on every call
- **File**: `src/security/sandbox.rs:247-250`
- **Description**: Performance issue - allowed_paths canonicalized repeatedly instead of cached
- **Fix**: Cache canonicalized allowed_paths on initialization or first call

**Bug 2**: Missing test coverage for ssrf.rs
- **File**: `src/security/ssrf.rs`
- **Description**: No unit tests for validate_url_host, is_internal_ip functions
- **Fix**: Add tests for IPv4, IPv6, IPv4-mapped IPv6, localhost variations

### Storage Module (1 bug)

**Bug 1**: redundant migration execution in init()
- **File**: `src/storage/mod.rs`
- **Description**: `migrate()` called twice (once in Database::init(), once in `init()` function)
- **Fix**: Remove duplicate migrate() call - keep only in Database::init()

### Snapshot Module (2 independent bugs)

**Bug 1**: to_relative_path() silently falls back
- **File**: `src/snapshot/mod.rs`
- **Description**: Returns relative path without checking if path is actually safe
- **Fix**: Return Err if path would escape project_root

**Bug 2**: no validation for zero limits in SnapshotOptions
- **File**: `src/snapshot/mod.rs`
- **Description**: Zero max_files or max_bytes not validated
- **Fix**: Add validation returning error for zero limits

---

## Wave 3: Bugs Requiring Coordination (Cannot Parallelize Due to Dependencies)

### Provider Module (4 confirmed bugs - CAN parallelize within module)

**Note**: These Provider bugs are independent of each other and can be parallelized within the module.

**Bug 1**: Google Provider ToolCall ID Ignored
- **File**: `src/provider/google.rs:353`
- **Description**: Response ID discarded, new UUID generated instead of using functionCall.id
- **Fix**: Extract and use the id field from functionCall response

**Bug 2**: Anthropic Beta Header Not Configurable
- **File**: `src/provider/anthropic.rs:178`
- **Description**: Beta header "prompt-caching-2024-07-31" is hardcoded
- **Fix**: Allow headers map to override or make configurable via provider config

**Bug 3**: OpenAI Tool Arguments Double Serialization
- **File**: `src/provider/openai.rs:178`, `src/provider/openai_compatible.rs:132`
- **Description**: arguments.to_string() may re-serialize already-serialized JSON
- **Fix**: Check if arguments are already a string before calling to_string()

**Bug 4**: register_env_fallback_provider Silent Failure
- **File**: `src/provider/mod.rs:364-368`
- **Description**: Empty key silently ignored at debug level (may be invisible in production)
- **Fix**: Log warning or return Err when env config invalid

### Snapshot Module (2 bugs with dependencies)

**Bug 3**: error swallowing in restore() and restore_to_path()
- **File**: `src/snapshot/mod.rs`
- **Description**: spawn_blocking inner closure errors silently discarded
- **Fix**: Propagate errors from spawn_blocking closure via oneshot channel

**Bug 4**: TOCTOU race in restore_to_path()
- **File**: `src/snapshot/mod.rs`
- **Description**: Time-of-check-time-of-use between canonicalize and write
- **Fix**: Use rename operation atomically or hold lock for duration

### TUI Module (5 bugs - partial dependencies)

**Bug 1**: Hardcoded PATH at lines 5797 and 5817
- **File**: `src/tui/app/mod.rs:5797, 5817` (get_git_branch, check_git_dirty)
- **Description**: Uses hardcoded "/usr/local/bin:/usr/bin:/bin" instead of user's actual PATH
- **Fix**: Use `std::env::var_os("PATH")` like other modules

**Bug 2**: SelectTreeSession result unused - dead code
- **File**: `src/tui/app/mod.rs:1623-1634`
- **Description**: Result of SelectTreeSession stored but never used
- **Fix**: Remove dead code or implement intended functionality

**Bug 3**: InfoDialog memory leak potential
- **File**: `src/tui/app/mod.rs`
- **Description**: info_dialog not set to None when closed
- **Fix**: Set info_dialog = None in close handler

**Bug 4**: render_dialog early return when FocusManager empty but dialog open
- **File**: `src/tui/app/mod.rs`
- **Description**: State inconsistency - debug_assert was removed, now silently returns
- **Fix**: Log error and reset state instead of silent return

**Bug 5**: State inconsistency handler incomplete
- **File**: `src/tui/app/mod.rs`
- **Description**: Inconsistent state handling doesn't fully reset or report
- **Fix**: Add proper error logging and state recovery

### Server Module (9 bugs - dependency analysis)

**Can parallelize (no dependencies):**
- Health Check Duplicated (http.rs vs routes/health.rs)
- Auth returns 401 when disabled without token (middleware/auth.rs:37-41)
- TuiSessionState.model Hardcoded (ws.rs:470)
- rate_limit_key Format Issue (ws.rs:541)

**Requires coordination:**
- submit_permission doesn't actually submit (routes/permission.rs:23-45) - core design issue, needs understanding of intended flow
- WebSocket Auth Logic Inconsistent (ws.rs:43 vs middleware/auth.rs:12) - requires middleware changes

**Bug 1**: submit_permission doesn't actually submit
- **File**: `src/server/routes/permission.rs:23-45`
- **Description**: Function exists but never actually submits permission request
- **Fix**: Implement actual submission logic or connect to bus

**Bug 2**: Health Check Duplicated
- **File**: `src/server/http.rs` vs `src/server/routes/health.rs`
- **Description**: Two different health check implementations
- **Fix**: Consolidate to single implementation in routes/health.rs

**Bug 3**: Auth returns 401 when disabled without token
- **File**: `src/server/middleware/auth.rs:37-41`
- **Description**: Returns 401 Unauthorized even when auth disabled
- **Fix**: Return 200 or continue when auth disabled and no token provided

**Bug 4**: TuiSessionState.model Hardcoded
- **File**: `src/server/ws.rs:470`
- **Description**: Model hardcoded instead of using client's configured model
- **Fix**: Read model from TuiSessionState or client config

**Bug 5**: rate_limit_key Format Issue
- **File**: `src/server/ws.rs:541`
- **Description**: Rate limit key format inconsistent with other parts
- **Fix**: Normalize key format across server

**Bug 6**: WebSocket Auth Logic Inconsistent
- **File**: `src/server/ws.rs:43` vs `src/server/middleware/auth.rs:12`
- **Description**: Auth validation differs between WebSocket upgrade and HTTP auth
- **Fix**: Unify auth validation logic

---

## Wave 4: Already Fixed (Reference)

Items fixed in previous PRs - documented for reference only, no action needed.

| Module | Issue | Fix Date |
|--------|-------|----------|
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

## Implementation Notes

### Parallelization Strategy

**Wave 1 (Documentation)** can be completed by multiple agents working in parallel on different documentation files. No code coordination needed.

**Wave 2 (Independent Code Bugs)** can be parallelized across modules. Each module's bugs are independent - e.g., TTS bugs don't affect resilience bugs.

**Wave 3 (Dependent Bugs)** requires sequential work:
1. Server middleware/auth changes must complete before WebSocket auth consistency
2. Provider serialization fixes should be coordinated (openai.rs + openai_compatible.rs)
3. Snapshot TOCTOU fix depends on understanding error propagation pattern

### Testing Recommendations

**TTS Module:**
- Test concurrent speak/stop race condition
- Test speak with empty string returns error
- Test stop() error propagation

**Resilience Module:**
- Test last_failure_time reset on recovery
- Test HalfOpen timeout enforcement

**Security Module:**
- Add ssrf.rs unit tests for IPv4/IPv6/localhost variants
- Benchmark validate_path_safety with cached vs uncached paths

**Provider Module:**
- Test SSE parser with boundary conditions
- Test double serialization edge case
- Test clock skew tolerance

**Server Module:**
- Integration test for permission submission flow
- Auth disabled behavior test

### Priority Order Recommendation

1. **Wave 2** first (quick wins, no dependencies)
2. **Wave 1** in parallel (documentation can be updated alongside code)
3. **Wave 3** last (requires more analysis and coordination)

### Estimated Effort

- Wave 1: ~2-3 hours (15 documentation files)
- Wave 2: ~4-6 hours (12 independent bugs)
- Wave 3: ~6-8 hours (15 dependent bugs)
- **Total**: ~12-17 hours across multiple engineers

---

## Future Items (Not in Current Scope)

The following are longer-term improvements identified during review but not in current scope:

### Tool System Enhancements (from tooluse.md)
- Implement deferred/lazy tool loading (ToolSearch pattern)
- Add `defer_loading` field to ToolDefinition
- Integrate with provider capability detection
- Consider BM25/embeddings-based search upgrade path
- Eggsact crate integration for math/text tools

### Memory Module Enhancements (from memory-improvements.md)
- On-demand memory loading optimization
- Git-aware project scoping improvements
- During-session memory commands

### Architecture Documentation
- Team Coordination system (~680 lines) undocumented in agent module
- EventProcessor module undocumented
- Prompt Template System undocumented
- Mention System undocumented