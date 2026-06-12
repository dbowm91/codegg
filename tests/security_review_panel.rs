use std::path::PathBuf;

use codegg::security::workflow::{
    filter_panel_items, parse_security_review_args, project_receipt_to_panel_items,
    resolve_security_review_item_path, SecurityConfidence, SecurityEvidenceKind,
    SecurityReviewCommandArgs, SecurityReviewFilter, SecurityReviewFinding, SecurityReviewOutput,
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
    };

    let result = resolve_security_review_item_path(&receipt, &item);
    assert!(result.is_ok(), "existing file should resolve: {result:?}");
    let path = result.unwrap();
    assert!(path.ends_with("src/lib.rs"));
    assert!(path.is_absolute());
}
