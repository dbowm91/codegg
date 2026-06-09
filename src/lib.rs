#![deny(unsafe_code)]

pub mod agent;
pub mod auth;
pub mod crypto;
pub mod exec;
pub mod hooks;
pub mod ide;
pub mod memory;
pub mod model_profile;
pub mod security;
pub mod tts;

pub use tts::TtsEngine;
pub mod bus;
#[cfg(feature = "server")]
pub mod client;
pub mod command;
pub use codegg_config as config;
pub mod core;
pub mod error;
pub mod goal;
pub mod lsp;
pub mod mcp;
pub mod permission;
pub mod plugin;
pub mod protocol;
pub use codegg_providers as provider;
pub mod research;
pub mod resilience;
pub mod search;
pub mod search_backend;
#[cfg(feature = "server")]
pub mod server;
pub mod session;
pub mod shell_session;
pub mod skills;
pub mod snapshot;
pub mod storage;
pub mod task_state;
pub mod theme;
pub mod tool;
pub mod tui;
pub mod upgrade;
pub mod util;
pub mod worktree;
