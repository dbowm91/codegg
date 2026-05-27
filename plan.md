# Implementation Plan

**Status**: IN PROGRESS
**Last Updated**: 2026-05-27

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

## Implementation Completed 2026-05-06

### Wave 0: Quick Wins
| PR | Item | Status | Notes |
|----|------|--------|-------|
| #7 | QW-3: Duplicate handle_slash_command | ✅ | Removed duplicate implementations |
| #9 | QW-5: Early return bug | ✅ | Fixed return statement in /goto command |
| #8 | QW-6: DoomLoop threshold configurable | ✅ | Added `doomloop_threshold` to config |
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

## Architecture Review Items (New - 2026-05-27)

This section consolidates ~72 items identified during architecture review sessions (batches 1-5). Items are grouped by module and organized into waves for parallelization.

### Wave Structure

| Wave | Focus | Items | Type | Parallel Potential |
|------|-------|-------|------|-------------------|
| R0 | Documentation-Only Fixes | ~25 items | Docs | All fully parallel |
| R1 | Code Fixes (Low Risk) | ~15 items | Code/Docs | All fully parallel |
| R2 | Code Fixes (Medium Risk) | ~20 items | Code/Docs | By module group |
| R3 | Incomplete Implementation | ~12 items | Code | Some dependencies |

---

## Wave R0: Documentation-Only Fixes (~25 items)

All items in this wave are pure documentation fixes - no code changes required. These can be done in parallel by multiple agents.

### R0-DOCS-1: Count/Number Corrections

| ID | Item | Location | Fix |
|----|------|----------|-----|
| B1-1 | Fix LSP server count: "40 servers" → "39 servers" | `architecture/lsp.md:229` | Change number |
| B1-2 | Fix Session Lifecycle count: "(16 variants)" → "(19 variants)" | `architecture/protocol.md:69` | Change number |
| B1-3 | Fix compaction docs inequality: "7 or more" → "more than 6" | `architecture/compaction.md:91` | Change inequality |
| B3-1 | Fix built-in command count: "39" → "46" | `architecture/command.md` | Regenerate table |
| B3-2 | Add missing commands to table: `/stats`, `/tts`, `/pr`, `/issue`, `/checkpoint` | `architecture/command.md` | Regenerate table |
| B5-1 | Fix SSE Parser line numbers: "16-382" → "16-24" | `architecture/provider.md:526` | Update range |

### R0-DOCS-2: Stale Reference Fixes

| ID | Item | Location | Fix |
|----|------|----------|-----|
| B1-4 | Replace hook dispatch table line numbers with function names | `architecture/agent.md:621-628` | Use function names |
| B1-5 | Replace all line number references with function/class names | `architecture/bus.md` | General cleanup |
| B2-1 | Update app/mod.rs line count: "~5978" → "6003" | `architecture/tui.md` | Update count |
| B2-2 | Update worktree.md line references: `is_git_file()` line 36→172, `is_git_worktree()` line 56→180 | `architecture/worktree.md:117` | Update line refs |
| B4-1 | Fix stale line number references in server docs | `architecture/server.md` | Use more generic refs |
| B4-2 | Verify client timeout code location and update reference | `architecture/server.md:465-468` vs `src/client/attach.rs` | Verify and fix |

### R0-DOCS-3: Documentation Completeness

| ID | Item | Location | Fix |
|----|------|----------|-----|
| B1-6 | Mark "Dead tui_config code removed" section as historical (dated 2026-05-22) | `architecture/config.md:247-249` | Add historical note |
| B1-7 | Document ProviderConfig::merge() behavior (field-level merge vs full replace) | `architecture/config.md` | Add merge behavior docs |
| B1-8 | Add note: multiedit exists but NOT in default ToolRegistry | `architecture/agent.md:818` | Add clarifying note |
| B1-9 | Clarify shutdown sequence wording: "10x 100ms waits" → "up to 10 attempts with 100ms delays" | `architecture/agent.md:375-378` | Improve wording |
| B1-10 | Clarify PermissionChecker struct range at line 392 | `architecture/permission.md:392` | Clarify range |
| B1-11 | Document missing PermissionChecker methods (check_bash, check_git, check_with_args, always_allow_legacy, always_deny_legacy) | `architecture/permission.md:156-173` | Add to Key Methods |
| B1-12 | Add InprocCoreClient field names to docs: subagent_pool, memory_store, bg_scheduler, pool | `architecture/core.md:37` | Add field names |
| B1-13 | Explain why snapshot events are NOT mapped via map_app_event_to_core_event | `architecture/core.md` | Add explanation |
| B1-14 | Add line number ranges for plugin builtin/mod.rs in Project Structure | `architecture/plugin.md` | Add line ranges |
| B1-15 | Document path canonicalization security checks in Security table | `architecture/plugin.md:136-156,183-212` | Add security docs |
| B2-3 | Add `resize_debounce: Option<std::time::Instant>` to UiState docs | `architecture/tui.md` | Add field to UiState section |
| B2-4 | Update Component trait docs: `Send` → `Send + Any` | `architecture/tui.md:284` vs `src/tui/components/component.rs:84` | Update bound |
| B2-5 | Add `Stats` variant to Dialog enum documentation | `architecture/tui.md:189-196` vs `src/tui/app/types.rs:21` | Add variant |
| B2-6 | Add documentation for `pricing.rs`: ModelPricing struct, PricingService, calculate_cost() | `src/util/pricing.rs` | Document module |
| B3-3 | Remove stale historical note about "Removed orphaned src/tui/app/commands.rs" | `architecture/command.md:212` | Remove stale content |
| B3-4 | Document `/pr` and `/issue` use GitHub MCP templates | `architecture/command.md` | Add template docs |
| B3-5 | Expand Shell Session architecture (currently brief 80 lines) | `architecture/shell_session.md` | Expand documentation |
| B3-6 | Document memory eviction criteria (lowest importance when at limit) | `architecture/memory.md` | Add eviction policy |
| B3-7 | Document consolidate_session limitations with binary data | `architecture/memory.md` | Add limitations section |
| B4-3 | Document RateLimiter vs WsRateLimiter mutex implementation divergence | `architecture/server.md` | Add implementation note |
| B4-4 | Update skills path docs for platform-specific paths (macOS: ~/Library/Application Support/) | `architecture/skills.md:44-56` | Update platform docs |
| B4-5 | Document specific index names in session migration v1: session.project_idx, etc. | `architecture/session.md:192-199` | Add index names |
| B4-6 | Fix migrate() pattern description to reflect actual version-check implementation | `architecture/session.md:206-216` | Update description |
| B5-2 | Fix IPv6 unique local description: "fc00::/8 and fd00::/8" → "fc00::/7 (unique local: fc00::/8 and fd00::/8)" | `architecture/security.md:197` | Update range description |
| B5-3 | Add CANONICAL_PATHS_CACHE known issue to security.md (already in AGENTS.md) | `architecture/security.md` | Sync known issues |
| B5-4 | Clarify Question Channel immediate-answer behavior in exec docs | `architecture/exec.md:168-169` | Update docs |
| B5-5 | Consider documenting EncryptedData visibility intent (struct not pub but fields are) | `architecture/crypto.md` | Document design intent |
| B5-6 | Consider clarifying Argon2idParams last param is output key length | `architecture/crypto.md:63` | Add param explanation |
| B5-7 | Consider clarifying ProviderError::api() url field behavior | `architecture/error.md:106-108` | Add clarification |
| B5-8 | Note Encryption exclusion in McpError::is_retryable docs | `architecture/error.md:188-192` | Add note |

---

## Wave R1: Code Fixes - Low Risk (~15 items)

These are code fixes that are isolated, low-risk, and can be done in parallel.

### R1-CODE-1: Tool Module Fixes

| ID | Item | Location | Type | Fix | Status |
|----|------|----------|------|-----|--------|
| B2-7 | ~~Fix ImageTool registration~~ | ~~`src/tool/image.rs`~~ | ~~Code~~ | ~~ImageTool IS registered - REMOVE~~ | VERIFIED FALSE - already registered |
| B2-8 | Clarify tool count: ImageTool IS registered, so count is 27 not 26 | `architecture/tool.md:11,190` | Docs | Update count | VERIFIED CORRECT |

### R1-CODE-2: Server Module Fixes

| ID | Item | Location | Type | Fix | Status |
|----|------|----------|------|-----|--------|
| B4-7 | Fix WebSocket validate_ws_auth() inconsistency: returns 500 when no token configured, but HTTP allows | `src/server/ws.rs:103-106` | Code | Make consistent | NEW |

### R1-CODE-3: Session Module Fixes

| ID | Item | Location | Type | Fix | Status |
|----|------|----------|------|-----|--------|
| B4-8 | ~~Fix tool name in redact_for_export~~ | ~~`src/session/import.rs:133`~~ | ~~Code~~ | ~~Uses `terminal` correctly~~ | VERIFIED FALSE - code is correct |

### R1-CODE-4: MCP Module Fixes

| ID | Item | Location | Type | Fix | Status |
|----|------|----------|------|-----|--------|
| B3-8 | Wire up SSE event processing in agent loop (connect_sse() exists but never called) | `src/mcp/remote.rs:698-740` | Code/Docs | Wire to agent loop | UNIMPLEMENTED - dead code |
| B3-9 | run_socket() exists but not called anywhere (Unix socket server for IDE MCP) | `src/mcp/ide_server.rs:121-144` | Code | Document or wire up | UNIMPLEMENTED - dead code |
| B3-10 | McpCli Debug command is STUB only - doesn't actually test connections | `src/mcp/cli.rs:309-318` | Code | Implement properly or remove | STUB ONLY |

### R1-CODE-5: Agent/Compaction Fixes

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B1-16 | Remove auto_compact() wrapper or document why both exist | `src/agent/compaction.rs:550,594` | Code | Remove or document |
| B3-10 | Verify OV-1: codegg_zen vs codegg_go naming in overview.md | `architecture/overview.md` | Docs | Verify and fix |

### R1-CODE-6: Command Module Fixes

| ID | Item | Location | Type | Fix | Status |
|----|------|----------|------|-----|--------|
| B2-9 | ~~Investigate /stats command~~ | ~~`src/tui/command.rs:147`~~ | ~~Code~~ | ~~Dialog::Stats EXISTS, but StatsDialog may not exist~~ | NEEDS VERIFY - Dialog exists but handler may not |

### R1-CODE-7: Client Module Fixes

| ID | Item | Location | Type | Fix | Status |
|----|------|----------|------|-----|--------|
| B3-11 | Add handle_remote_event line number to docs | `src/tui/app/mod.rs:794` | Docs | Add line ref | NEW |

### R1-CODE-8: Snapshot Module Fixes

| ID | Item | Location | Type | Fix | Status |
|----|------|----------|------|-----|--------|
| B2-10 | Update storage migration version in docs: "v1-v14" → "v1-v15" | `architecture/storage.md:106` | Docs | Update version | NEW - Verified actual is v1-v15 |

---

## Wave R2: Code Fixes - Medium Risk (~20 items)

These involve actual code changes with moderate complexity. Grouped by module for parallelization.

### R2-CODE-1: Snapshot Module (Code Changes)

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B2-11 | Add atomic write pattern to restore() method (use temp file + rename like restore_to_path()) | `src/snapshot/mod.rs:292` | Code | Add atomic write |
| B2-12 | Decide on unified hash algorithm (MD5 at line 431 vs SHA256 elsewhere) | `src/snapshot/mod.rs:431,143` | Code | Decide and unify |

### R2-CODE-2: MCP Module (Incomplete Implementation)

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B3-12 | Implement or remove McpCli Debug command (currently just prints message) | `src/mcp/cli.rs:309-318` | Code | Implement or remove |
| B3-13 | OAuthManager sync methods unused: load_tokens_sync() and load_used_codes_sync() marked #[allow(dead_code)] | `src/mcp/auth.rs` | Code | Implement or remove |

### R2-CODE-3: Exec Module (Documentation Clarification)

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B5-9 | Error code count discrepancy: doc lists 25 but implementation has 26 (CLIPBOARD_ERROR, TUI_ERROR) | `architecture/exec.md:124-154` | Docs | Update count |

### R2-CODE-4: Server Module (Documentation vs Code)

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B4-9 | Clarify auth middleware docs: "allow when no token" applies to HTTP only, not WebSocket | `src/server/middleware/auth.rs:37-40` | Docs | Clarify behavior |

### R2-CODE-5: Config/Provider Module

| ID | Item | Location | Type | Fix |
|----|------|----------|------|-----|
| B5-10 | Verify ProviderConfig::api_key(prefix) method exists at schema.rs | `src/config/schema.rs` | Code | Verify and document |

---

## Wave R3: Incomplete Implementation (~12 items)

These items involve incomplete implementations that may require more design work.

### R3-IMPL-1: MCP SSE Integration (High Priority)

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B3-12 | connect_sse() exists but never called automatically - no consumer for take_sse_events() in agent loop | `src/mcp/remote.rs:698-740` | Dead code | Wire SSE events to agent loop OR document limitation |

### R3-IMPL-2: IdeServer Socket (Medium Priority)

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B3-13 | run_socket() returns Ok(()) without socket handling - Unix socket server for IDE MCP not wired up | `src/mcp/ide_server.rs:121-144` | Unused | Implement IDE integration or remove from docs |

### R3-IMPL-3: MCP Debug Command (Medium Priority)

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B3-14 | McpCli Debug command just prints arguments and help message - does NOT test connections | `src/mcp/cli.rs:309-318` | Stub | Implement actual connection test OR strip from CLI |

### R3-IMPL-4: OAuth Sync Methods (Low Priority - Previously Overlooked)

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B3-15 | load_tokens_sync() called in OAuthManager::new() at auth.rs:119 but errors silently ignored via `let _` | `src/mcp/auth.rs:119` | Silent error | Handle errors properly or remove sync method |

### R3-IMPL-5: Pricing Module Documentation (Medium Priority)

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B2-11 | pricing.rs (84 lines) completely undocumented | `src/util/pricing.rs` | Missing docs | Add comprehensive documentation |

### R3-IMPL-6: Server Route Verification (Low Priority)

| ID | Item | Location | Issue | Action |
|----|------|----------|-------|--------|
| B2-12 | Verify src/server/routes/workspace.rs and project.rs exist and use is_git_file/is_git_worktree | `architecture/worktree.md:117-118` | May be stale | Verify and update or remove references |

---

## Summary by Module

| Module | R0-Docs | R1-Code | R2-Code | R3-Impl | Total |
|--------|---------|---------|---------|---------|-------|
| Agent | 3 | 1 | - | - | 4 |
| Bus | 1 | - | - | - | 1 |
| Client | 1 | - | - | - | 1 |
| Command | 2 | - | - | - | 2 |
| Compaction | 1 | - | - | - | 1 |
| Config | 1 | - | 1 | - | 2 |
| Core | 2 | - | - | - | 2 |
| Crypto | 2 | - | - | - | 2 |
| Error | 2 | - | - | - | 2 |
| Exec | 1 | - | 1 | - | 2 |
| Hooks | 1 | - | - | - | 1 |
| IDE/LSP | - | - | - | - | 0 |
| MCP | 1 | 3 | - | 4 | 8 |
| Memory | 2 | - | - | - | 2 |
| Permission | 2 | - | - | - | 2 |
| Plugin | 2 | - | - | - | 2 |
| Provider | 1 | - | 1 | - | 2 |
| Security | 2 | - | - | - | 2 |
| Server | 4 | 1 | 1 | - | 6 |
| Session | 2 | - | - | - | 2 |
| Shell Session | 1 | - | - | - | 1 |
| Skills | 1 | - | - | - | 1 |
| Storage | 1 | - | - | - | 1 |
| Snapshot | - | - | 2 | - | 2 |
| TTS | - | - | - | - | 0 |
| Tool | 1 | - | - | - | 1 |
| TUI | 4 | 1 | - | - | 5 |
| Upgrade | - | - | - | - | 0 |
| Util | 1 | - | - | - | 1 |
| Worktree | 1 | - | - | 1 | 2 |
| **Total** | **38** | **6** | **5** | **5** | **54** |

---

## Status Summary

| Category | Status |
|----------|--------|
| Historical Completed | ✅ |
| TUI Input Repair (Completed 2026-05-01) | ✅ |
| TUI Scrolling Fix (Completed 2026-05-06) | ✅ |
| TUI Message Flow (Completed 2026-05-05) | ✅ |
| Wave 0-3 Original Plan | ✅ COMPLETE |
| Wave R0: Documentation-Only | ⏳ NEW - ~38 items |
| Wave R1: Code Fixes (Low Risk) | ⏳ NEW - ~6 items |
| Wave R2: Code Fixes (Medium Risk) | ⏳ NEW - ~5 items |
| Wave R3: Incomplete Implementation | ⏳ NEW - ~5 items |
| Wave 4: Large Refactors | ⏳ DEFERRED |
| Agent Capabilities | ✅ PARTIAL |
| Documentation | ⏳ FUTURE |

---

## Consolidated Statistics

| Metric | Value |
|--------|-------|
| Waves 0-3 Completed | ✅ All (via 25+ PRs) |
| Architecture Review Items (R0-R3) | ~54 new items |
| Documentation-Only (R0) | ~38 items |
| Code Changes Required | ~16 items (R1+R2+R3) |
| PRs Created (Waves 0-3 + Features) | 33 |
| Wave 4 (Large Refactors) | ⏳ DEFERRED |

---

## Notes for Future Agents

### Architecture Review Items Guidance

1. **R0 items are pure documentation** - no code changes, safe to do in parallel
2. **R1 items are isolated code fixes** - can be done in parallel, low risk
3. **R2 items involve actual code changes** - review carefully before merging
4. **R3 items are incomplete implementations** - may need design discussion before implementation
5. **Batches 1-5 review files are source of truth** - see:
   - `/var/folders/2j/dlwhrpps66scv9bw8f7vdfg40000gq/T/opencode/consolidated_batch1.md`
   - `/var/folders/2j/dlwhrpps66scv9bw8f7vdfg40000gq/T/opencode/consolidated_batch2.md`
   - `/var/folders/2j/dlwhrpps66scv9bw8f7vdfg40000gq/T/opencode/consolidated_batch3.md`
   - `/var/folders/2j/dlwhrpps66scv9bw8f7vdfg40000gq/T/opencode/consolidated_batch4.md`
   - `/var/folders/2j/dlwhrpps66scv9bw8f7vdfg40000gq/T/opencode/consolidated_batch5.md`

### Critical Implementation Notes (From AGENTS.md)

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

### Known Issues (Previously Documented - Confirmed Accurate)

| Issue | Location | Status |
|-------|----------|--------|
| PermissionRegistry/QuestionRegistry lack session_id in keys | `src/bus/mod.rs` | CONFIRMED |
| check_external_directory unused | `src/permission/mod.rs:1237-1248` | CONFIRMED |
| PermissionResponse unused | `src/permission/mod.rs:1141-1145` | CONFIRMED |
| ToolExecutor deprecated | `src/tool/executor.rs:8` | CONFIRMED |
| STATIC CANONICAL_PATHS_CACHE never clears | `src/security/sandbox.rs:237` | CONFIRMED - needs docs sync |

---

*(End of file)*
