#![deny(unsafe_code)]

pub mod agent;
pub mod crypto;
pub mod exec;
pub mod hooks;
pub mod ide;
pub mod memory;
pub mod security;
pub mod tts;

pub use tts::TtsEngine;
pub mod bus;
#[cfg(feature = "server")]
pub mod client;
pub mod command;
pub mod config;
pub mod error;
pub mod lsp;
pub mod mcp;
pub mod permission;
pub mod plugin;
pub mod provider;
pub mod pty;
pub mod resilience;
#[cfg(feature = "server")]
pub mod server;
pub mod session;
pub mod skills;
pub mod snapshot;
pub mod storage;
pub mod tool;
pub mod tui;
pub mod upgrade;
pub mod util;
pub mod worktree;
