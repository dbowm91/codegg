//! LSP - Language Server Protocol integration.
//!
//! Provides IDE-like language features:
//! - Server management (download, launch, lifecycle)
//! - Language detection and server mapping
//! - Diagnostics collection and reporting
//! - File sync (open, change, close)
//!
//! Supported servers: rust-analyzer, pyright, typescript, go, clangd, etc.

pub mod client;
pub mod diagnostics;
pub mod download;
pub mod language;
pub mod launch;
pub mod operations;
pub mod root;
pub mod server;
pub mod service;

use std::path::Path;
use std::sync::Arc;

pub use diagnostics::DiagnosticsCollector;
pub use operations::LspOperations;
pub use service::LspService;

use crate::config::schema::LspConfig;

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
        self.service.open_file(path, content).await
    }

    pub async fn update_file(
        &self,
        path: &Path,
        content: &str,
    ) -> Result<(), crate::error::LspError> {
        self.service.update_file(path, content).await
    }

    pub async fn close_file(&self, path: &Path) -> Result<(), crate::error::LspError> {
        self.service.close_file(path).await
    }

    pub async fn save_file(
        &self,
        path: &Path,
        content: Option<&str>,
    ) -> Result<(), crate::error::LspError> {
        self.service.save_file(path, content).await
    }

    pub async fn shutdown(&self) {
        self.service.shutdown_all().await
    }
}
