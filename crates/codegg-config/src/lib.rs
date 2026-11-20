#[cfg(test)]
mod config_catalog;
pub mod document;
pub mod encryption;
pub mod error;
pub mod paths;
pub mod provenance;
pub mod schema;
pub mod watcher;

pub use document::{
    parse_yaml, DocumentErrorClass, DocumentFormat, DocumentParseError, StructuredValue,
};
pub use error::{AppError, ConfigError};
pub use paths::{
    find_project_config, find_project_config_from, global_config_path, interpolate_env_vars,
    load_config, load_config_document, merge_configs, parse_config, parse_config_document,
    resolve_config_layer_paths, resolve_config_paths, system_config_path, ConfigLayerKind,
};
pub use provenance::{
    analyze, analyze_with, is_credential_path, load_layers, render_json, render_text, strategy_for,
    AttributionRule, Entry, Layer, LayerContents, LayerStatus, MergeShape, ProvenanceError, Report,
    ShadowedValue, StrategyEntry, CREDENTIAL_CONTAINERS, CREDENTIAL_FIELDS, CREDENTIAL_NAMES,
    STRATEGY_TABLE,
};
pub use schema::{
    AuthConfig, Config, DiscoveryConfig, DiscoveryMode, DiscoveryRootConfig, EggworkConfig,
    EggworkIsolationPolicy, EggworkNetworkPolicy, EggworkNodeProfile, ModelProfileConfig,
    ModelRouteConfig, ModelRouterConfig, OrchestrationConfig, OrchestrationTier,
    ReasoningCapabilityRecord, ReasoningCapabilityStatus, SchedulerConfig, SchedulerFairnessConfig,
    SchedulerQueueConfig, SchedulerQuotaConfig, SchedulerResourceConfig, SchedulerRolloutConfig,
    ScratchConfig, ScratchRetentionSettings, SymlinkPolicy, ToolAdvisorConfig,
    ToolAdvisorRemoteConfig, ToolAdvisorTrainingDataConfig, MAX_REASONING_CAPABILITY_LABELS,
    MAX_REASONING_CAPABILITY_LABEL_BYTES, SCRATCH_ORPHAN_GRACE_HOURS_DEFAULT,
    SCRATCH_ORPHAN_GRACE_HOURS_FLOOR, SCRATCH_RETENTION_DAYS_DEFAULT, SCRATCH_RETENTION_DAYS_FLOOR,
    SCRATCH_SWEEP_INTERVAL_MINUTES_DEFAULT, SCRATCH_SWEEP_INTERVAL_MINUTES_FLOOR,
    VIRTUAL_MODEL_PREFIX,
};
pub use watcher::ConfigWatcher;

/// Process-wide lock for the tests that mutate the environment.
///
/// The environment is process-global, so two tests that set or remove a
/// variable must not overlap — not even when they set different variables,
/// because the environment block is shared. Every env-mutating test in this
/// crate takes this lock.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
