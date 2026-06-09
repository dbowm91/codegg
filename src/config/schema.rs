use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::auth::AuthConfig;

pub const CONFIG_VERSION: &str = "1";
pub const MIN_SUPPORTED_VERSION: &str = "1";

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum AutoupdateConfig {
    Bool(bool),
    Notify(String),
}

impl Default for AutoupdateConfig {
    fn default() -> Self {
        AutoupdateConfig::Bool(true)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
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
    pub daemon: Option<DaemonConfig>,
    pub catalog: Option<CatalogConfig>,
    pub tool_deferral: Option<ToolDeferralConfig>,
    pub model_profile: Option<HashMap<String, crate::model_profile::ModelProfileConfig>>,
    pub security: Option<SecurityConfig>,
    pub research: Option<ResearchConfig>,
    pub theme: Option<ThemeConfig>,
    pub search: Option<SearchConfig>,
    /// Per-domain tool backend selection (Phase 3 of the native tool
    /// crates plan). Each entry is a generic selector that the
    /// `ToolRegistry` resolves into the actual implementation.
    pub tool_backends: Option<ToolBackendConfigSchema>,
}

/// Web search/fetch backend configuration.
///
/// Codegg's native `websearch` and `webfetch` tools are thin wrappers
/// around an external backend. The default backend is `eggsearch`, an
/// external MCP server that owns the provider list, fetching, and
/// extraction logic. Built-in providers are kept as an explicit
/// fallback for users who cannot install `eggsearch`.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct SearchConfig {
    /// Which backend to use.
    pub backend: Option<SearchBackendConfig>,
    /// Whether to expose raw `mcp__eggsearch__*` tools to the model.
    /// Defaults to `false` so the model only sees the native wrappers.
    pub expose_raw_mcp_tools: Option<bool>,
    /// If `true`, fall back to the legacy built-in implementation when
    /// the eggsearch backend is unavailable. Defaults to `false` to
    /// encourage explicit configuration.
    pub fallback_to_builtin: Option<bool>,
    /// Output cap for `websearch` results, in characters.
    pub max_search_output_chars: Option<usize>,
    /// Output cap for `webfetch` results, in characters.
    pub max_fetch_output_chars: Option<usize>,
    /// Eggsearch-specific configuration.
    pub eggsearch: Option<EggsearchConfig>,
}

impl SearchConfig {
    pub fn backend(&self) -> SearchBackendConfig {
        self.backend.unwrap_or(SearchBackendConfig::Eggsearch)
    }

    pub fn expose_raw_mcp_tools(&self) -> bool {
        self.expose_raw_mcp_tools.unwrap_or(false)
    }

    pub fn fallback_to_builtin(&self) -> bool {
        self.fallback_to_builtin.unwrap_or(false)
    }

    pub fn max_search_output_chars(&self) -> usize {
        self.max_search_output_chars.unwrap_or(12_000)
    }

    pub fn max_fetch_output_chars(&self) -> usize {
        self.max_fetch_output_chars.unwrap_or(20_000)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchBackendConfig {
    /// Use the external eggsearch MCP server.
    Eggsearch,
    /// Use Codegg's in-tree built-in providers only.
    Builtin,
    /// Disable web search/fetch entirely.
    Disabled,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct EggsearchConfig {
    /// Whether the eggsearch backend is enabled. When `false`, the
    /// wrapper tools behave as if `[search].backend = "disabled"`.
    pub enabled: Option<bool>,
    /// MCP server name to register under. Defaults to `eggsearch`.
    pub server_name: Option<String>,
    /// Command to spawn (e.g. `eggsearch`).
    pub command: Option<String>,
    /// Arguments to the command.
    pub args: Option<Vec<String>>,
    /// MCP call timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Environment variables to set on the spawned process.
    pub env: Option<HashMap<String, String>>,
}

impl EggsearchConfig {
    pub fn server_name(&self) -> &str {
        self.server_name.as_deref().unwrap_or("eggsearch")
    }

    pub fn command(&self) -> &str {
        self.command.as_deref().unwrap_or("eggsearch")
    }

    pub fn args(&self) -> Vec<String> {
        self.args
            .clone()
            .unwrap_or_else(|| vec!["mcp".to_string(), "stdio".to_string()])
    }

    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms.unwrap_or(60_000)
    }

    pub fn env(&self) -> HashMap<String, String> {
        self.env.clone().unwrap_or_default()
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct SnapshotConfig {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            max_files: 5_000,
            max_file_bytes: 1_000_000,
            max_total_bytes: 20_000_000,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct HookConfigEntry {
    pub event: String,
    #[serde(flatten)]
    pub hook: HookConfig,
}

impl Default for HookConfigEntry {
    fn default() -> Self {
        Self {
            event: "pre_tool_execute".to_string(),
            hook: HookConfig::ShellCommand {
                command: "echo 'default hook'".to_string(),
                timeout_secs: None,
            },
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookConfig {
    ShellCommand {
        command: String,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    #[deprecated(note = "InlineScript is not implemented. Use ShellCommand instead.")]
    InlineScript {
        script: String,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
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

impl ServerConfig {
    pub fn merge(&mut self, other: &ServerConfig) {
        if other.port.is_some() {
            self.port.clone_from(&other.port);
        }
        if other.hostname.is_some() {
            self.hostname.clone_from(&other.hostname);
        }
        if other.token.is_some() {
            self.token.clone_from(&other.token);
        }
        if other.mdns.is_some() {
            self.mdns.clone_from(&other.mdns);
        }
        if other.mdns_domain.is_some() {
            self.mdns_domain.clone_from(&other.mdns_domain);
        }
        if other.cors.is_some() {
            self.cors.clone_from(&other.cors);
        }
        if other.cors_origins.is_some() {
            self.cors_origins.clone_from(&other.cors_origins);
        }
        if other.tool_timeout_seconds.is_some() {
            self.tool_timeout_seconds
                .clone_from(&other.tool_timeout_seconds);
        }
        if other.max_parallel_tools.is_some() {
            self.max_parallel_tools
                .clone_from(&other.max_parallel_tools);
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
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
    /// New typed auth descriptor. When present, takes precedence over
    /// `api_key` / `encrypted_api_key` during credential resolution
    /// (see `crate::auth::resolver::AuthResolver`).
    pub auth: Option<AuthConfig>,
    /// Optional account id used to disambiguate multiple accounts in the
    /// user-level credential store.
    pub account_id: Option<String>,
}

impl ProviderConfig {
    pub fn api_key(&self, prefix: &str) -> Option<String> {
        let env_key = format!("{}_API_KEY", prefix.to_uppercase().replace('-', "_"));
        if let Ok(env_val) = std::env::var(&env_key) {
            return Some(env_val);
        }

        if let Some(ref api_key) = self.api_key {
            return Some(api_key.clone());
        }

        if let (Some(ref encrypted_api_key), Some(true)) = (&self.encrypted_api_key, self.encrypted)
        {
            if let Some(password) = crate::config::encryption::get_master_key() {
                if let Ok(decrypted) =
                    crate::crypto::decrypt_from_string(encrypted_api_key, &password)
                {
                    return Some(decrypted);
                }
            }
        }

        None
    }

    pub fn merge(&mut self, other: &ProviderConfig) {
        if other.api_key.is_some() {
            self.api_key.clone_from(&other.api_key);
        }
        if other.encrypted_api_key.is_some() {
            self.encrypted_api_key.clone_from(&other.encrypted_api_key);
        }
        if other.encrypted.is_some() {
            self.encrypted.clone_from(&other.encrypted);
        }
        if other.base_url.is_some() {
            self.base_url.clone_from(&other.base_url);
        }
        if other.enterprise_url.is_some() {
            self.enterprise_url.clone_from(&other.enterprise_url);
        }
        if other.set_cache_key.is_some() {
            self.set_cache_key.clone_from(&other.set_cache_key);
        }
        if other.timeout.is_some() {
            self.timeout.clone_from(&other.timeout);
        }
        if other.chunk_timeout.is_some() {
            self.chunk_timeout.clone_from(&other.chunk_timeout);
        }
        if other.whitelist.is_some() {
            self.whitelist.clone_from(&other.whitelist);
        }
        if other.blacklist.is_some() {
            self.blacklist.clone_from(&other.blacklist);
        }
        if other.models.is_some() {
            self.models.clone_from(&other.models);
        }
        if other.options.is_some() {
            self.options.clone_from(&other.options);
        }
        if other.auth.is_some() {
            self.auth.clone_from(&other.auth);
        }
        if other.account_id.is_some() {
            self.account_id.clone_from(&other.account_id);
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum ProviderTimeout {
    Ms(u64),
    Disabled(bool),
}

impl Default for ProviderTimeout {
    fn default() -> Self {
        ProviderTimeout::Ms(300000)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ModelConfig {
    pub name: Option<String>,
    pub variants: Option<HashMap<String, ModelVariant>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ModelVariant {
    pub disabled: Option<bool>,
    pub thinking_budget: Option<usize>,
    pub reasoning_effort: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct AgentConfig {
    pub name: Option<String>,
    pub role: Option<String>,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub mode: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub prompt: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub steps: Option<u32>,
    pub hidden: Option<bool>,
    pub disable: Option<bool>,
    pub permission: Option<HashMap<String, PermissionRule>>,
    pub tools: Option<HashMap<String, bool>>,
    pub options: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum PermissionRule {
    Action(String),
    Object(HashMap<String, String>),
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct McpEntry {
    #[serde(flatten)]
    pub inner: Option<McpServerConfig>,
    pub enabled: Option<bool>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct McpServerConfig {
    #[serde(rename = "type")]
    pub server_type: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub environment: Option<HashMap<String, String>>,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub transport: Option<String>,
    pub timeout: Option<u64>,
    pub oauth: Option<McpOAuthConfig>,
    pub reconnect: Option<McpReconnectConfig>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct McpReconnectConfig {
    pub enabled: Option<bool>,
    pub max_retries: Option<u64>,
    pub base_delay_secs: Option<u64>,
    pub max_delay_secs: Option<u64>,
    pub heartbeat_interval_secs: Option<u64>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct McpOAuthConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub scope: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct PermissionConfig {
    pub default: Option<String>,
    pub read: Option<PermissionRule>,
    pub edit: Option<PermissionRule>,
    pub glob: Option<PermissionRule>,
    pub grep: Option<PermissionRule>,
    pub list: Option<PermissionRule>,
    pub bash: Option<PermissionRule>,
    pub bash_allow_patterns: Option<Vec<String>>,
    pub bash_deny_patterns: Option<Vec<String>>,
    pub allow_all_bash: Option<bool>,
    pub task: Option<PermissionRule>,
    pub todowrite: Option<String>,
    pub question: Option<String>,
    pub webfetch: Option<String>,
    pub websearch: Option<String>,
    pub codesearch: Option<String>,
    pub lsp: Option<PermissionRule>,
    pub doom_loop: Option<String>,
    pub skill: Option<PermissionRule>,
    pub tools: Option<HashMap<String, String>>,
    pub paths: Option<Vec<String>>,
    pub doomloop_threshold: Option<usize>,
    pub sandbox_mode: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompactionModeConfig {
    Programmatic,
    Agent,
    Hybrid,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompactionPolicyConfig {
    Conservative,
    Balanced,
    Cheap,
    Emergency,
    LosslessDebug,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct CompactionConfig {
    pub enabled: Option<bool>,
    pub auto: Option<bool>,

    // New high-level controls
    pub mode: Option<CompactionModeConfig>,
    pub policy: Option<CompactionPolicyConfig>,

    // Existing controls
    pub prune: Option<bool>,
    pub max_tokens: Option<usize>,
    pub threshold: Option<f64>,
    pub reserved: Option<usize>,

    // Existing field retained as compatibility alias
    pub summarize_model: Option<String>,

    // Preferred new field. If unset, fall back to summarize_model, then active model
    pub model: Option<String>,

    // New budgets
    pub max_tool_output_tokens: Option<usize>,
    pub max_summary_tokens: Option<usize>,
    pub max_events: Option<usize>,
    pub keep_recent_messages: Option<usize>,

    // New safety/quality controls
    pub validate: Option<bool>,
    pub preserve_evidence: Option<bool>,
    pub inject_context_frame: Option<bool>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct SubagentConfig {
    pub max_concurrent: Option<usize>,
    pub max_depth: Option<usize>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct SkillsConfig {
    pub enabled: Option<bool>,
    pub paths: Option<Vec<String>>,
    pub urls: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct CommandConfig {
    pub template: String,
    pub description: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub subtask: Option<bool>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct SessionTemplate {
    pub name: String,
    pub description: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub instructions: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum FormatterConfig {
    Disabled(bool),
    Rules(HashMap<String, FormatterRule>),
}

impl Default for FormatterConfig {
    fn default() -> Self {
        FormatterConfig::Rules(HashMap::new())
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct FormatterRule {
    pub disabled: Option<bool>,
    pub command: Option<Vec<String>>,
    pub environment: Option<HashMap<String, String>>,
    pub extensions: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum LspConfig {
    Disabled(bool),
    Rules(HashMap<String, LspRule>),
}

impl Default for LspConfig {
    fn default() -> Self {
        LspConfig::Rules(HashMap::new())
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum LspRule {
    Disabled {
        disabled: bool,
    },
    Active {
        command: Vec<String>,
        extensions: Option<Vec<String>>,
        disabled: Option<bool>,
        env: Option<HashMap<String, String>>,
        initialization: Option<HashMap<String, serde_json::Value>>,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct WatcherConfig {
    pub ignore: Option<Vec<String>>,
    #[serde(default = "default_debounce_duration_ms")]
    pub debounce_duration_ms: Option<u64>,
}

fn default_debounce_duration_ms() -> Option<u64> {
    Some(500)
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct EnterpriseConfig {
    pub url: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ExperimentalConfig {
    pub disable_paste_summary: Option<bool>,
    pub batch_tool: Option<bool>,
    pub lsp_tool: Option<bool>,
    pub open_telemetry: Option<bool>,
    pub primary_tools: Option<Vec<String>>,
    pub continue_loop_on_deny: Option<bool>,
    pub mcp_timeout: Option<u64>,
    pub memory_auto_consolidate: Option<bool>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ModeConfig {
    pub description: Option<String>,
    pub default: Option<String>,
    pub inherit: Option<bool>,
    pub tools: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct QuietHoursConfig {
    pub start_hour: Option<u8>,
    pub end_hour: Option<u8>,
    pub timezone: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct NotificationConfig {
    pub enabled: Option<bool>,
    pub on_task_complete: Option<bool>,
    pub on_error: Option<bool>,
    pub audio: Option<NotificationAudioConfig>,
    pub quiet_hours: Option<QuietHoursConfig>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct NotificationAudioConfig {
    pub enabled: Option<bool>,
    pub backend: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub speak: Option<Vec<String>>,
    pub interrupt_on: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct DaemonConfig {
    pub enabled: Option<bool>,
    pub auto_start: Option<bool>,
    pub socket: Option<String>,
    pub project_scope: Option<String>,
    pub event_log_capacity: Option<usize>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct CatalogConfig {
    pub enabled: Option<bool>,
    pub deferred_tools: Option<Vec<String>>,
    pub search_max_results: Option<usize>,
}

/// Configuration for tool deferral and partitioning behavior.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ToolDeferralConfig {
    /// Whether tool deferral is enabled (default: true).
    pub defer_loading: Option<bool>,
    /// Tools that are never deferred, always included in initial requests.
    pub always_loaded: Option<Vec<String>>,
    /// Search mode for deferred tool discovery: "keyword", "bm25", "embeddings".
    pub search_mode: Option<String>,
    /// Maximum number of tools sent in the initial request.
    pub max_initial_tools: Option<usize>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum PluginSpec {
    Path(String),
    WithOptions(String, HashMap<String, serde_json::Value>),
}

impl Config {
    pub fn load() -> Result<Self, crate::error::ConfigError> {
        let paths = crate::config::paths::resolve_config_paths();
        if paths.is_empty() {
            tracing::warn!("No config files found, using defaults");
            return Ok(Config::default());
        }
        let configs: Result<Vec<_>, _> = paths
            .iter()
            .map(|p| crate::config::paths::load_config(p))
            .collect();
        let configs = configs?;
        let mut config = crate::config::paths::merge_configs(&configs);

        crate::config::encryption::decrypt_provider_keys(&mut config)
            .map_err(|e| crate::error::ConfigError::Invalid(e.to_string()))?;

        config.migrate();

        if let Err(errors) = config.validate() {
            let msg = errors.join("; ");
            tracing::warn!(errors = %msg, "config validation warnings");
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<(), crate::error::ConfigError> {
        let path = crate::config::paths::find_project_config()
            .or_else(crate::config::paths::global_config_path)
            .ok_or_else(|| {
                crate::error::ConfigError::Invalid("Could not determine config path to save".into())
            })?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                crate::error::ConfigError::Invalid(format!("Failed to create config dir: {}", e))
            })?;
        }

        let mut to_save = self.clone();
        crate::config::encryption::encrypt_provider_keys(&mut to_save).map_err(|e| {
            crate::error::ConfigError::Invalid(format!(
                "Failed to encrypt/migrate provider keys before save: {}",
                e
            ))
        })?;

        let content = serde_json::to_string_pretty(&to_save).map_err(|e| {
            crate::error::ConfigError::Parse(format!("Failed to serialize config: {}", e))
        })?;

        std::fs::write(&path, content).map_err(|e| {
            crate::error::ConfigError::Invalid(format!("Failed to write config file: {}", e))
        })?;

        Ok(())
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if let Some(ref level) = self.log_level {
            match level.as_str() {
                "debug" | "info" | "warn" | "error" | "trace" => {}
                _ => errors.push(format!(
                    "invalid log_level '{}': must be one of debug, info, warn, error, trace",
                    level
                )),
            }
        }

        if let Some(ref share) = self.share {
            match share.as_str() {
                "manual" | "auto" | "disabled" => {}
                _ => errors.push(format!(
                    "invalid share value '{}': must be one of manual, auto, disabled",
                    share
                )),
            }
        }

        if let Some(ref model) = self.model {
            if !model.contains('/') {
                errors.push(format!(
                    "invalid model '{}': must be in format provider/model",
                    model
                ));
            }
        }

        if let Some(ref small_model) = self.small_model {
            if !small_model.contains('/') {
                errors.push(format!(
                    "invalid small_model '{}': must be in format provider/model",
                    small_model
                ));
            }
        }

        if let Some(ref medium_model) = self.medium_model {
            if !medium_model.contains('/') {
                errors.push(format!(
                    "invalid medium_model '{}': must be in format provider/model",
                    medium_model
                ));
            }
        }

        if let Some(ref providers) = self.provider {
            for (name, provider) in providers {
                if let Some(ref models) = provider.models {
                    for (model_name, model_cfg) in models {
                        if let Some(ref variants) = model_cfg.variants {
                            for variant_name in variants.keys() {
                                if variant_name.is_empty() {
                                    errors.push(format!(
                                        "empty variant name in provider '{}' model '{}'",
                                        name, model_name
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(ref mcp) = self.mcp {
            for (name, entry) in mcp {
                if let Some(ref server) = entry.inner {
                    if let Some(ref stype) = server.server_type {
                        match stype.as_str() {
                            "local" => {
                                if server.command.is_none() {
                                    errors.push(format!(
                                        "MCP server '{}' of type 'local' requires a command",
                                        name
                                    ));
                                }
                            }
                            "remote" => {
                                if server.url.is_none() {
                                    errors.push(format!(
                                        "MCP server '{}' of type 'remote' requires a url",
                                        name
                                    ));
                                }
                            }
                            _ => errors.push(format!(
                                "invalid MCP server type '{}' for server '{}': must be local or remote",
                                stype, name
                            )),
                        }
                    }
                }
            }
        }

        if let Some(ref commands) = self.commands {
            for (name, cmd) in commands {
                if cmd.template.is_empty() {
                    errors.push(format!("command '{}' has an empty template", name));
                }
            }
        }

        if let Some(ref server) = self.server {
            if let Some(port) = server.port {
                if port < 1024 {
                    errors.push(format!("port {} is in privileged range (1024-65535)", port));
                }
            }
            if let Some(timeout) = server.tool_timeout_seconds {
                if timeout == 0 {
                    errors.push("tool_timeout_seconds cannot be 0".to_string());
                }
                if timeout > 3600 {
                    errors.push("tool_timeout_seconds exceeds 1 hour".to_string());
                }
            }
            if let Some(max) = server.max_parallel_tools {
                if max == 0 {
                    errors.push("max_parallel_tools cannot be 0".to_string());
                }
                if max > 100 {
                    errors.push("max_parallel_tools exceeds 100".to_string());
                }
            }
        }

        if let Some(ref compaction) = self.compaction {
            if let Some(threshold) = compaction.threshold {
                if !(0.1..=1.0).contains(&threshold) {
                    errors.push(format!(
                        "compaction threshold {} must be between 0.1 and 1.0",
                        threshold
                    ));
                }
            }
            if let Some(limit) = compaction.max_tokens {
                if limit < 1000 {
                    errors.push("compaction max_tokens must be at least 1000".to_string());
                }
            }
        }

        if let Some(ref agents) = self.agent {
            for (name, agent) in agents {
                if let Some(ref mode) = agent.mode {
                    match mode.as_str() {
                        "subagent" | "primary" | "all" => {}
                        _ => errors.push(format!(
                            "invalid mode '{}' for agent '{}': must be one of subagent, primary, all",
                            mode, name
                        )),
                    }
                }
                if let Some(ref color) = agent.color {
                    if !color.starts_with('#')
                        && !matches!(
                            color.as_str(),
                            "primary"
                                | "secondary"
                                | "accent"
                                | "success"
                                | "warning"
                                | "error"
                                | "info"
                        )
                    {
                        errors.push(format!(
                            "invalid color '{}' for agent '{}': must be hex color or theme color name",
                            color, name
                        ));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn migrate(&mut self) {
        let version = self.version.clone().unwrap_or_else(|| "0".to_string());

        if version == "0" {
            self.migrate_from_v0();
        }

        self.version = Some(CONFIG_VERSION.to_string());
    }

    fn migrate_from_v0(&mut self) {
        if let Some(ref version) = self.version {
            if version == "0" {
                tracing::info!("Migrating config from v0 to v1");
            }
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct SecurityConfig {
    pub enabled: bool,
    pub mode: SecurityMode,
    pub prompt_hints: bool,
    pub max_findings_in_prompt: usize,
    pub gates: SecurityGateConfig,
    pub profiles: SecurityProfileConfig,
    pub sensitive_paths: Vec<SensitivePathConfig>,
    pub allowed_network_domains: Vec<String>,
    pub denied_commands: Vec<String>,
    pub auto_invoke_review_agent: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: SecurityMode::Ambient,
            prompt_hints: true,
            max_findings_in_prompt: 5,
            gates: SecurityGateConfig::default(),
            profiles: SecurityProfileConfig::default(),
            sensitive_paths: Vec::new(),
            allowed_network_domains: Vec::new(),
            denied_commands: Vec::new(),
            auto_invoke_review_agent: true,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityMode {
    Off,
    Ambient,
    Strict,
    Review,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct SecurityGateConfig {
    pub ask_on_high_risk_command: bool,
    pub deny_critical_commands: bool,
    pub ask_on_network_exfiltration: bool,
    pub ask_on_secret_exposure: bool,
    pub ask_on_dependency_risk: bool,
    pub enforce_in_exec_mode: bool,
}

impl Default for SecurityGateConfig {
    fn default() -> Self {
        Self {
            ask_on_high_risk_command: true,
            deny_critical_commands: true,
            ask_on_network_exfiltration: true,
            ask_on_secret_exposure: true,
            ask_on_dependency_risk: false,
            enforce_in_exec_mode: false,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct SecurityProfileConfig {
    pub ambient_on_tool_call: bool,
    pub pre_commit_on_final: bool,
    pub dependency_delta_on_manifest_change: bool,
}

impl Default for SecurityProfileConfig {
    fn default() -> Self {
        Self {
            ambient_on_tool_call: true,
            pre_commit_on_final: false,
            dependency_delta_on_manifest_change: true,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct SensitivePathConfig {
    pub glob: String,
    pub reason: Option<String>,
    pub review_level: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ResearchConfig {
    pub search_provider: Option<SearchProviderConfig>,
    /// Trigger heuristic configuration. When enabled, the agent loop
    /// injects a hint at the start of a turn if the user's input
    /// matches a research-style pattern (comparison, library eval,
    /// API investigation, security review, architecture decision).
    /// The hint steers the model toward spawning a `research` subagent
    /// via the `task` tool. It does NOT auto-invoke the research
    /// pipeline.
    pub auto_trigger: Option<ResearchAutoTriggerConfig>,
}

/// Per-domain tool backend selection.
///
/// This is the generic pattern that the native tool crates plan
/// generalizes from the eggsearch integration. Each entry is a
/// generic selector that the `ToolRegistry` resolves into the
/// actual implementation. Today, no domain uses the non-native
/// option; the schema is in place so future wrappers (egglsp,
/// eggsentry, eggcontext) can opt into the same pattern without
/// re-inventing config types.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ToolBackendConfigSchema {
    /// Backend for the LSP domain.
    pub lsp: Option<ExternalToolBackendConfigSchema>,
    /// Backend for the deterministic security domain.
    pub security: Option<ExternalToolBackendConfigSchema>,
    /// Backend for context-packing helpers.
    pub context: Option<ExternalToolBackendConfigSchema>,
}

impl ToolBackendConfigSchema {
    /// Resolve the effective backend for a given domain key, defaulting
    /// to `Native` when unset.
    pub fn backend_for(&self, domain: &str) -> Option<ToolImplementationBackendSchema> {
        let section = match domain {
            "lsp" => self.lsp.as_ref(),
            "security" => self.security.as_ref(),
            "context" => self.context.as_ref(),
            _ => None,
        };
        section.and_then(|s| s.backend)
    }
}

/// Resolved per-domain backend configuration (config-time view).
///
/// Mirrors `tool::backend::ExternalToolBackendConfig` so config
/// parsing does not have to depend on the tool module.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ExternalToolBackendConfigSchema {
    /// Which backend kind to use.
    pub backend: Option<ToolImplementationBackendSchema>,
    /// Whether to expose raw `mcp__*__tool` definitions in the model
    /// catalog when a native wrapper exists. Defaults to `false` for
    /// Codegg-managed backends.
    pub expose_raw_mcp_tools: Option<bool>,
    /// Whether to fall back to the in-tree implementation if the
    /// configured backend is unavailable.
    pub fallback_to_native: Option<bool>,
    /// MCP server name (when `backend = Mcp`).
    pub server_name: Option<String>,
    /// Command to spawn (when `backend = Mcp` and the server is
    /// local stdio).
    pub command: Option<String>,
    /// Args for the spawned process.
    pub args: Option<Vec<String>>,
    /// Per-call timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Environment variables to set on the spawned process.
    pub env: Option<HashMap<String, String>>,
}

/// Which implementation backs a given tool domain (config-time view).
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolImplementationBackendSchema {
    /// Direct in-process Rust implementation.
    Native,
    /// External MCP server.
    Mcp,
    /// In-tree built-in / legacy implementation.
    Builtin,
    /// The tool domain is disabled; the wrapper tool should hide
    /// itself or return a clear "disabled" error.
    Disabled,
}

impl ToolImplementationBackendSchema {
    pub fn label(self) -> &'static str {
        match self {
            ToolImplementationBackendSchema::Native => "native",
            ToolImplementationBackendSchema::Mcp => "mcp",
            ToolImplementationBackendSchema::Builtin => "builtin",
            ToolImplementationBackendSchema::Disabled => "disabled",
        }
    }
}

/// User-facing theme configuration.
///
/// `name` selects the active theme. `source` overrides the format detection
/// for a single `path`. `directories` adds additional user directories to
/// scan for `.toml` theme files. `fallback` is consulted when `name` cannot
/// be resolved. `validate_contrast` enables contrast diagnostics on load.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: Option<String>,
    pub source: Option<ThemeSourceConfig>,
    pub path: Option<String>,
    pub directories: Option<Vec<String>>,
    pub validate_contrast: Option<bool>,
    pub fallback: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSourceConfig {
    Auto,
    Builtin,
    Native,
    Halloy,
}

impl Default for ResearchAutoTriggerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_confidence: 0.7,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct ResearchAutoTriggerConfig {
    pub enabled: bool,
    /// Minimum confidence (0.0–1.0) at which to inject the hint.
    pub min_confidence: f32,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct SearchProviderConfig {
    pub provider: Option<String>,
    pub api_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        Config, EggsearchConfig, ProviderConfig, SearchBackendConfig, SearchConfig,
        ToolImplementationBackendSchema,
    };

    #[test]
    fn test_provider_api_key_supports_master_key_for_decryption() {
        // Serialize against other tests that flip `CODEGG_MASTER_KEY`
        // (e.g. `auth::store::tests::*` and `auth::resolver::tests::*`).
        let _guard = crate::auth::test_support::env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        std::env::set_var("CODEGG_MASTER_KEY", "test-master-key");
        std::env::remove_var("CODEGG_ENCRYPTION_KEY");

        let encrypted = crate::crypto::encrypt_to_string("decrypted-value", "test-master-key")
            .expect("encryption should succeed");
        let provider = ProviderConfig {
            encrypted_api_key: Some(encrypted),
            encrypted: Some(true),
            ..Default::default()
        };

        let key = provider.api_key("openai");
        assert_eq!(key.as_deref(), Some("decrypted-value"));

        std::env::remove_var("CODEGG_MASTER_KEY");
    }

    #[test]
    fn provider_config_parses_typed_auth_descriptor() {
        let raw = r#"{
            "provider": {
                "xai": {
                    "auth": { "type": "api_key", "env": "XAI_API_KEY" }
                },
                "anthropic": {
                    "api_key": "sk-ant-legacy"
                }
            }
        }"#;
        let cfg: Config = json5::from_str(raw).expect("parses");
        let providers = cfg.provider.as_ref().expect("provider");
        let xai = providers.get("xai").expect("xai");
        match xai.auth.as_ref().expect("auth") {
            crate::auth::AuthConfig::ApiKey {
                env,
                value,
                encrypted_value,
            } => {
                assert_eq!(env.as_deref(), Some("XAI_API_KEY"));
                assert!(value.is_none());
                assert!(encrypted_value.is_none());
            }
            other => panic!("expected ApiKey auth, got {other:?}"),
        }
        let anthropic = providers.get("anthropic").expect("anthropic");
        assert!(anthropic.auth.is_none());
        assert_eq!(anthropic.api_key.as_deref(), Some("sk-ant-legacy"));
    }

    #[test]
    fn provider_config_merge_preserves_and_overrides_auth() {
        let mut base = ProviderConfig {
            api_key: Some("base-key".to_string()),
            auth: Some(crate::auth::AuthConfig::ApiKey {
                env: Some("BASE_ENV".to_string()),
                value: None,
                encrypted_value: None,
            }),
            account_id: Some("acct-base".to_string()),
            ..Default::default()
        };
        let other = ProviderConfig {
            api_key: Some("override-key".to_string()),
            auth: Some(crate::auth::AuthConfig::Stored {
                account_id: Some("acct-override".to_string()),
            }),
            account_id: Some("acct-override".to_string()),
            ..Default::default()
        };
        base.merge(&other);
        // Fields are overridden by the second provider.
        assert_eq!(base.api_key.as_deref(), Some("override-key"));
        match base.auth.as_ref().expect("auth") {
            crate::auth::AuthConfig::Stored { account_id } => {
                assert_eq!(account_id.as_deref(), Some("acct-override"));
            }
            other => panic!("expected Stored, got {other:?}"),
        }
        assert_eq!(base.account_id.as_deref(), Some("acct-override"));

        // When `other` doesn't specify auth, the base auth is preserved.
        let mut base2 = ProviderConfig {
            auth: Some(crate::auth::AuthConfig::ApiKey {
                env: Some("BASE_ENV".to_string()),
                value: None,
                encrypted_value: None,
            }),
            ..Default::default()
        };
        base2.merge(&ProviderConfig::default());
        assert!(base2.auth.is_some());
    }

    #[test]
    fn search_config_default_backend_is_eggsearch() {
        let cfg = SearchConfig::default();
        assert_eq!(cfg.backend(), SearchBackendConfig::Eggsearch);
        assert!(!cfg.expose_raw_mcp_tools());
        assert!(!cfg.fallback_to_builtin());
        assert_eq!(cfg.max_search_output_chars(), 12_000);
        assert_eq!(cfg.max_fetch_output_chars(), 20_000);
    }

    #[test]
    fn eggsearch_config_resolves_defaults() {
        let egg = EggsearchConfig::default();
        assert_eq!(egg.server_name(), "eggsearch");
        assert_eq!(egg.command(), "eggsearch");
        assert_eq!(egg.args(), vec!["mcp", "stdio"]);
        assert_eq!(egg.timeout_ms(), 60_000);
        assert!(egg.env().is_empty());
    }

    #[test]
    fn eggsearch_config_resolves_overrides() {
        let egg = EggsearchConfig {
            enabled: Some(true),
            server_name: Some("alt".to_string()),
            command: Some("/usr/local/bin/eggsearch".to_string()),
            args: Some(vec!["serve".to_string()]),
            timeout_ms: Some(15_000),
            env: Some(Default::default()),
        };
        assert_eq!(egg.server_name(), "alt");
        assert_eq!(egg.command(), "/usr/local/bin/eggsearch");
        assert_eq!(egg.args(), vec!["serve"]);
        assert_eq!(egg.timeout_ms(), 15_000);
    }

    #[test]
    fn search_section_parses() {
        let json = r#"{
            "search": {
                "backend": "eggsearch",
                "expose_raw_mcp_tools": true,
                "fallback_to_builtin": true,
                "max_search_output_chars": 5000,
                "max_fetch_output_chars": 8000,
                "eggsearch": {
                    "enabled": true,
                    "command": "eggsearch-test",
                    "args": ["mcp", "stdio"],
                    "timeout_ms": 30000
                }
            }
        }"#;
        let cfg: Config = serde_json::from_str(json).expect("parse");
        let s = cfg.search.expect("search section");
        assert_eq!(s.backend(), SearchBackendConfig::Eggsearch);
        assert!(s.expose_raw_mcp_tools());
        assert!(s.fallback_to_builtin());
        assert_eq!(s.max_search_output_chars(), 5000);
        assert_eq!(s.max_fetch_output_chars(), 8000);
        let egg = s.eggsearch.expect("eggsearch section");
        assert_eq!(egg.command(), "eggsearch-test");
        assert_eq!(egg.timeout_ms(), 30_000);
    }

    #[test]
    fn search_section_omitted_uses_defaults() {
        let json = "{}";
        let cfg: Config = serde_json::from_str(json).expect("parse");
        assert!(cfg.search.is_none());
        // Effective behavior: defaults to eggsearch.
        let s = cfg.search.unwrap_or_default();
        assert_eq!(s.backend(), SearchBackendConfig::Eggsearch);
    }

    #[test]
    fn explicit_mcp_eggsearch_does_not_force_search_section() {
        // `[mcp.eggsearch]` is a valid alternative to `[search.eggsearch]`.
        let json = r#"{
            "mcp": {
                "eggsearch": {
                    "type": "local",
                    "command": "eggsearch",
                    "args": ["mcp", "stdio"]
                }
            }
        }"#;
        let cfg: Config = serde_json::from_str(json).expect("parse");
        assert!(cfg.search.is_none());
        let mcp = cfg.mcp.expect("mcp");
        assert!(mcp.contains_key("eggsearch"));
    }

    #[test]
    fn tool_backends_section_defaults_to_native() {
        let cfg = Config::default();
        assert!(cfg.tool_backends.is_none());
        // Effective behavior: every domain defaults to Native.
        let resolved = cfg.tool_backends.unwrap_or_default();
        assert_eq!(
            resolved.backend_for("lsp"),
            None,
            "explicit None means 'no override' and resolves to the default Native"
        );
    }

    #[test]
    fn tool_backends_section_parses_lsp_section() {
        let json = r#"{
            "tool_backends": {
                "lsp": {
                    "backend": "native",
                    "expose_raw_mcp_tools": false,
                    "fallback_to_native": true
                },
                "security": {
                    "backend": "native"
                },
                "context": {
                    "backend": "mcp",
                    "server_name": "eggcontext",
                    "command": "eggcontext",
                    "args": ["mcp", "stdio"],
                    "timeout_ms": 30000
                }
            }
        }"#;
        let cfg: Config = serde_json::from_str(json).expect("parse");
        let tb = cfg.tool_backends.expect("tool_backends section");
        let lsp = tb.lsp.expect("lsp");
        assert_eq!(lsp.backend, Some(ToolImplementationBackendSchema::Native));
        assert_eq!(lsp.expose_raw_mcp_tools, Some(false));
        assert_eq!(lsp.fallback_to_native, Some(true));
        let security = tb.security.expect("security");
        assert_eq!(
            security.backend,
            Some(ToolImplementationBackendSchema::Native)
        );
        let context = tb.context.expect("context");
        assert_eq!(context.backend, Some(ToolImplementationBackendSchema::Mcp));
        assert_eq!(context.server_name.as_deref(), Some("eggcontext"));
        assert_eq!(context.timeout_ms, Some(30_000));
    }

    #[test]
    fn tool_backends_omitted_uses_defaults() {
        let json = "{}";
        let cfg: Config = serde_json::from_str(json).expect("parse");
        assert!(cfg.tool_backends.is_none());
        let resolved = cfg.tool_backends.unwrap_or_default();
        assert_eq!(resolved.backend_for("lsp"), None);
        assert_eq!(resolved.backend_for("security"), None);
        assert_eq!(resolved.backend_for("context"), None);
    }
}
