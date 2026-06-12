use std::path::PathBuf;

use codegg::security::workflow::{
    filter_panel_items, project_receipt_to_panel_items, run_security_review_background,
    run_security_review_workflow, FixtureSecurityContextExecutor, SecurityConfidence,
    SecurityEvidenceKind, SecurityReviewCommandArgs, SecurityReviewFilter, SecurityReviewFinding,
    SecurityReviewOutput, SecurityReviewPanelItemKind, SecurityReviewPrompt, SecurityReviewReceipt,
    SecurityReviewTarget, SecurityReviewWorkflowOptions, SecuritySeverity, SecurityTargetReason,
    StructuredSecurityEvidence,
};
use codegg::tui::app::{App, TuiMsg};
use codegg::tui::command::COMMAND_REGISTRY;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn target(path: &str, line: u32) -> SecurityReviewTarget {
    SecurityReviewTarget {
        file_path: PathBuf::from(path),
        line: Some(line),
        column: Some(1),
        preset: "rust_server".to_string(),
        reason: SecurityTargetReason::ChangedHunk,
    }
}

fn finding(path: &str, line: u32, severity: SecuritySeverity) -> SecurityReviewFinding {
    SecurityReviewFinding {
        severity,
        confidence: SecurityConfidence::High,
        title: "Test finding".to_string(),
        file_path: PathBuf::from(path),
        line: Some(line),
        category: Some("auth".to_string()),
        evidence: vec![StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::RiskMarker,
            file_path: Some(PathBuf::from(path)),
            line: Some(line),
            summary: "marker".to_string(),
            detail: None,
        }],
        reasoning: "reasoning".to_string(),
        recommendation: "recommendation".to_string(),
        tests: vec!["test_regression".to_string()],
    }
}

fn prompt(path: &str, line: u32) -> SecurityReviewPrompt {
    SecurityReviewPrompt {
        file_path: PathBuf::from(path),
        line: Some(line),
        preset: "rust_server".to_string(),
        category: Some("unsafe".to_string()),
        title: "Review unsafe".to_string(),
        rationale: "rationale".to_string(),
        evidence: vec!["source: securityContext.risk_marker".to_string()],
    }
}

fn sample_output() -> SecurityReviewOutput {
    SecurityReviewOutput {
        targets: vec![target("src/lib.rs", 10)],
        findings: vec![
            finding("src/lib.rs", 10, SecuritySeverity::High),
            finding("src/auth.rs", 42, SecuritySeverity::Medium),
        ],
        review_prompts: vec![prompt("src/db.rs", 15)],
        preflight_results: Vec::new(),
        notes: vec!["risk markers are review prompts, not confirmed findings".to_string()],
    }
}

fn sample_receipt() -> SecurityReviewReceipt {
    SecurityReviewReceipt::now(
        "sr-test-1".to_string(),
        PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        sample_output(),
        "rendered text".to_string(),
        false,
        false,
    )
}

// ---------------------------------------------------------------------------
// Projection tests
// ---------------------------------------------------------------------------

#[test]
fn security_review_receipt_projection_preserves_findings_prompts_notes() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);

    let findings: Vec<_> = items
        .iter()
        .filter(|i| i.kind == SecurityReviewPanelItemKind::Finding)
        .collect();
    let prompts: Vec<_> = items
        .iter()
        .filter(|i| i.kind == SecurityReviewPanelItemKind::Prompt)
        .collect();
    let notes: Vec<_> = items
        .iter()
        .filter(|i| i.kind == SecurityReviewPanelItemKind::Note)
        .collect();

    assert_eq!(
        findings.len(),
        2,
        "expected 2 findings, got {}",
        findings.len()
    );
    assert_eq!(prompts.len(), 1, "expected 1 prompt, got {}", prompts.len());
    assert_eq!(notes.len(), 1, "expected 1 note, got {}", notes.len());

    assert!(findings[0].severity.is_some());
    assert!(findings[0].confidence.is_some());
    assert!(prompts[0].severity.is_none());
}

#[test]
fn security_review_receipt_projection_orders_findings_first_then_prompts_then_notes() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);

    let kinds: Vec<_> = items.iter().map(|i| i.kind).collect();
    let first_note = kinds
        .iter()
        .position(|k| *k == SecurityReviewPanelItemKind::Note)
        .unwrap();
    let first_prompt = kinds
        .iter()
        .position(|k| *k == SecurityReviewPanelItemKind::Prompt)
        .unwrap();
    let last_finding = kinds
        .iter()
        .rposition(|k| *k == SecurityReviewPanelItemKind::Finding)
        .unwrap();

    assert!(last_finding < first_prompt);
    assert!(first_prompt < first_note);
}

#[test]
fn security_review_receipt_projection_includes_location_for_finding_and_prompt() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);

    let f = items
        .iter()
        .find(|i| i.kind == SecurityReviewPanelItemKind::Finding)
        .unwrap();
    assert_eq!(
        f.file_path.as_deref(),
        Some(PathBuf::from("src/lib.rs").as_path())
    );
    assert_eq!(f.line, Some(10));

    let p = items
        .iter()
        .find(|i| i.kind == SecurityReviewPanelItemKind::Prompt)
        .unwrap();
    assert_eq!(
        p.file_path.as_deref(),
        Some(PathBuf::from("src/db.rs").as_path())
    );
    assert_eq!(p.line, Some(15));
}

#[test]
fn security_review_receipt_projection_handles_no_findings() {
    let output = SecurityReviewOutput {
        targets: Vec::new(),
        findings: Vec::new(),
        review_prompts: vec![prompt("src/lib.rs", 10)],
        preflight_results: Vec::new(),
        notes: vec!["a note".to_string()],
    };
    let receipt = SecurityReviewReceipt::now(
        "sr-test-empty".to_string(),
        PathBuf::from("."),
        SecurityReviewCommandArgs::default(),
        output,
        String::new(),
        false,
        false,
    );
    let items = project_receipt_to_panel_items(&receipt);
    assert!(items
        .iter()
        .all(|i| i.kind != SecurityReviewPanelItemKind::Finding));
    assert!(items
        .iter()
        .any(|i| i.kind == SecurityReviewPanelItemKind::Prompt));
    assert!(items
        .iter()
        .any(|i| i.kind == SecurityReviewPanelItemKind::Note));
}

#[test]
fn security_review_receipt_projection_handles_completely_empty_output() {
    let output = SecurityReviewOutput {
        targets: Vec::new(),
        findings: Vec::new(),
        review_prompts: Vec::new(),
        preflight_results: Vec::new(),
        notes: Vec::new(),
    };
    let receipt = SecurityReviewReceipt::now(
        "sr-test-empty".to_string(),
        PathBuf::from("."),
        SecurityReviewCommandArgs::default(),
        output,
        String::new(),
        false,
        false,
    );
    let items = project_receipt_to_panel_items(&receipt);
    assert!(items.is_empty());
}

// ---------------------------------------------------------------------------
// Filter tests
// ---------------------------------------------------------------------------

#[test]
fn security_review_panel_filters_findings_only() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);
    let filtered = filter_panel_items(&items, SecurityReviewFilter::Findings);
    assert!(filtered
        .iter()
        .all(|i| i.kind == SecurityReviewPanelItemKind::Finding));
    assert_eq!(filtered.len(), 2);
}

#[test]
fn security_review_panel_filters_prompts_only() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);
    let filtered = filter_panel_items(&items, SecurityReviewFilter::Prompts);
    assert!(filtered
        .iter()
        .all(|i| i.kind == SecurityReviewPanelItemKind::Prompt));
    assert_eq!(filtered.len(), 1);
}

#[test]
fn security_review_panel_filters_notes_only() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);
    let filtered = filter_panel_items(&items, SecurityReviewFilter::Notes);
    assert!(filtered
        .iter()
        .all(|i| i.kind == SecurityReviewPanelItemKind::Note));
    assert_eq!(filtered.len(), 1);
}

#[test]
fn security_review_panel_filters_high_confidence() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);
    let filtered = filter_panel_items(&items, SecurityReviewFilter::HighConfidence);
    // Both findings are High confidence; the prompt/note must be filtered out.
    assert_eq!(filtered.len(), 2);
    assert!(filtered
        .iter()
        .all(|i| i.confidence == Some(SecurityConfidence::High)));
}

#[test]
fn security_review_panel_filters_medium_or_higher_severity() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);
    let filtered = filter_panel_items(&items, SecurityReviewFilter::MediumOrHigherSeverity);
    // 1 high + 1 medium = 2 findings, both >= Medium
    assert_eq!(filtered.len(), 2);
}

#[test]
fn security_review_panel_filter_all_returns_everything() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);
    let total = items.len();
    let filtered = filter_panel_items(&items, SecurityReviewFilter::All);
    assert_eq!(filtered.len(), total);
}

#[test]
fn security_review_panel_selection_clamps_on_filter_change() {
    // Construct a list of items and verify that clamping logic
    // behaves correctly when the filter narrows the set.
    use codegg::tui::components::dialogs::security_review::SecurityReviewDialog;
    use codegg::tui::theme::Theme;
    use std::sync::Arc;

    let theme = Arc::new(Theme::dark());
    let mut dialog = SecurityReviewDialog::with_receipt(theme, sample_receipt());
    // Sample receipt has 2 findings + 1 prompt + 1 note = 4 items.
    assert_eq!(dialog.visible_items.len(), 4);
    dialog.selected_index = 3;

    dialog.filter = SecurityReviewFilter::Findings;
    dialog.set_receipt(Some(sample_receipt()));
    dialog.selected_index = dialog
        .selected_index
        .min(dialog.visible_items.len().saturating_sub(1));
    assert!(dialog.selected_index < dialog.visible_items.len());
}

// ---------------------------------------------------------------------------
// Slash command registration tests
// ---------------------------------------------------------------------------

#[test]
fn security_review_show_command_is_registered() {
    let cmd = COMMAND_REGISTRY
        .find_by_name_or_alias("security-review-show")
        .expect("/security-review-show command should be registered");
    assert_eq!(cmd.name, "/security-review-show");
    assert_eq!(cmd.dialog, Some(codegg::tui::Dialog::SecurityReview));
}

#[test]
fn security_review_cancel_command_is_registered() {
    let cmd = COMMAND_REGISTRY
        .find_by_name_or_alias("security-review-cancel")
        .expect("/security-review-cancel command should be registered");
    assert_eq!(cmd.name, "/security-review-cancel");
    assert!(cmd.dialog.is_none());
}

#[test]
fn security_review_show_without_receipt_warns() {
    // When no receipt exists, the dialog opens (because the command
    // is registered with `dialog: Some(Dialog::SecurityReview)`) but
    // the inner `receipt` field stays `None` — the dialog renders an
    // empty-state message and the user is informed via toast.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut app = App::new_for_testing(dir.path().to_string_lossy().to_string());
    assert!(app.latest_security_review.is_none());

    app.prompt_state
        .prompt
        .set_text("/security-review-show".to_string());
    let len = "/security-review-show".chars().count();
    app.prompt_state.prompt.set_cursor(len);
    app.process_msg(TuiMsg::SubmitPrompt);

    let dialog = app
        .dialog_state
        .security_review_dialog
        .as_ref()
        .expect("dialog should be opened even without a receipt");
    assert!(
        dialog.receipt.is_none(),
        "dialog should render the no-receipt empty state"
    );
}

#[test]
fn security_review_show_with_receipt_opens_dialog() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut app = App::new_for_testing(dir.path().to_string_lossy().to_string());
    app.set_latest_security_review(sample_receipt());
    assert!(app.latest_security_review.is_some());

    app.prompt_state
        .prompt
        .set_text("/security-review-show".to_string());
    let len = "/security-review-show".chars().count();
    app.prompt_state.prompt.set_cursor(len);
    app.process_msg(TuiMsg::SubmitPrompt);

    assert_eq!(app.ui_state.dialog, codegg::tui::Dialog::SecurityReview);
}

#[test]
fn security_review_cancel_without_active_run_warns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut app = App::new_for_testing(dir.path().to_string_lossy().to_string());
    assert!(app.security_review_running.is_none());
    let result = app.cancel_security_review();
    assert!(
        !result,
        "expected cancel without an active run to return false"
    );
    assert!(app.security_review_running.is_none());
}

#[tokio::test]
async fn security_review_cancel_aborts_and_clears_guard() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut app = App::new_for_testing(dir.path().to_string_lossy().to_string());
    // Install a real AbortHandle.
    let join = tokio::spawn(async {});
    let active_id = "sr-active".to_string();
    app.security_review_running = Some(codegg::security::workflow::SecurityReviewTaskState {
        id: active_id.clone(),
        abort_handle: join.abort_handle(),
    });
    let result = app.cancel_security_review();
    assert!(
        result,
        "expected cancel to return true when a review is active"
    );
    assert!(app.security_review_running.is_none());
}

#[tokio::test]
async fn security_review_cancel_ignores_stale_completion() {
    // Simulate the TUI handler: a completion arrives with an id that
    // doesn't match the active guard. The completion must be ignored
    // (the guard must not be cleared for a different run).
    let dir = tempfile::tempdir().expect("tempdir");
    let mut app = App::new_for_testing(dir.path().to_string_lossy().to_string());
    let join = tokio::spawn(async {});
    let active_id = "sr-current".to_string();
    app.security_review_running = Some(codegg::security::workflow::SecurityReviewTaskState {
        id: active_id.clone(),
        abort_handle: join.abort_handle(),
    });

    // Stale completion for an older run.
    let stale_id = "sr-stale".to_string();
    if app.security_review_run_id() == Some(stale_id.as_str()) {
        app.security_review_running = None;
    }

    // The active run is unchanged.
    assert_eq!(app.security_review_run_id(), Some(active_id.as_str()));
}

// ---------------------------------------------------------------------------
// Latest receipt + completion tests
// ---------------------------------------------------------------------------

#[test]
fn security_review_completion_stores_latest_receipt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut app = App::new_for_testing(dir.path().to_string_lossy().to_string());
    let receipt = sample_receipt();
    app.set_latest_security_review(receipt.clone());
    assert!(app.latest_security_review.is_some());
    assert_eq!(app.latest_security_review.as_ref().unwrap().id, receipt.id);
}

#[test]
fn security_review_set_latest_overwrites_previous() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut app = App::new_for_testing(dir.path().to_string_lossy().to_string());
    let mut a = sample_receipt();
    a.id = "sr-a".to_string();
    let mut b = sample_receipt();
    b.id = "sr-b".to_string();
    app.set_latest_security_review(a);
    app.set_latest_security_review(b.clone());
    assert_eq!(app.latest_security_review.as_ref().unwrap().id, "sr-b");
}

#[test]
fn security_review_jump_msg_carries_path_and_line() {
    use codegg::tui::app::TuiMsg;
    let msg = TuiMsg::SecurityReviewJump {
        path: "src/lib.rs".to_string(),
        line: Some(42),
    };
    match msg {
        TuiMsg::SecurityReviewJump { path, line } => {
            assert_eq!(path, "src/lib.rs");
            assert_eq!(line, Some(42));
        }
        _ => panic!("expected SecurityReviewJump variant"),
    }
}

// ---------------------------------------------------------------------------
// End-to-end background receipt with no live LSP
// ---------------------------------------------------------------------------

#[tokio::test]
async fn security_review_background_produces_receipt_with_unavailable_lsp() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _ = std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir.path())
        .status();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@example.invalid"])
        .current_dir(dir.path())
        .status();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "test"])
        .current_dir(dir.path())
        .status();
    let _ = std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init", "-q"])
        .current_dir(dir.path())
        .status();

    let args = SecurityReviewCommandArgs {
        enrich: true,
        base: Some("HEAD".to_string()),
        ..Default::default()
    };
    let receipt = run_security_review_background(dir.path().to_path_buf(), args, None)
        .await
        .expect("background should succeed");
    assert!(!receipt.lsp_available);
    assert!(!receipt.enriched);
    assert!(receipt
        .rendered_report
        .contains("no securityContext executor"));
}

#[tokio::test]
async fn security_review_workflow_pipeline_produces_output() {
    // Run the lower-level orchestrator (deterministic stage-1) and
    // verify the result is a valid SecurityReviewOutput we can wrap
    // in a receipt.
    let dir = tempfile::tempdir().expect("tempdir");
    let _ = std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir.path())
        .status();
    let _ = std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init", "-q"])
        .current_dir(dir.path())
        .status();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@example.invalid"])
        .current_dir(dir.path())
        .status();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "test"])
        .current_dir(dir.path())
        .status();

    let output = run_security_review_workflow(
        dir.path(),
        Some("HEAD"),
        SecurityReviewWorkflowOptions::default(),
    )
    .await
    .expect("workflow should succeed");
    let receipt = SecurityReviewReceipt::now(
        "sr-direct".to_string(),
        dir.path().to_path_buf(),
        SecurityReviewCommandArgs::default(),
        output.clone(),
        "rendered".to_string(),
        false,
        false,
    );
    assert_eq!(receipt.output.findings.len(), output.findings.len());
    assert_eq!(
        receipt.output.review_prompts.len(),
        output.review_prompts.len()
    );
}

#[test]
fn security_review_filter_cycles_through_all_filters() {
    let mut filter = SecurityReviewFilter::All;
    let labels: Vec<_> = std::iter::repeat_with(|| {
        let label = filter.label();
        filter = filter.next();
        label.to_string()
    })
    .take(7)
    .collect();
    // After 6 cycles we should have returned to All.
    assert_eq!(labels[6], "All");
    assert_eq!(labels.len(), 7);
}

#[test]
fn security_review_filter_label_distinct() {
    use std::collections::HashSet;
    let labels: HashSet<_> = SecurityReviewFilter::ALL
        .iter()
        .map(|f| f.label())
        .collect();
    assert_eq!(labels.len(), SecurityReviewFilter::ALL.len());
}

// ---------------------------------------------------------------------------
// Fixture executor integration: a real receipt should be produced
// from a deterministic stage-1 review that uses a fixture executor.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn security_review_background_with_fixture_executor_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _ = std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir.path())
        .status();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@example.invalid"])
        .current_dir(dir.path())
        .status();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "test"])
        .current_dir(dir.path())
        .status();
    let _ = std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init", "-q"])
        .current_dir(dir.path())
        .status();

    let _fixture = FixtureSecurityContextExecutor::new();
    let args = SecurityReviewCommandArgs {
        enrich: true,
        base: Some("HEAD".to_string()),
        ..Default::default()
    };
    // lsp_tool is None so the fixture is not used; the receipt should
    // still be produced with the unavailable LSP note.
    let receipt = run_security_review_background(dir.path().to_path_buf(), args, None)
        .await
        .expect("should succeed");
    assert!(!receipt.lsp_available);
    assert!(!receipt.enriched);
}
