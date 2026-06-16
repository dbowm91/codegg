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
///
/// `server_generation` and `post_restart` carry the per-client
/// generation metadata introduced in Pass 5 (Phase 17). `None`/`false`
/// for snapshots synthesized manually (e.g. tests) and for
/// `Unavailable` snapshots where the underlying entry is absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnosticSnapshot {
    pub file_path: PathBuf,
    pub diagnostics: Vec<FileDiagnostic>,
    pub age_ms: i64,
    pub source: LspDiagnosticSource,
    pub freshness: LspDiagnosticFreshness,
    /// Server generation that produced these diagnostics.
    ///
    /// `None` when no cache entry exists (e.g. on `Unavailable`
    /// snapshots) or when constructed manually. Otherwise the
    /// authoritative `server_generation` of the cache entry at the
    /// time the snapshot was built.
    #[serde(default)]
    pub server_generation: Option<u64>,
    /// `true` when these diagnostics were produced by a server that
    /// has been restarted at least once since the start of this
    /// client key. See `DiagnosticCacheEntry::post_restart` for the
    /// authoritative definition.
    #[serde(default)]
    pub post_restart: bool,
}

impl Default for LspDiagnosticSnapshot {
    fn default() -> Self {
        Self {
            file_path: PathBuf::new(),
            diagnostics: Vec::new(),
            age_ms: 0,
            source: LspDiagnosticSource::Unknown,
            freshness: LspDiagnosticFreshness::Unavailable,
            server_generation: None,
            post_restart: false,
        }
    }
}

impl LspDiagnosticSnapshot {
    pub fn unavailable(file_path: PathBuf) -> Self {
        Self {
            file_path,
            diagnostics: Vec::new(),
            age_ms: 0,
            source: LspDiagnosticSource::Unknown,
            freshness: LspDiagnosticFreshness::Unavailable,
            server_generation: None,
            post_restart: false,
        }
    }

    /// Build a new snapshot with `server_generation` overwritten to
    /// `generation`. The new snapshot's `post_restart` is preserved
    /// from the input. Other fields are copied verbatim.
    ///
    /// Used by the restart coordinator to mark retained diagnostics
    /// as belonging to the previous generation so the freshness
    /// classifier returns [`LspDiagnosticFreshness::Stale`] until the
    /// new server emits its first push.
    pub fn with_generation(snap: LspDiagnosticSnapshot, generation: u64) -> Self {
        Self {
            file_path: snap.file_path,
            diagnostics: snap.diagnostics,
            age_ms: snap.age_ms,
            source: snap.source,
            freshness: snap.freshness,
            server_generation: Some(generation),
            post_restart: snap.post_restart,
        }
    }

    pub fn is_usable_evidence(&self) -> bool {
        matches!(
            self.freshness,
            LspDiagnosticFreshness::Fresh | LspDiagnosticFreshness::PossiblyStale
        )
    }

    pub fn diagnostics_may_still_be_warming(&self) -> bool {
        matches!(self.freshness, LspDiagnosticFreshness::PossiblyStale)
            && self.diagnostics.is_empty()
    }

    /// Returns `age_ms` as a non-negative `u64`.
    ///
    /// `LspDiagnosticSnapshot::age_ms` is `i64` because the underlying
    /// timing primitive can yield a negative duration on some
    /// platforms (e.g. when the monotonic clock is observed to step
    /// backwards). Consumers that want a non-negative age should
    /// call this helper instead of casting directly.
    pub fn age_ms_or_default(&self) -> u64 {
        self.age_ms.max(0) as u64
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
            server_generation: None,
            post_restart: false,
        };
        assert!(fresh.is_usable_evidence());

        let possibly_stale = LspDiagnosticSnapshot {
            file_path: PathBuf::from("/tmp/b.rs"),
            diagnostics: vec![],
            age_ms: 100,
            source: LspDiagnosticSource::Pushed,
            freshness: LspDiagnosticFreshness::PossiblyStale,
            server_generation: None,
            post_restart: false,
        };
        assert!(possibly_stale.is_usable_evidence());

        let stale = LspDiagnosticSnapshot {
            file_path: PathBuf::from("/tmp/c.rs"),
            diagnostics: vec![],
            age_ms: 0,
            source: LspDiagnosticSource::Pushed,
            freshness: LspDiagnosticFreshness::Stale,
            server_generation: None,
            post_restart: false,
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

    #[test]
    fn age_ms_or_default_is_non_negative() {
        let snap = LspDiagnosticSnapshot {
            file_path: PathBuf::from("/tmp/a.rs"),
            diagnostics: vec![],
            age_ms: -10, // degenerate: should not happen, but be safe
            source: LspDiagnosticSource::Pushed,
            freshness: LspDiagnosticFreshness::Fresh,
            server_generation: None,
            post_restart: false,
        };
        assert_eq!(snap.age_ms_or_default(), 0);

        let snap = LspDiagnosticSnapshot {
            file_path: PathBuf::from("/tmp/b.rs"),
            diagnostics: vec![],
            age_ms: 1234,
            source: LspDiagnosticSource::Pushed,
            freshness: LspDiagnosticFreshness::Fresh,
            server_generation: None,
            post_restart: false,
        };
        assert_eq!(snap.age_ms_or_default(), 1234);
    }

    #[test]
    fn snapshot_carries_generation_metadata() {
        // Verify that the new fields are wired through the public
        // surface: a manually-constructed snapshot carries the
        // generation/post_restart fields verbatim, and serde
        // default-missing fields round-trip cleanly.
        let snap = LspDiagnosticSnapshot {
            file_path: PathBuf::from("/tmp/x.rs"),
            diagnostics: vec![],
            age_ms: 0,
            source: LspDiagnosticSource::Pushed,
            freshness: LspDiagnosticFreshness::Fresh,
            server_generation: Some(7),
            post_restart: true,
        };
        assert_eq!(snap.server_generation, Some(7));
        assert!(snap.post_restart);

        // Default has no generation and is not post-restart.
        let default_snap = LspDiagnosticSnapshot::default();
        assert_eq!(default_snap.server_generation, None);
        assert!(!default_snap.post_restart);
        assert_eq!(default_snap.freshness, LspDiagnosticFreshness::Unavailable);

        // Unavailable snapshots have None/False metadata.
        let unavailable = LspDiagnosticSnapshot::unavailable(PathBuf::from("/tmp/y.rs"));
        assert_eq!(unavailable.server_generation, None);
        assert!(!unavailable.post_restart);

        // Serde round-trip with `#[serde(default)]` on the new
        // fields — deserializing JSON without the new fields
        // succeeds and yields None/false.
        let legacy_json = serde_json::json!({
            "file_path": "/tmp/z.rs",
            "diagnostics": [],
            "age_ms": 0,
            "source": "Pushed",
            "freshness": "Fresh"
        })
        .to_string();
        let parsed: LspDiagnosticSnapshot =
            serde_json::from_str(&legacy_json).expect("legacy payload should deserialize");
        assert_eq!(parsed.server_generation, None);
        assert!(!parsed.post_restart);
    }

    #[test]
    fn with_generation_returns_new_snapshot() {
        // `with_generation` must produce a snapshot with the new
        // generation, preserved diagnostics/source/freshness, and
        // preserved post_restart flag.
        let snap = LspDiagnosticSnapshot {
            file_path: PathBuf::from("/tmp/g.rs"),
            diagnostics: vec![],
            age_ms: 0,
            source: LspDiagnosticSource::Pushed,
            freshness: LspDiagnosticFreshness::PossiblyStale,
            server_generation: Some(2),
            post_restart: true,
        };
        let updated = LspDiagnosticSnapshot::with_generation(snap, 9);
        assert_eq!(updated.server_generation, Some(9));
        assert!(updated.post_restart);
        assert_eq!(updated.freshness, LspDiagnosticFreshness::PossiblyStale);
        assert_eq!(updated.source, LspDiagnosticSource::Pushed);
        assert_eq!(updated.file_path, PathBuf::from("/tmp/g.rs"));

        // Input is moved (by value), so the post_restart is
        // preserved.
        let snap2 = LspDiagnosticSnapshot {
            file_path: PathBuf::from("/tmp/h.rs"),
            diagnostics: vec![],
            age_ms: 0,
            source: LspDiagnosticSource::Pushed,
            freshness: LspDiagnosticFreshness::Stale,
            server_generation: None,
            post_restart: false,
        };
        let updated2 = LspDiagnosticSnapshot::with_generation(snap2, 3);
        assert_eq!(updated2.server_generation, Some(3));
        assert!(!updated2.post_restart);
        assert_eq!(updated2.freshness, LspDiagnosticFreshness::Stale);
    }
}
