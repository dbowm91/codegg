//! Configuration loading, validation, and watching.
//!
//! This module handles loading configuration from various sources including
//! global config, project config, and environment variables. It also provides
//! configuration watching for hot-reloading during development.

pub mod encryption;
pub mod paths;
pub mod schema;
pub mod watcher;

pub use paths::{
    find_project_config, find_project_config_from, find_tui_config, global_config_path,
    interpolate_env_vars, load_config, load_tui_config, merge_configs, parse_config,
    resolve_config_paths, system_config_path,
};
pub use schema::Config;
pub use watcher::ConfigWatcher;
