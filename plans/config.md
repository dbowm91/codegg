# Config Architecture Review

## Architecture Document
- Path: architecture/config.md

## Source Code Location
- src/config/

## Verification Summary
**Pass** - The architecture document is largely accurate with only minor discrepancies.

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| Config struct fields (lines 22-62) | Pass | All 45 fields match exactly including `$schema`, `instructions`, `mode`, `keybinds`, `hooks`, `notifications`, `catalog` |
| ProviderConfig fields (lines 67-81) | Pass | All 11 fields match exactly |
| ProviderConfig.merge() method | Pass | Field-by-field merge at schema.rs:207-244 |
| ProviderConfig.api_key() method | Pass | Checks env vars first at schema.rs:183-205 |
| Config.load() behavior | Pass | Lines 528-553 match doc description |
| Config.save() encrypts keys | Pass | schema.rs:555-585 calls encrypt_provider_keys() |
| Config.validate() produces warnings | Pass | Returns Result<Vec<String>> which caller logs as warnings |
| resolve_config_paths() discovery order | Pass | paths.rs:12-39 follows CODEGG_TUI_CONFIG → system → global → project |
| Project config search: `.codegg/codegg.json` | Pass | paths.rs:49-51 checks both .codegg and codegg dirs |
| parse_config() uses JSONC + JSON5 | Pass | paths.rs:98-101 strips comments then parses with json5 |
| merge_configs() HashMap behavior | Pass | providers/agents/mcp/commands/modes use full replace (not field-by-field), instructions concat |
| interpolate_env_vars() with ${VAR} syntax | Pass | paths.rs:286-311 implements this |
| ConfigWatcher struct fields | Pass | watcher.rs:12-21 matches exactly |
| ConfigWatcher::new() default 500ms debounce | Pass | watcher.rs:32 |
| ConfigWatcher::with_config() | Pass | watcher.rs:38-46 configures debounce and ignore |
| ConfigWatcher::start() non-recursive | Pass | watcher.rs:82 uses RecursiveMode::NonRecursive |
| ConfigWatcher::recv() hash deduplication | Pass | watcher.rs:93-115 uses double-hash technique |
| encrypt_provider_keys() | Pass | encryption.rs:44-107 |
| decrypt_provider_keys() | Pass | encryption.rs:12-42 |
| Master key lookup order | Pass | encryption.rs:5-10: CODEGG_MASTER_KEY → CODEGG_ENCRYPTION_KEY → OPENCODE_ENCRYPTION_KEY |
| decrypt_provider_keys() in Config::load() | Pass | schema.rs:542 called during load |
| decrypt_provider_keys() in ConfigWatcher::reload_config() | Pass | watcher.rs:157 called on hot-reload |
| ProviderConfig::merge() field-by-field | Pass | schema.rs:207-244 implements proper field merge |
| medium_model validation added | Pass | schema.rs:628-635 checks `provider/model` format |
| Dead tui_config code removed | Pass | mod.rs only re-exports resolve_config_paths, find_project_config, global_config_path, interpolate_env_vars, load_config, merge_configs, parse_config |
| log_level validation | Pass | schema.rs:590-598 validates debug|info|warn|error|trace |
| share validation | Pass | schema.rs:600-608 validates manual|auto|disabled |
| model format validation | Pass | schema.rs:610-617 validates provider/model |
| small_model format validation | Pass | schema.rs:619-626 validates provider/model |
| medium_model format validation | Pass | schema.rs:628-635 validates provider/model |
| port >= 1024 validation | Pass | schema.rs:696-699 |
| agent mode validation | Pass | schema.rs:737-745 validates subagent|primary|all |
| agent color validation | Pass | schema.rs:746-763 validates hex or theme colors |
| MCP server type validation | Pass | schema.rs:656-685 validates local requires command, remote requires url |

## Issues Found

### Inconsistencies

1. **WatcherConfig field naming**: The architecture doc shows `ignore_patterns` (line 120) but the actual struct uses `ignore` (schema.rs:468). This is a minor documentation inconsistency - the doc mentions "ignore patterns" in text but references the wrong field name in the struct listing.

2. **Additional validations not documented**: The architecture only mentions 6 validation items (lines 163-169) but the actual implementation validates:
   - `compaction.threshold` must be 0.1-1.0 (schema.rs:720-727)
   - `compaction.max_tokens` must be >= 1000 (schema.rs:728-733)
   - `server.tool_timeout_seconds` cannot be 0 or > 3600 (schema.rs:701-708)
   - `server.max_parallel_tools` cannot be 0 or > 100 (schema.rs:709-716)
   - Empty variant names in provider models (schema.rs:642-648)
   - Empty command templates (schema.rs:688-693)

3. **Encrypted keys documentation mentions "2026-05-21" but code shows "2026-05-22"**: The "Known Issues Fixed" section at lines 219 and 223 reference older dates. The actual fixes were applied on 2026-05-22 per AGENTS.md records.

### Missing Documentation

1. **ExperimentalConfig fields not documented**: The architecture shows `experimental` in Config but does not document its fields. Actual fields (schema.rs:485-494):
   - `disable_paste_summary: Option<bool>`
   - `batch_tool: Option<bool>`
   - `lsp_tool: Option<bool>`
   - `open_telemetry: Option<bool>`
   - `primary_tools: Option<Vec<String>>`
   - `continue_loop_on_deny: Option<bool>`
   - `mcp_timeout: Option<u64>`
   - `memory_auto_consolidate: Option<bool>`

2. **ServerConfig additional fields**: Not documented in architecture:
   - `hostname: Option<String>`
   - `token: Option<String>`
   - `mdns: Option<bool>`
   - `mdns_domain: Option<String>`
   - `cors: Option<Vec<String>>`
   - `cors_origins: Option<Vec<String>>`
   - `tool_timeout_seconds: Option<u64>`
   - `max_parallel_tools: Option<usize>`

3. **WatcherConfig struct is documented with wrong fields**: Architecture shows `watched_paths`, `started` as fields but these are internal state, not config fields. The actual WatcherConfig (schema.rs:467-471) only has:
   - `ignore: Option<Vec<String>>`
   - `debounce_duration_ms: Option<u64>`

4. **CommandConfig fields not documented**: schema.rs:396-402 has `template`, `description`, `agent`, `model`, `subtask` (deprecated).

5. **SessionTemplate fields not documented**: schema.rs:406-413 has `name`, `description`, `agent`, `model`, `instructions`, `tags`.

6. **Migration system not documented**: Config::migrate() at schema.rs:775-792 exists but not mentioned.

7. **Config::save() path selection**: Not documented that save() prefers project config over global config (schema.rs:556-560).

## Recommendations

1. **Update WatcherConfig documentation**: Remove `watched_paths`, `started` from struct display since these are internal fields. The WatcherConfig struct in the architecture should show only `ignore` and `debounce_duration_ms`.

2. **Expand Validation section**: Document all validation rules including compaction thresholds, timeout ranges, parallel tool limits, and empty value checks.

3. **Document ExperimentalConfig fields**: Add a subsection listing all experimental flags.

4. **Document additional ServerConfig fields**: The server section should include all fields like hostname, token, mdns, cors, timeouts.

5. **Add migration documentation**: Note that Config handles version migrations via migrate() method.

6. **Fix date references in "Known Issues Fixed"**: Update dates to 2026-05-22 to match actual fix dates.
