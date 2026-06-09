//! Language Server Protocol client crate.
//!
//! This crate owns the LSP client, server registry, file sync, and
//! diagnostics collection. Codegg wires this behind its native `lsp`
//! tool and config types via `From` impls at the boundary.

pub mod client;
pub mod config;
pub mod diagnostics;
pub mod download;
pub mod error;
pub mod language;
pub mod launch;
pub mod operations;
pub mod root;
pub mod server;
pub mod service;

pub use config::{LspConfig, LspRule};
pub use diagnostics::DiagnosticsCollector;
pub use error::LspError;
pub use operations::LspOperations;
pub use service::LspService;

pub use lsp_types;
