# Config Module

The `config` module handles configuration loading, validation, and hot-reloading.

## Overview

**Location**: `src/config/`

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
    pub api_key: Option<String>,
    pub encrypted_api_key: Option<String>,
    pub encrypted: Option<bool>,
    pub base_url: Option<String>,
    pub enterprise_url: Option<String>,
    pub set_cache_key: Option<bool>,
    pub timeout: Option<ProviderTimeout>,
    pub chunk_timeout: Option<u64>,
    pub whitelist: Option<Vec<String>>,
    pub blacklist: Option<Vec<String>>,
    pub models: Option<HashMap<String, ModelConfig>>,
    pub options: Option<HashMap<String, serde_json::Value>>,
}
```

ProviderConfig has a `merge()` method for field-by-field merging and an `api_key()` method that checks environment variables first.

ServerConfig has a `merge()` method at `schema.rs:134-162` that performs field-by-field merging, copying non-None fields from other config.

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
- `merge_configs()` - Combines multiple configs (HashMap fields use full replace for agents/mcp/commands/modes)
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
3. merge_configs() → later files override earlier (HashMaps merge field-by-field)
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
**Fix**: `ConfigWatcher::reload_config()` now calls `decrypt_provider_keys()` at `watcher.rs:157-158`.

### Encrypted keys not decrypting on load
**Bug**: API keys work after `save()` but fail on subsequent loads.
**Fix**: `decrypt_provider_keys()` is called automatically in `Config::load()` at `schema.rs:542`.

### Provider config fields lost during merge
**Bug**: Provider settings from global config disappear when project config specifies the same provider.
**Fix**: `ProviderConfig::merge()` method implemented for field-level merging at `schema.rs:175-212`.

### medium_model not validated
**Bug**: Invalid `medium_model` values not caught by validation.
**Fix**: `medium_model` validation added at `schema.rs:594-601`.

### Dead tui_config code removed
**Bug**: `find_tui_config()` and `load_tui_config()` were exported but never used anywhere in the codebase.
**Fix**: Removed from `paths.rs` and `mod.rs` to clean up dead code (2026-05-22).

## See Also

- [crypto.md](crypto.md) - AES-256-GCM encryption details
- [agent.md](agent.md) - Uses config