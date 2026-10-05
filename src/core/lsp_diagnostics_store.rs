//! M006-B: digest-keyed diagnostics tracking (ADR-0012 §1–2).
//!
//! The publisher's job is narrow and its correctness property is the whole
//! point of the design: **publish only when the diagnostic content actually
//! changed**.
//!
//! It does that by keeping, per tracked file, the digest of the last
//! *published* set and a monotonic sequence. A tick whose digest matches
//! publishes nothing. That is what lets a dropped envelope be recoverable:
//! `publishDiagnostics` replaces a document's entire set, so every envelope
//! carries the complete current state, and a client that sees a `sequence`
//! gap knows to re-pull rather than trusting a partial view.
//!
//! The change signal deliberately **excludes** `age_ms`. That field is elapsed
//! time since the server last spoke, so it changes on every read; including it
//! would make "publish on change" fire continuously and turn the stream into
//! a busy loop.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use codegg_protocol::lsp::LspFileDiagnosticsDto;

/// Maximum files tracked for one project.
///
/// Firing this cap is reported through `files_for`'s `truncated` flag rather
/// than applied silently: a client must be able to tell that the set it
/// received is not the whole project.
pub const MAX_TRACKED_FILES_PER_PROJECT: usize = 512;

/// What a tracking tick observed for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticsChange {
    /// Content is identical to what was last published; publish nothing.
    Unchanged,
    /// Content differs; publish with this incremented sequence.
    Changed { sequence: u64, digest: String },
    /// The file is newly tracked; publish it for the first time.
    First { sequence: u64, digest: String },
}

/// Per-file tracking state.
#[derive(Debug, Clone)]
struct TrackedFile {
    last_digest: String,
    sequence: u64,
    payload: LspFileDiagnosticsDto,
}

/// A digest-keyed tracker for one project's diagnostics.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsTracker {
    files: BTreeMap<String, TrackedFile>,
    /// `true` once [`MAX_TRACKED_FILES_PER_PROJECT`] has been reached and
    /// further files were refused.
    saturated: bool,
}

impl DiagnosticsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the tracked-file cap is currently suppressing new files.
    pub fn is_saturated(&self) -> bool {
        self.saturated
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Record what a tick observed for `path` and report whether to publish.
    ///
    /// Returns `None` when the file cannot be tracked because the cap is
    /// already reached, so a caller can count the refusal instead of
    /// mistaking it for an unchanged file.
    pub fn observe(
        &mut self,
        path: &str,
        payload: LspFileDiagnosticsDto,
    ) -> Option<DiagnosticsChange> {
        // Compute the digest here rather than trusting `payload.digest`.
        //
        // "Publish only when the content changed" is the correctness property
        // the whole resync contract rests on, so it must be a property of this
        // store and not something every caller has to remember. A caller that
        // passed a stale, empty, or hand-rolled digest would otherwise make an
        // unchanged file look changed (spamming the stream) or a changed file
        // look unchanged (silently stalling diagnostics) — both failures that
        // look like "LSP is flaky" rather than like a bug.
        let digest = diagnostics_digest(&payload.diagnostics);
        let mut payload = payload;
        payload.digest = digest.clone();
        match self.files.get_mut(path) {
            Some(tracked) if tracked.last_digest == digest => Some(DiagnosticsChange::Unchanged),
            Some(tracked) => {
                tracked.sequence = tracked.sequence.saturating_add(1);
                tracked.last_digest = digest;
                tracked.payload = payload;
                tracked.payload.sequence = tracked.sequence;
                Some(DiagnosticsChange::Changed {
                    sequence: tracked.sequence,
                    digest: tracked.last_digest.clone(),
                })
            }
            None => {
                if self.files.len() >= MAX_TRACKED_FILES_PER_PROJECT {
                    self.saturated = true;
                    return None;
                }
                let sequence = 1;
                let mut payload = payload;
                payload.sequence = sequence;
                self.files.insert(
                    path.to_string(),
                    TrackedFile {
                        last_digest: digest.clone(),
                        sequence,
                        payload,
                    },
                );
                Some(DiagnosticsChange::First { sequence, digest })
            }
        }
    }

    /// The recorded set for one file, for the resync authority.
    pub fn file(&self, path: &str) -> Option<&LspFileDiagnosticsDto> {
        self.files.get(path).map(|tracked| &tracked.payload)
    }

    /// Every recorded file, ordered by path so a resync is deterministic.
    pub fn files(&self) -> Vec<LspFileDiagnosticsDto> {
        self.files
            .values()
            .map(|tracked| tracked.payload.clone())
            .collect()
    }

    /// Stop tracking a file, e.g. when its project closes.
    pub fn forget(&mut self, path: &str) -> bool {
        self.files.remove(path).is_some()
    }
}

/// Per-project diagnostics tracking, shared by the publisher and the
/// resync authority.
///
/// The two must read the *same* state: a client that reconciles against a
/// different view than the one it was streamed would never converge, which
/// would defeat the point of having a pull at all.
#[derive(Debug, Clone, Default)]
pub struct LspDiagnosticsStore {
    projects: BTreeMap<String, DiagnosticsTracker>,
}

impl LspDiagnosticsStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The tracker for a project, creating it on first use.
    pub fn tracker_mut(&mut self, project_id: &str) -> &mut DiagnosticsTracker {
        self.projects.entry(project_id.to_string()).or_default()
    }

    /// The authoritative recorded set for a project.
    ///
    /// `None` means the daemon has never observed this project, which is
    /// deliberately distinct from an observed-but-empty set: reporting empty
    /// for a project the daemon knows nothing about would tell a client the
    /// project is clean.
    pub fn files_for(&self, project_id: &str) -> Option<Vec<LspFileDiagnosticsDto>> {
        self.projects.get(project_id).map(DiagnosticsTracker::files)
    }

    /// Whether a project's tracked-file cap is suppressing new files.
    pub fn tracker_saturated(&self, project_id: &str) -> bool {
        self.projects
            .get(project_id)
            .is_some_and(DiagnosticsTracker::is_saturated)
    }

    /// Whether a project has any recorded state.
    pub fn tracks(&self, project_id: &str) -> bool {
        self.projects.contains_key(project_id)
    }

    /// Drop a project's state, e.g. when it closes.
    pub fn forget_project(&mut self, project_id: &str) -> bool {
        self.projects.remove(project_id).is_some()
    }
}

/// A digest of a diagnostic set's *content*.
///
/// `age_ms` is deliberately not an input: it is elapsed time since the server
/// last spoke, so two reads of an unchanged file would produce different
/// digests and the tracker would republish forever. The sort makes the digest
/// independent of the server's emission order, which is not meaningful.
pub fn diagnostics_digest(diagnostics: &[codegg_protocol::lsp::LspDiagnosticDto]) -> String {
    let mut keys: Vec<String> = diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{}|{}|{}|{}|{}:{}|{}:{}|{}|{}",
                diagnostic.severity,
                diagnostic.tag,
                diagnostic.code.clone().unwrap_or_default(),
                diagnostic.range.path,
                diagnostic.range.start_line,
                diagnostic.range.start_column,
                diagnostic.range.end_line,
                diagnostic.range.end_column,
                diagnostic.message,
                diagnostic.source.clone().unwrap_or_default(),
            )
        })
        .collect();
    keys.sort_unstable();
    let mut hasher = DefaultHasher::new();
    for key in keys {
        key.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::lsp::{LspDiagnosticDto, LspRangeDto};

    fn payload(message: &str) -> LspFileDiagnosticsDto {
        LspFileDiagnosticsDto {
            path: "src/lib.rs".to_string(),
            sequence: 0,
            digest: String::new(),
            diagnostics: vec![LspDiagnosticDto {
                range: LspRangeDto {
                    path: "src/lib.rs".to_string(),
                    start_line: 1,
                    start_column: 2,
                    end_line: 1,
                    end_column: 5,
                },
                severity: 1,
                tag: 0,
                code: Some("E0308".to_string()),
                message: message.to_string(),
                source: None,
            }],
            post_restart: false,
            truncated: false,
        }
    }

    #[test]
    fn the_first_observation_publishes() {
        let mut tracker = DiagnosticsTracker::new();
        let change = tracker
            .observe("src/lib.rs", payload("mismatched types"))
            .expect("first observation is tracked");
        assert!(matches!(
            change,
            DiagnosticsChange::First { sequence: 1, .. }
        ));
    }

    #[test]
    fn an_unchanged_file_publishes_nothing() {
        let mut tracker = DiagnosticsTracker::new();
        tracker.observe("src/lib.rs", payload("mismatched types"));
        let change = tracker
            .observe("src/lib.rs", payload("mismatched types"))
            .expect("tracked");
        assert_eq!(change, DiagnosticsChange::Unchanged);
        // And the sequence did not move, so a client sees no gap.
        assert_eq!(tracker.file("src/lib.rs").expect("tracked").sequence, 1);
    }

    #[test]
    fn a_changed_file_increments_the_sequence() {
        let mut tracker = DiagnosticsTracker::new();
        tracker.observe("src/lib.rs", payload("mismatched types"));
        let change = tracker
            .observe("src/lib.rs", payload("unused import"))
            .expect("tracked");
        assert_eq!(
            change,
            DiagnosticsChange::Changed {
                sequence: 2,
                // Derived from the recorded payload rather than hand-written,
                // so this test does not duplicate the digest format string and
                // break for the wrong reason when that format changes.
                digest: diagnostics_digest(
                    &tracker.file("src/lib.rs").expect("tracked").diagnostics
                ),
            }
        );
        assert_eq!(tracker.file("src/lib.rs").expect("tracked").sequence, 2);
    }

    #[test]
    fn a_sequence_gap_is_visible_to_a_client() {
        // The property the resync contract rests on: a client that missed an
        // envelope can compare its last streamed sequence against the
        // authoritative one and detect the gap.
        let mut tracker = DiagnosticsTracker::new();
        tracker.observe("src/lib.rs", payload("a"));
        let client_last_seen = 1;
        tracker.observe("src/lib.rs", payload("b"));
        let authoritative = tracker.file("src/lib.rs").expect("tracked").sequence;
        assert_eq!(authoritative, 2);
        assert!(
            authoritative > client_last_seen,
            "a client holding {client_last_seen} must detect a gap against {authoritative}"
        );
    }

    #[test]
    fn the_digest_ignores_order_but_not_content() {
        let one = LspDiagnosticDto {
            range: LspRangeDto {
                path: "a.rs".to_string(),
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 1,
            },
            severity: 1,
            tag: 0,
            code: None,
            message: "first".to_string(),
            source: None,
        };
        let two = LspDiagnosticDto {
            message: "second".to_string(),
            ..one.clone()
        };
        assert_eq!(
            diagnostics_digest(&[one.clone(), two.clone()]),
            diagnostics_digest(&[two.clone(), one.clone()]),
            "server emission order is not meaningful"
        );
        assert_ne!(
            diagnostics_digest(std::slice::from_ref(&one)),
            diagnostics_digest(std::slice::from_ref(&two)),
            "a changed message must change the digest"
        );
        let mut widened = one.clone();
        widened.range.end_line = 99;
        assert_ne!(
            diagnostics_digest(std::slice::from_ref(&one)),
            diagnostics_digest(&[widened]),
            "a changed extent must change the digest"
        );
    }

    #[test]
    fn the_file_cap_is_reported_rather_than_silent() {
        let mut tracker = DiagnosticsTracker::new();
        for index in 0..MAX_TRACKED_FILES_PER_PROJECT {
            let mut entry = payload("m");
            let path = format!("src/file{index}.rs");
            entry.path = path.clone();
            assert!(tracker.observe(&path, entry).is_some());
        }
        let overflow = payload("m");
        assert_eq!(
            tracker.observe("src/one-too-many.rs", overflow),
            None,
            "a refused file must be distinguishable from an unchanged one"
        );
        assert!(tracker.is_saturated());
    }
}
