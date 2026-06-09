use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use lsp_types::*;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing::debug;

use crate::error::LspError;
use crate::service::LspService;

const DEBOUNCE_MS: u64 = 150;
const MAX_ENTRIES: usize = 1000;
const TTL_MS: u64 = 60_000;

#[derive(Debug, Clone)]
pub struct FileDiagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub source: Option<String>,
    pub code: Option<String>,
}

pub struct DiagnosticsOutput {
    pub diagnostics_may_still_be_warming: bool,
    pub diagnostics: Vec<FileDiagnostic>,
}

pub struct DiagnosticsCollector {
    service: Arc<LspService>,
    last_update: Arc<Mutex<HashMap<String, Instant>>>,
}

impl DiagnosticsCollector {
    pub fn new(service: Arc<LspService>) -> Self {
        Self {
            service,
            last_update: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn should_debounce(&self, uri: &str) -> bool {
        let mut last = self.last_update.lock().await;
        let now = Instant::now();

        last.retain(|_, instant| now.duration_since(*instant) < Duration::from_millis(TTL_MS));

        if last.len() >= MAX_ENTRIES {
            let oldest_instant = last.values().min().copied();
            if let Some(oldest) = oldest_instant {
                last.retain(|_, instant| *instant != oldest);
            }
        }

        if let Some(prev) = last.get(uri) {
            if now.duration_since(*prev) < Duration::from_millis(DEBOUNCE_MS) {
                return true;
            }
        }

        last.insert(uri.to_string(), now);
        false
    }

    pub async fn get_diagnostics_for_file(
        &self,
        file_path: &Path,
    ) -> Result<DiagnosticsOutput, LspError> {
        let (key, uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;

        if self.should_debounce(&uri_str).await {
            debug!(uri = %uri_str, "debouncing diagnostics");
            return Ok(DiagnosticsOutput {
                diagnostics_may_still_be_warming: false,
                diagnostics: Vec::new(),
            });
        }

        let warming = self
            .service
            .diagnostics_may_still_be_warming(&key, &uri_str)
            .await;

        let raw = self.service.get_diagnostics_for_key(&key, &uri_str).await?;

        Ok(DiagnosticsOutput {
            diagnostics_may_still_be_warming: warming,
            diagnostics: raw
                .into_iter()
                .map(|d| FileDiagnostic {
                    file: uri_str.clone(),
                    line: d.range.start.line,
                    column: d.range.start.character,
                    message: d.message,
                    severity: d.severity.unwrap_or(DiagnosticSeverity::ERROR),
                    source: d.source,
                    code: d.code.as_ref().map(|c| match c {
                        NumberOrString::Number(n) => n.to_string(),
                        NumberOrString::String(s) => s.clone(),
                    }),
                })
                .collect(),
        })
    }

    pub async fn get_all_diagnostics(
        &self,
    ) -> Result<HashMap<String, Vec<FileDiagnostic>>, LspError> {
        let keys = self.service.client_keys().await;
        let mut all = HashMap::new();

        for key in keys {
            let raw = self.service.get_all_diagnostics_for_key(&key).await?;
            for (uri, ds) in raw {
                let fds: Vec<FileDiagnostic> = ds
                    .into_iter()
                    .map(|d| FileDiagnostic {
                        file: uri.clone(),
                        line: d.range.start.line,
                        column: d.range.start.character,
                        message: d.message,
                        severity: d.severity.unwrap_or(DiagnosticSeverity::ERROR),
                        source: d.source,
                        code: d.code.as_ref().map(|c| match c {
                            NumberOrString::Number(n) => n.to_string(),
                            NumberOrString::String(s) => s.clone(),
                        }),
                    })
                    .collect();
                all.entry(uri).or_insert_with(Vec::new).extend(fds);
            }
        }

        Ok(all)
    }

    pub async fn has_errors(&self, file_path: &Path) -> Result<bool, LspError> {
        let output = self.get_diagnostics_for_file(file_path).await?;
        Ok(output
            .diagnostics
            .iter()
            .any(|d| d.severity >= DiagnosticSeverity::ERROR))
    }
}
