# Consolidated Architecture Review

**Reviewed**: 2026-05-28
**Batches**: 0-8 (35 architecture documents)

## Executive Summary

This review covers 35 architecture documentation files across 9 batches, verified against the actual codebase. The documentation is generally well-maintained, with most struct definitions, enum variants, and algorithm descriptions matching source code. However, a systematic pattern of stale line-number references (especially in provider.md and permission.md, many off by 14-26 lines), several incorrect counts and field descriptions, and some misleading behavioral documentation were found. The most critical issues include: tool count is 28 not 27, database has 13 tables not 7, exec.md describes wrong method names and wrong behavior, and multiple code blocks in tool.md and agent.md are outdated. Cross-module issues include inconsistent TTL values (300s vs 310s) and misleading config merge behavior documentation.

## Cross-Module Issues

| Issue | Modules Affected | Severity |
|-------|-----------------|----------|
| Tool count discrepancy: doc says 27, actual is 28 (tool_search not counted) | overview.md, agent.md, tool.md | HIGH |
| TTL value inconsistency: docs say 300s, code uses 310s | permission.md, bus.md | MEDIUM |
| Line number drift across provider.md (15+ references off by 14+ lines) | provider.md | MEDIUM |
| Line number drift in permission.md (8+ references off by 13-26 lines) | permission.md | MEDIUM |
| Config merge behavior documentation is incomplete/incorrect | config.md | MEDIUM |
| Provider auto-registration description misleading | overview.md, provider.md | MEDIUM |

## Documentation Fixes (by severity)

### HIGH

| # | File | Line(s) | Issue | Action |
|---|------|---------|-------|--------|
| 1 | tool.md | 153-187 | `with_defaults()` code block is outdated: missing ImageTool (line 102) and LspTool (lines 113-115), shows BatchTool which is NOT registered | UPDATE code block to match actual `ToolRegistry::with_defaults()` |
| 2 | tool.md | 356-374 | ToolExecutor section references `src/tool/executor.rs` which does not exist | REMOVE section entirely |
| 3 | agent.md | 44-59 | AgentLoop struct listing missing 9 actual fields: `config`, `question_tx`, `question_rx`, `session_id`, `mcp_service`, `tool_def_cache`, `file_change_rx`, `usage_store`, `pricing_service` | UPDATE struct listing |
| 4 | exec.md | 169 | Doc says `setup_question_channel()` is called and exec mode returns "[question not supported]" immediately. Actual code calls `setup_question_channel_for_exec()` which DOES set `question_rx`, so exec mode waits 300s. The "[question not supported]" string is in the else branch (non-exec path) | UPDATE entire question behavior description |
| 5 | exec.md | 169 | Doc does not mention `setup_question_channel_for_exec()` exists. `setup_question_channel()` (non-exec version) is defined but never called (dead code) | ADD `setup_question_channel_for_exec()`, mark non-exec version as dead code |
| 6 | overview.md | 130-146 | Database tables listed as 7 but actual schema has 13 (missing: migration_version, project, session_share, cached_models, task, checkpoints) | UPDATE table list to all 13 tables |
| 7 | config.md | 86-100 | `ProviderConfig merge()` signature shown as returning `ProviderConfig` - actual returns `()` (mutates `&mut self`) | UPDATE signature |
| 8 | tui.md | 301-305 | DialogType enum missing `Stats` variant (actual has 23 variants, doc shows 22) | UPDATE enum listing |
| 9 | tui.md | 286-295 | Component trait missing 5 focus-related methods: `focus_next`, `focus_prev`, `focusable_count`, `focused_index`, `set_focused` | UPDATE trait documentation |

### MEDIUM

| # | File | Line(s) | Issue | Action |
|---|------|---------|-------|--------|
| 10 | overview.md | 106-107 | Provider: "Auto-registered: codegg_zen only" - `codegg_go` is also auto-registered via `register_builtin_with_config()` | UPDATE to include codegg_go |
| 11 | provider.md | 205 | "Registers only `codegg_go` as auto-registered via `register_builtin()`" - misleading: `codegg_go` uses `register_env_fallback_provider`, not `register_builtin()` | UPDATE description |
| 12 | config.md | 172 | "merge_configs() → later files override earlier (HashMaps merge field-by-field)" - Only `provider` HashMap uses field-by-field merging; `agents`, `mcp`, `commands`, `modes` use full key replacement | UPDATE merge behavior matrix |
| 13 | overview.md | 64 | Feature gate: doc says `plugin`, code has `plugins` (plural) | UPDATE feature name |
| 14 | bus.md | 126, 334, 338, 345 | TTL documented as 300 seconds but code uses 310 seconds | UPDATE all TTL references |
| 15 | permission.md | 401 | PermissionRegistry cleanup TTL stated as 300s. Actual: `Duration::from_secs(310)` | UPDATE value |
| 16 | mcp.md | 95-99 | `RemoteClient.session_id` shown as `Mutex<Option<String>>` but actual is `Arc<Mutex<Option<String>>>`. Same issue for `request_id`: `Arc<AtomicU64>` not `AtomicU64` | UPDATE field types |
| 17 | mcp.md | 24-28 | `McpClientType` shown with `#[derive(Debug, Clone, Serialize, Deserialize)]` but actual only has `#[derive(Clone)]` | UPDATE derive attributes |
| 18 | mcp.md | 138 | `max_retries: 5` with "Max 5 retry attempts" - code checks `retry_count >= self.max_retries` yielding 4 actual attempts, not 5 | UPDATE retry description |
| 19 | permission.md | 489-491 | References `PermissionResponse` at lines 1141-1145 - this type does not exist in codebase; lines contain `merge_rulesets()` | UPDATE reference |
| 20 | security.md | 109-122 | `SandboxConfig` struct documented without `mode` field. Actual has `mode: SandboxMode` | ADD mode field and SandboxMode enum |
| 21 | security.md | 202 | `CANONICAL_PATHS_CACHE` stated as "never clears" - cache has 300s TTL and 100-entry cap with eviction | UPDATE stale claim |
| 22 | command.md | 51 | Claims "39 hardcoded commands" - actual count is 42 `Command::new()` calls | UPDATE count |
| 23 | command.md | 114 | "Built-in Commands (46 total)" - conflates command entries (42) with aliases (46 total) | UPDATE to clarify |
| 24 | session.md | 524 | `redact_for_export` lists "tail" as redacted tool name - code uses "terminal" | UPDATE tool name |
| 25 | storage.md | 38-45 | `init()` code example shows calling `Database::new()` then `migrate()` - actual `init()` calls `connect_and_configure()` directly | UPDATE code example |
| 26 | storage.md | 119 | v15 described as "Additional fields" - v15 creates the `usage` table | UPDATE description |
| 27 | tui.md | 389 | Keyboard shortcuts: `↑/j` for NavigateUp but `j` is NavigateDown in vim mode (j/k swapped) | FIX keybinding |
| 28 | util.md | entire file | Missing documentation for `pricing.rs` (ModelPricing, PricingService, calculate_cost) | ADD pricing section |
| 29 | util.md | entire file | Missing documentation for `interner.rs` (StringInterner, tool_interner) | ADD interner section |
| 30 | crypto.md | 49 | Doc says "EncryptedData is not `pub`" but code shows `pub struct EncryptedData` | UPDATE visibility claim |
| 31 | lsp.md | 74-77 | `DiagnosticEntry` shown under `diagnostics.rs` but actually defined in `client.rs:33` | UPDATE file reference |

### LOW

| # | File | Line(s) | Issue | Action |
|---|------|---------|-------|--------|
| 32 | overview.md | 118 | `agent/mod.rs:147-262` off by 9 lines (should be 147-271) | UPDATE line reference |
| 33 | overview.md | 48-80 | Missing `exec` and `error` from module table | ADD to module table |
| 34 | overview.md | 120-127 | Missing `arboard` and `debug-logging` features from feature gates table | ADD to feature table |
| 35 | protocol.md | 158-160 | `TurnFailed.turn_id`, `ToolStarted.turn_id`, `ToolCompleted.turn_id` shown as required but code has `Option<String>` | UPDATE field optionality |
| 36 | protocol.md | 217-219 | TuiMessage "Special (2)" section counts EventEnvelope as special but it's already listed under Server-to-Client Events | UPDATE special count |
| 37 | command.md | 163 | `/issue` aliases shown as `bugs`, `features` without slashes - code stores as `"/bugs"`, `"/features"` | UPDATE alias format |
| 38 | core.md | 37 | `pool` field type: doc says "all wrapped in `Option<Arc<T>>`" but pool is `Option<SqlitePool>` (not Arc-wrapped) | UPDATE type description |
| 39 | core.md | 137 | `map_app_event_to_core_event` location off by ~5-8 lines | UPDATE line reference |
| 40 | agent.md | 674-678 | Instruction files listed as `.codegg/instructions.md`, `INSTRUCTIONS.md`, `~/.config/codegg/instructions.md` - primary sources are `AGENTS.md`, `CLAUDE.md`, `CONTEXT.md` via `INSTRUCTION_FILES` constant | UPDATE instruction file listing |
| 41 | tui.md | 313 | Typo: `pubruct FocusManager` should be `pub struct FocusManager` | FIX typo |
| 42 | tui.md | 313 | FocusManager struct missing `focus_index: usize` field | ADD field |
| 43 | tui.md | 31 | `mod.rs` described as "6003 lines" but actual is 5995 lines | UPDATE line count |
| 44 | resilience.md | 76-106 | State diagram appears duplicated (two identical diagrams in sequence) | REMOVE duplicate |
| 45 | permission.md | 270 | DoomLoopDetector check in agent/loop.rs off by ~13 lines | UPDATE line reference |
| 46 | permission.md | 355 | Registration-before-publish off by 11 lines | UPDATE line reference |
| 47 | permission.md | 236-263 | DoomLoopDetector algorithm described as checking "the most recent tool" but code uses `tool_name:hash(arguments)` - detection is per-tool+args, not per-tool | UPDATE algorithm description |
| 48 | provider.md | 9-526 | 15+ line number references all off by 14+ lines (Provider trait, ChatRequest, Message, ContentPart, ChatEvent, ModelInfo, ModelVariant, TokenUsage, ToolDefinition, ProviderRegistry, register_builtin, register_builtin_with_config, CircuitBreaker, ModelCatalog, ModelDiscoveryService, SseParser, ProviderCache) | UPDATE all line references |
| 49 | resilience.md | 76-78 | `call()` method location off (actual is 101-137, doc says 114-127) | UPDATE line reference |
| 50 | mcp.md | 138 | `max_retries: 5` yields 4 actual attempts due to `>=` check | UPDATE to clarify |
| 51 | permission.md | 231 | DoomLoopDetector location off by ~20 lines | UPDATE line reference |
| 52 | permission.md | 250-263 | `is_doom_loop()` location off by 16 lines | UPDATE line reference |

## Code Issues (by severity)

### HIGH

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | agent/loop.rs | `setup_question_channel()` (non-exec) is defined but never called anywhere - dead code | `src/agent/loop.rs:784-786` | High |

### MEDIUM

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 2 | provider | `register_builtin_with_config()` calls `register_builtin()` only when registry is empty. Adding ANY provider to config disables ALL env-var providers. Undocumented behavior. | `src/provider/mod.rs:534-536` | Medium |
| 3 | config | Config merge behavior: only `provider` HashMap merges field-by-field; `agents`, `mcp`, `commands`, `modes` use full key replacement. Documentation misleads users. | `src/config/schema.rs` | Medium |
| 4 | permission | 310s TTL in PermissionRegistry (bus/mod.rs:59) vs 300s timeout in agent loop (loop.rs:494) creates a 10s window where cleanup could remove a pending permission before timeout fires | `src/bus/mod.rs:59` + `src/agent/loop.rs:494` | Medium |

### LOW

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 5 | server/ws.rs | `handle_tui` function has stale `#[allow(dead_code)]` attribute despite being actively used | `src/server/ws.rs:357` | Low |
| 6 | provider | `ProviderCache::clear()` only removes expired entries, not all entries. Misleading method name. | `src/provider/cache.rs:75-82` | Low |
| 7 | resilience | `max_half_open_duration` hardcoded to 30s in constructor despite being a field in `CircuitBreakerInner` - not configurable | `src/resilience/circuit.rs:66` | Low |
| 8 | mcp | `OAuthManager::load_tokens_sync()` errors silently ignored in `new()` | `src/mcp/auth.rs:119` | Low |
| 9 | mcp | `connect_sse()` at `remote.rs:699` - SSE connection method exists but is never called | `src/mcp/remote.rs:699` | Info |
| 10 | mcp | `run_socket()` at `ide_server.rs:121` - Unix socket server exists but is never wired up | `src/mcp/ide_server.rs:121` | Info |
| 11 | permission | `check_external_directory()` is `#[allow(dead_code)]` - unused utility | `src/permission/mod.rs:1264` | Low |
| 12 | lsp | `DiagnosticEntry` (client.rs:33) vs `FileDiagnostic` (diagnostics.rs:20) - different types with similar names, potential confusion | `src/lsp/client.rs:33` | Low |

## Improvement Opportunities (by impact)

### High Impact

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | exec | Document the actual exec mode question behavior: `setup_question_channel_for_exec()` sets up the channel, meaning questions wait up to 300s then timeout. Consider adding a short timeout (e.g., 5s) for exec mode. | Reduces CI/CD pipeline waste from 300s waits on questions |
| 2 | config | Document the full merge behavior matrix: which fields use field-by-field merging vs. key replacement vs. append (instructions) | Prevents user confusion about config behavior |
| 3 | provider | Document interaction between `register_builtin_with_config()` and `register_builtin()` fallback - adding ANY provider to config disables all env-var-only providers | Prevents user confusion about missing providers |
| 4 | tool | Sync `with_defaults()` code block with actual register() calls to prevent confusion about built-in tools | Prevents contributor confusion |
| 5 | agent | Document the `INSTRUCTION_FILES` constant (`AGENTS.md`, `CLAUDE.md`, `CONTEXT.md`) - primary instruction loading mechanism not documented anywhere | Security/reproducibility |

### Medium Impact

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 6 | permission | Align 310s PermissionRegistry TTL with 300s agent loop timeout, or document the intentional gap | Prevents subtle timing bugs |
| 7 | provider | Rename `ProviderCache::clear()` to `purge_expired()` to accurately describe behavior | API clarity |
| 8 | resilience | Make `max_half_open_duration` a constructor parameter instead of hardcoded | Configurability |
| 9 | mcp | Document `McpPrompt`, `McpResource`, `McpResourceContent` structs and `McpService` methods (`list_prompts`, `get_prompt`, `list_resources`, `read_resource`) | Complete documentation coverage |
| 10 | session | Document `create_from_template`, `list_all_with_offset`, `list_deleted`, `set_tags` methods | Completes SessionStore API documentation |
| 11 | util | Document `pricing.rs` with `ModelPricing` struct and `PricingService::calculate_cost` formula | Completes util module documentation |
| 12 | util | Document `interner.rs` StringInterner for deduplicating Arc<str> allocations | Documents memory optimization |
| 13 | plugin | Document the `api.rs` module which defines a complete parallel API type system for plugin authors | Complete documentation coverage |
| 14 | plugin | Document the `event_bus.rs` `PluginEventBus` circular buffer behavior and `max_log_size` limits | Operational clarity |
| 15 | tui | Document `focus_index` field and Tab-focus cycling behavior in FocusManager | Helps developers understand focus management |

### Low Impact

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 16 | protocol | Add `#[serde(skip_serializing_if = "Option::is_none")]` to optional fields in CoreEvent for cleaner JSON | Cleaner wire format |
| 17 | storage | Document two code paths: `Database::new()` (runs migrations) vs `init()` (does NOT run migrations) | Prevents confusion |
| 18 | snapshot | Document that `diff_files` always returns single-element Vec and `capture_incremental()` returns `Ok(None)` on no changes | API contract clarity |
| 19 | worktree | Document `find_git_root` behavior when starting path is already a git root | Edge case clarity |
| 20 | tts | Document TUI integration points (toggle via /tts or /voice, auto-stop on AgentFinished) separately from TTS module | Separates module scope from TUI integration |
| 21 | memory | Add note that `consolidate_session`'s 20-memory limit is per-run, not a hard namespace cap | Clarifies consolidation behavior |
| 22 | crypto | Consider making `EncryptedData` `pub(crate)` if internal-only, or document its public API clearly | Clarifies API surface |
| 23 | skills | Document that `.skills/` directory is repo-level documentation, not runtime-loaded | Clarifies the two skill locations |
| 24 | util | Add note about Histogram 1000-entry bound as a design decision | Prevents confusion about memory usage |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | tool.md | ToolExecutor section (lines 356-374) referencing `src/tool/executor.rs` | File does not exist - deleted |
| 2 | tool.md | `with_defaults()` code block (lines 153-187) | Outdated - missing ImageTool, LspTool; shows BatchTool which isn't registered |
| 3 | tool.md | Note at line 190 "ImageTool is NOT in with_defaults()" | Wrong - ImageTool IS in with_defaults() |
| 4 | agent.md | Lines 674-678 instruction file listing | Stale - primary sources are AGENTS.md, CLAUDE.md, CONTEXT.md via INSTRUCTION_FILES |
| 5 | command.md | "39 hardcoded commands" (line 51) | Wrong count - should be 42 |
| 6 | command.md | "Built-in Commands (46 total)" (line 114) | Misleading - conflates entries (42) with aliases |
| 7 | resilience.md | Duplicate state diagram (lines 46-73 and 79-106 are identical) | Accidental duplication |
| 8 | storage.md | Code example showing `init()` calling `session::schema::migrate()` | Misleading - init() never calls migrate |
| 9 | bus.md | "TTL: 300 seconds (5 minutes)" repeated in multiple places | Should be 310 seconds to match code |
| 10 | crypto.md | "The `EncryptedData` struct is not `pub`" | Incorrect - struct is `pub` |

## Per-Batch Summary

| Batch | Files | Issues Found | Key Findings |
|-------|-------|--------------|--------------|
| 0 - Overview | 1 (overview.md) | 8 doc + 3 new | Tool count 27→28, DB tables 7→13, orphaned git/ module, missing exec/error from module table |
| 1 - Protocol/Agent | 4 (protocol.md, agent.md, compaction.md, command.md) | 12 doc + 4 code | TurnFailed turn_id optionality wrong, AgentLoop missing 9 fields, instruction files wrong, command count 39→42 |
| 2 - Core/Server | 4 (core.md, server.md, client.md, exec.md) | 6 doc + 2 code | exec.md wrong method name and wrong behavior, pool type incorrect, handle_tui stale annotation |
| 3 - Provider/Resilience | 4 (provider.md, resilience.md, error.md, config.md) | 29 doc + 4 code | 15+ line references off by 14+, ProviderConfig merge signature wrong, config merge behavior misleading |
| 4 - Permission/Security | 2 (permission.md, security.md) | 19 doc + 3 code | TTL 300→310, PermissionResponse phantom type, DoomLoop key semantics wrong, CANONICAL_PATHS_CACHE stale |
| 5 - Session/Storage | 5 (session.md, storage.md, snapshot.md, git.md, worktree.md) | 5 doc + 1 code | redact_for_export "tail"→"terminal", storage.md init() wrong, v15 description wrong |
| 6 - MCP/LSP/Plugin | 4 (mcp.md, lsp.md, plugin.md, hooks.md) | 6 doc + 4 code | RemoteClient field types wrong, McpClientType derive wrong, DiagnosticEntry misplacement, retry count wrong |
| 7 - TUI/Tool/Skills | 3 (tui.md, tool.md, skills.md) | 12 doc + 3 code | FocusManager typo, missing Component methods, DialogType Stats variant missing, tool code block stale, ToolExecutor deleted file |
| 8 - Bus/Memory/Misc | 8 (bus.md, memory.md, shell_session.md, tts.md, upgrade.md, util.md, crypto.md, ide.md) | 8 doc + 2 code | TTL 300→310, util.md missing pricing/interner, crypto.md EncryptedData visibility wrong |

## Verified Counts (Correct in Docs)

| Item | Count | Verified Against |
|------|-------|------------------|
| LSP servers | 39 | `src/lsp/server.rs:27-384` |
| UiState fields | 26 | `src/tui/app/state/ui.rs:27-76` |
| AppEvent variants | 36 | `src/bus/events.rs:5-150` |
| Built-in commands | 46 (entries + aliases) | `src/tui/command.rs:83-182` |
| Built-in agents | 7 | `src/agent/mod.rs:147-271` |
| Migrations | 15 (v1-v15) | `src/session/schema.rs:25-69` |
| GlobalEventBus buffer | 2048 | `src/bus/global.rs:13` |
| Protocol version | 1 | `src/protocol/core.rs:3` |
| CoreRequest variants | 35 | `src/protocol/core.rs:50-175` |
| CoreResponse variants | 7 | `src/protocol/core.rs:24-46` |
| CoreEvent variants | 17 | `src/protocol/core.rs:179-272` |
| TuiMessage variants | 16 | `src/protocol/tui.rs:3-75` |
| AppError variants | 17 | `src/error.rs` |
| ProviderError variants | 8 | `src/error.rs` |
| ToolError variants | 8 | `src/error.rs` |
| McpError variants | 6 | `src/error.rs` |
| LspError variants | 9 | `src/error.rs` |
| PluginError variants | 5 | `src/error.rs` |
| ServerRuntimeError variants | 5 | `src/error.rs` |
| ClientError variants | 5 | `src/error.rs` |
| Config fields | 34 | `src/config/schema.rs:22-64` |
| ProviderConfig fields | 12 | `src/config/schema.rs:167-180` |
| ConfigWatcher debounce | 500ms | `src/config/schema.rs:32` |
| WASM max size | 10MB | `src/plugin/loader.rs:9` |
| WASM fuel per hook | 1,000,000 | `src/plugin/loader.rs:11` |
| Max plugin fuel budget | 10,000,000 | `src/plugin/loader.rs:15` |
| WASM hook timeout | 30s | `src/plugin/loader.rs:13` |
| Outer hook timeout | 5s | `src/plugin/service.rs:18` |
| HookType variants | 13 | `src/plugin/hooks.rs:4-20` |
| HookEvent variants | 6 | `src/hooks/mod.rs:17-24` |
| McpServerStatus variants | 4 | `src/mcp/mod.rs:61-68` |
| Permission types | 16 | `src/permission/mod.rs:70-87` |
| DoomLoop window cap | 1000 | `src/permission/mod.rs:1190` |
| DoomLoop threshold cap | 100 | `src/permission/mod.rs:1191` |
| SessionStatus variants | 5 | `src/session/status.rs:5-12` |
| PartData variants | 5 | `src/session/message.rs:38-59` |
| ToolStatus variants | 4 | `src/session/message.rs:63-69` |
| CheckpointStore methods | 7 | `src/session/checkpoint.rs:53-147` |
| WAL pragmas | 8 | `src/storage/mod.rs:66-76` |
| Max DB connections | 10 | `src/storage/mod.rs:60` |
| Import limits | 100K msgs, 500K parts, 500MB | `src/session/import.rs:68-70` |
| Snapshot excluded dirs | 4 (.git, node_modules, target, .codegg) | `src/snapshot/mod.rs:389` |
| Default retryable status codes | [429, 500, 502, 503, 504] | `src/provider/fallback.rs:17` |
| Exponential backoff | 2^i capped at 30s | `src/provider/fallback.rs:107` |
| Circuit breaker defaults | failure=3, timeout=60, success=2 | `src/provider/fallback.rs:23` |
| AgentLoopState fields | 6 | `src/agent/loop.rs:534-541` |
| ExecutionLimits | 100 turns, 1M tokens, 600s | `src/agent/loop.rs:549-557` |
| SubAgentPool defaults | max_concurrent=5, max_depth=3 | `src/agent/worker.rs:85-94` |
| AgentMode variants | 3 | `src/agent/mod.rs:46-53` |
| Prompt files count | 8 | `src/agent/prompts/` |
| DoomLoopDetector threshold | 20 | `src/agent/loop.rs:664-671` |
| ToolDefCache tuple | (Option<String>, bool, bool, usize, u64, Vec\<ToolDefinition\>) | `src/agent/loop.rs:60-67` |
| drop_middle_messages keep_each_side | 2 | `src/agent/compaction.rs:460` |
| truncate_tool_outputs threshold | 500 chars | `src/agent/compaction.rs:306` |
| prune_tool_outputs max_tokens | 10,000 | `src/agent/compaction.rs:494` |
| AES-256-GCM nonce | 12 bytes | `src/crypto/mod.rs:8` |
| Argon2id params | m=19456, t=2, p=1, output=32 | `src/crypto/mod.rs:35` |
| SSRF IPv4 ranges | 10/8, 172.16/12, 192.168/16, etc. | `src/security/ssrf.rs:6-17` |
| SSRF fc00::/7 coverage | fc00::/8 + fd00::/8 | `src/security/ssrf.rs:25` |
| DEBOUNCE_MS (LSP) | 150 | `src/lsp/diagnostics.rs:15` |
| TUI_EVENT_BUFFER_MAX | 1024 | `src/server/ws.rs:26` |
| WsRateLimiter | max_requests=100, window=60s | `src/server/http.rs:208` |
| Health check timeout | 10s | `src/client/sdk.rs:40` |
| WebSocket timeout | 30s, 3 retries, backoff 1s/2s/4s | `src/client/attach.rs:39-43` |
| Default terminal size | 80x24 | `src/shell_session/session.rs:29-30` |
| Memory consolidation threshold | >= 8.0 | `src/memory/mod.rs:246` |
| PermissionRegistry sync | fn (not async fn) | `src/bus/mod.rs` |
| QuestionRegistry sync | fn (not async fn) | `src/bus/mod.rs` |
| GitSession fields | 5 | `src/git/mod.rs:7-13` |
| GitStatus fields | 4 | `src/git/mod.rs:16-21` |
| Worktree fields | 4 (path, branch, is_current, is_detached) | `src/worktree/mod.rs:8-13` |
