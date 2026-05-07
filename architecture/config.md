# Config Module

The `config` module handles configuration loading, validation, and hot-reloading.

## Overview

**Location**: `src/config/`

**Key Responsibilities**:
- Configuration file discovery and loading
- Schema validation
- Hot-reload via file watching
- Environment variable overrides
- Config encryption

## Key Types

### Config Schema

```rust
pub struct Config {
    pub agent: AgentConfig,
    pub provider: ProviderConfig,
    pub tools: ToolsConfig,
    pub permission: PermissionConfig,
    pub mcp: McpConfig,
    pub plugins: PluginsConfig,
    pub session: SessionConfig,
    pub ui: UIConfig,
}
```

### AgentConfig

```rust
pub struct AgentConfig {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub system_prompt: String,
    pub compaction_tokens: usize,
}
```

## Components

### schema.rs - Config Definitions

All configuration options with defaults and validation.

### paths.rs - Config Discovery

```rust
pub struct ConfigPaths {
    pub global: PathBuf,    // ~/.config/codegg/config.toml
    pub project: PathBuf,  // .codegg/config.toml
}

impl ConfigPaths {
    pub fn discover() -> Result<Self>;
    pub fn load() -> Result<Config>;
}
```

**Discovery Order** (later overrides earlier):
1. Global config: `~/.config/codegg/config.toml`
2. Project config: `.codegg/config.toml`
3. Environment variables: `CODAGG_*`

### watcher.rs - Hot Reload

```rust
pub struct ConfigWatcher {
    #[cfg(feature = "watch")]
    watcher: RecommendedWatcher,
}

impl ConfigWatcher {
    pub fn watch(&self, path: &Path) -> Result<()>;
}
```

Uses `notify` crate for file system watching. Publishes `ConfigChanged` events via GlobalEventBus.

### encryption.rs - Config Encryption

```rust
pub fn encrypt_config(config: &Config, key: &Key) -> Result<Vec<u8>>;
pub fn decrypt_config(data: &[u8], key: &Key) -> Result<Config>;
```

Used for encrypting API keys and secrets in config files.

## Configuration Example

```toml
[agent]
model = "claude-sonnet-4-20250514"
temperature = 0.7
compaction_tokens = 100000

[provider]
default = "anthropic"

[providers.anthropic]
api_key = "sk-ant-..."

[tools]
allowed = ["bash", "read", "edit", "glob", "grep"]
denied = ["rm"]

[permission]
default_level = "Ask"

[mcp]
enabled = true
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `CODAGG_AGENT_MODEL` | Default model |
| `CODAGG_ANTHROPIC_API_KEY` | Anthropic API key |
| `CODAGG_OPENAI_API_KEY` | OpenAI API key |
| `CODAGG_CONFIG_PATH` | Custom config path |

## See Also

- [agent.md](agent.md) - Uses config
- [crypto.md](crypto.md) - Config encryption
