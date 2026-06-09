#![deny(unsafe_code)]

pub mod agent;
pub mod auth;
pub mod crypto;
pub mod exec;
pub mod hooks;
pub mod ide;
pub use codegg_core::memory;
pub use codegg_core::model_profile;
pub mod security;
pub mod tts;

pub use tts::TtsEngine;
pub use codegg_core::bus;
#[cfg(feature = "server")]
pub mod client;
pub mod command;
pub use codegg_config as config;
pub mod core;
pub mod error;
pub use codegg_core::goal;
pub mod lsp;
pub mod mcp;
pub mod permission;
pub mod plugin;
pub use codegg_protocol as protocol;
pub mod protocol_conversions;
pub use codegg_providers as provider;
pub mod research;
pub use codegg_core::resilience;
pub mod search;
pub mod search_backend;
#[cfg(feature = "server")]
pub mod server;
pub use codegg_core::session;
pub mod shell_session;
pub mod skills;
pub use codegg_core::snapshot;
pub use codegg_core::storage;
pub use codegg_core::task_state;
pub mod theme;
pub mod tool;
pub mod tui;
pub mod upgrade;
pub mod util;
pub use codegg_core::worktree;
