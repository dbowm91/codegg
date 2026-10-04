//! M006-E trajectory: an agent change is reviewed before it reaches disk,
//! and a daemon refusal is surfaced with the daemon's own words.
//!
//! This qualifies the boundary the milestone exists to protect: the review
//! surface shows a change and waits, rejecting costs nothing, and accepting
//! produces exactly the request the pre-existing `/lsp-preview-apply` path
//! produces — no second apply semantics.

use codegg::tui::app::state::change_review::{ChangeReviewState, ReviewStatus, ReviewVerdict};
use codegg::tui::components::diff::DiffViewer;
use codegg::tui::unified_diff::{parse_patch, parse_review};

const PREVIEW: &str = "@@ -1,3 +1,3 @@\n fn main() {\n-    old();\n+    new();\n }\n";

/// The message `src/lsp/mutation.rs` produces for a dirty target. Quoted
/// verbatim so a change to that rejection is a visible diff in this test
/// rather than a silent divergence between the daemon and the review surface.
const DIRTY_BUFFER_REFUSAL: &str =
    "src/lib.rs has unsaved editor changes; save and regenerate the preview";

fn open_review() -> ChangeReviewState {
    let mut state = ChangeReviewState::default();
    state.open(
        "preview-1".to_string(),
        "edit".to_string(),
        "Replace the helper body".to_string(),
        "agent".to_string(),
        false,
        parse_review(&[("src/lib.rs".to_string(), PREVIEW.to_string())]),
    );
    state
}

#[test]
fn opening_a_review_applies_nothing_and_waits_for_a_decision() {
    let state = open_review();
    assert_eq!(state.status, ReviewStatus::Reviewing);
    assert_eq!(state.verdict, ReviewVerdict::Pending);
    assert_eq!(state.review.patches.len(), 1);
    assert_eq!(
        state.active_patch().map(|p| p.path.as_str()),
        Some("src/lib.rs")
    );
    // The parsed change is exactly what the user is being asked to approve.
    let hunk = &state.active_patch().expect("patch").hunks;
    assert_eq!(hunk.len(), 1);
    let removed: Vec<_> = hunk[0]
        .lines
        .iter()
        .filter(|l| l.tag == similar::ChangeTag::Delete)
        .collect();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].content, "    old();");
}

#[test]
fn rejecting_issues_no_request_and_leaves_nothing_behind() {
    let mut state = open_review();
    // A reject is purely local: the state is cleared and no request id is ever
    // consumed, which is the observable proxy for "nothing was sent".
    let requests_before = state.request.request_id();
    state.reject();
    assert_eq!(state.status, ReviewStatus::Closed);
    assert!(state.review.patches.is_empty());
    assert!(state.preview_id.is_empty());
    assert_eq!(
        state.request.request_id(),
        requests_before,
        "a reject must not start or consume a request"
    );
}

#[test]
fn a_daemon_refusal_is_recorded_verbatim_and_keeps_the_review_readable() {
    let mut state = open_review();
    let (generation, request) = (state.generation, state.request.request_id());
    state.begin_accept();

    assert!(state.apply_refusal(request, generation, DIRTY_BUFFER_REFUSAL.to_string()));
    assert_eq!(
        state.verdict,
        ReviewVerdict::Refused {
            message: DIRTY_BUFFER_REFUSAL.to_string()
        }
    );
    assert!(
        state.is_open(),
        "a refusal must leave the review open so the user can read why"
    );
    assert_eq!(state.verdict_summary(), "refused");
}

#[test]
fn a_completion_after_a_reject_cannot_apply_anything() {
    let mut state = open_review();
    let (generation, request) = (state.generation, state.request.request_id());
    state.reject();
    assert!(
        !state.apply_success(
            request,
            generation,
            vec!["src/lib.rs".to_string()],
            "checkpoint-1".to_string()
        ),
        "a late completion must not apply a rejected change"
    );
}

#[test]
fn the_parsed_diff_feeds_the_existing_viewer_without_a_second_renderer() {
    // The review reuses `DiffViewer` by replacing its public hunks. This
    // asserts the parsed hunk is renderable by that viewer, which is what
    // keeps the review from forking diff rendering.
    let parsed = parse_patch("src/lib.rs", PREVIEW);
    let mut viewer = DiffViewer::new(
        String::new().into_boxed_str(),
        String::new().into_boxed_str(),
        "src/lib.rs".to_string().into_boxed_str(),
    );
    viewer.hunks = parsed.hunks;
    assert_eq!(
        viewer.total_lines(),
        4,
        "three diff lines plus the hunk header"
    );
    assert_eq!(viewer.hunks.len(), 1);
}

#[test]
fn a_candidate_with_several_files_is_reviewed_file_by_file() {
    let mut state = ChangeReviewState::default();
    state.open(
        "preview-2".to_string(),
        "edit".to_string(),
        "Multi-file change".to_string(),
        "agent".to_string(),
        false,
        parse_review(&[
            ("src/a.rs".to_string(), PREVIEW.to_string()),
            ("src/b.rs".to_string(), PREVIEW.to_string()),
        ]),
    );
    assert_eq!(
        state.active_patch().map(|p| p.path.as_str()),
        Some("src/a.rs")
    );
    state.move_file(1);
    assert_eq!(
        state.active_patch().map(|p| p.path.as_str()),
        Some("src/b.rs")
    );
}

#[test]
fn a_stale_candidate_is_flagged_before_a_decision_is_possible() {
    let mut state = ChangeReviewState::default();
    state.open(
        "preview-3".to_string(),
        "edit".to_string(),
        "Stale change".to_string(),
        "agent".to_string(),
        true,
        parse_review(&[("src/lib.rs".to_string(), PREVIEW.to_string())]),
    );
    assert!(state.stale, "a stale candidate must be visible to the user");
    // The state still allows a decision; the *view* is what warns. Refusing to
    // open at all is the command's job, not the state's.
    assert!(state.is_open());
}
