---
name: config
description: Configuration loading, validation, file watching, and encryption in opencode-rs
version: 1.3.0
tags:
  - config
  - loading
  - validation
  - encryption
  - watching
---

# Config Module Guide

This skill covers the config module in `src/config/` which handles configuration loading, validation, file watching, and API key encryption.

## Module Structure

```
src/config/
├── mod.rs          # Module exports and public API
├── schema.rs       # Config struct, validation, load(), save()
├── paths.rs        # File discovery, JSONC parsing, env interpolation, merge
├── watcher.rs      # ConfigWatcher for hot-reload
└── encryption.rs   # API key encryption/decryption
```

## Config Struct (`schema.rs`)

Central configuration type with ~45 optional fields:

```rust
pub struct Config {
    pub version: Option<String>,
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
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

## Loading Flow (`Config::load()`)

```
1. resolve_config_paths() → collect config file paths
2. load_config() → parse each file (JSONC → JSON5)
3. merge_configs() → later files override earlier (HashMaps merge field-by-field)
4. decrypt_provider_keys() → decrypt API keys if encrypted
5. migrate() → apply version migrations
6. validate() → validate config values (warnings, not errors)
```

### Config File Discovery (`paths.rs`)

Resolution order (later overrides earlier):
1. `CODEGG_TUI_CONFIG` environment variable
2. System config (`/Library/Application Support/codegg/codegg.json` on macOS, `/etc/codegg/codegg.json` on Unix)
3. Global config (`~/.config/codegg/codegg.jsonc`)
4. Project config (searches upward for `.codegg/codegg.json` or `.codegg/codegg.jsonc`)

## Key Types

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

ProviderConfig has a `merge()` method for field-by-field merging when multiple configs define the same provider. The `api_key()` method checks environment variables first, then falls back to decrypted or plain api_key.

### WatcherConfig

```rust
pub struct WatcherConfig {
    pub ignore: Option<Vec<String>>,
    pub debounce_duration_ms: Option<u64>,  // default: 500ms
}
```

## Environment Variable Interpolation

Syntax: `${VAR_NAME}` in config values.

```json
{
  "provider": {
    "anthropic": {
      "api_key": "${ANTHROPIC_API_KEY}"
    }
  }
}
```

## JSONC Support

Config files support JSON with comments (JSONC):
- Line comments: `// comment`
- Block comments: `/* comment */`

The `strip_jsonc_comments()` function removes these before parsing.

## API Key Encryption (`encryption.rs`)

### Master Key Lookup

Checked in order:
1. `CODEGG_MASTER_KEY`
2. `CODEGG_ENCRYPTION_KEY`
3. `OPENCODE_ENCRYPTION_KEY`

### Decryption (on load)

`decrypt_provider_keys()` is called automatically in `Config::load()` to decrypt `encrypted_api_key` fields.

### Encryption (on save)

`encrypt_provider_keys()` encrypts plain API keys and migrates legacy v1 ciphertext to v2 format.

### Crypto Version Prefix

`FORMAT_V2_PREFIX: &str = "v2:"` - ciphertexts with this prefix are v2 format.

## Validation (`Config::validate()`)

Validation failures produce **warnings** not errors - the app starts with a partially invalid config.

Validated fields:
- `log_level`: must be `debug|info|warn|error|trace`
- `share`: must be `manual|auto|disabled`
- `model`, `small_model`, `medium_model`: must be in `provider/model` format
- `port`: must be >= 1024
- Agent `mode`: must be `subagent|primary|all`
- Agent `color`: must be hex color or theme color name
- MCP server types: `local` requires `command`, `remote` requires `url`

## ConfigWatcher (`watcher.rs`)

Hot-reload watcher using `notify` crate:

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

Note: `reload_config()` (called by both `recv()` and `reload_now()`) decrypts provider keys, so hot-reloaded configs work correctly with encrypted API keys.

### Content Hash Deduplication

The watcher uses content hashing to avoid spurious reloads:
1. File change notification received
2. Wait for debounce duration
3. Drain any additional notifications during debounce
4. Compute hash of all config file contents
5. Only reload if hash actually changed

## Merge Semantics

### Simple Fields
Later config files override earlier ones via `merge_option!` macro.

### HashMap Fields (providers, agents, mcp, commands, modes)
Full replace merge - later config files completely replace earlier ones for the same key.

### Instructions
Instructions are concatenated, not replaced.

## Common Issues

### Encrypted keys not decrypting on hot-reload

**Symptom**: API keys work after `save()` but fail on hot-reload (file watcher triggers reload).

**Cause**: `ConfigWatcher::reload_config()` was not calling `decrypt_provider_keys()`.

**Fix**: Added `decrypt_provider_keys()` call at `watcher.rs:153-154`.

### Encrypted keys not decrypting on load

**Symptom**: API keys work after `save()` but fail on subsequent loads.

**Cause**: `decrypt_provider_keys()` was not being called in `Config::load()`.

**Fix**: Now called automatically at `schema.rs:542`.

### Provider config fields lost during merge

**Symptom**: Provider settings from global config disappear when project config specifies the same provider.

**Cause**: HashMap merge was doing replace-all rather than field-by-field merge.

**Fix**: `ProviderConfig::merge()` method implemented for field-level merging.

### medium_model not validated

**Symptom**: Invalid `medium_model` values not caught by validation.

**Cause**: Validation only checked `model` and `small_model`.

**Fix**: `medium_model` validation added at `schema.rs:553-561`.

## Related Skills

- See `.opencode/skills/crypto/SKILL.md` for AES-256-GCM encryption details
- See `.opencode/skills/provider/SKILL.md` for provider configuration
- See `AGENTS.md` for project-wide patterns