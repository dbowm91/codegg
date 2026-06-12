use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lsp_types::*;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing::debug;

use crate::error::LspError;
use crate::service::LspService;

pub use crate::client::DiagnosticCacheEntry;

const DEBOUNCE_MS: u64 = 150;
const MAX_ENTRIES: usize = 1000;
const TTL_MS: u64 = 60_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub source: Option<String>,
    pub code: Option<String>,
}

/// How a diagnostics snapshot was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LspDiagnosticSource {
    /// Server pushed via `publishDiagnostics`.
    Pushed,
    /// Pulled via a `textDocument/diagnostic` request.
    Pulled,
    /// Unknown or mixed provenance.
    Unknown,
}

/// Freshness label for diagnostics metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LspDiagnosticFreshness {
    /// Diagnostics reflect the latest file content.
    Fresh,
    /// File content changed since the last diagnostics push; data may
    /// be stale but is still the most recent available.
    PossiblyStale,
    /// Server restarted or workspace root changed; cached diagnostics
    /// are definitively stale.
    Stale,
    /// No diagnostics are available for this file.
    Unavailable,
}

/// A diagnostics snapshot with explicit freshness metadata.
///
/// `age_ms` is the elapsed time (in milliseconds) since diagnostics were
/// received from the language server, not an absolute generation timestamp.
///
/// Consumers may display stale diagnostics with appropriate labels but
/// should never treat them as high-confidence evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnosticSnapshot {
    pub file_path: PathBuf,
    pub diagnostics: Vec<FileDiagnostic>,
    pub age_ms: i64,
    pub source: LspDiagnosticSource,
    pub freshness: LspDiagnosticFreshness,
}

impl LspDiagnosticSnapshot {
    pub fn unavailable(file_path: PathBuf) -> Self {
        Self {
            file_path,
            diagnostics: Vec::new(),
            age_ms: 0,
            source: LspDiagnosticSource::Unknown,
            freshness: LspDiagnosticFreshness::Unavailable,
        }
    }

    pub fn is_usable_evidence(&self) -> bool {
        matches!(
            self.freshness,
            LspDiagnosticFreshness::Fresh | LspDiagnosticFreshness::PossiblyStale
        )
    }

    pub fn diagnostics_may_still_be_warming(&self) -> bool {
        matches!(
            self.freshness,
            LspDiagnosticFreshness::PossiblyStale
        ) && self.diagnostics.is_empty()
    }
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

        let snapshot = self
            .service
            .get_diagnostic_snapshot_for_key(&key, &uri_str)
            .await?;

        let warming = snapshot.diagnostics_may_still_be_warming();

        Ok(DiagnosticsOutput {
            diagnostics_may_still_be_warming: warming,
            diagnostics: snapshot.diagnostics,
        })
    }

    /// Returns a legacy freshness-blind bulk view of diagnostics.
    ///
    /// This method does not include freshness metadata. Callers that need
    /// reliability metadata (freshness, age_ms, source) should use
    /// [`get_all_diagnostic_snapshots`] or per-file snapshots instead.
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

    pub async fn get_all_diagnostic_snapshots(
        &self,
    ) -> Result<HashMap<String, LspDiagnosticSnapshot>, LspError> {
        let keys = self.service.client_keys().await;
        let mut snapshots = HashMap::new();

        for key in keys {
            let raw = self.service.get_all_diagnostics_for_key(&key).await?;
            for uri in raw.keys() {
                let snapshot = self
                    .service
                    .get_diagnostic_snapshot_for_key(&key, uri)
                    .await?;
                snapshots.insert(uri.clone(), snapshot);
            }
        }

        Ok(snapshots)
    }

    pub async fn get_diagnostic_snapshot_for_file(
        &self,
        file_path: &Path,
    ) -> Result<LspDiagnosticSnapshot, LspError> {
        let (key, uri_str) = self.service.ensure_file_open_from_disk(file_path).await?;
        self.service
            .get_diagnostic_snapshot_for_key(&key, &uri_str)
            .await
    }

    pub async fn has_errors(&self, file_path: &Path) -> Result<bool, LspError> {
        let output = self.get_diagnostics_for_file(file_path).await?;
        Ok(output
            .diagnostics
            .iter()
            .any(|d| d.severity >= DiagnosticSeverity::ERROR))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn snapshot_unavailable_has_zero_age() {
        let snap = LspDiagnosticSnapshot::unavailable(PathBuf::from("/tmp/test.rs"));
        assert_eq!(snap.age_ms, 0);
        assert_eq!(snap.freshness, LspDiagnosticFreshness::Unavailable);
        assert!(snap.diagnostics.is_empty());
        assert_eq!(snap.source, LspDiagnosticSource::Unknown);
    }

    #[test]
    fn usable_evidence_fresh_and_possibly_stale() {
        let fresh = LspDiagnosticSnapshot {
            file_path: PathBuf::from("/tmp/a.rs"),
            diagnostics: vec![],
            age_ms: 0,
            source: LspDiagnosticSource::Pushed,
            freshness: LspDiagnosticFreshness::Fresh,
        };
        assert!(fresh.is_usable_evidence());

        let possibly_stale = LspDiagnosticSnapshot {
            file_path: PathBuf::from("/tmp/b.rs"),
            diagnostics: vec![],
            age_ms: 100,
            source: LspDiagnosticSource::Pushed,
            freshness: LspDiagnosticFreshness::PossiblyStale,
        };
        assert!(possibly_stale.is_usable_evidence());

        let stale = LspDiagnosticSnapshot {
            file_path: PathBuf::from("/tmp/c.rs"),
            diagnostics: vec![],
            age_ms: 0,
            source: LspDiagnosticSource::Pushed,
            freshness: LspDiagnosticFreshness::Stale,
        };
        assert!(!stale.is_usable_evidence());

        let unavailable = LspDiagnosticSnapshot::unavailable(PathBuf::from("/tmp/d.rs"));
        assert!(!unavailable.is_usable_evidence());
    }

    #[test]
    fn snapshot_age_is_non_negative() {
        let snap = LspDiagnosticSnapshot::unavailable(PathBuf::from("/tmp/test.rs"));
        assert!(snap.age_ms >= 0);
    }
}
