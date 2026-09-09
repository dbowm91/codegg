//! Agent definitions and management.
//!
//! This module provides the core Agent struct and built-in agent configurations.
//! Agents define how the AI assistant behaves, including permissions, model selection,
//! and system prompts. Codegg supports multiple agent modes: Primary (full access),
//! Subagent (limited), and All (combines multiple agents).
//!
//! Module-root responsibility (M003): this file is the declaration,
//! re-export, and composition surface only. Agent definition/resolution
//! lives in [`definition`], file-based loading in [`file_agents`], and the
//! turn orchestrator in [`r#loop`].

pub mod agent_loop_factory;
pub mod asset_context;
pub mod asset_refresh;
pub mod asset_snapshot;
pub mod asset_snapshot_builder;
pub mod builtins;
pub mod compaction;
pub mod context_frame;
mod context_runtime;
pub mod convergence;
mod coordinator;
pub mod definition;
pub mod file_agents;
mod follow_up;
mod habit_observation;
pub mod instructions;
pub mod r#loop;
mod loop_output;
pub mod mention;
pub mod policy;
pub mod processor;
pub mod progress_recovery;
pub mod prompt;
mod provider_turn;
pub mod registry;
mod request_preparation;
pub mod router;
pub mod run_control;
pub mod run_integration;
mod snapshot_capture;
pub mod specialized_runtime;
pub mod task_tool_runtime;
mod tool_batch;
mod tool_inspect;
pub mod tool_program_recovery;
pub mod tool_surface;
mod turn_completion;
pub mod turn_runtime;
pub mod worker;

use crate::config::schema::Config;
use crate::error::AgentError;
pub use definition::{
    builtin_agents, is_model_alias, resolve_agents_with_context, resolve_model_alias, Agent,
    AgentMode, AgentRuntimeKind, ResolvedAgentExecutionProfile, EMERGENCY_DEFAULT_MODEL,
    EMERGENCY_DEFAULT_WORKHORSE_MODEL, MODEL_ALIAS_FRONTIER, MODEL_ALIAS_WORKHORSE,
};
pub use file_agents::{
    find_agent_by_name, find_default_agent, list_visible_agents, load_agent_from_file,
    load_agent_from_toml, load_agents_from_dir, AgentPermissionSpec, BashPermissionSpec, FileAgent,
    OverlayFlags, PathPermissionSpec,
};

pub fn resolve_agents(config: &Config) -> Result<Vec<Agent>, AgentError> {
    // CLI bootstrap path. Reads cwd exactly once at this boundary so
    // the registry no longer reads process-global state. Daemon code
    // must call `resolve_agents_with_context` with an explicit context.
    let project_root = std::env::current_dir().ok();
    resolve_agents_with_context(config, project_root.as_deref())
}
