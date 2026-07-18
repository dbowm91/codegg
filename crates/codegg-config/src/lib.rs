pub mod encryption;
pub mod error;
pub mod paths;
pub mod schema;
pub mod watcher;

pub use error::{AppError, ConfigError};
pub use paths::{
    find_project_config, find_project_config_from, global_config_path, interpolate_env_vars,
    load_config, merge_configs, parse_config, resolve_config_paths, system_config_path,
};
pub use schema::{
    AuthConfig, Config, DiscoveryConfig, DiscoveryMode, DiscoveryRootConfig, ModelProfileConfig,
    SchedulerConfig, SchedulerFairnessConfig, SchedulerQueueConfig, SchedulerResourceConfig,
    SchedulerRolloutConfig, SymlinkPolicy,
};
pub use watcher::ConfigWatcher;
