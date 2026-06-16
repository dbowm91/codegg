//! LSP - Language Server Protocol integration.
//!
//! The actual client and service live in the `egglsp` crate. This
//! module is a **compatibility shim** that re-exports the entire
//! `egglsp` module tree so existing call sites that used
//! `crate::lsp::service::LspService` and
//! `crate::lsp::language::extension_to_language_id` continue to
//! work. New code should prefer direct `egglsp::...` imports. The
//! shim is intentionally narrow: a `From<egglsp::LspError>` for
//! `crate::error::LspError`, the legacy `Lsp` aggregator struct,
//! and the `crate::config::schema::LspConfig` → `egglsp::LspConfig`
//! `From` impl. See `plans/native_tool_crates_hardening.md` Phase 7.

use std::path::Path;
use std::sync::Arc;

pub mod client {
    pub use egglsp::client::*;
}
pub mod config {
    pub use egglsp::config::*;
}
pub mod diagnostics {
    pub use egglsp::diagnostics::*;
}
pub mod download {
    pub use egglsp::download::*;
}
pub mod edit {
    pub use egglsp::edit::*;
}
pub mod language {
    pub use egglsp::language::*;
}
pub mod launch {
    pub use egglsp::launch::*;
}
pub mod operations {
    pub use egglsp::operations::*;
}
pub mod root {
    pub use egglsp::root::*;
}
pub mod server {
    pub use egglsp::server::*;
}
pub mod overlay {
    pub use egglsp::overlay::*;
}
pub mod hunk_nav;
pub mod hunk_nav_collector;
pub mod hunk_nav_parser;
pub mod hunk_nav_policy;
pub mod hunk_nav_prompt;
pub mod hunk_nav_ranges;
pub mod semantic_context;
pub mod service {
    pub use egglsp::service::*;
}

pub use egglsp::config::{LspConfig, LspRule};
pub use egglsp::diagnostics::DiagnosticsCollector;
pub use egglsp::error::LspError;
pub use egglsp::operations::LspOperations;
pub use egglsp::service::LspService;

pub use egglsp::lsp_types;

pub fn config_lsp_to_egglsp(c: crate::config::schema::LspConfig) -> egglsp::LspConfig {
    // The shapes are intentionally identical; serde round-trips both.
    serde_json::from_value(serde_json::to_value(c).unwrap_or_default()).unwrap_or_default()
}

pub struct Lsp {
    pub service: Arc<LspService>,
    pub operations: Arc<LspOperations>,
    pub diagnostics: Arc<DiagnosticsCollector>,
}

impl Lsp {
    pub fn new(config: LspConfig) -> Self {
        let service = LspService::new_arc(config);
        let operations = Arc::new(LspOperations::new(service.clone()));
        let diagnostics = Arc::new(DiagnosticsCollector::new(service.clone()));

        Self {
            service,
            operations,
            diagnostics,
        }
    }

    pub async fn open_file(
        &self,
        path: &Path,
        content: &str,
    ) -> Result<(), crate::error::LspError> {
        self.service
            .open_file(path, content)
            .await
            .map_err(Into::into)
    }

    pub async fn update_file(
        &self,
        path: &Path,
        content: &str,
    ) -> Result<(), crate::error::LspError> {
        self.service
            .update_file(path, content)
            .await
            .map_err(Into::into)
    }

    pub async fn close_file(&self, path: &Path) -> Result<(), crate::error::LspError> {
        self.service.close_file(path).await.map_err(Into::into)
    }

    pub async fn save_file(
        &self,
        path: &Path,
        content: Option<&str>,
    ) -> Result<(), crate::error::LspError> {
        self.service
            .save_file(path, content)
            .await
            .map_err(Into::into)
    }

    pub async fn shutdown(&self) {
        self.service.shutdown_all().await
    }
}
