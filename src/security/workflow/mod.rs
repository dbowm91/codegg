//! Security review vertical slice: diff parsing, preset selection, target
//! building, securityContext request construction, review-prompt generation,
//! and evidence-based finding synthesis.
//!
//! This module is intentionally decoupled from the LSP layer so it can run
//! without a language server.  Risk markers become review prompts unless
//! additional evidence supports a concrete finding.  Finding synthesis is
//! conservative: severity and confidence are deterministic enums, structured
//! evidence tracks provenance, and outputs are never proof of exploitability.

pub mod context;
pub mod diff;
pub mod enrichment;
pub mod evidence;
pub mod preflight;
pub mod report;
pub mod types;

pub use context::*;
pub use diff::*;
pub use enrichment::*;
pub use evidence::*;
pub use preflight::*;
pub use report::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    // -- Hunk parser tests --

    #[test]
    fn security_review_parse_single_hunk() {
        let patch = "\
diff --git a/src/lib.rs b/src/lib.rs
index abc1234..def5678 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,6 +10,8 @@ fn example() {
     let x = 1;
     let y = 2;
+    let z = x + y;
+    assert!(z > 0);
 }
";

        let hunks = parse_changed_hunks(patch);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, PathBuf::from("src/lib.rs"));
        assert_eq!(hunks[0].old_start, 10);
        assert_eq!(hunks[0].old_count, 6);
        assert_eq!(hunks[0].new_start, 10);
        assert_eq!(hunks[0].new_count, 8);
    }

    #[test]
    fn security_review_parse_multiple_files() {
        let patch = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,4 @@
+use std::path::Path;
 fn a() {}
 fn b() {}
@@ -10,2 +11,3 @@
+    let p = Path::new(\".\");
     println!(\"hi\");
 }
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -5,3 +5,4 @@
     let a = 1;
+    let b = 2;
+    let c = 3;
     println!(\"{a}\");
";

        let hunks = parse_changed_hunks(patch);
        assert_eq!(hunks.len(), 3);
        assert_eq!(hunks[0].file_path, PathBuf::from("src/a.rs"));
        assert_eq!(hunks[1].file_path, PathBuf::from("src/a.rs"));
        assert_eq!(hunks[2].file_path, PathBuf::from("src/b.rs"));
    }

    #[test]
    fn security_review_parse_omitted_hunk_counts() {
        // @@ -1 +1,2 @@ means old_count omitted (treated as 1)
        let hunk = parse_hunk_header("@@ -1 +1,2 @@", Some(Path::new("a.rs")));
        assert!(hunk.is_some());
        let h = hunk.unwrap();
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 1);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 2);
    }

    #[test]
    fn security_review_skips_deleted_file() {
        let patch = "\
diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
--- a/src/old.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-fn old() {}
-fn also_old() {}
-fn third() {}
";
        let hunks = parse_changed_hunks(patch);
        assert!(hunks.is_empty());
    }

    #[test]
    fn security_review_skips_binary_file() {
        let patch = "\
diff --git a/image.png b/image.png
Binary files a/image.png and b/image.png differ
";
        let hunks = parse_changed_hunks(patch);
        assert!(hunks.is_empty());
    }

    #[test]
    fn parse_range_basic() {
        assert_eq!(parse_range("10,6"), Some((10, 6)));
        assert_eq!(parse_range("1"), Some((1, 1)));
        assert_eq!(parse_range("0"), Some((0, 1)));
        assert!(parse_range("").is_none());
        assert!(parse_range("abc").is_none());
    }

    #[test]
    fn parse_hunk_line_no_file_returns_none() {
        let hunk = parse_hunk_header("@@ -1,3 +1,4 @@", None);
        assert!(hunk.is_none());
    }

    // -- Exclusion tests --

    #[test]
    fn security_review_excludes_vendor_target_node_modules() {
        assert!(is_security_review_excluded_path(Path::new("vendor/foo.rs")));
        assert!(is_security_review_excluded_path(Path::new(
            "target/debug/binary"
        )));
        assert!(is_security_review_excluded_path(Path::new(
            "node_modules/pkg/index.js"
        )));
        assert!(is_security_review_excluded_path(Path::new(
            "third_party/lib.rs"
        )));
        assert!(is_security_review_excluded_path(Path::new(
            "dist/bundle.js"
        )));
        assert!(is_security_review_excluded_path(Path::new(
            "build/output.rs"
        )));
        assert!(is_security_review_excluded_path(Path::new(
            "src/bundle.min.js"
        )));
        assert!(is_security_review_excluded_path(Path::new(".git/HEAD")));
        assert!(is_security_review_excluded_path(Path::new(
            "__pycache__/mod.pyc"
        )));
    }

    #[test]
    fn security_review_keeps_cargo_manifest_lock_and_build_rs() {
        assert!(!is_security_review_excluded_path(Path::new("Cargo.toml")));
        assert!(!is_security_review_excluded_path(Path::new("Cargo.lock")));
        assert!(!is_security_review_excluded_path(Path::new("build.rs")));
        assert!(!is_security_review_excluded_path(Path::new("src/lib.rs")));
        assert!(!is_security_review_excluded_path(Path::new("README.md")));
    }

    #[test]
    fn should_skip_hidden_files() {
        assert!(should_skip_file(Path::new(".gitignore")));
        assert!(should_skip_file(Path::new(".DS_Store")));
        assert!(should_skip_file(Path::new(".hidden")));
    }

    #[test]
    fn should_not_skip_env_files() {
        assert!(!should_skip_file(Path::new(".env")));
        assert!(!should_skip_file(Path::new(".env.local")));
        assert!(!should_skip_file(Path::new(".env.production")));
    }

    #[test]
    fn should_skip_lock_files() {
        // .lock files should NOT be skipped (kept for dependency_review)
        assert!(!should_skip_file(Path::new("Cargo.lock")));
        // Binary extensions should still be skipped
        assert!(should_skip_file(Path::new("lib.dll")));
        assert!(should_skip_file(Path::new("lib.so")));
        assert!(should_skip_file(Path::new("lib.dylib")));
    }

    // -- Preset selection tests --

    #[test]
    fn security_review_selects_dependency_review_for_cargo_toml() {
        assert_eq!(
            select_security_preset(Path::new("Cargo.toml"), None),
            "dependency_review"
        );
    }

    #[test]
    fn security_review_selects_dependency_review_for_cargo_lock() {
        assert_eq!(
            select_security_preset(Path::new("Cargo.lock"), None),
            "dependency_review"
        );
    }

    #[test]
    fn security_review_selects_dependency_review_for_build_rs() {
        assert_eq!(
            select_security_preset(Path::new("build.rs"), None),
            "dependency_review"
        );
    }

    #[test]
    fn security_review_selects_dependency_review_for_package_json() {
        assert_eq!(
            select_security_preset(Path::new("src/package.json"), None),
            "dependency_review"
        );
    }

    #[test]
    fn security_review_selects_unsafe_review_for_unsafe_content() {
        assert_eq!(
            select_security_preset(
                Path::new("src/lib.rs"),
                Some("fn foo() { unsafe { ptr::read() } }")
            ),
            "unsafe_review"
        );
    }

    #[test]
    fn security_review_selects_unsafe_review_for_unsafe_path() {
        assert_eq!(
            select_security_preset(Path::new("src/unsafe_ops.rs"), None),
            "unsafe_review"
        );
    }

    #[test]
    fn security_review_selects_unsafe_review_for_ffi_content() {
        assert_eq!(
            select_security_preset(
                Path::new("src/lib.rs"),
                Some("extern \"C\" { fn malloc(); }")
            ),
            "unsafe_review"
        );
    }

    #[test]
    fn security_review_selects_web_backend_for_auth_handler() {
        assert_eq!(
            select_security_preset(Path::new("src/auth/handler.rs"), None),
            "web_backend"
        );
    }

    #[test]
    fn security_review_selects_web_backend_for_auth_content() {
        assert_eq!(
            select_security_preset(
                Path::new("src/handler.rs"),
                Some("fn handle_request(session: &Session) { }")
            ),
            "web_backend"
        );
    }

    #[test]
    fn security_review_selects_rust_cli_for_command_process() {
        assert_eq!(
            select_security_preset(Path::new("src/command/process.rs"), None),
            "rust_cli"
        );
    }

    #[test]
    fn security_review_selects_rust_cli_for_cli_content() {
        assert_eq!(
            select_security_preset(
                Path::new("src/main.rs"),
                Some("let args: Vec<String> = std::env::args().collect();")
            ),
            "rust_cli"
        );
    }

    #[test]
    fn security_review_defaults_to_rust_server_for_rs_file() {
        assert_eq!(
            select_security_preset(Path::new("src/lib.rs"), None),
            "rust_server"
        );
        assert_eq!(
            select_security_preset(Path::new("src/model.rs"), None),
            "rust_server"
        );
    }

    #[test]
    fn select_preset_for_file_legacy() {
        assert_eq!(
            select_preset_for_file(Path::new("Cargo.toml")),
            "dependency_review"
        );
        assert_eq!(
            select_preset_for_file(Path::new("src/lib.rs")),
            "rust_server"
        );
    }

    // -- Target building tests --

    #[test]
    fn security_review_builds_targets_from_hunks() {
        let hunks = vec![
            ChangedHunk {
                file_path: PathBuf::from("src/lib.rs"),
                old_start: 10,
                old_count: 3,
                new_start: 10,
                new_count: 5,
            },
            ChangedHunk {
                file_path: PathBuf::from("src/auth.rs"),
                old_start: 20,
                old_count: 2,
                new_start: 20,
                new_count: 4,
            },
        ];

        let targets = build_security_review_targets(&hunks, |_| None);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].file_path, PathBuf::from("src/lib.rs"));
        assert_eq!(targets[0].line, Some(10));
        assert_eq!(targets[0].column, Some(1));
        assert_eq!(targets[0].preset, "rust_server");
        assert_eq!(targets[1].file_path, PathBuf::from("src/auth.rs"));
        assert_eq!(targets[1].preset, "web_backend");
    }

    #[test]
    fn security_review_dedupes_targets() {
        let hunks = vec![
            ChangedHunk {
                file_path: PathBuf::from("src/lib.rs"),
                old_start: 10,
                old_count: 3,
                new_start: 10,
                new_count: 5,
            },
            ChangedHunk {
                file_path: PathBuf::from("src/lib.rs"),
                old_start: 10,
                old_count: 3,
                new_start: 10,
                new_count: 5,
            },
        ];

        let targets = build_security_review_targets(&hunks, |_| None);
        assert_eq!(targets.len(), 1);
    }

    #[test]
    fn security_review_assigns_reason_from_preset_or_content() {
        let hunks = vec![
            ChangedHunk {
                file_path: PathBuf::from("Cargo.toml"),
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 2,
            },
            ChangedHunk {
                file_path: PathBuf::from("src/unsafe_block.rs"),
                old_start: 5,
                old_count: 1,
                new_start: 5,
                new_count: 3,
            },
        ];

        let targets = build_security_review_targets(&hunks, |_| None);
        assert_eq!(targets.len(), 2);

        let cargo_target = targets
            .iter()
            .find(|t| t.file_path == *"Cargo.toml")
            .unwrap();
        assert_eq!(
            cargo_target.reason,
            SecurityTargetReason::DependencyMetadata
        );

        let unsafe_target = targets
            .iter()
            .find(|t| t.file_path == *"src/unsafe_block.rs")
            .unwrap();
        assert_eq!(unsafe_target.reason, SecurityTargetReason::UnsafeCode);
    }

    #[test]
    fn security_review_skips_excluded_paths_in_targets() {
        let hunks = vec![
            ChangedHunk {
                file_path: PathBuf::from("vendor/lib.rs"),
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 2,
            },
            ChangedHunk {
                file_path: PathBuf::from("src/lib.rs"),
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 2,
            },
        ];

        let targets = build_security_review_targets(&hunks, |_| None);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].file_path, PathBuf::from("src/lib.rs"));
    }

    // -- Request builder tests --

    #[test]
    fn security_review_builds_security_context_request_with_preset() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/handler.rs"),
            line: Some(42),
            column: Some(1),
            preset: "web_backend".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };

        let req = build_security_context_request(&target);
        assert_eq!(req["operation"], "securityContext");
        assert_eq!(req["file_path"], "src/handler.rs");
        assert_eq!(req["security_preset"], "web_backend");
        assert_eq!(req["max_risk_markers"], 80);
        assert_eq!(req["call_depth"], 0);
        assert_eq!(req["line"], 42);
        assert_eq!(req["column"], 1);
    }

    #[test]
    fn security_review_request_omits_line_column_when_target_unpositioned() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("Cargo.toml"),
            line: None,
            column: None,
            preset: "dependency_review".to_string(),
            reason: SecurityTargetReason::DependencyMetadata,
        };

        let req = build_security_context_request(&target);
        assert_eq!(req["operation"], "securityContext");
        assert!(req.get("line").is_none());
        assert!(req.get("column").is_none());
    }

    #[test]
    fn security_review_request_keeps_call_depth_zero() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(1),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };

        let req = build_security_context_request(&target);
        assert_eq!(req["call_depth"], 0);
    }

    // -- Prompt generation tests --

    #[test]
    fn security_review_marker_becomes_review_prompt() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(42),
            column: Some(1),
            preset: "web_backend".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };

        let context_json = serde_json::json!({
            "risk_markers": [
                {
                    "category": "auth",
                    "label": "jwt handling",
                    "file": "src/auth.rs",
                    "line": 42,
                    "matched_text": "jwt::decode(token)",
                    "rationale": "Token flows from request to decode call"
                }
            ],
            "truncated": false
        });

        let prompts = prompts_from_security_context(&target, &context_json);
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].title, "Review auth: jwt handling");
        assert_eq!(prompts[0].file_path, PathBuf::from("src/auth.rs"));
        assert_eq!(prompts[0].line, Some(42));
        assert_eq!(prompts[0].preset, "web_backend");
        assert!(!prompts[0].evidence.is_empty());
    }

    #[test]
    fn security_review_marker_only_does_not_create_finding() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };

        let context_json = serde_json::json!({
            "risk_markers": [
                {
                    "category": "unsafe",
                    "label": "unsafe block",
                    "line": 10,
                    "matched_text": "unsafe { }",
                    "rationale": "Potential unsafe code usage"
                }
            ],
            "truncated": false
        });

        let prompts = prompts_from_security_context(&target, &context_json);
        // Markers become prompts, never findings
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].title.starts_with("Review "));
    }

    #[test]
    fn security_review_truncated_context_adds_prompt_evidence() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };

        let context_json = serde_json::json!({
            "risk_markers": [
                {
                    "category": "crypto",
                    "label": "hardcoded key",
                    "line": 10,
                    "matched_text": "KEY = b\"secret\"",
                    "rationale": "Hardcoded cryptographic key"
                }
            ],
            "truncated": true
        });

        let prompts = prompts_from_security_context(&target, &context_json);
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].evidence.iter().any(|e| e.contains("truncated")));
    }

    #[test]
    fn security_review_malformed_json_returns_empty_prompts() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };

        let context_json = serde_json::json!({
            "not_risk_markers": []
        });

        let prompts = prompts_from_security_context(&target, &context_json);
        assert!(prompts.is_empty());
    }

    // -- Report assembly tests --

    #[test]
    fn security_review_report_includes_marker_not_finding_note() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];

        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            preset: "rust_server".to_string(),
            category: Some("unsafe".to_string()),
            title: "Review unsafe: block".to_string(),
            rationale: "Potential issue".to_string(),
            evidence: vec!["unsafe".to_string()],
        }];

        let report = assemble_security_review_report(targets, prompts, Vec::new());
        assert!(report.findings.is_empty());
        assert_eq!(report.targets.len(), 1);
        assert_eq!(report.review_prompts.len(), 1);
        assert!(report
            .notes
            .iter()
            .any(|n| n.contains("not confirmed findings")));
    }

    // -- Plan from diff test --

    #[test]
    fn security_review_plan_from_diff_basic() {
        let patch = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,2 +10,4 @@
+    let z = x + y;
+    assert!(z > 0);
 }
";

        let report = plan_security_review_from_diff(patch, Path::new("."));
        assert_eq!(report.targets.len(), 1);
        assert_eq!(report.review_prompts.len(), 1);
        assert!(report.findings.is_empty());
        assert!(report.notes.iter().any(|n| n.contains("planned from diff")));
        assert!(report
            .notes
            .iter()
            .any(|n| n.contains("not confirmed findings")));
    }

    #[test]
    fn security_review_plan_from_diff_cargo_toml() {
        let patch = "\
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -1,3 +1,4 @@
+serde = { version = \"1\", features = [\"derive\"] }
 [package]
 name = \"my-app\"
";

        let report = plan_security_review_from_diff(patch, Path::new("."));
        assert_eq!(report.targets.len(), 1);
        assert_eq!(report.targets[0].preset, "dependency_review");
        assert_eq!(
            report.targets[0].reason,
            SecurityTargetReason::DependencyMetadata
        );
    }

    #[test]
    fn security_review_plan_from_diff_unsafe() {
        let patch = "\
diff --git a/src/unsafe_block.rs b/src/unsafe_block.rs
--- a/src/unsafe_block.rs
+++ b/src/unsafe_block.rs
@@ -5,2 +5,4 @@
+    unsafe {
+        *ptr = 42;
+    }
 }
";

        let report = plan_security_review_from_diff(patch, Path::new("."));
        assert_eq!(report.targets.len(), 1);
        assert_eq!(report.targets[0].preset, "unsafe_review");
        assert_eq!(report.targets[0].reason, SecurityTargetReason::UnsafeCode);
    }

    // -- Synthesize findings tests (vertical slice: always prompts) --

    #[test]
    fn synthesize_findings_marker_only_produces_prompt() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: None,
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];

        let markers = vec![SecurityRiskMarkerFromWorkflow {
            category: "unsafe_code".to_string(),
            label: "unsafe block".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line: 10,
            column: 5,
            matched_text: "unsafe { }".to_string(),
            rationale: "Potential unsafe code usage".to_string(),
        }];

        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: Vec::new(),
        }];

        let (findings, prompts) = synthesize_findings(&targets, &markers, &preflight);

        // Vertical slice: findings are always empty
        assert!(findings.is_empty());
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].category, Some("unsafe_code".to_string()));
        assert_eq!(prompts[0].line, Some(10));
    }

    #[test]
    fn synthesize_findings_marker_with_flow_still_produces_prompt() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(42),
            column: None,
            preset: "web_backend".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];

        let markers = vec![SecurityRiskMarkerFromWorkflow {
            category: "auth".to_string(),
            label: "jwt handling".to_string(),
            file_path: PathBuf::from("src/auth.rs"),
            line: 42,
            column: 0,
            matched_text: "jwt::decode(token)".to_string(),
            rationale: "Token flows from request to decode call".to_string(),
        }];

        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: Vec::new(),
        }];

        let (findings, prompts) = synthesize_findings(&targets, &markers, &preflight);

        // Vertical slice: even "flow" markers become prompts, not findings
        assert!(findings.is_empty());
        assert_eq!(prompts.len(), 1);
    }

    #[test]
    fn synthesize_findings_preflight_failure_produces_prompt() {
        let targets = vec![];
        let markers = vec![];
        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: vec!["api_key.rs: secret pattern in name".to_string()],
            structured_evidence: Vec::new(),
            notes: vec!["Secret-like patterns found".to_string()],
        }];

        let (findings, prompts) = synthesize_findings(&targets, &markers, &preflight);

        assert!(findings.is_empty());
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].title.contains("secret_filename_hint_scan"));
    }

    #[test]
    fn synthesize_findings_preflight_pass_no_prompts() {
        let targets = vec![];
        let markers = vec![];
        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: Vec::new(),
        }];

        let (findings, prompts) = synthesize_findings(&targets, &markers, &preflight);

        assert!(findings.is_empty());
        assert!(prompts.is_empty());
    }

    // -- Preflight check tests --

    #[test]
    fn run_preflight_checks_empty_targets() {
        let results = run_preflight_checks(&[]);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.status == PreflightStatus::Pass));
    }

    #[test]
    fn run_preflight_checks_secret_in_name() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("api_key.rs"),
            line: None,
            column: None,
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];

        let results = run_preflight_checks(&targets);
        let secret_result = results
            .iter()
            .find(|r| r.check_name == "secret_filename_hint_scan")
            .unwrap();
        assert_eq!(secret_result.status, PreflightStatus::Fail);
    }

    #[test]
    fn run_preflight_checks_normal_file_names() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("handler.rs"),
            line: None,
            column: None,
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];

        let results = run_preflight_checks(&targets);
        assert!(results.iter().all(|r| r.status == PreflightStatus::Pass));
    }

    // -- Risk reason tests --

    #[test]
    fn is_high_risk_reason_check() {
        assert!(is_high_risk_reason(&SecurityTargetReason::UnsafeCode));
        assert!(is_high_risk_reason(&SecurityTargetReason::ProcessExecution));
        assert!(is_high_risk_reason(&SecurityTargetReason::NetworkBoundary));
        assert!(is_high_risk_reason(
            &SecurityTargetReason::AuthOrSecretHandling
        ));

        assert!(!is_high_risk_reason(&SecurityTargetReason::ChangedHunk));
        assert!(!is_high_risk_reason(&SecurityTargetReason::RiskMarker));
        assert!(!is_high_risk_reason(&SecurityTargetReason::PublicBoundary));
        assert!(!is_high_risk_reason(
            &SecurityTargetReason::FilesystemAccess
        ));
    }

    // -- Diff parsing edge cases --

    #[test]
    fn parse_unified_diff_hunks_empty() {
        let hunks = parse_changed_hunks("");
        assert!(hunks.is_empty());
    }

    #[test]
    fn parse_unified_diff_hunks_no_hunks() {
        let patch = "\
diff --git a/src/lib.rs b/src/lib.rs
index abc1234..def5678 100644
--- a/src/lib.rs
+++ b/src/lib.rs
";
        let hunks = parse_changed_hunks(patch);
        assert!(hunks.is_empty());
    }

    #[test]
    fn should_not_skip_normal_files() {
        assert!(!should_skip_file(Path::new("src/lib.rs")));
        assert!(!should_skip_file(Path::new("README.md")));
        assert!(!should_skip_file(Path::new("src/main.rs")));
    }

    // -- Per-file diff parser tests --

    #[test]
    fn security_review_parse_hunks_for_file_without_diff_git_header() {
        // A per-file patch that only contains hunk headers, no diff --git line
        let patch = "\
@@ -10,6 +10,8 @@ fn example() {
     let x = 1;
     let y = 2;
+    let z = x + y;
+    assert!(z > 0);
 }
";
        let hunks = parse_changed_hunks_for_file(patch, Path::new("src/lib.rs"));
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, Path::new("src/lib.rs"));
        assert_eq!(hunks[0].new_start, 10);
        assert_eq!(hunks[0].new_count, 8);
    }

    #[test]
    fn security_review_parse_hunks_for_file_prefers_embedded_diff_path() {
        // When a full diff --git header is present, it takes precedence
        let patch = "\
diff --git a/src/other.rs b/src/other.rs
--- a/src/other.rs
+++ b/src/other.rs
@@ -1,3 +1,4 @@
+use std::path::Path;
 fn a() {}
 fn b() {}
";
        let hunks = parse_changed_hunks_for_file(patch, Path::new("src/lib.rs"));
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, Path::new("src/other.rs"));
    }

    #[test]
    fn security_review_parse_hunks_for_file_skips_deleted_or_binary() {
        // A per-file patch with +++ /dev/null (deletion marker) should produce no hunks
        let patch = "\
--- a/src/old.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-fn old() {}
-fn also_old() {}
-fn third() {}
";
        let hunks = parse_changed_hunks_for_file(patch, Path::new("src/old.rs"));
        assert!(hunks.is_empty());
    }

    #[test]
    fn security_review_parse_hunks_for_file_empty_patch() {
        let hunks = parse_changed_hunks_for_file("", Path::new("src/lib.rs"));
        assert!(hunks.is_empty());
    }

    // -- File-level target helper tests --

    #[test]
    fn security_review_file_level_target_uses_content_hint() {
        let target =
            build_file_level_security_review_target(Path::new("src/lib.rs"), Some("unsafe { }"));
        assert!(target.is_some());
        let t = target.unwrap();
        assert_eq!(t.preset, "unsafe_review");
        assert!(t.line.is_none());
        assert!(t.column.is_none());
    }

    #[test]
    fn security_review_file_level_target_skips_excluded_path() {
        let target = build_file_level_security_review_target(Path::new("vendor/lib.rs"), None);
        assert!(target.is_none());
    }

    #[test]
    fn security_review_file_level_target_unpositioned() {
        let target = build_file_level_security_review_target(Path::new("src/lib.rs"), None);
        assert!(target.is_some());
        let t = target.unwrap();
        assert!(t.line.is_none());
        assert!(t.column.is_none());
    }

    #[test]
    fn security_review_file_level_target_selects_preset_from_content() {
        let target = build_file_level_security_review_target(
            Path::new("src/handler.rs"),
            Some("fn handle_auth(session: &Session) {}"),
        );
        assert!(target.is_some());
        let t = target.unwrap();
        assert_eq!(t.preset, "web_backend");
        assert_eq!(t.reason, SecurityTargetReason::AuthOrSecretHandling);
    }

    // -- Prompt source evidence tests --

    #[test]
    fn security_review_plan_prompt_has_changed_hunk_source() {
        let patch = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,2 +10,4 @@
+    let z = x + y;
}
";
        let report = plan_security_review_from_diff(patch, Path::new("."));
        assert_eq!(report.review_prompts.len(), 1);
        let prompt = &report.review_prompts[0];
        assert!(prompt.evidence.iter().any(|e| e == "source: changed_hunk"));
        assert!(prompt.title.starts_with("Review changed hunk:"));
    }

    #[test]
    fn security_review_marker_prompt_has_security_context_marker_source() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(42),
            column: Some(1),
            preset: "web_backend".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };

        let context_json = serde_json::json!({
            "risk_markers": [
                {
                    "category": "auth",
                    "label": "jwt handling",
                    "file": "src/auth.rs",
                    "line": 42,
                    "matched_text": "jwt::decode(token)",
                    "rationale": "Token flows from request to decode call"
                }
            ],
            "truncated": false
        });

        let prompts = prompts_from_security_context(&target, &context_json);
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0]
            .evidence
            .iter()
            .any(|e| e == "source: securityContext.risk_marker"));
    }

    // -- Evidence-based finding type/eligibility tests --

    #[test]
    fn security_finding_marker_only_not_eligible() {
        let evidence = vec![StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::RiskMarker,
            file_path: Some(PathBuf::from("src/lib.rs")),
            line: Some(10),
            summary: "marker only".to_string(),
            detail: None,
        }];
        assert!(!is_finding_eligible(&evidence));
        assert!(!marker_only_is_finding_eligible(&evidence));
    }

    #[test]
    fn security_finding_changed_hunk_only_not_eligible() {
        let evidence = vec![StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::ChangedHunk,
            file_path: Some(PathBuf::from("src/lib.rs")),
            line: Some(10),
            summary: "changed hunk only".to_string(),
            detail: None,
        }];
        assert!(!is_finding_eligible(&evidence));
    }

    #[test]
    fn security_finding_marker_plus_changed_hunk_eligible() {
        let evidence = vec![
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::RiskMarker,
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: Some(10),
                summary: "marker".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::ChangedHunk,
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: Some(10),
                summary: "changed hunk".to_string(),
                detail: None,
            },
        ];
        assert!(is_finding_eligible(&evidence));
    }

    #[test]
    fn security_finding_preflight_plus_changed_hunk_eligible() {
        let evidence = vec![
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::Preflight,
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: None,
                summary: "preflight fail".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::ChangedHunk,
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: Some(10),
                summary: "changed hunk".to_string(),
                detail: None,
            },
        ];
        assert!(is_finding_eligible(&evidence));
    }

    #[test]
    fn security_finding_truncation_lowers_confidence() {
        let evidence = vec![
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::RiskMarker,
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: Some(10),
                summary: "marker".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::ChangedHunk,
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: Some(10),
                summary: "changed hunk".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::TruncationNotice,
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: None,
                summary: "truncated".to_string(),
                detail: None,
            },
        ];
        let (_, confidence) = classify_finding(Some("auth"), &evidence, true);
        assert_eq!(confidence, SecurityConfidence::Low);
    }

    // -- Evidence conversion tests --

    #[test]
    fn security_evidence_from_changed_target() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };
        let evidence = evidence_from_target(&target);
        assert_eq!(evidence.kind, SecurityEvidenceKind::ChangedHunk);
        assert_eq!(evidence.file_path, Some(PathBuf::from("src/lib.rs")));
        assert_eq!(evidence.line, Some(10));
    }

    #[test]
    fn security_evidence_from_risk_marker_prompt() {
        let prompt = SecurityReviewPrompt {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(42),
            preset: "web_backend".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth: jwt".to_string(),
            rationale: "token handling".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        };
        let evidence = evidence_from_review_prompt(&prompt);
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].kind, SecurityEvidenceKind::RiskMarker);
        assert_eq!(evidence[0].file_path, Some(PathBuf::from("src/auth.rs")));
        assert_eq!(evidence[0].line, Some(42));
    }

    #[test]
    fn security_evidence_from_preflight_failure() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("secret.rs"),
            line: None,
            column: None,
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };
        let results = run_content_preflight_checks(&[target], |_| {
            Some("api_key = \"hardcoded\"".to_string())
        });
        let secret_result = results
            .iter()
            .find(|r| r.check_name == "secret_content_scan")
            .unwrap();
        assert_eq!(secret_result.status, PreflightStatus::Fail);
    }

    #[test]
    fn security_evidence_preserves_file_and_line() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/handler.rs"),
            line: Some(42),
            column: Some(1),
            preset: "web_backend".to_string(),
            reason: SecurityTargetReason::AuthOrSecretHandling,
        };
        let evidence = evidence_from_target(&target);
        assert_eq!(evidence.file_path, Some(PathBuf::from("src/handler.rs")));
        assert_eq!(evidence.line, Some(42));
    }

    // -- Synthesis tests --

    #[test]
    fn security_synthesis_marker_only_remains_prompt() {
        // No targets means no ChangedHunk evidence from targets — truly marker-only
        let targets = vec![];

        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            preset: "rust_server".to_string(),
            category: Some("unsafe".to_string()),
            title: "Review unsafe: block".to_string(),
            rationale: "Potential issue".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }];

        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: Vec::new(),
        }];

        let (findings, remaining) =
            synthesize_evidence_based_findings(&targets, &prompts, &preflight);
        // Marker-only: no findings, prompt preserved
        assert!(findings.is_empty());
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn security_synthesis_marker_plus_changed_hunk_emits_finding() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(42),
            column: None,
            preset: "web_backend".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];

        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(42),
            preset: "web_backend".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth: jwt".to_string(),
            rationale: "Token handling".to_string(),
            evidence: vec![
                "source: securityContext.risk_marker".to_string(),
                "auth".to_string(),
                "jwt::decode(token)".to_string(),
            ],
        }];

        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: Vec::new(),
        }];

        let (findings, remaining) =
            synthesize_evidence_based_findings(&targets, &prompts, &preflight);
        assert_eq!(findings.len(), 1);
        assert!(remaining.is_empty());
        assert_eq!(findings[0].severity, SecuritySeverity::Medium);
    }

    #[test]
    fn security_synthesis_preflight_filename_only_remains_prompt() {
        let targets = vec![];

        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("api_key.rs"),
            line: None,
            preset: "rust_server".to_string(),
            category: Some("secret_filename_hint_scan".to_string()),
            title: "Preflight check failed: secret_filename_hint_scan".to_string(),
            rationale: "Secret-like filename".to_string(),
            evidence: vec!["api_key.rs: file name matches secret hint".to_string()],
        }];

        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: vec!["api_key.rs: file name matches secret hint".to_string()],
            structured_evidence: Vec::new(),
            notes: vec!["Secret-like filename".to_string()],
        }];

        let (findings, remaining) =
            synthesize_evidence_based_findings(&targets, &prompts, &preflight);
        // Preflight filename-only without changed hunk: no finding
        assert!(findings.is_empty());
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn security_synthesis_content_preflight_plus_changed_hunk_emits_finding() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/db.rs"),
            line: Some(15),
            column: None,
            preset: "web_backend".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];

        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/db.rs"),
            line: Some(15),
            preset: "web_backend".to_string(),
            category: Some("sql".to_string()),
            title: "Review sql: query".to_string(),
            rationale: "SQL construction".to_string(),
            evidence: vec!["source: changed_hunk".to_string(), "sql".to_string()],
        }];

        let preflight = vec![SecurityPreflightResult {
            check_name: "sql_injection_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: vec!["src/db.rs: SQL format interpolation".to_string()],
            structured_evidence: vec![SecurityPreflightEvidence {
                file_path: PathBuf::from("src/db.rs"),
                line: Some(15),
                summary: "SQL string construction with format interpolation".to_string(),
                detail: Some("test".to_string()),
            }],
            notes: vec!["SQL found".to_string()],
        }];

        let (findings, remaining) =
            synthesize_evidence_based_findings(&targets, &prompts, &preflight);
        assert_eq!(findings.len(), 1);
        assert!(remaining.is_empty());
    }

    #[test]
    fn security_synthesis_ineligible_prompts_are_preserved() {
        let targets = vec![];

        let prompts = vec![
            SecurityReviewPrompt {
                file_path: PathBuf::from("src/a.rs"),
                line: Some(1),
                preset: "rust_server".to_string(),
                category: None,
                title: "Review changed hunk: src/a.rs".to_string(),
                rationale: "Changed".to_string(),
                evidence: vec!["source: changed_hunk".to_string()],
            },
            SecurityReviewPrompt {
                file_path: PathBuf::from("src/b.rs"),
                line: Some(5),
                preset: "rust_server".to_string(),
                category: None,
                title: "Review changed hunk: src/b.rs".to_string(),
                rationale: "Changed".to_string(),
                evidence: vec!["source: changed_hunk".to_string()],
            },
        ];

        let preflight = vec![];

        let (findings, remaining) =
            synthesize_evidence_based_findings(&targets, &prompts, &preflight);
        assert!(findings.is_empty());
        assert_eq!(remaining.len(), 2);
    }

    // -- Classification tests --

    #[test]
    fn security_classify_auth_with_call_path_medium_or_high() {
        let evidence = vec![
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::RiskMarker,
                file_path: None,
                line: None,
                summary: "marker".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::ChangedHunk,
                file_path: None,
                line: None,
                summary: "hunk".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::CallPath,
                file_path: None,
                line: None,
                summary: "call path".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::CodeReasoning,
                file_path: None,
                line: None,
                summary: "reasoning".to_string(),
                detail: None,
            },
        ];
        let (severity, _) = classify_finding(Some("auth"), &evidence, false);
        assert!(severity >= SecuritySeverity::Medium);
    }

    #[test]
    fn security_classify_secret_with_content_preflight_medium() {
        let evidence = vec![
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::RiskMarker,
                file_path: None,
                line: None,
                summary: "marker".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::ChangedHunk,
                file_path: None,
                line: None,
                summary: "hunk".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::Preflight,
                file_path: None,
                line: None,
                summary: "preflight fail".to_string(),
                detail: None,
            },
        ];
        let (severity, confidence) = classify_finding(Some("secret"), &evidence, false);
        assert_eq!(severity, SecuritySeverity::Medium);
        assert_eq!(confidence, SecurityConfidence::High);
    }

    #[test]
    fn security_classify_filename_hint_low_confidence() {
        let evidence = vec![StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::Preflight,
            file_path: None,
            line: None,
            summary: "filename hint".to_string(),
            detail: None,
        }];
        let (_, confidence) = classify_finding(None, &evidence, false);
        assert_eq!(confidence, SecurityConfidence::Low);
    }

    #[test]
    fn security_classify_no_critical_by_default() {
        let evidence = vec![
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::RiskMarker,
                file_path: None,
                line: None,
                summary: "marker".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::ChangedHunk,
                file_path: None,
                line: None,
                summary: "hunk".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::CallPath,
                file_path: None,
                line: None,
                summary: "call path".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::CodeReasoning,
                file_path: None,
                line: None,
                summary: "reasoning".to_string(),
                detail: None,
            },
        ];
        let (severity, _) = classify_finding(Some("auth"), &evidence, false);
        assert_ne!(severity, SecuritySeverity::Critical);
    }

    // -- Text tests --

    #[test]
    fn security_recommendation_auth_is_defensive() {
        let rec = finding_recommendation(Some("auth"));
        assert!(rec.contains("validation") || rec.contains("tests"));
        assert!(!rec.contains("exploit"));
        assert!(!rec.contains("attack"));
    }

    #[test]
    fn security_recommendation_process_avoids_shell_interpolation() {
        let rec = finding_recommendation(Some("process"));
        assert!(rec.contains("shell interpolation") || rec.contains("separately"));
        assert!(!rec.contains("exploit"));
    }

    #[test]
    fn security_recommendation_sql_mentions_parameterized_queries() {
        let rec = finding_recommendation(Some("sql"));
        assert!(rec.contains("parameterized"));
        assert!(!rec.contains("exploit"));
    }

    #[test]
    fn security_tests_are_defensive_regression_tests() {
        let tests = finding_tests(Some("auth"));
        assert!(!tests.is_empty());
        for t in &tests {
            assert!(t.starts_with("test_"));
            assert!(!t.contains("exploit"));
            assert!(!t.contains("payload"));
        }
    }

    // -- Content preflight tests --

    #[test]
    fn security_content_preflight_detects_hardcoded_secret() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/config.rs"),
            line: Some(5),
            column: None,
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };
        let results =
            run_content_preflight_checks(&[target], |_| Some("api_key = \"sk-1234\"".to_string()));
        let secret = results
            .iter()
            .find(|r| r.check_name == "secret_content_scan")
            .unwrap();
        assert_eq!(secret.status, PreflightStatus::Fail);
    }

    #[test]
    fn security_content_preflight_detects_process_exec() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/runner.rs"),
            line: Some(10),
            column: None,
            preset: "rust_cli".to_string(),
            reason: SecurityTargetReason::ProcessExecution,
        };
        let results = run_content_preflight_checks(&[target], |_| {
            Some("let child = Command::new(\"sh\")".to_string())
        });
        let proc = results
            .iter()
            .find(|r| r.check_name == "process_exec_scan")
            .unwrap();
        assert_eq!(proc.status, PreflightStatus::Fail);
    }

    #[test]
    fn security_content_preflight_clean_file_passes() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(1),
            column: None,
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };
        let results = run_content_preflight_checks(&[target], |_| {
            Some("fn add(a: i32, b: i32) -> i32 { a + b }".to_string())
        });
        assert!(results.iter().all(|r| r.status == PreflightStatus::Pass));
    }

    // -- Report assembly with findings tests --

    #[test]
    fn security_report_with_findings_includes_conservative_notes() {
        let targets = vec![];
        let prompts = vec![];
        let findings = vec![];
        let preflight = vec![];
        let report = assemble_security_review_report_with_findings(
            targets,
            prompts,
            findings,
            preflight,
            Vec::new(),
        );
        assert!(report
            .notes
            .iter()
            .any(|n| n.contains("not proof of exploitability")));
        assert!(report
            .notes
            .iter()
            .any(|n| n.contains("additional evidence")));
    }

    #[test]
    fn security_preflight_structured_evidence_has_file_path() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/secret.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];
        let results = run_content_preflight_checks(&targets, |path| {
            if path == Path::new("src/secret.rs") {
                Some("api_key = \"test\"\n".to_string())
            } else {
                None
            }
        });
        let secret_result = results
            .iter()
            .find(|r| r.check_name == "secret_content_scan")
            .unwrap();
        assert_eq!(secret_result.status, PreflightStatus::Fail);
        assert!(!secret_result.structured_evidence.is_empty());
        for se in &secret_result.structured_evidence {
            assert_eq!(se.file_path, PathBuf::from("src/secret.rs"));
        }
    }

    #[test]
    fn security_preflight_structured_evidence_has_line_for_content_match() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(1),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];
        let results = run_content_preflight_checks(&targets, |path| {
            if path == Path::new("src/auth.rs") {
                Some("let x = 1;\napi_key = \"test\"\nlet y = 2;\n".to_string())
            } else {
                None
            }
        });
        let secret_result = results
            .iter()
            .find(|r| r.check_name == "secret_content_scan")
            .unwrap();
        assert_eq!(secret_result.status, PreflightStatus::Fail);
        let se = &secret_result.structured_evidence[0];
        assert_eq!(se.line, Some(2));
    }

    #[test]
    fn security_synthesis_preflight_different_file_does_not_support_finding() {
        let targets = vec![];
        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("file_a.rs"),
            line: Some(10),
            preset: "rust_server".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth: test".to_string(),
            rationale: "test".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }];
        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_content_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: vec!["file_b.rs: hardcoded secret".to_string()],
            structured_evidence: vec![SecurityPreflightEvidence {
                file_path: PathBuf::from("file_b.rs"),
                line: Some(5),
                summary: "hardcoded secret-like assignment".to_string(),
                detail: Some("test".to_string()),
            }],
            notes: vec!["test".to_string()],
        }];
        let (findings, _prompts) =
            synthesize_evidence_based_findings(&targets, &prompts, &preflight);
        assert!(
            findings.is_empty(),
            "File A should not get a finding from File B evidence"
        );
    }

    #[test]
    fn security_synthesis_preflight_same_file_supports_finding() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];
        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            preset: "rust_server".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth: test".to_string(),
            rationale: "test".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }];
        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_content_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: vec!["src/auth.rs: hardcoded secret".to_string()],
            structured_evidence: vec![SecurityPreflightEvidence {
                file_path: PathBuf::from("src/auth.rs"),
                line: Some(12),
                summary: "hardcoded secret-like assignment".to_string(),
                detail: Some("test".to_string()),
            }],
            notes: vec!["test".to_string()],
        }];
        let (findings, _prompts) =
            synthesize_evidence_based_findings(&targets, &prompts, &preflight);
        assert!(
            !findings.is_empty(),
            "Same-file preflight should support a finding"
        );
    }

    #[test]
    fn security_synthesis_preflight_same_file_distant_line_does_not_support_positioned_group() {
        let targets = vec![];
        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            preset: "rust_server".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth: test".to_string(),
            rationale: "test".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }];
        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_content_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: vec!["src/auth.rs: hardcoded secret".to_string()],
            structured_evidence: vec![SecurityPreflightEvidence {
                file_path: PathBuf::from("src/auth.rs"),
                line: Some(500),
                summary: "hardcoded secret-like assignment".to_string(),
                detail: Some("test".to_string()),
            }],
            notes: vec!["test".to_string()],
        }];
        let (findings, _prompts) =
            synthesize_evidence_based_findings(&targets, &prompts, &preflight);
        assert!(
            findings.is_empty(),
            "Distant-line preflight should not support positioned finding"
        );
    }

    #[test]
    fn security_synthesis_preflight_same_file_nearby_line_supports_positioned_group() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];
        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            preset: "rust_server".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth: test".to_string(),
            rationale: "test".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }];
        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_content_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: vec!["src/auth.rs: hardcoded secret".to_string()],
            structured_evidence: vec![SecurityPreflightEvidence {
                file_path: PathBuf::from("src/auth.rs"),
                line: Some(12),
                summary: "hardcoded secret-like assignment".to_string(),
                detail: Some("test".to_string()),
            }],
            notes: vec!["test".to_string()],
        }];
        let (findings, _prompts) =
            synthesize_evidence_based_findings(&targets, &prompts, &preflight);
        assert!(
            !findings.is_empty(),
            "Nearby-line preflight should support positioned finding"
        );
    }

    #[test]
    fn security_synthesis_legacy_string_preflight_does_not_globally_support_group() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];
        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            preset: "rust_server".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth: test".to_string(),
            rationale: "test".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }];
        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_content_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: vec!["src/auth.rs: hardcoded secret".to_string()],
            structured_evidence: Vec::new(),
            notes: vec!["test".to_string()],
        }];
        let (_findings, _prompts) =
            synthesize_evidence_based_findings(&targets, &prompts, &preflight);
    }

    #[test]
    fn security_content_preflight_hunk_local_ignores_distant_secret() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/app.rs"),
            line: Some(50),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];
        let results = run_content_preflight_checks_for_targets(&targets, |path| {
            if path == Path::new("src/app.rs") {
                let mut lines = vec!["let x = 1;".to_string(); 199];
                lines.push("api_key = \"secret_value\"".to_string());
                Some(lines.join("\n"))
            } else {
                None
            }
        });
        let secret_result = results
            .iter()
            .find(|r| r.check_name == "secret_content_scan")
            .unwrap();
        assert_eq!(
            secret_result.status,
            PreflightStatus::Pass,
            "Distant secret should not be detected by hunk-local scan"
        );
    }

    #[test]
    fn security_content_preflight_hunk_local_detects_nearby_secret() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("src/app.rs"),
            line: Some(5),
            column: Some(1),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];
        let results = run_content_preflight_checks_for_targets(&targets, |path| {
            if path == Path::new("src/app.rs") {
                Some("let x = 1;\nlet y = 2;\napi_key = \"secret\"\nlet z = 3;\n".to_string())
            } else {
                None
            }
        });
        let secret_result = results
            .iter()
            .find(|r| r.check_name == "secret_content_scan")
            .unwrap();
        assert_eq!(
            secret_result.status,
            PreflightStatus::Fail,
            "Nearby secret should be detected"
        );
    }

    #[test]
    fn security_prompt_only_synthesis_name_preserves_marker_only_behavior() {
        let targets = vec![];
        let markers = vec![SecurityRiskMarkerFromWorkflow {
            category: "auth".to_string(),
            label: "test marker".to_string(),
            file_path: PathBuf::from("src/auth.rs"),
            line: 10,
            column: 1,
            matched_text: "test".to_string(),
            rationale: "test marker".to_string(),
        }];
        let preflight = vec![];
        let (findings, prompts) = synthesize_review_prompts_only(&targets, &markers, &preflight);
        assert!(
            findings.is_empty(),
            "Prompt-only synthesis should never produce findings"
        );
        assert_eq!(prompts.len(), 1);
    }

    // -- Orchestrator tests --

    #[test]
    fn security_workflow_default_options_are_bounded() {
        let opts = SecurityReviewWorkflowOptions::default();
        assert!(opts.include_prompts);
        assert!(opts.include_findings);
        assert!(opts.run_filename_preflight);
        assert!(opts.run_content_preflight);
        assert!(opts.hunk_local_content_preflight);
        assert!(opts.max_findings <= 100);
        assert!(opts.max_prompts <= 200);
    }

    // -- Escalation tests --

    #[test]
    fn security_context_escalation_none_for_low_risk_changed_hunk() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(5),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };
        let level = choose_security_context_escalation(&target, None, None);
        assert_eq!(level, SecurityContextEscalationLevel::None);
    }

    #[test]
    fn security_context_escalation_basic_for_marker_prompt() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(5),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };
        let prompt = SecurityReviewPrompt {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            preset: "rust_server".to_string(),
            category: Some("auth".to_string()),
            title: "auth marker".to_string(),
            rationale: "found auth pattern".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        };
        let level = choose_security_context_escalation(&target, None, Some(&prompt));
        assert_eq!(level, SecurityContextEscalationLevel::Basic);
    }

    #[test]
    fn security_context_escalation_depth1_for_medium_finding() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(5),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };
        let finding = SecurityReviewFinding {
            severity: SecuritySeverity::Medium,
            confidence: SecurityConfidence::Medium,
            title: "test finding".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            category: Some("auth".to_string()),
            evidence: vec![],
            reasoning: "test".to_string(),
            recommendation: "test".to_string(),
            tests: vec![],
        };
        let level = choose_security_context_escalation(&target, Some(&finding), None);
        assert_eq!(level, SecurityContextEscalationLevel::CallDepth1);
    }

    #[test]
    fn security_context_escalation_depth2_for_high_confident_auth_finding() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            column: Some(5),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::AuthOrSecretHandling,
        };
        let finding = SecurityReviewFinding {
            severity: SecuritySeverity::High,
            confidence: SecurityConfidence::High,
            title: "auth bypass".to_string(),
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            category: Some("auth".to_string()),
            evidence: vec![],
            reasoning: "test".to_string(),
            recommendation: "test".to_string(),
            tests: vec![],
        };
        let level = choose_security_context_escalation(&target, Some(&finding), None);
        assert_eq!(level, SecurityContextEscalationLevel::CallDepth2);
    }

    #[test]
    fn security_context_escalation_never_depth2_for_low_confidence() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            column: Some(5),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::AuthOrSecretHandling,
        };
        let finding = SecurityReviewFinding {
            severity: SecuritySeverity::High,
            confidence: SecurityConfidence::Low,
            title: "auth bypass".to_string(),
            file_path: PathBuf::from("src/auth.rs"),
            line: Some(10),
            category: Some("auth".to_string()),
            evidence: vec![],
            reasoning: "test".to_string(),
            recommendation: "test".to_string(),
            tests: vec![],
        };
        let level = choose_security_context_escalation(&target, Some(&finding), None);
        assert_eq!(level, SecurityContextEscalationLevel::CallDepth1);
    }

    #[test]
    fn security_context_request_sets_call_depth_and_caps() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("src/lib.rs"),
            line: Some(10),
            column: Some(5),
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };

        let req = build_escalated_security_context_request(
            &target,
            SecurityContextEscalationLevel::CallDepth1,
        );
        assert_eq!(req["call_depth"], 1);
        assert_eq!(req["max_call_nodes"], 32);
        assert_eq!(req["max_risk_markers"], 80);

        let req = build_escalated_security_context_request(
            &target,
            SecurityContextEscalationLevel::CallDepth2,
        );
        assert_eq!(req["call_depth"], 2);
        assert_eq!(req["max_call_nodes"], 64);
    }

    // -- Rendering tests --

    #[test]
    fn security_review_summary_renders_counts() {
        let output = SecurityReviewOutput {
            targets: vec![],
            findings: vec![],
            review_prompts: vec![],
            preflight_results: vec![],
            notes: vec!["test note".to_string()],
        };
        let rendered = render_security_review_summary(&output);
        assert!(rendered.contains("Targets: 0"));
        assert!(rendered.contains("Findings: 0"));
        assert!(rendered.contains("Review prompts: 0"));
        assert!(rendered.contains("test note"));
    }

    #[test]
    fn security_review_finding_render_shows_severity_confidence() {
        let output = SecurityReviewOutput {
            targets: vec![],
            findings: vec![SecurityReviewFinding {
                severity: SecuritySeverity::High,
                confidence: SecurityConfidence::Medium,
                title: "test finding".to_string(),
                file_path: PathBuf::from("src/lib.rs"),
                line: Some(10),
                category: Some("auth".to_string()),
                evidence: vec![],
                reasoning: "test".to_string(),
                recommendation: "use validation".to_string(),
                tests: vec!["test_auth".to_string()],
            }],
            review_prompts: vec![],
            preflight_results: vec![],
            notes: vec![],
        };
        let rendered = render_security_review_findings(&output);
        assert!(rendered.contains("[high/medium]"));
        assert!(rendered.contains("test finding"));
        assert!(rendered.contains("use validation"));
    }

    #[test]
    fn security_review_prompt_render_has_no_severity() {
        let output = SecurityReviewOutput {
            targets: vec![],
            findings: vec![],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: PathBuf::from("src/lib.rs"),
                line: Some(10),
                preset: "rust_server".to_string(),
                category: None,
                title: "check auth".to_string(),
                rationale: "auth marker found".to_string(),
                evidence: vec![],
            }],
            preflight_results: vec![],
            notes: vec![],
        };
        let rendered = render_security_review_prompts(&output);
        assert!(rendered.contains("check auth"));
        assert!(rendered.contains("auth marker found"));
        assert!(!rendered.contains("high/medium")); // no severity for prompts
    }

    // -- Evidence window tests --

    #[test]
    fn security_evidence_window_rejects_different_file() {
        let evidence = StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::Preflight,
            file_path: Some(PathBuf::from("src/other.rs")),
            line: Some(10),
            summary: "test".to_string(),
            detail: None,
        };
        assert!(!evidence_matches_group(
            &evidence,
            &PathBuf::from("src/lib.rs"),
            Some(10)
        ));
    }

    #[test]
    fn security_evidence_window_accepts_same_file_no_line() {
        let evidence = StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::Preflight,
            file_path: Some(PathBuf::from("src/lib.rs")),
            line: None,
            summary: "test".to_string(),
            detail: None,
        };
        assert!(evidence_matches_group(
            &evidence,
            &PathBuf::from("src/lib.rs"),
            Some(10)
        ));
    }

    #[test]
    fn security_evidence_window_accepts_nearby_line() {
        let evidence = StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::Preflight,
            file_path: Some(PathBuf::from("src/lib.rs")),
            line: Some(12),
            summary: "test".to_string(),
            detail: None,
        };
        assert!(evidence_matches_group(
            &evidence,
            &PathBuf::from("src/lib.rs"),
            Some(10)
        ));
    }

    #[test]
    fn security_evidence_window_rejects_distant_line() {
        let evidence = StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::Preflight,
            file_path: Some(PathBuf::from("src/lib.rs")),
            line: Some(30),
            summary: "test".to_string(),
            detail: None,
        };
        assert!(!evidence_matches_group(
            &evidence,
            &PathBuf::from("src/lib.rs"),
            Some(10)
        ));
    }

    #[test]
    fn security_context_escalation_plan_none_for_low_risk() {
        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: "src/lib.rs".into(),
                line: Some(10),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::ChangedHunk,
            }],
            review_prompts: vec![],
            findings: vec![],
            preflight_results: vec![],
            notes: vec![],
        };
        let plans = plan_security_context_escalations(&output);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].level, SecurityContextEscalationLevel::None);
        assert!(plans[0].request.is_none());
    }

    #[test]
    fn security_context_escalation_plan_basic_for_prompt() {
        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: "src/lib.rs".into(),
                line: Some(10),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::ChangedHunk,
            }],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: "src/lib.rs".into(),
                line: Some(10),
                preset: "rust_server".to_string(),
                category: None,
                title: "Review changed code".to_string(),
                rationale: "Changed hunk".to_string(),
                evidence: vec![],
            }],
            findings: vec![],
            preflight_results: vec![],
            notes: vec![],
        };
        let plans = plan_security_context_escalations(&output);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].level, SecurityContextEscalationLevel::Basic);
        assert!(plans[0].request.is_some());
    }

    #[test]
    fn security_context_escalation_plan_depth1_for_medium_finding() {
        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: "src/lib.rs".into(),
                line: Some(10),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::ChangedHunk,
            }],
            review_prompts: vec![],
            findings: vec![SecurityReviewFinding {
                severity: SecuritySeverity::Medium,
                confidence: SecurityConfidence::Medium,
                title: "Potential issue".to_string(),
                file_path: "src/lib.rs".into(),
                line: Some(10),
                category: Some("process".to_string()),
                evidence: vec![StructuredSecurityEvidence {
                    kind: SecurityEvidenceKind::Preflight,
                    file_path: Some("src/lib.rs".into()),
                    line: Some(10),
                    summary: "test".to_string(),
                    detail: None,
                }],
                reasoning: "test".to_string(),
                recommendation: "test".to_string(),
                tests: vec![],
            }],
            preflight_results: vec![],
            notes: vec![],
        };
        let plans = plan_security_context_escalations(&output);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].level, SecurityContextEscalationLevel::CallDepth1);
        assert!(plans[0].request.is_some());
    }

    #[test]
    fn security_context_escalation_plan_depth2_for_high_confident_auth() {
        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: "src/auth.rs".into(),
                line: Some(5),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::AuthOrSecretHandling,
            }],
            review_prompts: vec![],
            findings: vec![SecurityReviewFinding {
                severity: SecuritySeverity::High,
                confidence: SecurityConfidence::High,
                title: "Auth bypass".to_string(),
                file_path: "src/auth.rs".into(),
                line: Some(5),
                category: Some("auth".to_string()),
                evidence: vec![StructuredSecurityEvidence {
                    kind: SecurityEvidenceKind::Preflight,
                    file_path: Some("src/auth.rs".into()),
                    line: Some(5),
                    summary: "test".to_string(),
                    detail: None,
                }],
                reasoning: "test".to_string(),
                recommendation: "test".to_string(),
                tests: vec![],
            }],
            preflight_results: vec![],
            notes: vec![],
        };
        let plans = plan_security_context_escalations(&output);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].level, SecurityContextEscalationLevel::CallDepth2);
        let req = plans[0].request.as_ref().unwrap();
        assert_eq!(req["call_depth"], 2);
        assert_eq!(req["max_call_nodes"], 64);
    }

    #[test]
    fn security_context_escalation_plan_has_bounded_request_caps() {
        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: "src/lib.rs".into(),
                line: Some(10),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::ChangedHunk,
            }],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: "src/lib.rs".into(),
                line: Some(10),
                preset: "rust_server".to_string(),
                category: None,
                title: "Review".to_string(),
                rationale: "test".to_string(),
                evidence: vec![],
            }],
            findings: vec![SecurityReviewFinding {
                severity: SecuritySeverity::High,
                confidence: SecurityConfidence::High,
                title: "Issue".to_string(),
                file_path: "src/lib.rs".into(),
                line: Some(10),
                category: Some("auth".to_string()),
                evidence: vec![StructuredSecurityEvidence {
                    kind: SecurityEvidenceKind::Preflight,
                    file_path: Some("src/lib.rs".into()),
                    line: Some(10),
                    summary: "test".to_string(),
                    detail: None,
                }],
                reasoning: "test".to_string(),
                recommendation: "test".to_string(),
                tests: vec![],
            }],
            preflight_results: vec![],
            notes: vec![],
        };
        let plans = plan_security_context_escalations(&output);
        let req = plans[0].request.as_ref().unwrap();
        let max_call_nodes = req["max_call_nodes"].as_u64().unwrap_or(0);
        assert!(max_call_nodes <= 64);
        let max_risk = req["max_risk_markers"].as_u64().unwrap_or(0);
        assert!(max_risk <= 80);
    }

    // -- Command parsing tests --

    #[test]
    fn security_review_command_parses_default() {
        let args = parse_security_review_args("");
        assert!(!args.json);
        assert!(!args.prompts_only);
        assert!(!args.findings_only);
        assert!(args.base.is_none());
    }

    #[test]
    fn security_review_command_parses_json() {
        let args = parse_security_review_args("--json");
        assert!(args.json);
    }

    #[test]
    fn security_review_command_parses_prompts_only() {
        let args = parse_security_review_args("--prompts-only");
        assert!(args.prompts_only);
        assert!(!args.findings_only);
    }

    #[test]
    fn security_review_command_parses_findings_only() {
        let args = parse_security_review_args("--findings-only");
        assert!(args.findings_only);
        assert!(!args.prompts_only);
    }

    #[test]
    fn security_review_command_parses_changed() {
        let args = parse_security_review_args("--changed");
        assert_eq!(args.base.as_deref(), Some("HEAD"));
    }

    #[test]
    fn security_review_command_parses_base() {
        let args = parse_security_review_args("--base main");
        assert_eq!(args.base.as_deref(), Some("main"));
    }

    #[test]
    fn security_review_command_parses_max_flags() {
        let args = parse_security_review_args("--max-findings 10 --max-prompts 5");
        assert_eq!(args.max_findings, Some(10));
        assert_eq!(args.max_prompts, Some(5));
    }

    #[test]
    fn security_review_command_renders_summary_findings_prompts() {
        let output = SecurityReviewOutput {
            targets: vec![],
            review_prompts: vec![],
            findings: vec![],
            preflight_results: vec![],
            notes: vec!["test note".to_string()],
        };
        let rendered = render_security_review_summary(&output);
        assert!(rendered.contains("Security Review Summary"));
        assert!(rendered.contains("test note"));
    }

    // -- Enrichment option tests --

    #[test]
    fn security_enrichment_options_default_disabled() {
        let opts = SecurityReviewWorkflowOptions::default();
        assert!(!opts.enable_lsp_enrichment);
        assert_eq!(opts.max_lsp_enriched_targets, 8);
        assert_eq!(opts.max_lsp_requests, 8);
        assert_eq!(opts.lsp_request_timeout_ms, 2500);
    }

    // -- Enrich command parsing tests --

    #[test]
    fn security_review_command_enrich_flag_parses() {
        let args = parse_security_review_args("--enrich");
        assert!(args.enrich);
    }

    #[test]
    fn security_review_command_enrich_with_caps_parses() {
        let args =
            parse_security_review_args("--enrich --max-enriched-targets 4 --lsp-timeout-ms 5000");
        assert!(args.enrich);
        assert_eq!(args.max_enriched_targets, Some(4));
        assert_eq!(args.lsp_timeout_ms, Some(5000));
    }

    #[test]
    fn security_review_command_default_does_not_enrich() {
        let args = parse_security_review_args("");
        assert!(!args.enrich);
    }

    // -- Executor tests --

    #[tokio::test]
    async fn security_enrichment_skips_none_plans() {
        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: PathBuf::from("low_risk.rs"),
                line: Some(10),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::ChangedHunk,
            }],
            findings: vec![],
            review_prompts: vec![],
            preflight_results: vec![],
            notes: vec![],
        };

        let executor = NoopSecurityContextExecutor;
        let opts = SecurityReviewWorkflowOptions::default();
        let results = run_security_context_enrichment(&output, &executor, &opts).await;
        // No plans have non-None level for low-risk target with no findings/prompts
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn security_enrichment_caps_request_count() {
        let mut targets = Vec::new();
        let mut prompts = Vec::new();
        for i in 0..20 {
            let path = PathBuf::from(format!("file_{}.rs", i));
            targets.push(SecurityReviewTarget {
                file_path: path.clone(),
                line: Some(10),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::UnsafeCode,
            });
            prompts.push(SecurityReviewPrompt {
                file_path: path,
                line: Some(10),
                preset: "rust_server".to_string(),
                category: Some("unsafe".to_string()),
                title: "Review unsafe".to_string(),
                rationale: "test".to_string(),
                evidence: vec!["source: securityContext.risk_marker".to_string()],
            });
        }

        let output = SecurityReviewOutput {
            targets,
            findings: vec![],
            review_prompts: prompts,
            preflight_results: vec![],
            notes: vec![],
        };

        let executor = FixtureSecurityContextExecutor::new();
        let opts = SecurityReviewWorkflowOptions {
            max_lsp_enriched_targets: 3,
            max_lsp_requests: 2,
            ..Default::default()
        };
        let results = run_security_context_enrichment(&output, &executor, &opts).await;
        // Should be capped at max_lsp_requests
        assert!(results.len() <= 2);
    }

    #[tokio::test]
    async fn security_enrichment_records_executor_failure_as_note() {
        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: PathBuf::from("auth.rs"),
                line: Some(5),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::AuthOrSecretHandling,
            }],
            findings: vec![],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: PathBuf::from("auth.rs"),
                line: Some(5),
                preset: "rust_server".to_string(),
                category: Some("auth".to_string()),
                title: "Review auth".to_string(),
                rationale: "test".to_string(),
                evidence: vec!["source: securityContext.risk_marker".to_string()],
            }],
            preflight_results: vec![],
            notes: vec![],
        };

        let executor = FixtureSecurityContextExecutor::with_failure(
            PathBuf::from("auth.rs"),
            "LSP crashed".to_string(),
        );
        let opts = SecurityReviewWorkflowOptions::default();
        let results = run_security_context_enrichment(&output, &executor, &opts).await;
        assert_eq!(results.len(), 1);
        assert!(!results[0].notes.is_empty());
        assert!(results[0].notes[0].contains("executor error"));
    }

    #[tokio::test]
    async fn security_enrichment_records_timeout_as_note() {
        use std::time::Duration;

        struct SlowExecutor;

        #[async_trait::async_trait]
        impl SecurityContextExecutor for SlowExecutor {
            async fn security_context(
                &self,
                _request: serde_json::Value,
            ) -> Result<serde_json::Value, String> {
                tokio::time::sleep(Duration::from_secs(10)).await;
                Ok(serde_json::json!({}))
            }
        }

        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: PathBuf::from("net.rs"),
                line: Some(1),
                column: None,
                preset: "web_backend".to_string(),
                reason: SecurityTargetReason::NetworkBoundary,
            }],
            findings: vec![],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: PathBuf::from("net.rs"),
                line: Some(1),
                preset: "web_backend".to_string(),
                category: Some("network".to_string()),
                title: "Review network".to_string(),
                rationale: "test".to_string(),
                evidence: vec!["source: securityContext.risk_marker".to_string()],
            }],
            preflight_results: vec![],
            notes: vec![],
        };

        let executor = SlowExecutor;
        let opts = SecurityReviewWorkflowOptions {
            lsp_request_timeout_ms: 50,
            ..Default::default()
        };
        let results = run_security_context_enrichment(&output, &executor, &opts).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].notes.iter().any(|n| n.contains("timed out")));
    }

    #[tokio::test]
    async fn security_enrichment_converts_marker_response_to_prompt() {
        let response = serde_json::json!({
            "risk_markers": [
                {
                    "category": "auth",
                    "label": "hardcoded token",
                    "line": 42,
                    "matched_text": "api_key = \"secret\"",
                    "rationale": "hardcoded secret"
                }
            ]
        });

        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: PathBuf::from("auth.rs"),
                line: Some(40),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::AuthOrSecretHandling,
            }],
            findings: vec![],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: PathBuf::from("auth.rs"),
                line: Some(40),
                preset: "rust_server".to_string(),
                category: Some("auth".to_string()),
                title: "Review auth".to_string(),
                rationale: "test".to_string(),
                evidence: vec!["source: securityContext.risk_marker".to_string()],
            }],
            preflight_results: vec![],
            notes: vec![],
        };

        let executor =
            FixtureSecurityContextExecutor::with_response(PathBuf::from("auth.rs"), response);
        let opts = SecurityReviewWorkflowOptions::default();
        let results = run_security_context_enrichment(&output, &executor, &opts).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].prompts.len(), 1);
        assert!(results[0].prompts[0].title.contains("hardcoded token"));
        assert!(!results[0].evidence.is_empty());
    }

    #[tokio::test]
    async fn security_enrichment_converts_call_graph_to_call_path_evidence() {
        let response = serde_json::json!({
            "call_expansion": {
                "nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
                "edges": [{"from": "a", "to": "b"}, {"from": "b", "to": "c"}],
                "depth": 1
            }
        });

        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: PathBuf::from("proc.rs"),
                line: Some(10),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::ProcessExecution,
            }],
            findings: vec![],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: PathBuf::from("proc.rs"),
                line: Some(10),
                preset: "rust_server".to_string(),
                category: Some("process".to_string()),
                title: "Review process".to_string(),
                rationale: "test".to_string(),
                evidence: vec!["source: securityContext.risk_marker".to_string()],
            }],
            preflight_results: vec![],
            notes: vec![],
        };

        let executor =
            FixtureSecurityContextExecutor::with_response(PathBuf::from("proc.rs"), response);
        let opts = SecurityReviewWorkflowOptions::default();
        let results = run_security_context_enrichment(&output, &executor, &opts).await;
        assert_eq!(results.len(), 1);
        assert!(results[0]
            .evidence
            .iter()
            .any(|e| e.kind == SecurityEvidenceKind::CallPath));
        let call_path = results[0]
            .evidence
            .iter()
            .find(|e| e.kind == SecurityEvidenceKind::CallPath)
            .unwrap();
        assert!(call_path.summary.contains("3 nodes"));
        assert!(call_path.summary.contains("2 edges"));
    }

    #[tokio::test]
    async fn security_enrichment_converts_diagnostic_to_diagnostic_evidence() {
        let response = serde_json::json!({
            "security_relevant_diagnostics": [
                {"message": "unused variable", "line": 42}
            ]
        });

        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: PathBuf::from("code.rs"),
                line: Some(40),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::ChangedHunk,
            }],
            findings: vec![],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: PathBuf::from("code.rs"),
                line: Some(40),
                preset: "rust_server".to_string(),
                category: None,
                title: "Review code".to_string(),
                rationale: "test".to_string(),
                evidence: vec!["source: securityContext.risk_marker".to_string()],
            }],
            preflight_results: vec![],
            notes: vec![],
        };

        let executor =
            FixtureSecurityContextExecutor::with_response(PathBuf::from("code.rs"), response);
        let opts = SecurityReviewWorkflowOptions::default();
        let results = run_security_context_enrichment(&output, &executor, &opts).await;
        assert_eq!(results.len(), 1);
        assert!(results[0]
            .evidence
            .iter()
            .any(|e| e.kind == SecurityEvidenceKind::Diagnostic));
    }

    #[tokio::test]
    async fn security_enrichment_converts_truncation_to_truncation_notice() {
        let response = serde_json::json!({
            "truncated": true,
            "limits": {"call_expansion_truncated": true}
        });

        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: PathBuf::from("big.rs"),
                line: Some(1),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::ChangedHunk,
            }],
            findings: vec![],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: PathBuf::from("big.rs"),
                line: Some(1),
                preset: "rust_server".to_string(),
                category: None,
                title: "Review big".to_string(),
                rationale: "test".to_string(),
                evidence: vec!["source: securityContext.risk_marker".to_string()],
            }],
            preflight_results: vec![],
            notes: vec![],
        };

        let executor =
            FixtureSecurityContextExecutor::with_response(PathBuf::from("big.rs"), response);
        let opts = SecurityReviewWorkflowOptions::default();
        let results = run_security_context_enrichment(&output, &executor, &opts).await;
        assert_eq!(results.len(), 1);
        assert!(results[0]
            .evidence
            .iter()
            .any(|e| e.kind == SecurityEvidenceKind::TruncationNotice));
        assert!(results[0].notes.iter().any(|n| n.contains("truncated")));
    }

    // -- Evidence conversion tests --

    #[test]
    fn security_evidence_from_security_context_extracts_risk_markers() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("auth.rs"),
            line: Some(10),
            column: None,
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::AuthOrSecretHandling,
        };
        let json = serde_json::json!({
            "risk_markers": [
                {
                    "category": "auth",
                    "label": "token",
                    "file": "auth.rs",
                    "line": 42,
                    "matched_text": "token"
                }
            ]
        });
        let evidence = evidence_from_security_context(&target, &json);
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].kind, SecurityEvidenceKind::RiskMarker);
        assert_eq!(evidence[0].file_path, Some(PathBuf::from("auth.rs")));
        assert_eq!(evidence[0].line, Some(42));
    }

    #[test]
    fn security_evidence_from_security_context_accepts_file_path_field() {
        let target = SecurityReviewTarget {
            file_path: PathBuf::from("code.rs"),
            line: Some(5),
            column: None,
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        };
        let json = serde_json::json!({
            "risk_markers": [
                {
                    "category": "sql",
                    "label": "injection",
                    "file_path": "code.rs",
                    "line": 20,
                    "matched_text": "query"
                }
            ]
        });
        let evidence = evidence_from_security_context(&target, &json);
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].file_path, Some(PathBuf::from("code.rs")));
    }

    // -- Enriched synthesis tests --

    #[test]
    fn security_enriched_call_path_plus_marker_promotes_finding() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("auth.rs"),
            line: Some(10),
            column: None,
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::AuthOrSecretHandling,
        }];
        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("auth.rs"),
            line: Some(10),
            preset: "rust_server".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth".to_string(),
            rationale: "test".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }];
        let extra_evidence = vec![StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::CallPath,
            file_path: Some(PathBuf::from("auth.rs")),
            line: Some(10),
            summary: "call expansion returned 3 nodes and 2 edges at depth 1".to_string(),
            detail: None,
        }];

        let (findings, _) = synthesize_evidence_based_findings_with_extra_evidence(
            &targets,
            &prompts,
            &[],
            &extra_evidence,
        );
        // Marker + CallPath = eligible finding
        assert!(!findings.is_empty());
        assert!(findings[0]
            .evidence
            .iter()
            .any(|e| e.kind == SecurityEvidenceKind::CallPath));
    }

    #[test]
    fn security_enriched_diagnostic_plus_marker_promotes_finding() {
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("code.rs"),
            line: Some(5),
            column: None,
            preset: "rust_server".to_string(),
            reason: SecurityTargetReason::ChangedHunk,
        }];
        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("code.rs"),
            line: Some(5),
            preset: "rust_server".to_string(),
            category: Some("sql".to_string()),
            title: "Review sql".to_string(),
            rationale: "test".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }];
        let extra_evidence = vec![StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::Diagnostic,
            file_path: Some(PathBuf::from("code.rs")),
            line: Some(5),
            summary: "LSP diagnostic: sql injection risk".to_string(),
            detail: None,
        }];

        let (findings, _) = synthesize_evidence_based_findings_with_extra_evidence(
            &targets,
            &prompts,
            &[],
            &extra_evidence,
        );
        // Marker + Diagnostic = eligible finding
        assert!(!findings.is_empty());
        assert!(findings[0]
            .evidence
            .iter()
            .any(|e| e.kind == SecurityEvidenceKind::Diagnostic));
    }

    #[test]
    fn security_enriched_marker_only_still_not_finding_without_support() {
        // No matching target in the same file, so only RiskMarker evidence exists.
        // RiskMarker alone is not finding-eligible.
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("other_target.rs"),
            line: Some(5),
            column: None,
            preset: "dependency_review".to_string(),
            reason: SecurityTargetReason::DependencyMetadata,
        }];
        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("code.rs"),
            line: Some(5),
            preset: "dependency_review".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth".to_string(),
            rationale: "test".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }];
        // Extra evidence is for a different file
        let extra_evidence = vec![StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::CallPath,
            file_path: Some(PathBuf::from("other.rs")),
            line: Some(1),
            summary: "call expansion in other file".to_string(),
            detail: None,
        }];

        let (findings, _) = synthesize_evidence_based_findings_with_extra_evidence(
            &targets,
            &prompts,
            &[],
            &extra_evidence,
        );
        // Marker-only for code.rs, no matching target, extra evidence for other.rs => no finding
        assert!(findings.is_empty());
    }

    #[test]
    fn security_enriched_different_file_evidence_does_not_promote() {
        // Target for a different file so no target evidence in the auth.rs group.
        let targets = vec![SecurityReviewTarget {
            file_path: PathBuf::from("other_target.rs"),
            line: Some(10),
            column: None,
            preset: "dependency_review".to_string(),
            reason: SecurityTargetReason::DependencyMetadata,
        }];
        let prompts = vec![SecurityReviewPrompt {
            file_path: PathBuf::from("auth.rs"),
            line: Some(10),
            preset: "dependency_review".to_string(),
            category: Some("auth".to_string()),
            title: "Review auth".to_string(),
            rationale: "test".to_string(),
            evidence: vec!["source: securityContext.risk_marker".to_string()],
        }];
        let extra_evidence = vec![StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::Diagnostic,
            file_path: Some(PathBuf::from("unrelated.rs")),
            line: Some(99),
            summary: "diagnostic in unrelated file".to_string(),
            detail: None,
        }];

        let (findings, _) = synthesize_evidence_based_findings_with_extra_evidence(
            &targets,
            &prompts,
            &[],
            &extra_evidence,
        );
        // Different file evidence doesn't support finding for auth.rs
        assert!(findings.is_empty());
    }

    // -- Enrichment runner integration tests --

    #[tokio::test]
    async fn security_enrichment_with_fixture_executor_returns_prompts_and_evidence() {
        let response = serde_json::json!({
            "risk_markers": [
                {
                    "category": "process",
                    "label": "command injection",
                    "line": 15,
                    "matched_text": "Command::new(shell)"
                }
            ],
            "security_relevant_diagnostics": [
                {"message": "unused import", "line": 3}
            ]
        });

        let output = SecurityReviewOutput {
            targets: vec![SecurityReviewTarget {
                file_path: PathBuf::from("cmd.rs"),
                line: Some(10),
                column: None,
                preset: "rust_server".to_string(),
                reason: SecurityTargetReason::ProcessExecution,
            }],
            findings: vec![],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: PathBuf::from("cmd.rs"),
                line: Some(10),
                preset: "rust_server".to_string(),
                category: Some("process".to_string()),
                title: "Review process".to_string(),
                rationale: "test".to_string(),
                evidence: vec!["source: securityContext.risk_marker".to_string()],
            }],
            preflight_results: vec![],
            notes: vec![],
        };

        let executor =
            FixtureSecurityContextExecutor::with_response(PathBuf::from("cmd.rs"), response);
        let opts = SecurityReviewWorkflowOptions::default();
        let results = run_security_context_enrichment(&output, &executor, &opts).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].prompts.len(), 1);
        assert!(results[0].prompts[0].title.contains("command injection"));
        // Should have RiskMarker + Diagnostic evidence
        assert!(results[0]
            .evidence
            .iter()
            .any(|e| e.kind == SecurityEvidenceKind::RiskMarker));
        assert!(results[0]
            .evidence
            .iter()
            .any(|e| e.kind == SecurityEvidenceKind::Diagnostic));
    }

    // -- Merge enrichment results tests --

    #[test]
    fn security_merge_enrichment_deduplicates_prompts() {
        let output = SecurityReviewOutput {
            targets: vec![],
            findings: vec![],
            review_prompts: vec![SecurityReviewPrompt {
                file_path: PathBuf::from("auth.rs"),
                line: Some(10),
                preset: "rust_server".to_string(),
                category: Some("auth".to_string()),
                title: "Review auth: token".to_string(),
                rationale: "test".to_string(),
                evidence: vec![],
            }],
            preflight_results: vec![],
            notes: vec![],
        };

        let enrichment = vec![SecurityContextEnrichmentResult {
            target: output
                .targets
                .first()
                .cloned()
                .unwrap_or(SecurityReviewTarget {
                    file_path: PathBuf::from("auth.rs"),
                    line: Some(10),
                    column: None,
                    preset: "rust_server".to_string(),
                    reason: SecurityTargetReason::AuthOrSecretHandling,
                }),
            level: SecurityContextEscalationLevel::Basic,
            request: serde_json::json!({}),
            response: None,
            prompts: vec![SecurityReviewPrompt {
                file_path: PathBuf::from("auth.rs"),
                line: Some(10),
                preset: "rust_server".to_string(),
                category: Some("auth".to_string()),
                title: "Review auth: token".to_string(),
                rationale: "test".to_string(),
                evidence: vec![],
            }],
            evidence: vec![],
            notes: vec![],
        }];

        let (merged_prompts, _, _) = merge_enrichment_results(&output, &enrichment);
        // Should not duplicate the existing prompt
        let count = merged_prompts
            .iter()
            .filter(|p| p.title == "Review auth: token")
            .count();
        assert_eq!(count, 1);
    }

    // -- Noop executor test --

    #[tokio::test]
    async fn security_noop_executor_always_errors() {
        let executor = NoopSecurityContextExecutor;
        let result = executor.security_context(serde_json::json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no securityContext executor"));
    }

    // -- Fixture executor request tracking test --

    #[tokio::test]
    async fn security_fixture_executor_tracks_requests() {
        let executor = FixtureSecurityContextExecutor::new();
        let req = serde_json::json!({"file_path": "missing.rs"});
        let _ = executor.security_context(req).await;
        let requests = executor.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
    }
}
