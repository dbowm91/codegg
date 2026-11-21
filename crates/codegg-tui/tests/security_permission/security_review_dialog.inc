use std::path::PathBuf;

use codegg::security::workflow::{
    SecurityConfidence, SecurityEvidenceKind, SecurityReviewCommandArgs, SecurityReviewFilter,
    SecurityReviewFinding, SecurityReviewHunkLine, SecurityReviewHunkLineKind,
    SecurityReviewHunkRef, SecurityReviewOutput, SecurityReviewPrompt, SecurityReviewReceipt,
    SecurityReviewTarget, SecuritySeverity, SecurityTargetReason, StructuredSecurityEvidence,
};

#[test]
fn security_review_hunk_filter_clamps_selection() {
    use codegg_tui::components::dialogs::security_review::SecurityReviewDialog;
    let receipt_output = SecurityReviewOutput {
        targets: vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }],
        findings: vec![SecurityReviewFinding {
            severity: SecuritySeverity::High,
            confidence: SecurityConfidence::High,
            title: "Test finding".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            category: Some("auth".to_string()),
            evidence: vec![StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::RiskMarker,
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: Some(10),
                summary: "marker".to_string(),
                detail: None,
            }],
            reasoning: "reasoning".to_string(),
            recommendation: "recommendation".to_string(),
            tests: vec!["test_regression".to_string()],
        }],
        review_prompts: vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/db.rs"),
            line: Some(15),
            preset: "rust_server".to_string(),
            category: Some("unsafe".to_string()),
            title: "Review unsafe".to_string(),
            rationale: "rationale".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }],
        preflight_results: vec![],
        notes: vec!["risk markers are review prompts, not confirmed findings".to_string()],
        hunks: vec![SecurityReviewHunkRef {
            file_path: PathBuf::from("src/lib.rs"),
            old_start: Some(8),
            old_lines: Some(6),
            new_start: Some(10),
            new_lines: Some(5),
            header: "@@ -8,6 +10,5 @@".to_string(),
            lines: vec![],
        }],
    };
    let receipt = SecurityReviewReceipt::now(
        "sr-hunk-clamp".to_string(),
        PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        receipt_output,
        "rendered".to_string(),
        false,
        false,
        false,
        false,
        false,
        false,
        0,
        0,
    );

    let mut dialog = SecurityReviewDialog::with_receipt(receipt.clone());
    let all_count = dialog.visible_items.len();
    assert!(all_count > 0, "should have items");

    // Set selection beyond the hunk-backed subset
    dialog.selected_index = all_count; // out of bounds
    dialog.filter = SecurityReviewFilter::HunkBacked;
    dialog.set_receipt(Some(receipt));
    // After filter change, selection should be clamped
    assert!(
        dialog.selected_index < dialog.visible_items.len(),
        "selection should be clamped after HunkBacked filter, got {} (len={})",
        dialog.selected_index,
        dialog.visible_items.len()
    );
}

#[test]
fn security_review_hunk_navigation_wraps_next_previous() {
    use codegg_tui::components::component::Component;
    use codegg_tui::components::dialogs::security_review::SecurityReviewDialog;
    let output = SecurityReviewOutput {
        targets: vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/a.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }],
        findings: vec![
            SecurityReviewFinding {
                severity: SecuritySeverity::High,
                confidence: SecurityConfidence::High,
                title: "Finding A".to_string(),
                file_path: PathBuf::from("src/a.rs"),
                line: Some(10),
                category: None,
                evidence: vec![StructuredSecurityEvidence {
                    kind: SecurityEvidenceKind::RiskMarker,
                    file_path: Some(PathBuf::from("src/a.rs")),
                    line: Some(10),
                    summary: "marker".to_string(),
                    detail: None,
                }],
                reasoning: "r".to_string(),
                recommendation: "rec".to_string(),
                tests: vec![],
            },
            SecurityReviewFinding {
                severity: SecuritySeverity::Medium,
                confidence: SecurityConfidence::Medium,
                title: "Finding B".to_string(),
                file_path: PathBuf::from("src/b.rs"),
                line: None, // no line = no hunk
                category: None,
                evidence: vec![StructuredSecurityEvidence {
                    kind: SecurityEvidenceKind::RiskMarker,
                    file_path: Some(PathBuf::from("src/b.rs")),
                    line: None,
                    summary: "marker".to_string(),
                    detail: None,
                }],
                reasoning: "r".to_string(),
                recommendation: "rec".to_string(),
                tests: vec![],
            },
            SecurityReviewFinding {
                severity: SecuritySeverity::Low,
                confidence: SecurityConfidence::Low,
                title: "Finding C".to_string(),
                file_path: PathBuf::from("src/a.rs"),
                line: Some(12),
                category: None,
                evidence: vec![StructuredSecurityEvidence {
                    kind: SecurityEvidenceKind::RiskMarker,
                    file_path: Some(PathBuf::from("src/a.rs")),
                    line: Some(12),
                    summary: "marker".to_string(),
                    detail: None,
                }],
                reasoning: "r".to_string(),
                recommendation: "rec".to_string(),
                tests: vec![],
            },
        ],
        review_prompts: vec![],
        preflight_results: vec![],
        notes: vec![],
        hunks: vec![SecurityReviewHunkRef {
            file_path: PathBuf::from("src/a.rs"),
            old_start: Some(8),
            old_lines: Some(6),
            new_start: Some(10),
            new_lines: Some(5),
            header: "@@ -8,6 +10,5 @@".to_string(),
            lines: vec![
                SecurityReviewHunkLine {
                    old_line: Some(8),
                    new_line: Some(10),
                    kind: SecurityReviewHunkLineKind::Added,
                    text: "new code A".to_string(),
                },
                SecurityReviewHunkLine {
                    old_line: None,
                    new_line: Some(12),
                    kind: SecurityReviewHunkLineKind::Added,
                    text: "new code C".to_string(),
                },
            ],
        }],
    };
    let receipt = SecurityReviewReceipt::now(
        "sr-nav".to_string(),
        PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        output,
        "rendered".to_string(),
        false,
        false,
        false,
        false,
        false,
        false,
        0,
        0,
    );

    let mut dialog = SecurityReviewDialog::with_receipt(receipt);
    // Items: [Finding A (hunk), Finding B (no hunk), Finding C (hunk)] = 3 items
    assert_eq!(dialog.visible_items.len(), 3);

    // Start at index 0 (Finding A, hunk-backed)
    dialog.selected_index = 0;

    // Press ] (next hunk) should move to index 2 (Finding C, hunk-backed)
    let key_next = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(']'),
        crossterm::event::KeyModifiers::NONE,
    );
    dialog.handle_key(key_next);
    assert_eq!(dialog.selected_index, 2, "next hunk from A should go to C");

    // Press ] again should wrap to index 0 (Finding A)
    dialog.handle_key(key_next);
    assert_eq!(
        dialog.selected_index, 0,
        "next hunk from C should wrap to A"
    );

    // Press [ (prev hunk) from index 0 should wrap to index 2
    let key_prev = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('['),
        crossterm::event::KeyModifiers::NONE,
    );
    dialog.handle_key(key_prev);
    assert_eq!(
        dialog.selected_index, 2,
        "prev hunk from A should wrap to C"
    );

    // Press [ again should go to index 0
    dialog.handle_key(key_prev);
    assert_eq!(dialog.selected_index, 0, "prev hunk from C should go to A");
}

#[test]
fn security_review_hunk_jump_sets_detail_scroll() {
    use codegg_tui::components::component::Component;
    use codegg_tui::components::dialogs::security_review::SecurityReviewDialog;
    let output = SecurityReviewOutput {
        targets: vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }],
        findings: vec![SecurityReviewFinding {
            severity: SecuritySeverity::High,
            confidence: SecurityConfidence::High,
            title: "Test finding".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            category: Some("auth".to_string()),
            evidence: vec![StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::RiskMarker,
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: Some(10),
                summary: "marker".to_string(),
                detail: None,
            }],
            reasoning: "reasoning".to_string(),
            recommendation: "recommendation".to_string(),
            tests: vec!["test_regression".to_string()],
        }],
        review_prompts: vec![],
        preflight_results: vec![],
        notes: vec![],
        hunks: vec![SecurityReviewHunkRef {
            file_path: PathBuf::from("src/lib.rs"),
            old_start: Some(8),
            old_lines: Some(6),
            new_start: Some(10),
            new_lines: Some(5),
            header: "@@ -8,6 +10,5 @@".to_string(),
            lines: vec![SecurityReviewHunkLine {
                old_line: None,
                new_line: Some(11),
                kind: SecurityReviewHunkLineKind::Added,
                text: "new code".to_string(),
            }],
        }],
    };
    let receipt = SecurityReviewReceipt::now(
        "sr-jump".to_string(),
        PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        output,
        "rendered".to_string(),
        false,
        false,
        false,
        false,
        false,
        false,
        0,
        0,
    );

    let mut dialog = SecurityReviewDialog::with_receipt(receipt);
    assert_eq!(dialog.detail_scroll, 0);

    // Press h (jump to hunk) — should set detail_scroll > 0
    let key_h = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('h'),
        crossterm::event::KeyModifiers::NONE,
    );
    dialog.handle_key(key_h);
    assert!(
        dialog.detail_scroll > 0,
        "jump_to_hunk should set detail_scroll to a non-zero value, got {}",
        dialog.detail_scroll
    );
}

#[test]
fn security_review_hunk_key_noops_when_no_hunk() {
    use codegg_tui::components::component::Component;
    use codegg_tui::components::dialogs::security_review::SecurityReviewDialog;
    let output = SecurityReviewOutput {
        targets: vec![],
        findings: vec![SecurityReviewFinding {
            severity: SecuritySeverity::Medium,
            confidence: SecurityConfidence::Medium,
            title: "No-hunk finding".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line: None, // no line = no hunk attachment
            category: None,
            evidence: vec![StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::RiskMarker,
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: None,
                summary: "marker".to_string(),
                detail: None,
            }],
            reasoning: "r".to_string(),
            recommendation: "rec".to_string(),
            tests: vec![],
        }],
        review_prompts: vec![],
        preflight_results: vec![],
        notes: vec![],
        hunks: vec![], // no hunks
    };
    let receipt = SecurityReviewReceipt::now(
        "sr-no-hunk".to_string(),
        PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        output,
        "rendered".to_string(),
        false,
        false,
        false,
        false,
        false,
        false,
        0,
        0,
    );

    let mut dialog = SecurityReviewDialog::with_receipt(receipt);
    assert_eq!(dialog.detail_scroll, 0);
    assert!(dialog.selected_item().unwrap().hunk.is_none());

    let key_h = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('h'),
        crossterm::event::KeyModifiers::NONE,
    );
    dialog.handle_key(key_h);
    assert_eq!(
        dialog.detail_scroll, 0,
        "jump_to_hunk on non-hunk item should not change detail_scroll"
    );
}
