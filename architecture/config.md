# Config Module

## Purpose

The config crate (`crates/codegg-config/`) handles configuration discovery,
loading, JSONC parsing, field-level merging, encryption, hot-reload via
file watching, and schema validation. It is the single source of truth for
all runtime configuration.

**Re-export**: `codegg::config` via `pub use codegg_config as config`
in `src/lib.rs`.

## Where It Lives

| Path | Role |
|------|------|
| `crates/codegg-config/src/schema.rs` | Config struct, all type definitions |
| `crates/codegg-config/src/paths.rs` | Discovery, loading, merging, env interpolation |
| `crates/codegg-config/src/watcher.rs` | Hot-reload file watching with debounce |
| `crates/codegg-config/src/encryption.rs` | Master key lookup, encrypt/decrypt provider keys |
| `crates/codegg-config/src/error.rs` | ConfigError, AppError types |

## How It Works

### Discovery Order (later overrides earlier)

1. `CODEGG_TUI_CONFIG` environment variable
2. System config (`/Library/Application Support/codegg/codegg.json` on macOS,
   `/etc/codegg/codegg.json` on Unix, `%ProgramData%/codegg/codegg.json` on Windows)
3. Global config (`~/.config/codegg/codegg.jsonc`, `codegg.json`, or `config.json`)
4. Project config (searches upward from `$PWD` for `.codegg/codegg.{jsonc,json}`
   or `codegg/codegg.{jsonc,json}`)

### Loading Flow

```
Config::load()
  1. resolve_config_paths()    -> collect config file paths
  2. load_config() per path    -> JSONC comment stripping + JSON5 parse
  3. interpolate_env_vars()    -> expand ${VAR_NAME} syntax
  4. merge_configs()           -> combine with per-field strategies
  5. decrypt_provider_keys()   -> decrypt encrypted API keys
  6. validate()                -> produce warnings (not errors)
```

### Merge Strategies (`paths.rs:164`)

Different strategies per field type:

- **Field-by-field**: `provider` (via `ProviderConfig::merge()`),
  `server` (via `ServerConfig::merge()`), `watcher`, `search`,
  `discovery`
- **Key replacement**: `agent`, `mcp`, `commands`, `mode`, `model_profile` (insert
  overwrites existing keys)
- **Concatenation**: `instructions` (appended to list)
- **Simple override** (via `merge_option!`): all other fields including
  `schema`, `version`, `log_level`, `model`, `small_model`, `medium_model`,
  `auto_route_models`, `default_agent`, `username`, `share`, `autoupdate`,
  `disabled_providers`, `enabled_providers`, `permission`, `compaction`,
  `subagent`, `skills`, `templates`, `layout`, `tools`, `formatter`,
  `lsp`, `lsp_semantic_cache`, `snapshot`, `snapshot_config`, `plugin`,
  `enterprise`, `experimental`, `keybinds`, `vim_mode`, `hooks`,
  `notifications`, `catalog`, `context`, `context_packer`,
  `context_policy`, `daemon`, `scheduler`, `tool_deferral`,
  `security`, `research`, `theme`, `tool_backends`,
  `human_shell`, `shell`, `deterministic_tools`, `preflight`,
  `command_intent`, `orchestration`

### ProviderConfig Merge (`schema.rs:774`)

Field-by-field: non-None fields from override replace base. Unlike
HashMap fields (key replacement), `ProviderConfig::merge()` merges
each optional field independently. If global has `api_key` and project
has `base_url`, merged result has both.

## Key Types & APIs

### Config (`schema.rs:203`)

```rust
pub struct Config {
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
    pub provider_connections: Option<ProviderConnectionsConfig>,
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
    pub lsp_semantic_cache: Option<LspSemanticCacheConfig>,
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
    pub daemon: Option<DaemonConfig>,
    pub scheduler: Option<SchedulerConfig>,
    pub catalog: Option<CatalogConfig>,
    pub discovery: Option<DiscoveryConfig>,
    pub tool_deferral: Option<ToolDeferralConfig>,
    pub model_profile: Option<HashMap<String, ModelProfileConfig>>,
    pub security: Option<SecurityConfig>,
    pub research: Option<ResearchConfig>,
    pub theme: Option<ThemeConfig>,
    pub search: Option<SearchConfig>,
    pub tool_backends: Option<ToolBackendConfigSchema>,
    pub context: Option<ContextConfig>,
    pub context_packer: Option<ContextPackerConfig>,
    pub context_policy: Option<ContextPolicyConfig>,
    pub human_shell: Option<HumanShellConfig>,
    pub shell: Option<ShellConfig>,
    pub deterministic_tools: Option<DeterministicToolsConfig>,
    pub preflight: Option<PreflightConfig>,
    pub command_intent: Option<CommandIntentConfig>,
}
```

### ProviderConfig (`schema.rs:734`)

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
    pub auth: Option<AuthConfig>,
    pub account_id: Option<String>,
}
```

`api_key(&self, prefix)` checks `{PREFIX}_API_KEY` env var first, then
inline `api_key` field.

### AuthConfig (`schema.rs:13`)

```rust
pub enum AuthConfig {
    ApiKey { env, value, encrypted_value },
    Stored { account_id },
    ExternalCommand { command, args, timeout_ms },
    OAuthDevice { client_id, scopes, auth_url, token_url },
    None,
}
```

### ProviderConnectionsConfig (`schema.rs:284`)

Daemon-owned provider-connection refresh policy. Defaults:
`background_refresh=false`, `max_concurrent_refreshes=1`,
`global_refresh_cap=4`, `health_stale_after_ms=300000`.

### ServerConfig (`schema.rs:686`)

```rust
pub struct ServerConfig {
    pub port: Option<u16>,
    pub hostname: Option<String>,
    pub token: Option<String>,
    pub mdns: Option<bool>,
    pub mdns_domain: Option<String>,
    pub cors: Option<Vec<String>>,
    pub cors_origins: Option<Vec<String>>,
    pub tool_timeout_seconds: Option<u64>,
    pub max_parallel_tools: Option<usize>,
}
```

Has its own `merge()` method for field-by-field merging.

### ConfigWatcher (`watcher.rs:12`)

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

Key methods: `new()` (500ms default debounce),
`with_config(&WatcherConfig)` (configurable debounce + ignore patterns),
`start()` (watches config file parent dirs, non-recursive),
`recv()` (async, content-hash deduplication),
`reload_now()` (force immediate reload).

Uses `notify` crate. Content hash deduplication avoids spurious reloads.

### Encryption (`encryption.rs`)

```rust
pub fn get_master_key() -> Option<String>;
pub fn encrypt_provider_keys(config: &mut Config) -> Result<(), AppError>;
pub fn decrypt_provider_keys(config: &mut Config) -> Result<(), AppError>;
```

Master key lookup order:
1. `CODEGG_MASTER_KEY`
2. `CODEGG_ENCRYPTION_KEY`
3. `OPENCODE_ENCRYPTION_KEY`

### ModelProfileConfig (`schema.rs:98`)

Per-model tuning: `prompt_profile`, `family`, `context_window`,
`max_output_tokens`, `tool_call_reliability`, `instruction_adherence`,
`patch_reliability`, `supports_late_system_messages`,
`prefers_user_control_messages`, `prefers_small_patches`,
`requires_explicit_tool_contract`, `requires_post_tool_continue_nudge`,
`text_tool_repair`, `default_reasoning_effort`,
`default_thinking_budget`, `max_parallel_tools`, `preferred_tools`,
`disabled_tools`, `task_state_policy`.

### ContextPolicyConfig (`schema.rs:366`)

Gated active context policy. First use: tool-palette reduction driven
by effective-cost diagnostics. Disabled by default. Modes: `Observe`,
`Warn`, `ToolPaletteReduce`. Includes volatile-tail compaction fields.

### SearchConfig (`schema.rs:461`)

Web search/fetch backend: `backend` (Eggsearch/Builtin/Disabled),
`expose_raw_mcp_tools`, `fallback_to_builtin`, output caps per domain,
`eggsearch` sub-config (command, args, timeouts, env vars).

## Configuration Surface

### Environment Variables

| Variable | Description |
|----------|-------------|
| `CODEGG_TUI_CONFIG` | Custom config file path |
| `CODEGG_MASTER_KEY` | Master key for encryption |
| `CODEGG_ENCRYPTION_KEY` | Fallback encryption key |
| `OPENCODE_ENCRYPTION_KEY` | Legacy encryption key |
| `{PROVIDER}_API_KEY` | Provider API key fallback |

### Key Config Sections

- `model` / `small_model` / `medium_model` — model selection
  (format: `provider/model`)
- `provider.<id>` — per-provider config (api_key, base_url, auth, etc.)
- `disabled_providers` / `enabled_providers` — provider allow/deny list
- `server` — HTTP server settings (port, hostname, token, CORS)
- `agent` — agent definitions (model, prompt, permissions, etc.)
- `mcp` — MCP server entries
- `permission` — permission rules per tool
- `compaction` — context compaction settings
- `subagent` — delegation bounds (max_concurrent, max_depth, etc.)
- `search` — web search/fetch backend config
- `lsp` / `lsp_semantic_cache` — LSP integration
- `watcher` — file watching config
- `plugin` — plugin specifications
- `experimental` — experimental feature flags
- `mode` — named mode configurations
- `hooks` — event hooks (shell commands)
- `context` / `context_packer` / `context_policy` — context management
- `human_shell` / `shell` — human shell feature config
- `deterministic_tools` / `preflight` — eggsact-backed tools
- `command_intent` — command classification and routing
- `daemon` / `scheduler` — daemon and scheduler settings
- `discovery` — project discovery configuration
- `model_profile` — per-model tuning profiles
- `orchestration` — opt-in bounded convergence defaults and aggregate deadline
- `tool_backends` — per-domain tool backend selection

`orchestration.auto_convergence` defaults to `false`. The host clamps
`default_max_cycles` to 1–4, `max_producers_per_cycle` to 1–3, and
`max_wall_clock_ms` to at most 24 hours. Explicit convergence calls remain
available when automatic guidance is disabled.

## Validation

Validation produces **warnings**, not errors — the app starts with a
partially invalid config.

Validated fields:
- `log_level`: `debug|info|warn|error|trace`
- `share`: `manual|auto|disabled`
- `model`/`small_model`/`medium_model`: must be `provider/model` format
- `port`: >= 1024
- Agent `mode`: `subagent|primary|all`
- Agent `color`: hex color or theme name
- MCP types: `local` requires `command`, `remote` requires `url`
- `tool_timeout_seconds`: 1-3600
- `max_parallel_tools`: 1-100
- `compaction.threshold`: 0.1-1.0
- `compaction.max_tokens`: >= 1000
- `deterministic_tools.backend`: `native` or `disabled`
- `deterministic_tools.profile`: `codegg_core`, `codegg_core_min`,
  `default`, or `full`
- `preflight.mode`: `off`, `observe`, `warn`, `block_on_definite`

## Invariants & Gotchas

- **Merge is per-type**: HashMap fields use key replacement (later wins);
  `ProviderConfig`/`ServerConfig`/`WatcherConfig` use field-by-field;
  `instructions` concatenates.
- **Decryption on reload**: `ConfigWatcher::reload_config()` calls
  `decrypt_provider_keys()` so encrypted keys work after hot-reload.
- **Project config searches upward**: From `$PWD`, checks `.codegg/` and
  `codegg/` directories with both `.jsonc` and `.json` extensions.
- **AuthConfig::None**: Explicit "no auth" marker — all credential
  lookups are skipped.
- **ProviderConfig merge**: `auth` field merges like any other optional;
  a project config setting `auth: { type: "stored" }` overrides the
  global `api_key` path.
- **No encryption without master key**: `decrypt_provider_keys()` is
  a no-op when `CODEGG_MASTER_KEY` is not set.

## Testing

```bash
cargo test -p codegg-config                 # all config tests
cargo test -p codegg-config -- merge        # merge strategy tests
cargo test -p codegg-config -- watcher      # watcher tests
cargo test -p codegg-config -- validation   # validation tests
```

## Related Docs

- `architecture/provider.md` — provider config and credential resolution
- `architecture/crypto.md` — AES-256-GCM encryption details
- `architecture/search_backend.md` — search backend dispatch
- `architecture/lsp.md` — LSP semantic cache config
- `architecture/agent.md` — agent config usage
