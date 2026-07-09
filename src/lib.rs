#![deny(unsafe_code)]

// Extracted workspace crate re-exports.
pub use codegg_config as config;
pub use codegg_protocol as protocol;
pub use codegg_providers as provider;

// Extracted core modules re-exported for root compatibility.
pub use codegg_core::{
    bus, goal, memory, model_profile, resilience, session, snapshot, storage, task_state, worktree,
};

pub mod agent;
pub mod auth;
pub mod command;
pub mod command_intent;
pub mod command_planner;
pub mod command_routing;
pub mod context;
pub mod core;
pub mod eggsact;
pub mod error;
pub mod exec;
pub mod hooks;
pub mod ide;
pub mod lsp;
pub mod mcp;
pub mod permission;
pub mod plugin;
pub mod preflight;
pub mod protocol_conversions;
pub mod python_scripting;
pub mod research;
pub mod search;
pub mod search_backend;
pub mod security;
pub mod shell;
pub mod shell_session;
pub mod skills;
pub mod test_runner;
pub mod theme;
pub mod tool;
pub mod tts;
pub mod tui;
pub mod upgrade;
pub mod util;

pub use tts::TtsEngine;

#[cfg(feature = "server")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;
