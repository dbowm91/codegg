use std::path::PathBuf;

use codegg::security::workflow::{
    filter_panel_items, parse_security_review_args, project_receipt_to_panel_items,
    resolve_security_review_item_path, SecurityConfidence, SecurityEvidenceKind,
    SecurityReviewCommandArgs, SecurityReviewFilter, SecurityReviewFinding, SecurityReviewHunkLine,
    SecurityReviewHunkLineKind, SecurityReviewHunkRef, SecurityReviewOutput,
    SecurityReviewPanelItem, SecurityReviewPanelItemKind, SecurityReviewPrompt,
    SecurityReviewReceipt, SecurityReviewTarget, SecuritySeverity, SecurityTargetReason,
    StructuredSecurityEvidence,
};

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
        hunks: Vec::new(),
    }
}

fn sample_receipt() -> SecurityReviewReceipt {
    SecurityReviewReceipt::now(
        "sr-test-panel".to_string(),
        PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        sample_output(),
        "rendered text".to_string(),
        false,
        false,
    )
}

// ---------------------------------------------------------------------------
// 1. Default args do NOT auto-open panel
// ---------------------------------------------------------------------------

#[test]
fn security_review_completion_default_does_not_auto_open_panel() {
    let args = SecurityReviewCommandArgs::default();
    assert!(
        !args.open_panel_on_complete,
        "default SecurityReviewCommandArgs should have open_panel_on_complete == false"
    );
}

// ---------------------------------------------------------------------------
// 2. --panel flag sets open_panel_on_complete
// ---------------------------------------------------------------------------

#[test]
fn security_review_completion_panel_flag_opens_panel() {
    let args = parse_security_review_args("--panel");
    assert!(
        args.open_panel_on_complete,
        "--panel flag should set open_panel_on_complete = true"
    );
}

#[test]
fn security_review_completion_open_panel_alias() {
    let args = parse_security_review_args("--open-panel");
    assert!(
        args.open_panel_on_complete,
        "--open-panel alias should set open_panel_on_complete = true"
    );
}

#[test]
fn security_review_panel_flag_with_other_args() {
    let args = parse_security_review_args("--changed --panel --json");
    assert!(args.open_panel_on_complete);
    assert!(args.json);
    assert_eq!(args.base.as_deref(), Some("HEAD"));
}

// ---------------------------------------------------------------------------
// 7. resolve_security_review_item_path rejects path traversal
// ---------------------------------------------------------------------------

#[test]
fn security_review_source_preview_rejects_outside_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let receipt = SecurityReviewReceipt::now(
        "sr-traversal".to_string(),
        dir.path().to_path_buf(),
        SecurityReviewCommandArgs::default(),
        SecurityReviewOutput {
            targets: vec![],
            findings: vec![],
            review_prompts: vec![],
            preflight_results: vec![],
            notes: vec![],
            hunks: Vec::new(),
        },
        String::new(),
        false,
        false,
    );

    let item = SecurityReviewPanelItem {
        kind: SecurityReviewPanelItemKind::Finding,
        file_path: Some(PathBuf::from("../../etc/passwd")),
        line: None,
        title: "traversal".to_string(),
        severity: Some(SecuritySeverity::High),
        confidence: Some(SecurityConfidence::High),
        summary: String::new(),
        detail: vec![],
        hunk: None,
    };

    let result = resolve_security_review_item_path(&receipt, &item);
    assert!(
        result.is_err(),
        "path escaping root should fail, got: {result:?}"
    );
    let err = result.unwrap_err();
    // The path escapes the root, so resolution fails either with
    // "escapes review root" or "Cannot canonicalize" (the parent
    // resolves outside the root). Both are acceptable errors.
    assert!(
        err.contains("escapes review root") || err.contains("Cannot canonicalize"),
        "error should indicate traversal failure: {err}"
    );
}

#[test]
fn security_review_source_preview_rejects_absolute_path_outside_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let receipt = SecurityReviewReceipt::now(
        "sr-abs-traversal".to_string(),
        dir.path().to_path_buf(),
        SecurityReviewCommandArgs::default(),
        SecurityReviewOutput {
            targets: vec![],
            findings: vec![],
            review_prompts: vec![],
            preflight_results: vec![],
            notes: vec![],
            hunks: Vec::new(),
        },
        String::new(),
        false,
        false,
    );

    let item = SecurityReviewPanelItem {
        kind: SecurityReviewPanelItemKind::Finding,
        file_path: Some(PathBuf::from("/etc/passwd")),
        line: None,
        title: "abs traversal".to_string(),
        severity: Some(SecuritySeverity::High),
        confidence: Some(SecurityConfidence::High),
        summary: String::new(),
        detail: vec![],
        hunk: None,
    };

    let result = resolve_security_review_item_path(&receipt, &item);
    assert!(
        result.is_err(),
        "absolute path outside root should fail, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// 8. resolve_security_review_item_path handles missing file
// ---------------------------------------------------------------------------

#[test]
fn security_review_source_preview_handles_missing_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Create a parent directory so canonicalize can resolve it.
    let src_dir = dir.path().join("src");
    std::fs::create_dir(&src_dir).expect("create src dir");

    let receipt = SecurityReviewReceipt::now(
        "sr-missing".to_string(),
        dir.path().to_path_buf(),
        SecurityReviewCommandArgs::default(),
        SecurityReviewOutput {
            targets: vec![],
            findings: vec![],
            review_prompts: vec![],
            preflight_results: vec![],
            notes: vec![],
            hunks: Vec::new(),
        },
        String::new(),
        false,
        false,
    );

    // Item points to a file that does NOT exist, but the parent does.
    let item = SecurityReviewPanelItem {
        kind: SecurityReviewPanelItemKind::Finding,
        file_path: Some(PathBuf::from("src/missing.rs")),
        line: Some(10),
        title: "missing file".to_string(),
        severity: Some(SecuritySeverity::High),
        confidence: Some(SecurityConfidence::High),
        summary: String::new(),
        detail: vec![],
        hunk: None,
    };

    let result = resolve_security_review_item_path(&receipt, &item);
    assert!(
        result.is_ok(),
        "missing file with existing parent should succeed: {result:?}"
    );
    let path = result.unwrap();
    assert!(
        path.ends_with("src/missing.rs"),
        "resolved path should end with src/missing.rs, got: {path:?}"
    );
    assert!(
        path.is_absolute(),
        "resolved path should be absolute, got: {path:?}"
    );
}

#[test]
fn security_review_source_preview_rejects_missing_parent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let receipt = SecurityReviewReceipt::now(
        "sr-missing-parent".to_string(),
        dir.path().to_path_buf(),
        SecurityReviewCommandArgs::default(),
        SecurityReviewOutput {
            targets: vec![],
            findings: vec![],
            review_prompts: vec![],
            preflight_results: vec![],
            notes: vec![],
            hunks: Vec::new(),
        },
        String::new(),
        false,
        false,
    );

    let item = SecurityReviewPanelItem {
        kind: SecurityReviewPanelItemKind::Finding,
        file_path: Some(PathBuf::from("nonexistent_dir/file.rs")),
        line: None,
        title: "missing parent".to_string(),
        severity: Some(SecuritySeverity::High),
        confidence: Some(SecurityConfidence::High),
        summary: String::new(),
        detail: vec![],
        hunk: None,
    };

    let result = resolve_security_review_item_path(&receipt, &item);
    assert!(
        result.is_err(),
        "missing parent directory should fail, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// 8b. resolve_security_review_item_path with no file_path
// ---------------------------------------------------------------------------

#[test]
fn security_review_source_preview_no_file_path_returns_error() {
    let receipt = sample_receipt();
    let item = SecurityReviewPanelItem {
        kind: SecurityReviewPanelItemKind::Note,
        file_path: None,
        line: None,
        title: "note".to_string(),
        severity: None,
        confidence: None,
        summary: "a note".to_string(),
        detail: vec![],
        hunk: None,
    };
    let result = resolve_security_review_item_path(&receipt, &item);
    assert!(
        result.is_err(),
        "item with no file_path should fail: {result:?}"
    );
    assert!(result.unwrap_err().contains("no file path"));
}

// ---------------------------------------------------------------------------
// 11. Receipt Serialize/Deserialize roundtrip
// ---------------------------------------------------------------------------

#[test]
fn security_review_receipt_persistence_roundtrip() {
    let receipt = sample_receipt();
    let json = serde_json::to_string(&receipt).expect("serialize receipt");
    let restored: SecurityReviewReceipt = serde_json::from_str(&json).expect("deserialize receipt");
    assert_eq!(restored.id, receipt.id);
    assert_eq!(restored.root, receipt.root);
    assert_eq!(
        restored.output.findings.len(),
        receipt.output.findings.len()
    );
    assert_eq!(
        restored.output.review_prompts.len(),
        receipt.output.review_prompts.len()
    );
    assert_eq!(restored.rendered_report, receipt.rendered_report);
    assert_eq!(restored.enriched, receipt.enriched);
    assert_eq!(restored.lsp_available, receipt.lsp_available);
}

#[test]
fn security_review_receipt_default_panel_args_roundtrip() {
    let args = SecurityReviewCommandArgs::default();
    assert!(!args.open_panel_on_complete);
    let json = serde_json::to_string(&args).expect("serialize args");
    let restored: SecurityReviewCommandArgs =
        serde_json::from_str(&json).expect("deserialize args");
    assert!(!restored.open_panel_on_complete);
}

#[test]
fn security_review_receipt_panel_flag_roundtrip() {
    let args = SecurityReviewCommandArgs {
        open_panel_on_complete: true,
        base: Some("HEAD".to_string()),
        json: true,
        ..Default::default()
    };
    let json = serde_json::to_string(&args).expect("serialize args");
    let restored: SecurityReviewCommandArgs =
        serde_json::from_str(&json).expect("deserialize args");
    assert!(restored.open_panel_on_complete);
    assert_eq!(restored.base.as_deref(), Some("HEAD"));
    assert!(restored.json);
}

// ---------------------------------------------------------------------------
// 12. Prompt detail contains "Not a confirmed finding" marker
// ---------------------------------------------------------------------------

#[test]
fn security_review_prompt_detail_contains_not_confirmed_marker() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);
    let prompt_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == SecurityReviewPanelItemKind::Prompt)
        .collect();

    assert!(
        !prompt_items.is_empty(),
        "sample receipt should have at least one prompt"
    );

    for pi in &prompt_items {
        assert!(
            pi.detail
                .iter()
                .any(|d| d.contains("Not a confirmed finding")),
            "prompt detail should contain 'Not a confirmed finding' marker: {:?}",
            pi.detail
        );
    }
}

#[test]
fn security_review_prompt_detail_first_line_is_not_confirmed() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);
    let prompt_item = items
        .iter()
        .find(|i| i.kind == SecurityReviewPanelItemKind::Prompt)
        .expect("sample receipt should have a prompt");

    assert!(
        prompt_item.detail.first().map(|s| s.as_str())
            == Some("Not a confirmed finding — review prompt only."),
        "first detail line should be the not-confirmed marker, got: {:?}",
        prompt_item.detail.first()
    );
}

// ---------------------------------------------------------------------------
// 13. Notes filter includes Preflight items
// ---------------------------------------------------------------------------

#[test]
fn security_review_preflight_items_stay_under_notes_filter() {
    use codegg::security::workflow::{PreflightStatus, SecurityPreflightResult};

    let output = SecurityReviewOutput {
        targets: vec![],
        findings: vec![],
        review_prompts: vec![],
        preflight_results: vec![SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: vec!["api_key.rs: secret pattern".to_string()],
            structured_evidence: vec![],
            notes: vec!["Secret-like filename".to_string()],
        }],
        notes: vec!["a regular note".to_string()],
        hunks: Vec::new(),
    };

    let receipt = SecurityReviewReceipt::now(
        "sr-preflight-notes".to_string(),
        PathBuf::from("."),
        SecurityReviewCommandArgs::default(),
        output,
        String::new(),
        false,
        false,
    );

    let items = project_receipt_to_panel_items(&receipt);

    // All items should be either Note or Preflight (no findings/prompts).
    let notes_and_preflight: Vec<_> = items
        .iter()
        .filter(|i| {
            matches!(
                i.kind,
                SecurityReviewPanelItemKind::Note | SecurityReviewPanelItemKind::Preflight
            )
        })
        .collect();

    assert_eq!(
        notes_and_preflight.len(),
        items.len(),
        "all items should be Note or Preflight"
    );

    // Filter with Notes should include both kinds.
    let filtered = filter_panel_items(&items, SecurityReviewFilter::Notes);
    let has_preflight = filtered
        .iter()
        .any(|i| i.kind == SecurityReviewPanelItemKind::Preflight);
    let has_note = filtered
        .iter()
        .any(|i| i.kind == SecurityReviewPanelItemKind::Note);
    assert!(has_preflight, "Notes filter should include Preflight items");
    assert!(has_note, "Notes filter should include Note items");
    assert_eq!(filtered.len(), 2, "should have 1 preflight + 1 note");
}

#[test]
fn security_review_preflight_item_title_prefix() {
    use codegg::security::workflow::{PreflightStatus, SecurityPreflightResult};

    let output = SecurityReviewOutput {
        targets: vec![],
        findings: vec![],
        review_prompts: vec![],
        preflight_results: vec![SecurityPreflightResult {
            check_name: "sql_injection_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: vec![],
            structured_evidence: vec![],
            notes: vec![],
        }],
        notes: vec![],
        hunks: Vec::new(),
    };

    let receipt = SecurityReviewReceipt::now(
        "sr-preflight-title".to_string(),
        PathBuf::from("."),
        SecurityReviewCommandArgs::default(),
        output,
        String::new(),
        false,
        false,
    );

    let items = project_receipt_to_panel_items(&receipt);
    let pf = items
        .iter()
        .find(|i| i.kind == SecurityReviewPanelItemKind::Preflight)
        .expect("should have preflight item");

    assert!(
        pf.title.starts_with("[PREFLIGHT]"),
        "preflight title should start with [PREFLIGHT], got: {}",
        pf.title
    );
    assert!(
        pf.title.contains("sql_injection_scan"),
        "preflight title should contain check name, got: {}",
        pf.title
    );
}

// ---------------------------------------------------------------------------
// 15. Medium+ severity filter excludes prompts
// ---------------------------------------------------------------------------

#[test]
fn security_review_medium_plus_filter_excludes_prompts() {
    let receipt = sample_receipt();
    let items = project_receipt_to_panel_items(&receipt);
    let filtered = filter_panel_items(&items, SecurityReviewFilter::MediumOrHigherSeverity);

    assert!(
        filtered
            .iter()
            .all(|i| i.kind == SecurityReviewPanelItemKind::Finding),
        "Medium+ filter should only return findings, got kinds: {:?}",
        filtered.iter().map(|i| i.kind).collect::<Vec<_>>()
    );

    // Sample has 1 High + 1 Medium finding, both >= Medium.
    assert_eq!(filtered.len(), 2);
}

#[test]
fn security_review_medium_plus_filter_excludes_low_severity() {
    let output = SecurityReviewOutput {
        targets: vec![],
        findings: vec![finding("src/low.rs", 1, SecuritySeverity::Low)],
        review_prompts: vec![prompt("src/prompt.rs", 10)],
        preflight_results: vec![],
        notes: vec!["a note".to_string()],
        hunks: Vec::new(),
    };

    let receipt = SecurityReviewReceipt::now(
        "sr-low-sev".to_string(),
        PathBuf::from("."),
        SecurityReviewCommandArgs::default(),
        output,
        String::new(),
        false,
        false,
    );

    let items = project_receipt_to_panel_items(&receipt);
    let filtered = filter_panel_items(&items, SecurityReviewFilter::MediumOrHigherSeverity);

    // Low severity finding should be excluded, prompt excluded, note excluded.
    assert!(
        filtered.is_empty(),
        "Low severity finding should be excluded from Medium+ filter, got {}",
        filtered.len()
    );
}

// ---------------------------------------------------------------------------
// Resolve path with existing file
// ---------------------------------------------------------------------------

#[test]
fn security_review_source_preview_resolves_existing_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src_dir = dir.path().join("src");
    std::fs::create_dir(&src_dir).expect("create src dir");
    std::fs::write(src_dir.join("lib.rs"), "fn main() {}").expect("write file");

    let receipt = SecurityReviewReceipt::now(
        "sr-existing".to_string(),
        dir.path().to_path_buf(),
        SecurityReviewCommandArgs::default(),
        SecurityReviewOutput {
            targets: vec![],
            findings: vec![],
            review_prompts: vec![],
            preflight_results: vec![],
            notes: vec![],
            hunks: Vec::new(),
        },
        String::new(),
        false,
        false,
    );

    let item = SecurityReviewPanelItem {
        kind: SecurityReviewPanelItemKind::Finding,
        file_path: Some(PathBuf::from("src/lib.rs")),
        line: Some(5),
        title: "existing".to_string(),
        severity: Some(SecuritySeverity::High),
        confidence: Some(SecurityConfidence::High),
        summary: String::new(),
        detail: vec![],
        hunk: None,
    };

    let result = resolve_security_review_item_path(&receipt, &item);
    assert!(result.is_ok(), "existing file should resolve: {result:?}");
    let path = result.unwrap();
    assert!(path.ends_with("src/lib.rs"));
    assert!(path.is_absolute());
}

// ---------------------------------------------------------------------------
// Hunk mapping tests
// ---------------------------------------------------------------------------

fn sample_hunk_ref() -> SecurityReviewHunkRef {
    SecurityReviewHunkRef {
        file_path: PathBuf::from("src/lib.rs"),
        old_start: Some(8),
        old_lines: Some(6),
        new_start: Some(10),
        new_lines: Some(5),
        header: "@@ -8,6 +10,5 @@".to_string(),
        lines: vec![
            SecurityReviewHunkLine {
                old_line: Some(8),
                new_line: Some(10),
                kind: SecurityReviewHunkLineKind::Context,
                text: "let x = 1;".to_string(),
                is_focus: false,
            },
            SecurityReviewHunkLine {
                old_line: None,
                new_line: Some(11),
                kind: SecurityReviewHunkLineKind::Added,
                text: "let z = x + y;".to_string(),
                is_focus: true,
            },
            SecurityReviewHunkLine {
                old_line: Some(9),
                new_line: Some(12),
                kind: SecurityReviewHunkLineKind::Context,
                text: "let y = 2;".to_string(),
                is_focus: false,
            },
        ],
    }
}

#[test]
fn security_review_panel_item_includes_hunk_for_prompt_line_inside_hunk() {
    let output = SecurityReviewOutput {
        targets: vec![target("src/lib.rs", 11)],
        findings: vec![],
        review_prompts: vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(11),
            preset: "rust_server".to_string(),
            category: Some("unsafe".to_string()),
            title: "Review unsafe block".to_string(),
            rationale: "unsafe code detected".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }],
        preflight_results: vec![],
        notes: vec![],
        hunks: vec![sample_hunk_ref()],
    };
    let receipt = SecurityReviewReceipt::now(
        "sr-panel-hunk-prompt".to_string(),
        PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        output,
        "rendered".to_string(),
        false,
        false,
    );

    let items = project_receipt_to_panel_items(&receipt);
    let prompt_item = items
        .iter()
        .find(|i| i.kind == SecurityReviewPanelItemKind::Prompt)
        .expect("should have a prompt");

    // line 11 is inside new_start=10..10+5=15, so hunk should be attached
    assert!(
        prompt_item.hunk.is_some(),
        "prompt at line 11 should have hunk attached (inside range 10..15)"
    );
    let hunk = prompt_item.hunk.as_ref().unwrap();
    assert_eq!(hunk.file_path, PathBuf::from("src/lib.rs"));
    assert_eq!(hunk.lines.len(), 3);
}

#[test]
fn security_review_panel_item_includes_hunk_for_finding_line_inside_hunk() {
    let output = SecurityReviewOutput {
        targets: vec![target("src/lib.rs", 12)],
        findings: vec![finding("src/lib.rs", 12, SecuritySeverity::Medium)],
        review_prompts: vec![],
        preflight_results: vec![],
        notes: vec![],
        hunks: vec![sample_hunk_ref()],
    };
    let receipt = SecurityReviewReceipt::now(
        "sr-panel-hunk-finding".to_string(),
        PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        output,
        "rendered".to_string(),
        false,
        false,
    );

    let items = project_receipt_to_panel_items(&receipt);
    let finding_item = items
        .iter()
        .find(|i| i.kind == SecurityReviewPanelItemKind::Finding)
        .expect("should have a finding");

    // line 12 is inside new_start=10..10+5=15, so hunk should be attached
    assert!(
        finding_item.hunk.is_some(),
        "finding at line 12 should have hunk attached (inside range 10..15)"
    );
    let hunk = finding_item.hunk.as_ref().unwrap();
    assert_eq!(hunk.file_path, PathBuf::from("src/lib.rs"));
}

#[test]
fn security_review_panel_item_without_matching_hunk_has_none() {
    let output = SecurityReviewOutput {
        targets: vec![target("src/auth.rs", 50)],
        findings: vec![finding("src/auth.rs", 50, SecuritySeverity::High)],
        review_prompts: vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(50),
            preset: "web_backend".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth".to_string(),
            rationale: "auth code".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }],
        preflight_results: vec![],
        notes: vec![],
        hunks: vec![sample_hunk_ref()], // hunk is for src/lib.rs, not src/auth.rs
    };
    let receipt = SecurityReviewReceipt::now(
        "sr-panel-no-match".to_string(),
        PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        output,
        "rendered".to_string(),
        false,
        false,
    );

    let items = project_receipt_to_panel_items(&receipt);

    // Finding at line 50 in src/auth.rs — no matching hunk (hunk is for src/lib.rs)
    let finding_item = items
        .iter()
        .find(|i| i.kind == SecurityReviewPanelItemKind::Finding)
        .expect("should have a finding");
    assert!(
        finding_item.hunk.is_none(),
        "finding in src/auth.rs should not have hunk from src/lib.rs"
    );

    // Prompt at line 50 in src/auth.rs — same
    let prompt_item = items
        .iter()
        .find(|i| i.kind == SecurityReviewPanelItemKind::Prompt)
        .expect("should have a prompt");
    assert!(
        prompt_item.hunk.is_none(),
        "prompt in src/auth.rs should not have hunk from src/lib.rs"
    );
}

#[test]
fn security_review_hunk_context_is_bounded() {
    // Create a hunk with many lines and verify the count is reasonable
    let mut lines = Vec::new();
    for i in 0u32..50 {
        lines.push(SecurityReviewHunkLine {
            old_line: Some(i),
            new_line: Some(i),
            kind: SecurityReviewHunkLineKind::Context,
            text: format!("context line {i}"),
            is_focus: false,
        });
    }
    // 50 context lines is well under the 100 bound
    assert!(
        lines.len() < 100,
        "hunk lines count should be under 100, got {}",
        lines.len()
    );

    let hunk_ref = SecurityReviewHunkRef {
        file_path: PathBuf::from("src/big.rs"),
        old_start: Some(1),
        old_lines: Some(50),
        new_start: Some(1),
        new_lines: Some(50),
        header: "@@ -1,50 +1,50 @@".to_string(),
        lines,
    };

    let output = SecurityReviewOutput {
        targets: vec![target("src/big.rs", 25)],
        findings: vec![finding("src/big.rs", 25, SecuritySeverity::Low)],
        review_prompts: vec![],
        preflight_results: vec![],
        notes: vec![],
        hunks: vec![hunk_ref],
    };
    let receipt = SecurityReviewReceipt::now(
        "sr-hunk-bounded".to_string(),
        PathBuf::from("/tmp/proj"),
        SecurityReviewCommandArgs::default(),
        output,
        "rendered".to_string(),
        false,
        false,
    );

    let items = project_receipt_to_panel_items(&receipt);
    let finding_item = items
        .iter()
        .find(|i| i.kind == SecurityReviewPanelItemKind::Finding)
        .expect("should have a finding");
    let hunk = finding_item.hunk.as_ref().expect("should have hunk");
    assert!(
        hunk.lines.len() < 100,
        "hunk context lines should be bounded, got {}",
        hunk.lines.len()
    );
}
