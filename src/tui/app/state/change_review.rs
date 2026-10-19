//! M006-E: agent change-review state.
//!
//! One review is open at a time. The state records *what the user is being
//! asked to approve* and *what the daemon said*, and nothing else: it never
//! decides whether an apply is legal. That decision belongs to the daemon,
//! and this module's job is to make the daemon's answer legible.
//!
//! The dirty-buffer half of M006-E is deliberately absent. A change to a
//! saved document is reviewed here; whether an apply may merge into a *dirty*
//! buffer is a deferred ADR, and the rejection in `src/lsp/mutation.rs` is
//! correct as written and untouched.
//!
//! Since M016 the review can also be opened on a candidate the *editor*
//! carried — the material a preview read returned. That candidate travels
//! with the review, because the registry the other path resolves ids in does
//! not exist in socket mode. The request is still built in one place.

use crate::tui::app::state::AsyncUiRequestState;
use crate::tui::unified_diff::ParsedReview;
use codegg_protocol::lsp::LspPreviewCandidateDto;

/// What the daemon said about the apply.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ReviewVerdict {
    /// No apply has been attempted yet.
    #[default]
    Pending,
    /// The apply is in flight.
    Applying,
    /// The daemon applied it.
    Applied {
        written_files: Vec<String>,
        checkpoint_id: String,
    },
    /// The daemon refused. The message is shown verbatim, because it carries
    /// the actionable detail — notably the unsaved-changes instruction.
    Refused { message: String },
}

/// Where the review surface is in its own lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ReviewStatus {
    /// No review is open. `Default`, so the whole TUI is untouched until a
    /// user opens one.
    #[default]
    Closed,
    /// A review is open and awaiting a decision.
    Reviewing,
}

#[derive(Debug, Clone, Default)]
pub struct ChangeReviewState {
    pub status: ReviewStatus,
    pub preview_id: String,
    pub kind: String,
    pub title: String,
    pub provenance: String,
    /// The base content has changed since the candidate was created.
    pub stale: bool,
    /// Parsed diff, already bounded by the parser.
    pub review: ParsedReview,
    /// Index of the file whose diff is shown.
    pub file_index: usize,
    pub verdict: ReviewVerdict,
    /// Request tracking for the accept round trip.
    pub request: AsyncUiRequestState,
    /// The candidate the editor carried, when the review was opened on one.
    ///
    /// The registry path resolves its candidate by id at accept time; the
    /// carried path resolves it from here. Either way the request is built by
    /// the one builder, so the two paths cannot drift.
    pub carried: Option<LspPreviewCandidateDto>,
    /// Every path the candidate touches, relative to the workspace.
    ///
    /// This is the *candidate's* list, not the review's: the review parses at
    /// most [`MAX_REVIEW_PATCHES`](crate::tui::unified_diff::MAX_REVIEW_PATCHES)
    /// patches for display, and a rename may touch more than that. The list
    /// is what decides whether the editor's open document was one of the
    /// files the apply wrote.
    pub affected_paths: Vec<String>,
    /// Bumped whenever a review is opened, so a completion for a superseded
    /// review is discarded.
    pub generation: u64,
    /// True while the review owns keyboard focus.
    pub focused: bool,
}

/// The candidate's identity, as the review records it.
///
/// Four strings that always travel together, so they travel together: a
/// review that named one candidate's title and another's kind would be a
/// review of nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewIdentity {
    preview_id: String,
    kind: String,
    title: String,
    provenance: String,
}

impl ChangeReviewState {
    pub fn is_open(&self) -> bool {
        self.status == ReviewStatus::Reviewing
    }

    /// Begin a review for one candidate.
    ///
    /// Opening supersedes any previous review, which is what makes a late
    /// completion for the old one discardable.
    pub fn open(
        &mut self,
        preview_id: String,
        kind: String,
        title: String,
        provenance: String,
        stale: bool,
        review: ParsedReview,
        affected_paths: Vec<String>,
    ) -> (u64, u64) {
        self.begin(
            ReviewIdentity {
                preview_id,
                kind,
                title,
                provenance,
            },
            stale,
            review,
            None,
            affected_paths,
        )
    }

    /// Begin a review on a candidate the editor carried.
    ///
    /// The candidate is the material the daemon minted for a preview read. It
    /// travels with the review so the accept path can build exactly the
    /// request the review displayed, without resolving anything in a registry
    /// that does not exist in socket mode.
    pub fn open_carried(
        &mut self,
        candidate: LspPreviewCandidateDto,
        review: ParsedReview,
    ) -> (u64, u64) {
        let affected_paths = candidate
            .patches
            .iter()
            .map(|patch| patch.path.clone())
            .collect();
        self.begin(
            ReviewIdentity {
                preview_id: candidate.preview_id.clone(),
                kind: candidate.kind.clone(),
                title: candidate.title.clone(),
                provenance: candidate.provenance.clone(),
            },
            false,
            review,
            Some(candidate),
            affected_paths,
        )
    }

    /// Begin a review, with or without a carried candidate.
    fn begin(
        &mut self,
        identity: ReviewIdentity,
        stale: bool,
        review: ParsedReview,
        carried: Option<LspPreviewCandidateDto>,
        affected_paths: Vec<String>,
    ) -> (u64, u64) {
        self.status = ReviewStatus::Reviewing;
        self.preview_id = identity.preview_id;
        self.kind = identity.kind;
        self.title = identity.title;
        self.provenance = identity.provenance;
        self.stale = stale;
        self.review = review;
        self.file_index = 0;
        self.verdict = ReviewVerdict::Pending;
        self.focused = true;
        self.carried = carried;
        self.affected_paths = affected_paths;
        self.generation = self.generation.wrapping_add(1);
        let request_id = self.request.begin();
        (self.generation, request_id)
    }

    /// Close without contacting the daemon. This is reject: it changes
    /// nothing on disk and sends no request.
    pub fn reject(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.status = ReviewStatus::Closed;
        self.focused = false;
        self.verdict = ReviewVerdict::Pending;
        self.preview_id.clear();
        self.review = ParsedReview::default();
        self.file_index = 0;
        self.carried = None;
        self.affected_paths.clear();
    }

    /// Whether the candidate touches `path`.
    ///
    /// The comparison is on the workspace-relative path, which is how the
    /// candidate names its files and how the editor names its document.
    pub fn touches_path(&self, path: &str) -> bool {
        self.affected_paths.iter().any(|affected| affected == path)
    }

    /// Mark an accept as in flight.
    pub fn begin_accept(&mut self) {
        self.verdict = ReviewVerdict::Applying;
    }

    /// Apply a successful completion, if it belongs to this review.
    pub fn apply_success(
        &mut self,
        request_id: u64,
        generation: u64,
        written_files: Vec<String>,
        checkpoint_id: String,
    ) -> bool {
        if generation != self.generation || !self.is_open() || !self.request.finish(request_id) {
            return false;
        }
        self.verdict = ReviewVerdict::Applied {
            written_files,
            checkpoint_id,
        };
        true
    }

    /// Apply a refusal, if it belongs to this review.
    ///
    /// The review stays open on refusal so the user can read why, which is
    /// the whole point of surfacing the daemon's dirty-buffer message instead
    /// of letting it disappear into a toast.
    pub fn apply_refusal(&mut self, request_id: u64, generation: u64, message: String) -> bool {
        if generation != self.generation || !self.is_open() || !self.request.finish(request_id) {
            return false;
        }
        self.verdict = ReviewVerdict::Refused { message };
        true
    }

    /// Move the shown file, clamped to the parsed patch list.
    pub fn move_file(&mut self, delta: isize) {
        let count = self.review.patches.len();
        if count == 0 {
            self.file_index = 0;
            return;
        }
        let next = (self.file_index as isize + delta).clamp(0, count as isize - 1);
        self.file_index = next as usize;
    }

    /// The patch currently shown, if any.
    pub fn active_patch(&self) -> Option<&crate::tui::unified_diff::ParsedPatch> {
        self.review.patches.get(self.file_index)
    }

    /// One-line summary of the daemon's answer, for the view header.
    pub fn verdict_summary(&self) -> String {
        match &self.verdict {
            ReviewVerdict::Pending => "not applied".to_string(),
            ReviewVerdict::Applying => "applying…".to_string(),
            ReviewVerdict::Applied { written_files, .. } => {
                format!("applied to {} file(s)", written_files.len())
            }
            ReviewVerdict::Refused { .. } => "refused".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::unified_diff::parse_review;

    fn sample_review() -> ParsedReview {
        parse_review(&[("a.rs".to_string(), "@@ -1,1 +1,1 @@\n-x\n+y\n".to_string())])
    }

    fn open(state: &mut ChangeReviewState) -> (u64, u64) {
        state.open(
            "preview-1".to_string(),
            "edit".to_string(),
            "Rename".to_string(),
            "agent".to_string(),
            false,
            sample_review(),
            vec!["a.rs".to_string()],
        )
    }

    #[test]
    fn default_is_closed_so_the_tui_surface_is_untouched() {
        let state = ChangeReviewState::default();
        assert!(!state.is_open());
        assert!(!state.focused);
        assert_eq!(state.verdict, ReviewVerdict::Pending);
        assert!(state.review.patches.is_empty());
    }

    #[test]
    fn open_populates_the_review_and_takes_focus() {
        let mut state = ChangeReviewState::default();
        let (generation, request) = open(&mut state);
        assert!(state.is_open());
        assert!(state.focused);
        assert_eq!(state.preview_id, "preview-1");
        assert_eq!(state.file_index, 0);
        assert!(state.active_patch().is_some());
        assert_ne!(generation, 0);
        assert!(request > 0);
    }

    #[test]
    fn reject_clears_everything_and_sends_nothing() {
        let mut state = ChangeReviewState::default();
        open(&mut state);
        let generation_before = state.generation;
        state.reject();
        assert!(!state.is_open());
        assert!(!state.focused);
        assert!(state.preview_id.is_empty());
        assert!(state.review.patches.is_empty());
        assert_ne!(
            state.generation, generation_before,
            "reject must invalidate any in-flight completion"
        );
    }

    #[test]
    fn a_completion_after_reject_is_discarded() {
        let mut state = ChangeReviewState::default();
        let (generation, request) = open(&mut state);
        state.reject();
        assert!(
            !state.apply_success(
                request,
                generation,
                vec!["a.rs".to_string()],
                "cp-1".to_string()
            ),
            "a rejected review must not be able to apply afterwards"
        );
    }

    #[test]
    fn a_stale_generation_completion_is_discarded() {
        let mut state = ChangeReviewState::default();
        let (first, first_request) = open(&mut state);
        let (second, _) = open(&mut state);
        assert_ne!(first, second);
        assert!(
            !state.apply_success(first_request, first, vec![], "cp-1".to_string()),
            "a superseded review must not accept a completion"
        );
    }

    #[test]
    fn a_mismatched_request_id_is_discarded() {
        let mut state = ChangeReviewState::default();
        let (generation, _) = open(&mut state);
        assert!(!state.apply_success(999_999, generation, vec![], "cp-1".to_string()));
    }

    #[test]
    fn success_records_the_written_files_and_checkpoint() {
        let mut state = ChangeReviewState::default();
        let (generation, request) = open(&mut state);
        state.begin_accept();
        assert_eq!(state.verdict, ReviewVerdict::Applying);
        assert!(state.apply_success(
            request,
            generation,
            vec!["a.rs".to_string(), "b.rs".to_string()],
            "cp-7".to_string()
        ));
        assert_eq!(
            state.verdict,
            ReviewVerdict::Applied {
                written_files: vec!["a.rs".to_string(), "b.rs".to_string()],
                checkpoint_id: "cp-7".to_string(),
            }
        );
        assert_eq!(state.verdict_summary(), "applied to 2 file(s)");
    }

    #[test]
    fn a_refusal_keeps_the_review_open_and_surfaces_the_message_verbatim() {
        let mut state = ChangeReviewState::default();
        let (generation, request) = open(&mut state);
        state.begin_accept();
        let message =
            "src/lib.rs has unsaved editor changes; save and regenerate the preview".to_string();
        assert!(state.apply_refusal(request, generation, message.clone()));
        assert!(
            state.is_open(),
            "a refusal must stay readable rather than closing the review"
        );
        assert_eq!(
            state.verdict,
            ReviewVerdict::Refused {
                message: message.clone()
            }
        );
        assert_eq!(state.verdict_summary(), "refused");
    }

    #[test]
    fn moving_between_files_clamps_at_both_ends() {
        let mut state = ChangeReviewState::default();
        state.open(
            "p".to_string(),
            "edit".to_string(),
            "t".to_string(),
            "agent".to_string(),
            false,
            parse_review(&[
                ("a.rs".to_string(), "@@ -1,1 +1,1 @@\n-x\n+y\n".to_string()),
                ("b.rs".to_string(), "@@ -1,1 +1,1 @@\n-x\n+y\n".to_string()),
            ]),
            vec!["a.rs".to_string(), "b.rs".to_string()],
        );
        assert_eq!(state.file_index, 0);
        state.move_file(-1);
        assert_eq!(state.file_index, 0, "clamps at the first file");
        state.move_file(1);
        assert_eq!(state.active_patch().map(|p| p.path.as_str()), Some("b.rs"));
        state.move_file(1);
        assert_eq!(state.file_index, 1, "clamps at the last file");
    }

    #[test]
    fn moving_with_no_files_is_safe() {
        let mut state = ChangeReviewState::default();
        state.move_file(1);
        assert_eq!(state.file_index, 0);
        assert!(state.active_patch().is_none());
    }

    #[test]
    fn a_stale_candidate_is_flagged_at_open_time() {
        let mut state = ChangeReviewState::default();
        state.open(
            "p".to_string(),
            "edit".to_string(),
            "t".to_string(),
            "agent".to_string(),
            true,
            sample_review(),
            vec!["a.rs".to_string()],
        );
        assert!(
            state.stale,
            "a stale candidate must be visible in the state"
        );
    }

    #[test]
    fn a_carried_review_keeps_the_candidate_and_the_files_it_touches() {
        use codegg_protocol::lsp::{LspPreviewCandidateDto, LspPreviewPatchDto};

        let candidate = LspPreviewCandidateDto {
            preview_id: "editor-preview-3".to_string(),
            preview_revision: 3,
            preview_digest: "a".repeat(64),
            kind: "rename".to_string(),
            title: "Rename old to new".to_string(),
            provenance: "editor:rename:old->new".to_string(),
            patches: vec![
                LspPreviewPatchDto {
                    path: "src/a.rs".to_string(),
                    patch: "@@ -1 +1 @@\n-x\n+y\n".to_string(),
                    original_hash: "b".repeat(64),
                },
                LspPreviewPatchDto {
                    path: "src/b.rs".to_string(),
                    patch: "@@ -1 +1 @@\n-x\n+y\n".to_string(),
                    original_hash: "c".repeat(64),
                },
            ],
        };
        let mut state = ChangeReviewState::default();
        state.open_carried(candidate.clone(), sample_review());
        assert!(state.is_open());
        assert_eq!(state.preview_id, "editor-preview-3");
        assert_eq!(state.carried, Some(candidate));
        assert!(
            state.touches_path("src/b.rs"),
            "the editor must be able to tell that its own document is one of the files"
        );
        assert!(
            !state.touches_path("src/other.rs"),
            "a file the candidate does not touch must not read as touched"
        );
    }
}
