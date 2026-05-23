# Config Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| Config struct has all documented fields (version, log_level, model, etc.) | VERIFIED | schema.rs:22-64 matches exactly |
| ProviderConfig struct has all documented fields | VERIFIED | schema.rs:135-148 matches exactly |
| ProviderConfig has `merge()` method for field-by-field merging | VERIFIED | schema.rs:175-212 |
| ProviderConfig has `api_key()` method checking env vars first | VERIFIED | schema.rs:151-173 |
| Config::load() loads, merges, decrypts, migrates, validates | VERIFIED | schema.rs:496-519 |
| Config::save() encrypts keys before saving | VERIFIED | schema.rs:521-551 |
| Config::validate() validates config values (warnings, not errors) | VERIFIED | schema.rs:553-738 - returns Vec<String> which is logged as warnings |
| resolve_config_paths() collects config file paths | VERIFIED | paths.rs:12-39 |
| load_config() parses a single config file | VERIFIED | paths.rs:91-96 |
| parse_config() JSONC comment stripping + JSON5 parsing | VERIFIED | paths.rs:98-101 |
| merge_configs() combines multiple configs | VERIFIED | paths.rs:164-267 |
| interpolate_env_vars() expands ${VAR_NAME} syntax | VERIFIED | paths.rs:269-294 |
| ConfigWatcher struct has documented fields | VERIFIED | watcher.rs:12-21 matches exactly |
| ConfigWatcher::new() creates watcher with default 500ms debounce | VERIFIED | watcher.rs:24-36 |
| ConfigWatcher::with_config(&WatcherConfig) configure debounce/ignore | VERIFIED | watcher.rs:38-46 |
| ConfigWatcher::start() watches config file directories (non-recursive) | VERIFIED | watcher.rs:48-90 |
| ConfigWatcher::recv() async receiver with content hash deduplication | VERIFIED | watcher.rs:92-111 |
| ConfigWatcher::reload_now() force immediate reload | VERIFIED | watcher.rs:113-115 |
| encrypt_provider_keys/decrypt_provider_keys/get_master_key exist | VERIFIED | encryption.rs:6,13,45 |
| Master key lookup order: CODEGG_MASTER_KEY → CODEGG_ENCRYPTION_KEY → OPENCODE_ENCRYPTION_KEY | VERIFIED | encryption.rs:6-11 |
| Loading flow matches documented order | VERIFIED | schema.rs:496-519 |
| log_level validation: debug\|info\|warn\|error\|trace | VERIFIED | schema.rs:557-563 |
| share validation: manual\|auto\|disabled | VERIFIED | schema.rs:566-574 |
| model/small_model/medium_model validation: provider/model format | VERIFIED | schema.rs:576-601 |
| port validation: must be >= 1024 | VERIFIED | schema.rs:661-665 |
| Agent mode validation: subagent\|primary\|all | VERIFIED | schema.rs:703-710 |
| Agent color validation: hex color or theme color name | VERIFIED | schema.rs:712-730 |
| MCP server types: local requires command, remote requires url | VERIFIED | schema.rs:622-650 |
| Encrypted keys decrypt on hot-reload | VERIFIED | watcher.rs:153-154 |
| Encrypted keys decrypt on load | VERIFIED | schema.rs:508-509 |
| Provider config fields preserved during merge | VERIFIED | schema.rs:175-212, paths.rs:205-217 |
| medium_model validated | VERIFIED | schema.rs:594-601 |
| Dead tui_config code removed (find_tui_config, load_tui_config) | VERIFIED | mod.rs:12-15 exports correct functions |

## Bugs Found

### Critical

1. **ConfigWatcher reload does not call migrate() or validate()**
   - **Location**: watcher.rs:136-157
   - **Description**: `reload_config()` only merges configs and decrypts keys, but does NOT call `migrate()` or `validate()`. This means:
     - Version migrations are not applied on hot-reload
     - Validation is skipped on hot-reload
   - **Impact**: Configuration loaded via hot-reload may be stale/invalid
   - **Fix**: Add `config.migrate()` and `if let Err(errors) = config.validate() { ... }` to `reload_config()`

2. **ConfigWatcher::reload_now() ignores path argument**
   - **Location**: watcher.rs:113-115
   - **Description**: `reload_now(&self)` ignores `self` entirely and calls `Self::reload_config()` which internally re-collects paths. This is correct behavior but the signature implies it might accept a path argument.
   - **Impact**: Minor confusion in API design, not a functional bug.

### High

3. **Watcher event may fire before file write completes**
   - **Location**: watcher.rs:92-111
   - **Description**: After receiving a file change notification, the code sleeps for debounce_duration then reads the file. If the file write is slow or large, the read might happen before the write completes, potentially reading stale/empty content.
   - **Impact**: Config reload might read incomplete config file
   - **Fix**: Consider reading the file with retry logic or waiting for stable hash (read twice with delay, compare)

4. **Encryption failure leaves config partially modified**
   - **Location**: encryption.rs:45-93
   - **Description**: `encrypt_provider_keys()` modifies `provider.api_key = None` after encrypting. If migration fails for some providers but not others, the config is left in inconsistent state (some api_keys cleared, some still plaintext).
   - **Impact**: Data loss if encryption partially fails
   - **Fix**: Clone config before encryption, rollback on failure

### Medium

5. **strip_jsonc_comments fails on nested block comments**
   - **Location**: paths.rs:103-162
   - **Description**: The manual comment stripping doesn't handle nested block comments (e.g., `/* outer /* nested */ still outer */`). It also doesn't properly handle edge cases like `/"` inside strings or escaped slashes.
   - **Impact**: Malformed JSONC with certain comment patterns could cause parse failures
   - **Fix**: Consider using a proper JSONC parser library instead of manual stripping

6. **merge_configs does not recursively merge nested structs**
   - **Location**: paths.rs:164-267
   - **Description**: For HashMap fields like `provider`, `agent`, `mcp`, `commands`, `mode` - it does merge correctly. But for `ServerConfig`, `WatcherConfig`, etc., it uses simple clone_from which means later config completely replaces earlier. For example, if global config has `server.port=18789` and project config has `server.hostname="localhost"`, only `hostname` is kept (port lost).
   - **Impact**: Users expect field-by-field merging for nested structs but get whole-struct replacement
   - **Fix**: Implement recursive merge for nested structs like ServerConfig

7. **Config::load() silently returns default config if all paths fail**
   - **Location**: schema.rs:496-500
   - **Description**: If `resolve_config_paths()` returns empty, `Config::default()` is returned with no warning. This makes debugging missing configs difficult.
   - **Impact**: Silent fallback to defaults makes troubleshooting config loading issues hard
   - **Fix**: Log a warning when falling back to default config

8. **No file locking for concurrent config writes**
   - **Location**: schema.rs:521-551
   - **Description**: `Config::save()` writes directly to filesystem with no file locking. If multiple processes write simultaneously, data corruption could occur.
   - **Impact**: Potential data loss/corruption with concurrent access
   - **Fix**: Use file locking (e.g., `flock` via `std::fs::OpenOptions` with custom extension, or `deadpool` sqlite approach for atomic writes)

## Improvement Suggestions

### Performance

1. **ConfigWatcher computes hash of all config files on every notification**
   - **Location**: watcher.rs:121-134
   - **Description**: `compute_config_hash()` reads ALL config files even if only one changed. For large or many config files, this is wasteful.
   - **Suggestion**: Track which file triggered the event and only hash that file plus any with same modification timestamp.

2. **strip_jsonc_comments allocates new String for every input**
   - **Location**: paths.rs:103-162
   - **Description**: Manual character-by-character processing could be replaced with a regex-based approach or at least use `reserve()` more strategically.
   - **Suggestion**: Consider using `jsonc-parser` crate for proper JSONC parsing.

### Correctness

3. **Environment variable interpolation doesn't support defaults**
   - **Location**: paths.rs:269-294
   - **Description**: `${VAR_NAME:-default}` syntax (common in shell) is not supported. Missing vars become empty string.
   - **Suggestion**: Implement `${VAR_NAME:-default}` pattern for fallback values.

4. **Case-sensitive provider key lookups**
   - **Location**: schema.rs:151-153
   - **Description**: `prefix.to_uppercase().replace('-', "_")` means `anthropic` becomes `ANTHROPIC_API_KEY`. But the actual env var is `ANTHROPIC_API_KEY` (correct). However, if user has `Anthropic` or `ANTHROPIC` in config, behavior may be inconsistent.
   - **Suggestion**: Document that provider names should be lowercase in config.

### Maintainability

5. **ConfigWatcher reload doesn't preserve last_hash across reloads**
   - **Location**: watcher.rs:104-106
   - **Description**: `last_hash` is updated only after successful reload. If reload fails, next notification will re-trigger reload even if file hasn't changed. This is actually correct behavior (retry on failure), but makes testing harder.
   - **Suggestion**: Add a test that verifies this retry-on-failure behavior.

6. **No schema documentation in Config struct fields**
   - **Location**: schema.rs:22-64
   **Description**: Config fields lack doc comments explaining their purpose, valid values, and examples.
   - **Suggestion**: Add doc comments to all Config fields explaining their schema semantics.

7. **Magic numbers scattered in validation**
   - **Location**: schema.rs:661-682
   - **Description**: Magic numbers like `1024` (privileged port), `3600` (1 hour), `100` (max parallel tools) appear without explanation.
   - **Suggestion**: Define constants with descriptive names (e.g., `MIN_PRIVILEGED_PORT`, `MAX_TOOL_TIMEOUT_SECONDS`).

## Priority Actions (top 5 items to fix)

1. **[High] Fix ConfigWatcher::reload_config() missing migrate() and validate() calls** - watcher.rs:136-157
   - Hot-reloaded configs don't get validated, potentially loading invalid state

2. **[Critical] Fix encryption failure leaving config partially modified** - encryption.rs:45-93
   - Partial encryption failures cause data loss (api_key cleared but not encrypted)

3. **[Medium] Implement recursive merge for nested structs** - paths.rs:164-267
   - ServerConfig and other nested structs get whole-value replaced instead of field-merged

4. **[Medium] Add warning when falling back to default config** - schema.rs:496-500
   - Silent fallback makes troubleshooting config loading issues difficult

5. **[Low] Add doc comments to Config struct fields** - schema.rs:22-64
   - Improve maintainability by documenting field semantics and valid values