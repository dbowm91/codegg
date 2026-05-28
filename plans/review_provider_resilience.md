# Review: Batch 3 - Provider, Resilience, and Error

**Reviewed**: 2026-05-28
**Files**: architecture/provider.md, architecture/resilience.md, architecture/error.md, architecture/config.md

## Summary

This review verifies architecture documentation against the actual codebase for provider, resilience, error, and config modules. Most claims are accurate, but several line number references are stale, the ProviderConfig merge signature is misrepresented, and the auto-registration logic description is misleading. The config merge behavior documentation contradicts actual code behavior for agents/mcp/commands/modes HashMap fields.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | provider.md | 9 | `Provider Trait (src/provider/mod.rs:60-73)` - Actual trait is at lines 74-87. The 60-73 range includes `assistant_text_content_value()` and `openai_tool_arguments_value()` helper functions. | UPDATE line reference to `74-87` |
| 2 | provider.md | 37 | `ChatRequest (src/provider/mod.rs:97-109)` - Actual struct is at lines 112-123. | UPDATE line reference to `112-123` |
| 3 | provider.md | 54 | `Message Types (src/provider/mod.rs:111-128)` - Actual enum is at lines 125-142. | UPDATE line reference to `125-142` |
| 4 | provider.md | 67 | `ContentPart (src/provider/mod.rs:130-135)` - Actual enum is at lines 144-149. | UPDATE line reference to `144-149` |
| 5 | provider.md | 78 | `ChatEvent (src/provider/mod.rs:142-156)` - Actual enum is at lines 156-170. | UPDATE line reference to `156-170` |
| 6 | provider.md | 99 | `ModelInfo (src/provider/mod.rs:222-232)` - Actual struct is at lines 236-246. | UPDATE line reference to `236-246` |
| 7 | provider.md | 114 | `ModelVariant (src/provider/mod.rs:212-220)` - Actual struct is at lines 226-234. | UPDATE line reference to `226-234` |
| 8 | provider.md | 126 | `TokenUsage (src/provider/mod.rs:175-182)` - Actual struct is at lines 189-196. | UPDATE line reference to `189-196` |
| 9 | provider.md | 138 | `ToolDefinition (src/provider/mod.rs:184-210)` - Actual struct is at lines 198-203 (methods at 205-223). | UPDATE line reference to `198-223` |
| 10 | provider.md | 158 | `ProviderRegistry (src/provider/mod.rs:234-263)` - Actual struct is at lines 248-277. | UPDATE line reference to `248-277` |
| 11 | provider.md | 177 | `register_builtin() (src/provider/mod.rs:265-312)` - Actual function is at lines 279-326. | UPDATE line reference to `279-326` |
| 12 | provider.md | 199 | `register_builtin_with_config() (src/provider/mod.rs:376-523)` - Actual function is at lines 390-537. | UPDATE line reference to `390-537` |
| 13 | provider.md | 205 | "Registers only `codegg_go` as auto-registered via `register_builtin()`" - **Misleading**: `codegg_go` is NOT in `register_builtin()` at all. `register_builtin()` registers 15 providers via env vars. `codegg_go` is registered via `register_env_fallback_provider()`. The fallback call at line 534-536 calls `register_builtin()` only when registry is empty. | UPDATE to clarify: `register_builtin()` is a fallback that registers all 15 env-var providers; `codegg_go` uses `register_env_fallback_provider` like other env-based providers |
| 14 | provider.md | 434 | `Circuit Breaker (src/resilience/circuit.rs:44-186)` - Actual `CircuitBreaker` struct is at lines 43-46. The file is 261 lines total (not 186). | UPDATE line reference to `43-46` and note file is 261 lines |
| 15 | provider.md | 462 | `ModelCatalog (src/provider/catalog.rs:5-109)` - Actual struct is at lines 5-10. File is 109 lines total. | UPDATE to `5-10` |
| 16 | provider.md | 490 | `ModelDiscoveryService (src/provider/discovery.rs:9-265)` - Actual struct is at lines 9-16. File is 317 lines. | UPDATE line reference to `9-16` and note file is 317 lines |
| 17 | provider.md | 526 | `SseParser (src/provider/sse_parser.rs:16-382)` - Actual struct is at lines 19-27. File is 1025 lines. | UPDATE line reference to `19-27` and note file is 1025 lines |
| 18 | provider.md | 508 | `ProviderCache (src/provider/cache.rs:15-83)` - Actual struct is at lines 15-17. File is 83 lines. | UPDATE to `15-17` |
| 19 | provider.md | 607 | `HTTP Client Configuration (src/provider/mod.rs:46-56)` - Actual function is at lines 46-56. **CONFIRMED** correct. | No action |
| 20 | provider.md | 411 | `FallbackProvider (src/provider/fallback.rs:8-31)` - Actual struct is at lines 8-12, impl starts at line 14. | UPDATE to `8-31` (struct+new method) |
| 21 | config.md | 86-100 | `ProviderConfig merge()` signature shown as `pub fn merge(&self, other: &ProviderConfig) -> ProviderConfig` - Actual is `pub fn merge(&mut self, other: &ProviderConfig)` (returns `()`, mutates self). | UPDATE signature to `pub fn merge(&mut self, other: &ProviderConfig)` |
| 22 | config.md | 172 | "merge_configs() → later files override earlier (HashMaps merge field-by-field)" - **Misleading**: Only `provider` HashMap uses field-by-field merging (via `ProviderConfig::merge()`). `agents`, `mcp`, `commands`, `modes` HashMaps use **full key replacement** (insert overwrites). | UPDATE to clarify: only `provider` HashMap merges field-by-field; agents/mcp/commands/modes use key-level replacement |
| 23 | config.md | 22 | Config struct fields listed in doc but `config_version` field is missing from the doc's struct listing. Actual code has `version: Option<String>` (line 27). **CONFIRMED** correct. | No action |
| 24 | resilience.md | 76-78 | "call() method (circuit.rs:114-127) checks for HalfOpen timeout" - Actual `call()` is at lines 101-137. The HalfOpen timeout check is at lines 114-127 within `call()`. | UPDATE line reference to `101-137` |
| 25 | resilience.md | 76-106 | State diagram appears duplicated (two identical diagrams in sequence). | REMOVE duplicate diagram |
| 26 | error.md | 7 | `Location: src/error.rs` - **CONFIRMED** correct (file exists, single file not module directory). | No action |
| 27 | error.md | 188 | `McpError::is_retryable()` listed `McpError::OAuth(_)` as retryable - Code confirms this is correct. **CONFIRMED**. | No action |
| 28 | error.md | 209 | `PluginError`: doc lists `NotFound, LoadFailed, HookFailed, InstallFailed, InvalidManifest` - Code confirms all 5 variants. **CONFIRMED**. | No action |
| 29 | error.md | 251 | `ServerRuntimeError IntoResponse (src/error.rs:475-501)` - Actual impl is at lines 475-501. **CONFIRMED**. | No action |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | provider | `register_builtin_with_config()` calls `register_builtin()` only when registry is empty (line 534-536). This means if config registers ANY provider (e.g., just anthropic), ALL env-var providers (openai, google, etc.) are skipped. This may be intentional but is undocumented behavior. | `src/provider/mod.rs:534-536` | Medium |
| 2 | provider | `ProviderCache::clear()` only removes expired entries, not all entries. Misleading method name - should be `purge_expired()` or similar. | `src/provider/cache.rs:75-82` | Low |
| 3 | resilience | `max_half_open_duration` is hardcoded to 30s in `CircuitBreaker::new()` (line 66) and is NOT configurable via constructor parameters, despite being a field in `CircuitBreakerInner`. The doc lists it as a "configuration option" which is misleading. | `src/resilience/circuit.rs:66` | Low |
| 4 | config | `ProviderConfig::merge()` performs field-level merging (non-None from override replaces base), but the doc's example says "If global config has `api_key` and project config has `base_url`, the merged result has both" - this is correct, but the signature shown in the doc is wrong (shows return value, actual is `&mut self`). | `src/config/schema.rs:207` | Medium |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | provider | Document the interaction between `register_builtin_with_config()` and `register_builtin()` fallback behavior. Users need to understand that adding ANY provider to config disables all env-var-only providers. | Prevents user confusion about missing providers |
| 2 | provider | `ProviderCache::clear()` should be renamed to `purge_expired()` or `remove_expired()` to accurately describe its behavior (it doesn't clear all entries). | API clarity |
| 3 | resilience | Make `max_half_open_duration` a constructor parameter instead of hardcoded, since it's already a field in `CircuitBreakerInner`. | Configurability |
| 4 | resilience | Consider adding a `CircuitBreakerConfig` struct to bundle the 4 parameters (failure_threshold, timeout_secs, success_threshold, max_half_open_duration) for cleaner API. | API ergonomics |
| 5 | config | Document the full merge behavior matrix: which fields use field-by-field merging vs. key replacement vs. append (instructions). The current doc is ambiguous. | Documentation accuracy |
| 6 | error | `ProviderError::is_retryable()` includes `Auth(_)` as retryable. Auth failures are typically NOT retryable (wrong API key won't self-heal). Consider removing from retryable set. | Correctness question |
| 7 | config | Add validation for `snapshot_config.max_files`, `max_file_bytes`, `max_total_bytes` bounds (currently only defaults exist, no validation). | Robustness |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | provider.md | Line number references for Provider trait, ChatRequest, Message, ContentPart, ChatEvent, ModelInfo, ModelVariant, TokenUsage, ToolDefinition, ProviderRegistry, register_builtin, register_builtin_with_config, CircuitBreaker, ModelCatalog, ModelDiscoveryService, SseParser, ProviderCache | All off by 14+ lines; code has shifted since documentation was written |
| 2 | resilience.md | Duplicate state diagram (lines 46-73 and 79-106 are identical) | Accidental duplication |
| 3 | config.md | `ProviderConfig merge()` signature showing return type `ProviderConfig` | Method returns `()`, not `ProviderConfig` |

## Verified Correct Claims

| # | File | Claim | Status |
|---|------|-------|--------|
| 1 | provider.md | Provider trait has `id()`, `name()`, `clone_box()`, `stream()`, `models()`, `discover_models()`, `ping()` | CONFIRMED |
| 2 | provider.md | Default retryable status codes: `[429, 500, 502, 503, 504]` | CONFIRMED at fallback.rs:17 |
| 3 | provider.md | Exponential backoff formula `2^i` capped at 30s | CONFIRMED at fallback.rs:107 |
| 4 | provider.md | Circuit breaker default params: failure_threshold=3, timeout_secs=60, success_threshold=2 | CONFIRMED at fallback.rs:23 |
| 5 | provider.md | `codegg_go` base URL is `https://opencode.ai/go/v1` | CONFIRMED at additional.rs:177 |
| 6 | provider.md | MiniMax has embedded model definitions (6 models) | CONFIRMED at additional.rs:85-146 |
| 7 | provider.md | Embedded models: big-pickle, minimax-m2.5-free, nemotron-3-super-free, qwen3.6-plus-free | CONFIRMED at models.rs:3-46 |
| 8 | resilience.md | CircuitState enum has Closed, Open, HalfOpen | CONFIRMED at circuit.rs:8-13 |
| 9 | resilience.md | `is_available()` uses write lock from start to avoid TOCTOU | CONFIRMED at circuit.rs:79-99 |
| 10 | resilience.md | HalfOpen→Open timeout: 30s default via max_half_open_duration | CONFIRMED at circuit.rs:66,116 |
| 11 | error.md | AppError has 16 variants (Config, Storage, Provider, Agent, Tool, Permission, Mcp, Plugin, Lsp, Io, Json, Http, Other, Worktree, Upgrade, Clipboard, Tui) | CONFIRMED - actually 17 variants (I counted Tui) |
| 12 | error.md | ProviderError has 8 variants | CONFIRMED |
| 13 | error.md | ToolError has 8 variants | CONFIRMED |
| 14 | error.md | McpError has 6 variants (Connection, Server, ToolCall, OAuth, Encryption, Timeout) | CONFIRMED |
| 15 | error.md | LspError has 9 variants | CONFIRMED |
| 16 | error.md | PluginError has 5 variants | CONFIRMED |
| 17 | error.md | ServerRuntimeError has 5 variants | CONFIRMED |
| 18 | error.md | ClientError has 5 variants | CONFIRMED |
| 19 | config.md | Config has 34 top-level fields | CONFIRMED - matches schema.rs:22-64 |
| 20 | config.md | ProviderConfig has 12 fields | CONFIRMED at schema.rs:167-180 |
| 21 | config.md | ConfigWatcher has default 500ms debounce | CONFIRMED at watcher.rs:32 |
| 22 | config.md | Master key lookup order: CODEGG_MASTER_KEY → CODEGG_ENCRYPTION_KEY → OPENCODE_ENCRYPTION_KEY | CONFIRMED at encryption.rs:5-9 |
| 23 | config.md | Validation produces warnings not errors | CONFIRMED at schema.rs:550-553 |
| 24 | config.md | Config discovery order: env var → system → global → project | CONFIRMED at paths.rs:12-38 |
