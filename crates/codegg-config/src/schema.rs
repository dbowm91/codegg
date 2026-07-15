use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CONFIG_VERSION: &str = "1";
pub const MIN_SUPPORTED_VERSION: &str = "1";

// --- AuthConfig (inline copy from root crate) ---

/// Configuration-side auth descriptor. This is the shape that lives in
/// `ProviderConfig::auth` and lets providers express richer auth modes than a
/// single static API-key string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    /// API key from environment, inline value, or encrypted config value.
    ApiKey {
        /// Optional override for the env var name (defaults to
        /// `{PROVIDER}_API_KEY` for backward compatibility).
        env: Option<String>,
        /// Optional explicit API key value. Prefer env vars or the credential
        /// store over this field.
        value: Option<String>,
        /// Optional pre-encrypted value (see `crate::config::encryption`).
        encrypted_value: Option<String>,
    },
    /// Reference to a credential stored in the user-level credential store.
    Stored {
        /// Optional account id, used when multiple accounts exist for one
        /// provider.
        account_id: Option<String>,
    },
    /// External command that returns a credential on stdout (e.g. an
    /// officially-supported CLI that brokers access to a provider).
    ExternalCommand {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        timeout_ms: Option<u64>,
    },
    /// OAuth device-code / PKCE flow. Reserved for providers that publish a
    /// stable, public contract. The first pass parses this variant but
    /// resolution returns `AuthError::Unsupported`.
    OAuthDevice {
        client_id: String,
        #[serde(default)]
        scopes: Vec<String>,
        auth_url: String,
        token_url: String,
    },
    /// Explicitly no auth configured. Useful as a marker in defaults.
    None,
}

impl Default for AuthConfig {
    fn default() -> Self {
        AuthConfig::ApiKey {
            env: None,
            value: None,
            encrypted_value: None,
        }
    }
}

impl AuthConfig {
    /// Returns true if this variant represents an API-key-shaped credential.
    pub fn is_api_key(&self) -> bool {
        matches!(self, AuthConfig::ApiKey { .. })
    }
}

// --- ModelProfileConfig (inline copy from root crate) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptProfileKind {
    FrontierReasoning,
    FrontierExecutor,
    FastExecutor,
    LocalStrict,
    ToolFragile,
    LongContextPlanner,
    Reviewer,
    Summarizer,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReliabilityTier {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelProfileConfig {
    pub prompt_profile: Option<PromptProfileKind>,
    pub family: Option<String>,

    pub context_window: Option<usize>,
    pub max_output_tokens: Option<usize>,

    pub tool_call_reliability: Option<ReliabilityTier>,
    pub instruction_adherence: Option<ReliabilityTier>,
    pub patch_reliability: Option<ReliabilityTier>,

    pub supports_late_system_messages: Option<bool>,
    pub prefers_user_control_messages: Option<bool>,
    pub prefers_small_patches: Option<bool>,
    pub requires_explicit_tool_contract: Option<bool>,
    pub requires_post_tool_continue_nudge: Option<bool>,

    pub default_reasoning_effort: Option<String>,
    pub default_thinking_budget: Option<usize>,

    pub max_parallel_tools: Option<usize>,
    pub preferred_tools: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,

    pub task_state_policy: Option<TaskStatePolicyConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoMode {
    Disabled,
    SparsePlan,
    #[default]
    ExplicitTodo,
    GuidedCurrentTask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoUpdateFrequency {
    Never,
    MilestonesOnly,
    #[default]
    MilestonesAndTaskSwitches,
    HarnessManaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompletedTodoExposure {
    #[default]
    NoneUnlessAsked,
    SummaryOnly,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubagentTodoAccess {
    #[default]
    None,
    ReadOnlyScoped,
    NoMutation,
    Full,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskStatePolicyConfig {
    pub mode: Option<TodoMode>,
    pub update_frequency: Option<TodoUpdateFrequency>,
    pub max_total_items: Option<usize>,
    pub expose_completed_items: Option<CompletedTodoExposure>,
    pub allow_model_todo_read: Option<bool>,
    pub allow_model_todo_write: Option<bool>,
    pub require_single_in_progress: Option<bool>,
    pub require_blocker_reason: Option<bool>,
    pub inject_after_tool_calls: Option<usize>,
    pub inject_on_resume: Option<bool>,
    pub inject_after_compaction: Option<bool>,
    pub subagent_todo_access: Option<SubagentTodoAccess>,
}

// --- Schema types (from root config::schema) ---

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
    pub catalog: Option<CatalogConfig>,
    pub tool_deferral: Option<ToolDeferralConfig>,
    pub model_profile: Option<HashMap<String, ModelProfileConfig>>,
    pub security: Option<SecurityConfig>,
    pub research: Option<ResearchConfig>,
    pub theme: Option<ThemeConfig>,
    pub search: Option<SearchConfig>,
    /// Per-domain tool backend selection (Phase 3 of the native tool
    /// crates plan). Each entry is a generic selector that the
    /// `ToolRegistry` resolves into the actual implementation.
    pub tool_backends: Option<ToolBackendConfigSchema>,
    /// Context ledger and artifact projection settings.
    pub context: Option<ContextConfig>,
    /// Cache-aware context packer settings.
    pub context_packer: Option<ContextPackerConfig>,
    /// Gated active context policy (first use: tool-palette reduction driven by effective-cost diagnostics).
    /// Disabled by default; observe -> warn -> tool_palette_reduce rollout.
    pub context_policy: Option<ContextPolicyConfig>,
    /// Human shell feature configuration.
    pub human_shell: Option<HumanShellConfig>,
    /// Shell output projection configuration.
    pub shell: Option<ShellConfig>,
    /// Deterministic tools (eggsact-backed) configuration.
    pub deterministic_tools: Option<DeterministicToolsConfig>,
    /// Harness-side eggsact preflight configuration.
    pub preflight: Option<PreflightConfig>,
    /// Command intent classification and routing configuration.
    pub command_intent: Option<CommandIntentConfig>,
}

/// Configuration for the context ledger and artifact projection system.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ContextConfig {
    /// Enable the artifact store for persisting tool outputs.
    pub artifact_store: Option<bool>,
    /// Enable tool output projection (compression before model sees it).
    pub project_tool_outputs: Option<bool>,
    /// Maximum tokens for successful tool output before summarization.
    pub max_success_tokens: Option<usize>,
    /// Maximum tokens for failed tool output before summarization.
    pub max_failure_tokens: Option<usize>,
    /// Preserve full tool output in debug logs even when projected.
    pub lossless_debug: Option<bool>,
}

/// Configuration for the cache-aware context packer.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ContextPackerConfig {
    pub enabled: Option<bool>,
    pub observe_only: Option<bool>,
    pub stable_prefix: Option<bool>,
    pub max_stable_prefix_tokens: Option<usize>,
    pub max_volatile_tokens: Option<usize>,
    pub log_diagnostics: Option<bool>,
}

/// Gated active context policy modes. Defaults to Observe (no mutation).
/// Rollout: observe (diagnostics only) -> warn (logs decisions without change) -> tool_palette_reduce (first active use).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextPolicyMode {
    #[default]
    Observe,
    Warn,
    ToolPaletteReduce,
}

/// Volatile-tail compaction policy mode. Defaults to Observe (no mutation).
/// Rollout: observe (diagnostics only) -> warn (logs decisions without change) -> compact (apply tombstone replacements).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VolatileTailPolicyMode {
    #[default]
    Observe,
    Warn,
    Compact,
}

/// Configuration for gated active context policies.
/// First supported policy: deterministic tool-palette reduction driven by EffectiveCostAnalysis (ReviewToolPalette).
/// Second supported policy: volatile-tail compaction for late-context pressure relief.
/// All active behavior is disabled unless explicitly enabled with mode=tool_palette_reduce.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ContextPolicyConfig {
    /// Master enable for the policy layer. When false (default), policy decisions are Noop regardless of mode.
    pub enabled: Option<bool>,
    /// Policy mode controlling behavior: observe (log only), warn (warn on would-reduce), tool_palette_reduce (apply reductions to request.tools only).
    pub mode: Option<ContextPolicyMode>,
    /// Minimum number of cache observations (per-model ContextCacheStats entries) required before active reduction decisions.
    pub min_cache_observations: Option<usize>,
    /// If true, consider ReviewToolPalette recommendations from EffectiveCostAnalysis as a trigger for warn/reduce.
    pub review_tool_palette_threshold: Option<bool>,
    /// Hard cap on tool definitions sent to the provider when reduction is active. Required tools may cause overflow (logged, not truncated).
    pub max_tool_definitions: Option<usize>,
    /// Tool names that must always be kept if present in the filtered palette (e.g. context_read, tool_search, todowrite).
    pub always_include_tools: Option<Vec<String>>,
    /// Tool names that must never be removed by the reducer (even if not in always_include).
    pub never_reduce_tools: Option<Vec<String>>,
    /// Emit structured policy decision logs (info for decisions, debug for selected/omitted names).
    pub log_policy_decisions: Option<bool>,

    // --- Volatile-tail compaction fields ---
    /// Enable volatile-tail compaction. When false (default), no tail compaction occurs.
    pub volatile_tail_compaction: Option<bool>,
    /// Volatile-tail policy mode: observe (diagnostics only), warn (dry-run logs), compact (apply tombstone replacements).
    pub volatile_tail_mode: Option<VolatileTailPolicyMode>,
    /// Minimum total volatile-tail candidate tokens required before compaction is considered.
    pub min_volatile_tokens_for_compaction: Option<usize>,
    /// Number of recent transcript messages to preserve from compaction (always kept untouched).
    pub preserve_recent_messages: Option<usize>,
    /// Maximum tokens to compact in a single volatile-tail pass.
    pub max_compacted_tail_tokens: Option<usize>,
    /// When true, volatile-tail compaction only fires when EffectiveCostAction::CompactVolatileTailFirst is the recommended action.
    pub require_effective_cost_signal: Option<bool>,
    /// When true, only compact tool-result messages in the first pass (skip user/assistant messages).
    pub compact_tool_results_only_first: Option<bool>,
}

impl ContextPolicyConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }
    pub fn mode(&self) -> ContextPolicyMode {
        self.mode.unwrap_or_default()
    }
    pub fn min_cache_observations(&self) -> usize {
        self.min_cache_observations.unwrap_or(3)
    }
    pub fn review_tool_palette_threshold(&self) -> bool {
        self.review_tool_palette_threshold.unwrap_or(true)
    }
    pub fn max_tool_definitions(&self) -> usize {
        self.max_tool_definitions.unwrap_or(24)
    }
    pub fn always_include_tools(&self) -> Vec<String> {
        self.always_include_tools.clone().unwrap_or_else(|| {
            vec![
                "context_read".into(),
                "tool_search".into(),
                "todowrite".into(),
            ]
        })
    }
    pub fn never_reduce_tools(&self) -> Vec<String> {
        self.never_reduce_tools.clone().unwrap_or_default()
    }
    pub fn log_policy_decisions(&self) -> bool {
        self.log_policy_decisions.unwrap_or(true)
    }

    // --- Volatile-tail compaction accessors ---

    pub fn volatile_tail_compaction(&self) -> bool {
        self.volatile_tail_compaction.unwrap_or(false)
    }
    pub fn volatile_tail_mode(&self) -> VolatileTailPolicyMode {
        self.volatile_tail_mode.unwrap_or_default()
    }
    pub fn min_volatile_tokens_for_compaction(&self) -> usize {
        self.min_volatile_tokens_for_compaction.unwrap_or(12000)
    }
    pub fn preserve_recent_messages(&self) -> usize {
        self.preserve_recent_messages.unwrap_or(12)
    }
    pub fn max_compacted_tail_tokens(&self) -> usize {
        self.max_compacted_tail_tokens.unwrap_or(8000)
    }
    pub fn require_effective_cost_signal(&self) -> bool {
        self.require_effective_cost_signal.unwrap_or(true)
    }
    pub fn compact_tool_results_only_first(&self) -> bool {
        self.compact_tool_results_only_first.unwrap_or(true)
    }
}

/// Web search/fetch backend configuration.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct SearchConfig {
    /// Which backend to use.
    pub backend: Option<SearchBackendConfig>,
    /// Whether to expose raw `mcp__eggsearch__*` tools to the model.
    pub expose_raw_mcp_tools: Option<bool>,
    /// If `true`, fall back to the legacy built-in implementation when
    /// the eggsearch backend is unavailable.
    pub fallback_to_builtin: Option<bool>,
    /// Output cap for `websearch` results, in characters.
    pub max_search_output_chars: Option<usize>,
    /// Output cap for `webfetch` results, in characters.
    pub max_fetch_output_chars: Option<usize>,
    /// Output cap for repo search/fetch/map results, in characters.
    pub max_repo_output_chars: Option<usize>,
    /// Output cap for security search results, in characters.
    pub max_security_output_chars: Option<usize>,
    /// Output cap for research search results, in characters.
    pub max_research_output_chars: Option<usize>,
    /// Output cap for batch fetch results, in characters.
    pub max_batch_output_chars: Option<usize>,
    /// Output cap for evidence bundle results, in characters.
    pub max_evidence_output_chars: Option<usize>,
    /// Output cap for repo_search results, in characters.
    /// Falls back to `max_repo_output_chars` when unset.
    pub max_repo_search_output_chars: Option<usize>,
    /// Output cap for repo_fetch results, in characters.
    /// Falls back to `max_repo_output_chars` when unset.
    pub max_repo_fetch_output_chars: Option<usize>,
    /// Output cap for repo_map results, in characters.
    /// Falls back to `max_repo_output_chars` when unset.
    pub max_repo_map_output_chars: Option<usize>,
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

    pub fn max_repo_output_chars(&self) -> usize {
        self.max_repo_output_chars.unwrap_or(15_000)
    }

    pub fn max_security_output_chars(&self) -> usize {
        self.max_security_output_chars.unwrap_or(10_000)
    }

    pub fn max_research_output_chars(&self) -> usize {
        self.max_research_output_chars.unwrap_or(15_000)
    }

    pub fn max_batch_output_chars(&self) -> usize {
        self.max_batch_output_chars.unwrap_or(50_000)
    }

    pub fn max_evidence_output_chars(&self) -> usize {
        self.max_evidence_output_chars.unwrap_or(100_000)
    }

    pub fn max_repo_search_output_chars(&self) -> usize {
        self.max_repo_search_output_chars
            .unwrap_or_else(|| self.max_repo_output_chars())
    }

    pub fn max_repo_fetch_output_chars(&self) -> usize {
        self.max_repo_fetch_output_chars
            .unwrap_or_else(|| self.max_repo_output_chars())
    }

    pub fn max_repo_map_output_chars(&self) -> usize {
        self.max_repo_map_output_chars
            .unwrap_or_else(|| self.max_repo_output_chars())
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
    pub enabled: Option<bool>,
    pub server_name: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub timeout_ms: Option<u64>,
    pub repo_timeout_ms: Option<u64>,
    pub security_timeout_ms: Option<u64>,
    pub research_timeout_ms: Option<u64>,
    pub batch_fetch_timeout_ms: Option<u64>,
    pub provider_status_timeout_ms: Option<u64>,
    pub env: Option<HashMap<String, String>>,
}

/// Categorizes tool types for timeout lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTimeoutKind {
    /// Default timeout for web_search, web_fetch, repo_search, repo_fetch, repo_map.
    Default,
    /// Timeout for security_search.
    Security,
    /// Timeout for research_search.
    Research,
    /// Timeout for batch_fetch.
    BatchFetch,
    /// Timeout for provider_status (best-effort diagnostic).
    ProviderStatus,
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

    /// Return the timeout in milliseconds for the given tool kind.
    /// Falls back to the base `timeout_ms` when the specific field is unset.
    pub fn timeout_ms_for(&self, kind: ToolTimeoutKind) -> u64 {
        match kind {
            ToolTimeoutKind::Default => self.timeout_ms(),
            ToolTimeoutKind::Security => self.security_timeout_ms.unwrap_or(self.timeout_ms()),
            ToolTimeoutKind::Research => self.research_timeout_ms.unwrap_or(self.timeout_ms()),
            ToolTimeoutKind::BatchFetch => self.batch_fetch_timeout_ms.unwrap_or(self.timeout_ms()),
            ToolTimeoutKind::ProviderStatus => self.provider_status_timeout_ms.unwrap_or(15_000),
        }
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
    /// `api_key` / `encrypted_api_key` during credential resolution.
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

        // Encrypted key decryption is delegated to the root crate's crypto module.
        // The config crate does not own the crypto primitives.

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
    pub fallback_model: Option<String>,
    pub variant: Option<String>,
    pub mode: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub prompt: Option<String>,
    pub prompt_file: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub steps: Option<u32>,
    pub hidden: Option<bool>,
    pub disable: Option<bool>,
    pub permission: Option<HashMap<String, PermissionRule>>,
    pub tools: Option<HashMap<String, bool>>,
    pub options: Option<HashMap<String, serde_json::Value>>,
    pub runtime_kind: Option<String>,
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
    pub mode: Option<CompactionModeConfig>,
    pub policy: Option<CompactionPolicyConfig>,
    pub prune: Option<bool>,
    pub max_tokens: Option<usize>,
    pub threshold: Option<f64>,
    pub reserved: Option<usize>,
    pub summarize_model: Option<String>,
    pub model: Option<String>,
    pub max_tool_output_tokens: Option<usize>,
    pub max_summary_tokens: Option<usize>,
    pub max_events: Option<usize>,
    pub keep_recent_messages: Option<usize>,
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
    /// Execution runtime. `None` or `Template` uses the template path.
    pub runtime: Option<CommandRuntimeKind>,
    /// Process command executable (required when runtime = "process").
    pub command: Option<String>,
    /// Arguments for process-backed commands.
    pub args: Option<Vec<String>>,
    /// Stdin mode for process commands.
    pub stdin: Option<CommandStdinMode>,
    /// Stdout parsing mode for process commands.
    pub stdout: Option<CommandStdoutMode>,
    /// Timeout in milliseconds for process commands (default 5000).
    pub timeout_ms: Option<u64>,
    /// Working directory override for process commands.
    pub cwd: Option<String>,
    /// Extra environment variables (`KEY=VALUE`) for process commands.
    pub env: Option<Vec<String>>,
    /// Output surfaces for process command responses.
    pub output: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandRuntimeKind {
    #[default]
    Template,
    Process,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandStdinMode {
    #[default]
    None,
    Json,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandStdoutMode {
    Text,
    Json,
    #[default]
    Auto,
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

/// Restart mode as serialized in the config layer.
///
/// Mirrors the `egglsp` crate's `LspRestartModeConfig`.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LspRestartModeConfig {
    Disabled,
    OnUnexpectedExit,
}

/// Configuration for the LSP semantic memory cache.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct LspSemanticCacheConfig {
    /// Cache mode: "disabled" (default) or "memory".
    pub mode: Option<String>,
    /// Maximum number of cache entries (default 64).
    pub max_entries: Option<usize>,
    /// Maximum cache size in bytes (default 4194304 = 4 MB).
    pub max_bytes: Option<usize>,
    /// Cache TTL in seconds (default 300 = 5 minutes).
    pub ttl_seconds: Option<u64>,
}

/// Restart policy as serialized in the config layer.
///
/// Mirrors the `egglsp` crate's `LspRestartPolicyConfig`.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct LspRestartPolicyConfig {
    /// `"disabled"` or `"on_unexpected_exit"`.
    pub mode: Option<LspRestartModeConfig>,
    /// Max consecutive restart attempts before marking the server failed.
    pub max_attempts: Option<u32>,
    /// Initial backoff in milliseconds.
    pub initial_backoff_ms: Option<u64>,
    /// Maximum backoff in milliseconds.
    pub max_backoff_ms: Option<u64>,
    /// Seconds of health required before resetting the attempt counter.
    pub reset_after_healthy_secs: Option<u64>,
}

impl LspRestartPolicyConfig {
    /// Merge non-None fields from `other` into `self`.
    pub fn merge_with_profile(&mut self, other: &LspRestartPolicyConfig) {
        if other.mode.is_some() {
            self.mode.clone_from(&other.mode);
        }
        if other.max_attempts.is_some() {
            self.max_attempts.clone_from(&other.max_attempts);
        }
        if other.initial_backoff_ms.is_some() {
            self.initial_backoff_ms
                .clone_from(&other.initial_backoff_ms);
        }
        if other.max_backoff_ms.is_some() {
            self.max_backoff_ms.clone_from(&other.max_backoff_ms);
        }
        if other.reset_after_healthy_secs.is_some() {
            self.reset_after_healthy_secs
                .clone_from(&other.reset_after_healthy_secs);
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
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
        workspace_configuration: Option<HashMap<String, serde_json::Value>>,
        restart: Option<LspRestartPolicyConfig>,
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
        let paths = crate::paths::resolve_config_paths();
        if paths.is_empty() {
            tracing::warn!("No config files found, using defaults");
            return Ok(Config::default());
        }
        let configs: Result<Vec<_>, _> =
            paths.iter().map(|p| crate::paths::load_config(p)).collect();
        let configs = configs?;
        let mut config = crate::paths::merge_configs(&configs);

        // Decryption of `provider.<id>.encrypted_api_key` happens during
        // credential resolution in the providers crate via
        // `resolve_provider_credential`. The config crate intentionally
        // does not own the encryption pipeline, so this load hook is a
        // no-op.

        config.migrate();

        if let Err(errors) = config.validate() {
            let msg = errors.join("; ");
            tracing::warn!(errors = %msg, "config validation warnings");
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<(), crate::error::ConfigError> {
        let path = crate::paths::find_project_config()
            .or_else(crate::paths::global_config_path)
            .ok_or_else(|| {
                crate::error::ConfigError::Invalid("Could not determine config path to save".into())
            })?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                crate::error::ConfigError::Invalid(format!("Failed to create config dir: {}", e))
            })?;
        }

        // Provider credentials are persisted via the user credential store or
        // the typed `provider.<id>.auth.encrypted_value` field (handled by
        // the providers crate). The legacy `api_key` / `encrypted_api_key`
        // fields on `ProviderConfig` are surfaced as-is; the config crate
        // intentionally does not own the encryption pipeline.
        let to_save = self.clone();

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
                let is_process = cmd.runtime == Some(CommandRuntimeKind::Process);
                if is_process {
                    if cmd.command.is_none() {
                        errors.push(format!(
                            "command '{}' has runtime 'process' but no 'command' field",
                            name
                        ));
                    }
                } else if cmd.template.is_empty() {
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

        if let Some(ref hs) = self.human_shell {
            if let Err(hs_errors) = hs.validate() {
                errors.extend(hs_errors);
            }
        }

        if let Some(ref shell) = self.shell {
            if let Some(ref output) = shell.output {
                if let Err(output_errors) = output.validate() {
                    errors.extend(output_errors);
                }
            }
        }

        if let Some(ref dt) = self.deterministic_tools {
            if let Err(dt_errors) = dt.validate() {
                errors.extend(dt_errors);
            }
        }

        if let Some(ref pf) = self.preflight {
            if let Err(pf_errors) = pf.validate() {
                errors.extend(pf_errors);
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
    /// Trigger heuristic configuration.
    pub auto_trigger: Option<ResearchAutoTriggerConfig>,
}

/// Per-domain tool backend selection.
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
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ExternalToolBackendConfigSchema {
    /// Which backend kind to use.
    pub backend: Option<ToolImplementationBackendSchema>,
    /// Whether to expose raw `mcp__*__tool` definitions in the model
    /// catalog when a native wrapper exists.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnsiMode {
    #[default]
    SgrOnly,
    Strip,
    Raw,
}

impl AnsiMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AnsiMode::SgrOnly => "sgr-only",
            AnsiMode::Strip => "strip",
            AnsiMode::Raw => "raw",
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct HumanShellConfig {
    pub enabled: Option<bool>,
    pub default_timeout_secs: Option<u64>,
    pub max_history_entries: Option<usize>,
    pub max_bytes_per_command: Option<usize>,
    pub max_total_bytes: Option<usize>,
    pub ansi: Option<AnsiMode>,
    pub confirm_dangerous: Option<bool>,
    pub auto_promote_bangbang: Option<bool>,
}

impl HumanShellConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub fn default_timeout_secs(&self) -> u64 {
        self.default_timeout_secs.unwrap_or(300)
    }

    pub fn max_history_entries(&self) -> usize {
        self.max_history_entries.unwrap_or(100)
    }

    pub fn max_bytes_per_command(&self) -> usize {
        self.max_bytes_per_command.unwrap_or(1_000_000)
    }

    pub fn max_total_bytes(&self) -> usize {
        self.max_total_bytes.unwrap_or(8_000_000)
    }

    pub fn ansi(&self) -> AnsiMode {
        self.ansi.unwrap_or_default()
    }

    pub fn confirm_dangerous(&self) -> bool {
        self.confirm_dangerous.unwrap_or(true)
    }

    pub fn auto_promote_bangbang(&self) -> bool {
        self.auto_promote_bangbang.unwrap_or(true)
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if let Some(0) = self.default_timeout_secs {
            errors.push("human_shell.default_timeout_secs cannot be 0".to_string());
        }
        if let Some(t) = self.default_timeout_secs {
            if t > 3600 {
                errors.push("human_shell.default_timeout_secs exceeds 1 hour".to_string());
            }
        }
        if let Some(0) = self.max_history_entries {
            errors.push("human_shell.max_history_entries cannot be 0".to_string());
        }
        if let Some(n) = self.max_history_entries {
            if n > 10_000 {
                errors.push("human_shell.max_history_entries exceeds 10,000".to_string());
            }
        }
        if let Some(0) = self.max_bytes_per_command {
            errors.push("human_shell.max_bytes_per_command cannot be 0".to_string());
        }
        if let Some(b) = self.max_bytes_per_command {
            if b > 100_000_000 {
                errors.push("human_shell.max_bytes_per_command exceeds 100MB".to_string());
            }
        }
        if let Some(0) = self.max_total_bytes {
            errors.push("human_shell.max_total_bytes cannot be 0".to_string());
        }
        if let Some(b) = self.max_total_bytes {
            if b > 1_000_000_000 {
                errors.push("human_shell.max_total_bytes exceeds 1GB".to_string());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Projection policy for shell command output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionPolicyKind {
    /// Raw output only, minimal safety truncation, no redaction.
    Off,
    /// Native projectors + conservative fallbacks. Default.
    #[default]
    Safe,
    /// RTK backend (config only for now, falls back to safe).
    Rtk,
    /// Smaller budgets, more truncation.
    Aggressive,
}

/// Redaction policy for model-visible command output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionRedactPolicy {
    /// Never redact model-visible output.
    Off,
    /// Redact only for ModelContext target. Default.
    #[default]
    ModelOnly,
    /// Redact for all targets.
    All,
}

/// RTK sub-configuration (used when projection = "rtk").
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ShellOutputRtkConfig {
    pub enabled: Option<bool>,
    pub path: Option<String>,
    pub eligible_only: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub allow_side_effecting_commands: Option<bool>,
}

/// Per-command projection rule override.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ShellOutputRuleConfig {
    /// Glob pattern to match commands (e.g., "cargo test*").
    pub pattern: String,
    /// Projector name to use for matching commands.
    pub projector: Option<String>,
    /// Override max tokens for model output.
    pub max_model_output_tokens: Option<usize>,
    /// Override max bytes for output.
    pub max_output_bytes: Option<usize>,
}

/// Shell output projection configuration.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ShellOutputConfig {
    /// Projection policy: off | safe | rtk | aggressive (default: safe).
    pub projection: Option<ProjectionPolicyKind>,
    /// Whether to retain raw output for expansion handles (default: true).
    pub retain_raw: Option<bool>,
    /// Redaction policy for model-visible output (default: model_only).
    pub redact_model_visible_output: Option<ProjectionRedactPolicy>,
    /// Max tokens for model-facing projection (default: 4000).
    pub max_model_output_tokens: Option<usize>,
    /// Max bytes for TUI transcript projection (default: 200000).
    pub max_tui_output_bytes: Option<usize>,
    /// Show projection metadata in TUI (default: true).
    pub show_projection_metadata: Option<bool>,
    /// Prefer native projectors over generic fallbacks (default: true).
    pub prefer_native_projectors: Option<bool>,
    /// RTK sub-configuration (only used when projection = "rtk").
    pub rtk: Option<ShellOutputRtkConfig>,
    /// Per-command projection rules.
    pub rules: Option<Vec<ShellOutputRuleConfig>>,
}

impl ShellOutputConfig {
    pub fn projection_kind(&self) -> ProjectionPolicyKind {
        self.projection.unwrap_or_default()
    }

    pub fn retain_raw(&self) -> bool {
        self.retain_raw.unwrap_or(true)
    }

    pub fn redact_policy(&self) -> ProjectionRedactPolicy {
        self.redact_model_visible_output.unwrap_or_default()
    }

    pub fn max_model_output_tokens(&self) -> usize {
        self.max_model_output_tokens.unwrap_or(4000)
    }

    pub fn max_tui_output_bytes(&self) -> usize {
        self.max_tui_output_bytes.unwrap_or(200_000)
    }

    pub fn show_projection_metadata(&self) -> bool {
        self.show_projection_metadata.unwrap_or(true)
    }

    pub fn prefer_native_projectors(&self) -> bool {
        self.prefer_native_projectors.unwrap_or(true)
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if let Some(0) = self.max_model_output_tokens {
            errors.push("shell.output.max_model_output_tokens cannot be 0".to_string());
        }
        if let Some(t) = self.max_model_output_tokens {
            if t > 100_000 {
                errors.push("shell.output.max_model_output_tokens exceeds 100,000".to_string());
            }
        }
        if let Some(0) = self.max_tui_output_bytes {
            errors.push("shell.output.max_tui_output_bytes cannot be 0".to_string());
        }
        if let Some(b) = self.max_tui_output_bytes {
            if b > 10_000_000 {
                errors.push("shell.output.max_tui_output_bytes exceeds 10MB".to_string());
            }
        }
        if let Some(ref rtk) = self.rtk {
            if let Some(0) = rtk.timeout_ms {
                errors.push("shell.output.rtk.timeout_ms cannot be 0".to_string());
            }
            if let Some(t) = rtk.timeout_ms {
                if t > 60_000 {
                    errors.push("shell.output.rtk.timeout_ms exceeds 60s".to_string());
                }
            }
            if let Some(ref path) = rtk.path {
                if path.is_empty() {
                    errors.push("shell.output.rtk.path cannot be empty".to_string());
                }
            }
        }
        if self.projection == Some(ProjectionPolicyKind::Off) && self.retain_raw == Some(false) {
            errors.push(
                "shell.output: projection=off with retain_raw=false risks unbounded output"
                    .to_string(),
            );
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Top-level shell configuration.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ShellConfig {
    pub output: Option<ShellOutputConfig>,
}

/// Configuration for eggsact-backed deterministic tools.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct DeterministicToolsConfig {
    /// Enable deterministic tools.
    pub enabled: bool,
    /// Backend type: "native" or "disabled".
    pub backend: String,
    /// Eggsact profile name.
    pub profile: String,
    /// Tool audience for model-facing calls.
    pub model_audience: String,
    /// Tool audience for harness-side calls.
    pub harness_audience: String,
    /// Expose expert-tier tools to the model.
    pub expose_expert_tools: bool,
    /// Maximum output characters before truncation.
    pub max_output_chars: usize,
}

impl Default for DeterministicToolsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            backend: "native".to_string(),
            profile: "codegg_core".to_string(),
            model_audience: "model".to_string(),
            harness_audience: "harness".to_string(),
            expose_expert_tools: false,
            max_output_chars: 12_000,
        }
    }
}

impl DeterministicToolsConfig {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.backend != "native" && self.backend != "disabled" {
            errors.push(format!(
                "invalid deterministic_tools.backend: '{}' (expected 'native' or 'disabled')",
                self.backend
            ));
        }

        const KNOWN_PROFILES: &[&str] = &["codegg_core", "codegg_core_min", "default", "full"];
        if !KNOWN_PROFILES.contains(&self.profile.as_str()) {
            errors.push(format!(
                "unknown deterministic_tools.profile: '{}' (expected one of: {})",
                self.profile,
                KNOWN_PROFILES.join(", ")
            ));
        }

        if self.model_audience != "model" && self.model_audience != "harness" {
            errors.push(format!(
                "invalid deterministic_tools.model_audience: '{}' (expected 'model' or 'harness')",
                self.model_audience
            ));
        }
        if self.harness_audience != "harness" && self.harness_audience != "model" {
            errors.push(format!(
                "invalid deterministic_tools.harness_audience: '{}' (expected 'harness' or 'model')",
                self.harness_audience
            ));
        }

        if self.max_output_chars == 0 {
            errors.push("deterministic_tools.max_output_chars must be > 0".into());
        }
        if self.max_output_chars > 1_000_000 {
            errors.push(format!(
                "deterministic_tools.max_output_chars ({}) exceeds maximum of 1,000,000",
                self.max_output_chars
            ));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Configuration for command intent classification and routing.
///
/// **Observe-only invariant**: Setting `route_safe_commands = true` alone does
/// NOT enable active execution routing. The `mode` field controls whether
/// classification produces metadata only (Observe, default) or actively routes
/// commands to structured backends (Active).
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct CommandIntentConfig {
    /// Operating mode: `observe` (metadata only, raw shell) or `active` (active routing).
    /// Default: `observe`.
    pub mode: Option<CommandIntentMode>,
    /// Enable safe command routing (master toggle for family-level enablement).
    /// Controls metadata annotation (`routing: enabled/disabled`) in observe mode.
    /// Does NOT enable active routing — use `mode` for that.
    pub route_safe_commands: Option<bool>,
    /// Route level for test commands.
    pub route_tests: Option<RouteLevel>,
    /// Route level for read-only git commands.
    pub route_git_read: Option<RouteLevel>,
    /// Route level for safe search/list/read commands.
    pub route_search: Option<RouteLevel>,
    /// Route level for Python commands.
    pub route_python: Option<RouteLevel>,
    /// Route level for build commands (cargo build, make, etc.).
    pub route_build: Option<RouteLevel>,
    /// Route level for lint commands (cargo clippy, etc.).
    pub route_lint: Option<RouteLevel>,
    /// Route level for format commands (cargo fmt, etc.).
    pub route_format: Option<RouteLevel>,
    /// Track U: local-mutating git operations (add, commit, branch create/switch,
    /// stash push/apply/pop, restore, merge, rebase, cherry-pick, revert).
    /// Default: `off` (conservative). These operations are available via the
    /// typed `git` tool action API; command-intent routing only promotes
    /// bash-originated simple mutations when explicitly enabled.
    pub route_git_local_mutation: Option<RouteLevel>,
    /// Phase E: network git operations (fetch, pull, push, remote).
    /// Default: `off` (these operations are always available via the
    /// typed `git` tool action API; command-intent routing only routes
    /// when this is set to `observe` or `active`).
    pub route_git_network: Option<RouteLevel>,
    /// Phase E: destructive git operations (reset, clean).
    /// Default: `off`.
    pub route_git_destructive: Option<RouteLevel>,
}

impl Default for CommandIntentConfig {
    fn default() -> Self {
        Self {
            mode: Some(CommandIntentMode::Observe),
            route_safe_commands: Some(false),
            route_tests: None,
            route_git_read: None,
            route_search: None,
            route_python: None,
            route_build: None,
            route_lint: None,
            route_format: None,
            route_git_local_mutation: None,
            route_git_network: None,
            route_git_destructive: None,
        }
    }
}

impl CommandIntentConfig {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let errors = Vec::new();
        // TODO(Workstream M): Add validation rules:
        // - If any family has Active level, global mode should also be Active
        // - Warn if route_safe_commands is false but families have Active level
        // - All levels should be valid
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Returns the effective intent mode, defaulting to Observe.
    pub fn mode(&self) -> CommandIntentMode {
        self.mode.unwrap_or(CommandIntentMode::Observe)
    }

    /// Returns true if the mode is Active (or the deprecated Route alias).
    pub fn is_active_mode(&self) -> bool {
        matches!(
            self.mode(),
            CommandIntentMode::Active | CommandIntentMode::Route
        )
    }

    /// Returns true if the mode is Route (deprecated alias for Active).
    /// Prefer `is_active_mode()` for new code.
    pub fn is_route_mode(&self) -> bool {
        self.is_active_mode()
    }

    /// Returns the effective `RouteLevel` for a given family.
    ///
    /// If the family has a specific override, that is used. Otherwise,
    /// the global mode determines the default:
    /// - `Active` mode → `RouteLevel::Active`
    /// - `Observe` mode → `RouteLevel::Observe`
    pub fn family_level(&self, family: CommandIntentFamily) -> RouteLevel {
        let family_override = match family {
            CommandIntentFamily::Tests => self.route_tests,
            CommandIntentFamily::GitRead => self.route_git_read,
            CommandIntentFamily::GitLocalMutation => self.route_git_local_mutation,
            CommandIntentFamily::GitNetwork => self.route_git_network,
            CommandIntentFamily::GitDestructive => self.route_git_destructive,
            CommandIntentFamily::Search => self.route_search,
            CommandIntentFamily::Python => self.route_python,
            CommandIntentFamily::Build => self.route_build,
            CommandIntentFamily::Lint => self.route_lint,
            CommandIntentFamily::Format => self.route_format,
        };
        family_override.unwrap_or_else(|| match self.mode() {
            CommandIntentMode::Active | CommandIntentMode::Route => RouteLevel::Active,
            CommandIntentMode::Observe => RouteLevel::Observe,
        })
    }

    /// Returns true if metadata annotation is enabled for the given family.
    ///
    /// Requires `route_safe_commands = true` AND the family's effective level
    /// is not `Off`.
    pub fn is_enabled(&self, family: CommandIntentFamily) -> bool {
        let master = self.route_safe_commands.unwrap_or(false);
        if !master {
            return false;
        }
        !matches!(self.family_level(family), RouteLevel::Off)
    }

    /// Returns true if active routing is enabled for a specific family.
    ///
    /// Requires global mode == Active AND family effective level == Active.
    pub fn is_active_for_family(&self, family: CommandIntentFamily) -> bool {
        if !self.is_active_mode() {
            return false;
        }
        matches!(self.family_level(family), RouteLevel::Active)
    }
}

/// Command intent routing families for config-gated routing.
///
/// Track U split the Git families so local mutations, network operations,
/// and destructive operations can be gated independently. `GitRead` remains
/// the read-only family; the three new families cover the mutating surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandIntentFamily {
    Tests,
    GitRead,
    /// Local-mutating git operations (add, commit, branch create/switch,
    /// restore, stash push/apply/pop, merge, rebase, cherry-pick, revert).
    /// Operations that touch only the local repository — no network and no
    /// destructive scope.
    GitLocalMutation,
    /// Network git operations (fetch, pull, push, remote *).
    /// Operations that talk to a remote.
    GitNetwork,
    /// Destructive git operations (reset --hard/merge/keep, clean -f,
    /// force push, destructive branch/tag deletion).
    /// Operations that may discard history or worktree state.
    GitDestructive,
    Search,
    Python,
    Build,
    Lint,
    Format,
}

/// Per-family route level controlling how commands in a family are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RouteLevel {
    /// No routing, no metadata annotation.
    Off,
    /// Metadata annotation only (default for enabled families).
    #[default]
    Observe,
    /// Active routing to structured backends.
    Active,
}

/// Controls whether command intent classification produces metadata only (Observe)
/// or actively routes execution to structured backends (Active).
///
/// **Observe mode** (default): Classifies commands, computes routing metadata,
/// and appends it to model-visible output. All commands execute via raw shell.
///
/// **Active mode**: Routes commands to structured backends. Per-family control
/// via `RouteLevel` on each family field.
///
/// **Route** is a deprecated alias for Active, kept for deserialization
/// compatibility with existing configs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandIntentMode {
    /// Metadata only, raw shell execution. Safe default.
    #[default]
    Observe,
    /// Active routing to structured backends.
    Active,
    /// Deprecated alias for `Active`. Deserializes from `"route"` but
    /// serializes as `"active"`.
    #[serde(rename = "route", skip_serializing)]
    Route,
}

/// Configuration for harness-side eggsact preflight checks.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct PreflightConfig {
    /// Enable preflight checks.
    pub enabled: Option<bool>,
    /// Operating mode: off, observe, warn, block_on_definite.
    pub mode: Option<PreflightMode>,
    /// Enable patch/edit preflights.
    pub patch: Option<bool>,
    /// Enable config write preflights.
    pub config: Option<bool>,
    /// Enable shell command preflights.
    pub shell: Option<bool>,
    /// Enable unicode/identifier safety checks.
    pub unicode: Option<bool>,
    /// Log findings to tracing.
    pub log_findings: Option<bool>,
    /// Include findings in model-visible tool output.
    pub model_visible_findings: Option<bool>,
}

impl Default for PreflightConfig {
    fn default() -> Self {
        Self {
            enabled: Some(true),
            mode: Some(PreflightMode::Warn),
            patch: Some(true),
            config: Some(true),
            shell: Some(true),
            unicode: Some(true),
            log_findings: Some(true),
            model_visible_findings: Some(true),
        }
    }
}

impl PreflightConfig {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let errors = Vec::new();

        // PreflightMode is an enum so it always deserializes to a valid
        // variant; this block is for forward compatibility.
        if let Some(ref mode) = self.mode {
            let _ = match mode {
                PreflightMode::Off => "off",
                PreflightMode::Observe => "observe",
                PreflightMode::Warn => "warn",
                PreflightMode::BlockOnDefinite => "block_on_definite",
            };
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Preflight operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightMode {
    Off,
    Observe,
    Warn,
    BlockOnDefinite,
}

/// User-facing theme configuration.
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
    /// Minimum confidence (0.0-1.0) at which to inject the hint.
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
    use super::*;

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
            repo_timeout_ms: None,
            security_timeout_ms: None,
            research_timeout_ms: None,
            batch_fetch_timeout_ms: None,
            provider_status_timeout_ms: None,
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
        let s = cfg.search.unwrap_or_default();
        assert_eq!(s.backend(), SearchBackendConfig::Eggsearch);
    }

    #[test]
    fn explicit_mcp_eggsearch_does_not_force_search_section() {
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

    #[test]
    fn human_shell_config_valid_defaults() {
        let cfg = HumanShellConfig::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn human_shell_config_rejects_zero_timeout() {
        let cfg = HumanShellConfig {
            default_timeout_secs: Some(0),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("default_timeout_secs")));
    }

    #[test]
    fn human_shell_config_rejects_zero_history() {
        let cfg = HumanShellConfig {
            max_history_entries: Some(0),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("max_history_entries")));
    }

    #[test]
    fn human_shell_config_rejects_zero_bytes_per_command() {
        let cfg = HumanShellConfig {
            max_bytes_per_command: Some(0),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("max_bytes_per_command")));
    }

    #[test]
    fn human_shell_config_rejects_zero_total_bytes() {
        let cfg = HumanShellConfig {
            max_total_bytes: Some(0),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("max_total_bytes")));
    }

    #[test]
    fn human_shell_config_rejects_unreasonable_values() {
        let cfg = HumanShellConfig {
            default_timeout_secs: Some(7200),
            max_history_entries: Some(100_000),
            max_bytes_per_command: Some(200_000_000),
            max_total_bytes: Some(2_000_000_000),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.len() >= 4);
    }

    #[test]
    fn command_config_process_without_command_fails_validation() {
        let mut commands = std::collections::HashMap::new();
        commands.insert(
            "bad-cmd".to_string(),
            CommandConfig {
                template: String::new(),
                runtime: Some(CommandRuntimeKind::Process),
                command: None,
                ..Default::default()
            },
        );
        let cfg = Config {
            commands: Some(commands),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("no 'command' field")));
    }

    #[test]
    fn command_config_process_with_command_passes_validation() {
        let mut commands = std::collections::HashMap::new();
        commands.insert(
            "quota".to_string(),
            CommandConfig {
                template: String::new(),
                runtime: Some(CommandRuntimeKind::Process),
                command: Some("python3".to_string()),
                ..Default::default()
            },
        );
        let cfg = Config {
            commands: Some(commands),
            ..Default::default()
        };
        let result = cfg.validate();
        if let Err(errs) = result {
            assert!(
                !errs
                    .iter()
                    .any(|e| e.contains("quota") && e.contains("command")),
                "process command with command field should pass: {errs:?}"
            );
        }
    }

    #[test]
    fn command_config_template_empty_fails_validation() {
        let mut commands = std::collections::HashMap::new();
        commands.insert(
            "empty".to_string(),
            CommandConfig {
                template: String::new(),
                ..Default::default()
            },
        );
        let cfg = Config {
            commands: Some(commands),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("empty template")));
    }

    #[test]
    fn command_config_runtime_defaults_to_template() {
        let cfg = CommandConfig::default();
        assert_eq!(cfg.runtime, None);
        assert_eq!(cfg.command, None);
    }

    #[test]
    fn shell_output_config_defaults_to_safe() {
        let cfg = ShellOutputConfig::default();
        assert_eq!(cfg.projection_kind(), ProjectionPolicyKind::Safe);
        assert!(cfg.retain_raw());
        assert_eq!(cfg.redact_policy(), ProjectionRedactPolicy::ModelOnly);
        assert_eq!(cfg.max_model_output_tokens(), 4000);
        assert_eq!(cfg.max_tui_output_bytes(), 200_000);
        assert!(cfg.show_projection_metadata());
        assert!(cfg.prefer_native_projectors());
    }

    #[test]
    fn shell_output_config_deserializes_all_policies() {
        for (json, expected) in [
            (r#"{"projection": "off"}"#, ProjectionPolicyKind::Off),
            (r#"{"projection": "safe"}"#, ProjectionPolicyKind::Safe),
            (r#"{"projection": "rtk"}"#, ProjectionPolicyKind::Rtk),
            (
                r#"{"projection": "aggressive"}"#,
                ProjectionPolicyKind::Aggressive,
            ),
        ] {
            let cfg: ShellOutputConfig = serde_json::from_str(json).unwrap();
            assert_eq!(cfg.projection_kind(), expected);
        }
    }

    #[test]
    fn shell_output_config_invalid_projection_falls_back_to_default() {
        let json = r#"{"projection": "invalid"}"#;
        let result = serde_json::from_str::<ShellOutputConfig>(json);
        assert!(result.is_err());
    }

    #[test]
    fn shell_output_config_validation_rejects_zero_tokens() {
        let cfg = ShellOutputConfig {
            max_model_output_tokens: Some(0),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("max_model_output_tokens")));
    }

    #[test]
    fn shell_output_config_validation_rejects_zero_bytes() {
        let cfg = ShellOutputConfig {
            max_tui_output_bytes: Some(0),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("max_tui_output_bytes")));
    }

    #[test]
    fn shell_output_config_validation_rejects_off_with_no_retain_raw() {
        let cfg = ShellOutputConfig {
            projection: Some(ProjectionPolicyKind::Off),
            retain_raw: Some(false),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("retain_raw=false")));
    }

    #[test]
    fn shell_output_config_validation_rejects_zero_rtk_timeout() {
        let cfg = ShellOutputConfig {
            rtk: Some(ShellOutputRtkConfig {
                timeout_ms: Some(0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("rtk.timeout_ms")));
    }

    #[test]
    fn shell_output_config_validation_rejects_empty_rtk_path() {
        let cfg = ShellOutputConfig {
            rtk: Some(ShellOutputRtkConfig {
                path: Some(String::new()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let errs = cfg.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("rtk.path")));
    }

    #[test]
    fn shell_output_config_passes_validation_with_valid_values() {
        let cfg = ShellOutputConfig {
            projection: Some(ProjectionPolicyKind::Safe),
            max_model_output_tokens: Some(8000),
            max_tui_output_bytes: Some(500_000),
            rtk: Some(ShellOutputRtkConfig {
                timeout_ms: Some(5000),
                path: Some("rtk".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn shell_config_top_level_deserialization() {
        let json = r#"{
            "shell": {
                "output": {
                    "projection": "aggressive",
                    "max_model_output_tokens": 2000
                }
            }
        }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        let shell = cfg.shell.unwrap();
        let output = shell.output.unwrap();
        assert_eq!(output.projection_kind(), ProjectionPolicyKind::Aggressive);
        assert_eq!(output.max_model_output_tokens(), 2000);
    }
}
