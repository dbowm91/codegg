//! LSP - Language Server Protocol integration.
//!
//! The actual client and service live in the `egglsp` crate. This
//! module re-exports the entire `egglsp` module tree so existing
//! call sites that used `crate::lsp::service::LspService` and
//! `crate::lsp::language::extension_to_language_id` continue to
//! work.

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
pub mod service {
    pub use egglsp::service::*;
}

pub use egglsp::config::{LspConfig, LspRule};
pub use egglsp::diagnostics::DiagnosticsCollector;
pub use egglsp::error::LspError;
pub use egglsp::operations::LspOperations;
pub use egglsp::service::LspService;

impl From<crate::config::schema::LspConfig> for egglsp::LspConfig {
    fn from(c: crate::config::schema::LspConfig) -> Self {
        // The shapes are intentionally identical; serde round-trips both.
        serde_json::from_value(serde_json::to_value(c).unwrap_or_default())
            .unwrap_or_default()
    }
}

pub struct Lsp {
    pub service: Arc<LspService>,
    pub operations: Arc<LspOperations>,
    pub diagnostics: Arc<DiagnosticsCollector>,
}

impl Lsp {
    pub fn new(config: LspConfig) -> Self {
        let service = Arc::new(LspService::new(config));
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

impl From<egglsp::LspError> for crate::error::LspError {
    fn from(e: egglsp::LspError) -> Self {
        match e {
            egglsp::LspError::ServerNotFound(s) => crate::error::LspError::ServerNotFound(s),
            egglsp::LspError::DownloadFailed(s) => crate::error::LspError::DownloadFailed(s),
            egglsp::LspError::LaunchFailed(s) => crate::error::LspError::LaunchFailed(s),
            egglsp::LspError::NotInitialized(s) => crate::error::LspError::NotInitialized(s),
            egglsp::LspError::RequestFailed(s) => crate::error::LspError::RequestFailed(s),
            egglsp::LspError::RequestTimeout(s) => crate::error::LspError::RequestTimeout(s),
            egglsp::LspError::UnsupportedLanguage(s) => {
                crate::error::LspError::UnsupportedLanguage(s)
            }
            egglsp::LspError::Io(e) => crate::error::LspError::Io(e),
            egglsp::LspError::Json(e) => crate::error::LspError::Json(e),
        }
    }
}
