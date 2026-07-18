# Config Module

The `config` module handles configuration loading, validation, and hot-reloading.

## Overview

**Location**: `crates/codegg-config/` (the `codegg-config` crate)

**Re-export**: `codegg::config` via `pub use codegg_config as config` in `src/lib.rs`

**Key Responsibilities**:
- Configuration file discovery and loading
- JSONC/JSON5 parsing with comment support
- Schema validation (produces warnings, not errors)
- Hot-reload via file watching with content hash deduplication
- Environment variable interpolation
- API key encryption/decryption

## Key Types

### Config Schema (`schema.rs`)

```rust
pub struct Config {
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    pub version: Option<String>,
    pub log_level: Option<String>,
    pub model: Option<String>,
    pub small_model: Option<String>,
    pub medium_model: Option<String>,
    pub auto_route_models: Option<bool>,
    pub default_agent: Option<String>,
    pub username: Option<String>,
    pub share: Option<String>,
    pub autoupdate: Option<AutoupdateConfig>,
    pub server: Option<ServerConfig>,
    pub provider: Option<HashMap<String, ProviderConfig>>,
    pub disabled_providers: Option<Vec<String>>,
    pub enabled_providers: Option<Vec<String>>,
    pub agent: Option<HashMap<String, AgentConfig>>,
    pub mcp: Option<HashMap<String, McpEntry>>,
    pub permission: Option<PermissionConfig>,
    pub compaction: Option<CompactionConfig>,
    pub subagent: Option<SubagentConfig>,
    pub skills: Option<SkillsConfig>,
    pub commands: Option<HashMap<String, CommandConfig>>,
    pub templates: Option<HashMap<String, SessionTemplate>>,
    pub instructions: Option<Vec<String>>,
    pub layout: Option<String>,
    pub tools: Option<HashMap<String, bool>>,
    pub formatter: Option<FormatterConfig>,
    pub lsp: Option<LspConfig>,
    pub watcher: Option<WatcherConfig>,
    pub snapshot: Option<bool>,
    pub snapshot_config: Option<SnapshotConfig>,
    pub plugin: Option<Vec<PluginSpec>>,
    pub enterprise: Option<EnterpriseConfig>,
    pub experimental: Option<ExperimentalConfig>,
    pub mode: Option<HashMap<String, ModeConfig>>,
    pub keybinds: Option<HashMap<String, String>>,
    pub vim_mode: Option<bool>,
    pub hooks: Option<Vec<HookConfigEntry>>,
    pub notifications: Option<NotificationConfig>,
    pub catalog: Option<CatalogConfig>,
}
```

### ProviderConfig

```rust
pub struct ProviderConfig {
    pub api_key: Option<String>,                    // legacy inline API key
    pub encrypted_api_key: Option<String>,          // legacy ciphertext
    pub encrypted: Option<bool>,                    // legacy "is encrypted?" flag
    pub base_url: Option<String>,
    pub enterprise_url: Option<String>,
    pub set_cache_key: Option<bool>,
    pub timeout: Option<ProviderTimeout>,
    pub chunk_timeout: Option<u64>,
    pub whitelist: Option<Vec<String>>,
    pub blacklist: Option<Vec<String>>,
    pub models: Option<HashMap<String, ModelConfig>>,
    pub options: Option<HashMap<String, serde_json::Value>>,
    /// New typed auth descriptor. When present, takes precedence over
    /// `api_key` / `encrypted_api_key` during credential resolution (see
    /// `crate::auth::resolver::AuthResolver`).
    pub auth: Option<AuthConfig>,
    /// Optional account id used to disambiguate multiple accounts in the
    /// user-level credential store.
    pub account_id: Option<String>,
}
```

`AuthConfig` is the typed auth descriptor from `src/auth/`:

```rust
pub enum AuthConfig {
    ApiKey        { env: Option<String>, value: Option<String>, encrypted_value: Option<String> },
    Stored        { account_id: Option<String> },
    ExternalCommand { command: String, args: Vec<String>, timeout_ms: Option<u64> },
    OAuthDevice   { client_id: String, scopes: Vec<String>, auth_url: String, token_url: String },
    None,
}
```

`ProviderConfig` has a `merge()` method for field-by-field merging and an
`api_key()` method that checks environment variables first. The `auth`
field and `account_id` are merged like any other optional field; if a
project config sets `auth: { type: "stored" }` it overrides the global
`api_key` path.

```rust
pub fn api_key(&self, prefix: &str) -> Option<String>
```

The method checks environment variables first (e.g., `ANTHROPIC_API_KEY`),
then `api_key` field, then encrypted `encrypted_api_key` field.

**Resolution order at registration time** (see
`register_builtin_with_config` in `src/provider/mod.rs`):

1. Explicit `auth.env` env var
2. Conventional `{PROVIDER}_API_KEY`
3. Inline `auth.value`
4. Decrypted `auth.encrypted_value` (requires `CODEGG_MASTER_KEY`)
5. User-level `CredentialStore` lookup (matched by `account_id` /
   `auth.account_id`, filtered to `CredentialKind::ApiKey`)
6. Legacy `api_key` / `encrypted_api_key` fields (backwards compat)

If `auth` is `None`, the resolver falls back to conventional env, then
legacy `api_key`, then the user store. If `auth` is `Some(AuthConfig::None)`,
all lookups are skipped (explicit "no auth" marker). The user store
lookup is filtered to `CredentialKind::ApiKey` for both the
`AuthConfig::Stored` arm and the no-auth fallback, so a stored OAuth /
bearer-token record is treated as a miss today. Future OAuth refresh
support will need a separate `kind` selector or policy module.

Provider registration has a **single resolution path** that runs
through `resolve_provider_credential(...)`. `register_config_provider`
does not read `cfg.api_key` directly anymore; the legacy field is
honored by `AuthResolver` via `ctx.legacy_api_key`.

### ProviderConfig merge() behavior

`ProviderConfig` has a `merge()` method for field-level merging. Unlike HashMap fields which use key replacement, `ProviderConfig::merge()` performs **field-by-field merging**: non-None fields from the override config replace the corresponding fields in the base config.

```rust
pub fn merge(&mut self, other: &ProviderConfig)
```

Example: If global config has `api_key` and project config has `base_url`, the merged result has both.

### merge_configs() behavior

`merge_configs()` at `src/config/paths.rs:164-284` uses different merge strategies per field type:

- **Field-by-field merging**: `provider` (via `ProviderConfig::merge()`), `server` (via `ServerConfig::merge()`), `watcher` (manual field merge)
- **Key replacement**: `agent`, `mcp`, `commands`, `mode` (insert overwrites existing keys)
- **Concatenation**: `instructions` (appended to list)
- **Simple override**: all other fields via `merge_option!` macro (schema, version, log_level, model, small_model, medium_model, auto_route_models, default_agent, username, share, autoupdate, disabled_providers, enabled_providers, permission, compaction, subagent, skills, templates, layout, tools, formatter, lsp, snapshot, snapshot_config, plugin, enterprise, experimental, keybinds, vim_mode, hooks, notifications, catalog)

## Components

### schema.rs - Config Definitions

- `Config::load()` - Loads and merges configs, decrypts keys, migrates, validates
- `Config::save()` - Encrypts keys before saving
- `Config::validate()` - Validates config values (warnings, not errors)

### paths.rs - Config Discovery

**Discovery Order** (later overrides earlier):
1. `CODEGG_TUI_CONFIG` environment variable
2. System config (`/Library/Application Support/codegg/codegg.json` on macOS, `/etc/codegg/codegg.json` on Unix)
3. Global config (`~/.config/codegg/codegg.jsonc`)
4. Project config (searches upward for `.codegg/codegg.json` or `.codegg/codegg.jsonc`)

Key functions:
- `resolve_config_paths()` - Collects all config file paths
- `load_config()` - Parses a single config file
- `parse_config()` - JSONC comment stripping + JSON5 parsing
- `merge_configs()` - Combines multiple configs (strategies vary: field-by-field for provider/server/watcher, key replacement for agents/mcp/commands/mode, concatenation for instructions)
- `interpolate_env_vars()` - Expands `${VAR_NAME}` syntax

### watcher.rs - Hot Reload

```rust
pub struct ConfigWatcher {
    watcher: Option<RecommendedWatcher>,
    rx: mpsc::Receiver<()>,
    tx: mpsc::Sender<()>,
    watched_paths: Vec<PathBuf>,
    started: bool,
    debounce_duration: Duration,
    last_hash: Option<u64>,
    ignore_patterns: Vec<String>,
}
```

Key methods:
- `new()` - Creates watcher with default 500ms debounce
- `with_config(&WatcherConfig)` - Configure debounce and ignore patterns
- `start()` - Start watching config file directories (non-recursive)
- `recv()` - Async receiver with content hash deduplication
- `reload_now()` - Force immediate reload

Uses `notify` crate for file system watching with content hash deduplication to avoid spurious reloads.

### encryption.rs - Config Encryption

```rust
pub fn encrypt_provider_keys(config: &mut Config) -> Result<(), AppError>;
pub fn decrypt_provider_keys(config: &mut Config) -> Result<(), AppError>;
pub fn get_master_key() -> Option<String>;
```

Master key lookup order:
1. `CODEGG_MASTER_KEY`
2. `CODEGG_ENCRYPTION_KEY`
3. `OPENCODE_ENCRYPTION_KEY`

## Loading Flow

```
Config::load()
1. resolve_config_paths() → collect config file paths
2. load_config() → parse each file (JSONC → JSON5)
3. merge_configs() → later files override earlier (strategies vary: field-by-field for provider/server/watcher, key replacement for agent/mcp/commands/mode, concatenation for instructions)
4. decrypt_provider_keys() → decrypt API keys if encrypted
5. migrate() → apply version migrations
6. validate() → validate config values (warnings, not errors)
```

## Validation

Validation failures produce **warnings** not errors - the app starts with a partially invalid config.

Validated fields:
- `log_level`: must be `debug|info|warn|error|trace`
- `share`: must be `manual|auto|disabled`
- `model`, `small_model`, `medium_model`: must be in `provider/model` format
- `port`: must be >= 1024
- Agent `mode`: must be `subagent|primary|all`
- Agent `color`: must be hex color or theme color name
- MCP server types: `local` requires `command`, `remote` requires `url`
- `tool_timeout_seconds`: must be 1-3600 (0 = invalid, >3600 = invalid)
- `max_parallel_tools`: must be 1-100 (0 = invalid, >100 = invalid)
- `compaction.threshold`: must be 0.1-1.0 (threshold ratio for context compaction)
- `compaction.max_tokens`: must be at least 1000
- `deterministic_tools.backend`: must be `native` or `disabled`
- `deterministic_tools.profile`: must be `codegg_core`, `codegg_core_min`, `default`, or `full` (unknown profiles emit warning and fall back to `codegg_core`)
- `deterministic_tools.model_audience`: must be `model` or `harness`
- `deterministic_tools.harness_audience`: must be `harness` or `model`
- `deterministic_tools.max_output_chars`: must be > 0 and <= 1,000,000
- `preflight.mode`: enum validated at deserialization (`off`, `observe`, `warn`, `block_on_definite`)

## Configuration Example (JSONC)

```jsonc
{
  // Model configuration
  "model": "anthropic/claude-sonnet-4-20250514",
  "small_model": "anthropic/claude-sonnet-4-20250514",
  "medium_model": "anthropic/claude-opus-4-20250514",
  "auto_route_models": true,

  "server": {
    "port": 18789
  },

  "provider": {
    "anthropic": {
      "api_key": "${ANTHROPIC_API_KEY}",
      "encrypted": false
    }
  },

  "permission": {
    "default": "Ask"
  },

  "watcher": {
    "debounce_duration_ms": 500,
    "ignore": ["node_modules", ".git"]
  },

  "experimental": {
    "memory_auto_consolidate": true
  }
}
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CODEGG_TUI_CONFIG` | Custom config path |
| `CODEGG_MASTER_KEY` | Master key for encryption |
| `{PROVIDER}_API_KEY` | Provider API key fallback (e.g., `ANTHROPIC_API_KEY`) |

## Known Issues Fixed

### Encrypted keys not decrypting on hot-reload
**Bug**: API keys work after `save()` but fail on hot-reload (file watcher triggers reload).
**Fix**: `ConfigWatcher::reload_config()` now calls `decrypt_provider_keys()`.

### Encrypted keys not decrypting on load
**Bug**: API keys work after `save()` but fail on subsequent loads.
**Fix**: `decrypt_provider_keys()` is called automatically in `Config::load()`.

### Provider config fields lost during merge
**Bug**: Provider settings from global config disappear when project config specifies the same provider.
**Fix**: `ProviderConfig::merge()` method implemented for field-level merging.

### medium_model not validated
**Bug**: Invalid `medium_model` values not caught by validation.
**Fix**: `medium_model` validation added.

### Dead tui_config code removed (historical note - 2026-05-22)
**Historical**: This section documents a cleanup that was completed on 2026-05-22.

**Bug**: `find_tui_config()` and `load_tui_config()` were exported but never used anywhere in the codebase.
**Fix**: Removed from `paths.rs` and `mod.rs` to clean up dead code.

## Search Backend Config

The `[search]` section selects the backend for the native
`websearch` and `webfetch` tools. The default backend is the
external `eggsearch` MCP server, and the in-tree
`SearchProviderRegistry` is retained only as a legacy
compatibility fallback.

### Minimal config

```toml
# No [search] section is required if eggsearch is installed on PATH.
# Codegg defaults to spawning: eggsearch mcp stdio
```

When `eggsearch` is on `PATH`, Codegg will:

- resolve the search backend to `eggsearch` (the default),
- spawn `eggsearch mcp stdio` as an MCP subprocess,
- route `websearch` and `webfetch` through it, and
- hide the raw `mcp__eggsearch__*` tools from the model.

The `[search]` block only needs to be present to override a
default, point at a custom binary/args, forward provider API
keys, or change the fallback / cap behavior.

### Full schema

```toml
[search]
backend = "eggsearch"           # "eggsearch" | "builtin" | "disabled"
expose_raw_mcp_tools = false    # default false; set true to expose mcp__eggsearch__*
fallback_to_builtin = false     # default false
max_search_output_chars = 12000 # cap on websearch output
max_fetch_output_chars = 20000  # cap on webfetch output
max_repo_output_chars = 15000   # fallback for repo_* caps below
max_repo_search_output_chars = 15000   # optional, falls back to max_repo_output_chars
max_repo_fetch_output_chars = 15000    # optional, falls back to max_repo_output_chars
max_repo_map_output_chars = 15000      # optional, falls back to max_repo_output_chars
max_security_output_chars = 10000
max_research_output_chars = 15000
max_batch_output_chars = 50000
max_evidence_output_chars = 100000

[search.eggsearch]
enabled = true                  # if false, behaves as backend = "disabled"
server_name = "eggsearch"       # MCP server name; default "eggsearch"
command = "eggsearch"           # binary to spawn
args = ["mcp", "stdio"]         # default args
timeout_ms = 60000              # default call timeout for all tools
repo_timeout_ms = 60000         # optional per-domain overrides
security_timeout_ms = 60000
research_timeout_ms = 60000
batch_fetch_timeout_ms = 60000
provider_status_timeout_ms = 15000  # health check timeout (shorter)

[search.eggsearch.env]
# Optional provider keys passed only to the eggsearch subprocess.
BRAVE_SEARCH_API_KEY = "$BRAVE_SEARCH_API_KEY"
```

When `backend = "eggsearch"` the agent loop connects the
eggsearch MCP server at startup (`bootstrap::bootstrap_search_backend`)
and the native `websearch`/`webfetch` tools call
`mcp__<server>__web_search` / `mcp__<server>__web_fetch`
internally. Setting `backend = "builtin"` forces the legacy
in-tree `SearchProviderRegistry` path. Setting
`backend = "disabled"` makes both tools return a clear disabled
error.

`fallback_to_builtin` defaults to `false`. When `true`, a failed
eggsearch call falls through to the legacy implementation; the
built-in fetch path is not considered the preferred security
boundary (it exists for compatibility, not for defense in depth),
so leave this off in production unless you have a specific reason
to fall back.

### Adding new search providers

New providers belong in eggsearch, not in Codegg's built-in
registry (`src/search/`). The built-in registry is legacy fallback
only and should not grow. Eggsearch owns the provider list, the
fetching path, and any provider-specific extraction logic.

### Durable provider-connection compatibility

The existing `provider.<id>` configuration and environment-variable
registration path remains authoritative for legacy callers. Durable provider
connections are additive daemon-owned metadata: they reference an existing
encrypted credential-store account by an opaque secret reference and do not
copy inline or resolved credentials into SQLite. Configuration loading does
not automatically import legacy providers when endpoint or account mapping
is ambiguous; future connection workflows must make that selection explicit.

See
[`search_backend.md`](search_backend.md) and
[`architecture/search_backend.md`](search_backend.md)
for the full schema and dispatch details.

## See Also

- [crypto.md](crypto.md) - AES-256-GCM encryption details
- [search_backend.md](search_backend.md) - search/fetch backend dispatch
- [lsp.md](lsp.md#phase-12-semantic-memory-cache) - LSP semantic cache config (`[lsp_semantic_cache]`); disk cache deferred (Phase 16)
- [agent.md](agent.md) - Uses config

### Project discovery configuration

`Config::discovery` is disabled and empty by default. When enabled, each root
must provide an explicit local path plus a stable id or name.
`DiscoveryRootConfig` supports `git`, `directory`, and `mixed` modes, hidden-file
policy, no-follow symlink policy, ignore names/patterns, directory markers,
direct-child-only mode, and finite depth/entry/candidate/time/concurrency
bounds. Validation rejects control/NUL text, oversized values, invalid bounds,
duplicate ids or names, and lexically overlapping roots. Reload changes only
future scans; it does not remove catalog records or prior successful
generations.
