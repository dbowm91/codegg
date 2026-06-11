//! Security review workflow: target discovery, preflight checks, and finding synthesis.
//!
//! This module wires together diff analysis, risk markers, and preflight
//! heuristics to produce a structured `SecurityReviewOutput`. It is
//! intentionally decoupled from the LSP layer so it can run without a
//! language server.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Why a file/location was selected as a security review target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityTargetReason {
    ChangedHunk,
    RiskMarker,
    PublicBoundary,
    UnsafeCode,
    ProcessExecution,
    FilesystemAccess,
    NetworkBoundary,
    AuthOrSecretHandling,
}

/// A file/location selected for security review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReviewTarget {
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub preset: String,
    pub reason: SecurityTargetReason,
}

/// A parsed hunk from a unified diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedHunk {
    pub file_path: PathBuf,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
}

/// Deterministic preflight check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPreflightResult {
    pub check_name: String,
    pub status: PreflightStatus,
    pub evidence: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PreflightStatus {
    Pass,
    Fail,
    Warn,
    Skipped,
}

/// Evidence supporting a finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEvidence {
    pub location: String,
    pub description: String,
}

/// A security review finding with structured evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReviewFinding {
    pub severity: String,
    pub confidence: String,
    pub title: String,
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub evidence: Vec<SecurityEvidence>,
    pub reasoning: String,
    pub recommendation: String,
    pub tests: Vec<String>,
}

/// A review prompt (marker-only, not a confirmed finding).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReviewPrompt {
    pub category: String,
    pub label: String,
    pub file_path: PathBuf,
    pub line: u32,
    pub matched_text: String,
    pub rationale: String,
}

/// Simplified risk marker used by the workflow (avoids importing LSP types).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityRiskMarkerFromWorkflow {
    pub category: String,
    pub label: String,
    pub file_path: PathBuf,
    pub line: u32,
    pub column: u32,
    pub matched_text: String,
    pub rationale: String,
}

/// Complete output from the security review workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReviewOutput {
    pub targets: Vec<SecurityReviewTarget>,
    pub findings: Vec<SecurityReviewFinding>,
    pub review_prompts: Vec<SecurityReviewPrompt>,
    pub preflight_results: Vec<SecurityPreflightResult>,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Diff parsing
// ---------------------------------------------------------------------------

/// Parse unified diff output to extract hunks.
///
/// Each hunk is associated with the most recently seen file path from
/// `diff --git a/... b/...` lines.
pub fn parse_unified_diff_hunks(patch: &str) -> Vec<ChangedHunk> {
    let mut hunks = Vec::new();
    let mut current_file: Option<PathBuf> = None;

    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // Extract the "b/path" side of "a/path b/path".
            if let Some((_, b_part)) = rest.split_once(" b/") {
                current_file = Some(PathBuf::from(b_part));
            }
            continue;
        }

        if let Some(hunk) = parse_hunk_line(line, current_file.as_deref()) {
            hunks.push(hunk);
        }
    }

    hunks
}

fn parse_hunk_line(line: &str, current_file: Option<&Path>) -> Option<ChangedHunk> {
    // Format: @@ -old_start,old_count +new_start,new_count @@ context
    let rest = line.strip_prefix("@@ ")?;
    let plus_idx = rest.find(" +")?;
    let after_plus = &rest[plus_idx + 2..]; // after the " +"
    let end_idx = after_plus.find(" @")?;

    let old_range = &rest[..plus_idx]; // e.g. "-10,6"
    let new_range = &after_plus[..end_idx]; // e.g. "10,8" (no leading +)

    // Strip leading `-` from old range
    let old_range = old_range.strip_prefix('-')?;

    let (old_start, old_count) = parse_range(old_range)?;
    let (new_start, new_count) = parse_range(new_range)?;

    Some(ChangedHunk {
        file_path: current_file?.to_path_buf(),
        old_start,
        old_count,
        new_start,
        new_count,
    })
}

fn parse_range(s: &str) -> Option<(u32, u32)> {
    let (start_str, count_str) = if let Some(comma) = s.find(',') {
        (&s[..comma], &s[comma + 1..])
    } else {
        (s, "1")
    };
    let start = start_str.parse::<u32>().ok()?;
    let count = count_str.parse::<u32>().ok()?;
    Some((start, count))
}

// ---------------------------------------------------------------------------
// Preset selection
// ---------------------------------------------------------------------------

/// Deterministic preset selection heuristics for a file path.
pub fn select_preset_for_file(file_path: &Path) -> String {
    let name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let full = file_path.to_string_lossy();

    // Dependency files
    if name == "Cargo.toml"
        || name == "Cargo.lock"
        || name == "build.rs"
        || full.contains("package.json")
        || full.contains("go.sum")
    {
        return "dependency_review".to_string();
    }

    // Unsafe code
    if name.to_lowercase().contains("unsafe") {
        return "unsafe_review".to_string();
    }

    // Web backend segments
    let web_segments = [
        "auth",
        "middleware",
        "handler",
        "route",
        "server",
        "session",
        "jwt",
        "cookie",
        "sql",
        "database",
    ];
    for seg in &web_segments {
        if full.to_lowercase().contains(seg) {
            return "web_backend".to_string();
        }
    }

    // CLI segments
    let cli_segments = ["cli", "command", "process", "fs", "config", "main"];
    for seg in &cli_segments {
        if full.to_lowercase().contains(seg) {
            return "rust_cli".to_string();
        }
    }

    "rust_server".to_string()
}

// ---------------------------------------------------------------------------
// File skipping
// ---------------------------------------------------------------------------

/// Returns true if a file should be skipped from security review.
pub fn should_skip_file(file_path: &Path) -> bool {
    let full = file_path.to_string_lossy();

    // Skip known generated/vendor directories
    let skip_dirs = [
        "target/",
        "node_modules/",
        ".git/",
        "vendor/",
        "dist/",
        "build/",
        "__pycache__/",
        ".eggs/",
    ];
    for dir in &skip_dirs {
        if full.contains(dir) {
            return true;
        }
    }

    // Skip binary/generated extensions
    let skip_exts = [
        ".lock", ".sum", ".bin", ".exe", ".dll", ".so", ".dylib", ".o", ".a",
    ];
    if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
        let ext_with_dot = format!(".{ext}");
        if skip_exts.contains(&ext_with_dot.as_str()) {
            return true;
        }
    }

    // Skip hidden files (except .env patterns)
    if let Some(name) = file_path.file_name().and_then(|n| n.to_str()) {
        if name.starts_with('.') && name != ".env" && !name.starts_with(".env.") {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Risk reason classification
// ---------------------------------------------------------------------------

/// Returns true for high-risk target reasons.
pub fn is_high_risk_reason(reason: &SecurityTargetReason) -> bool {
    matches!(
        reason,
        SecurityTargetReason::UnsafeCode
            | SecurityTargetReason::ProcessExecution
            | SecurityTargetReason::NetworkBoundary
            | SecurityTargetReason::AuthOrSecretHandling
    )
}

// ---------------------------------------------------------------------------
// Target discovery from diff
// ---------------------------------------------------------------------------

/// Discover security review targets from a git diff.
///
/// Uses `egggit::diff_summary` and `egggit::file_diff` to get changed
/// files, parse hunks, and create targets with the appropriate preset.
pub async fn discover_targets_from_diff(
    root: &Path,
    base: Option<&str>,
) -> Result<Vec<SecurityReviewTarget>, String> {
    let summary = egggit::diff_summary(root, base)
        .await
        .map_err(|e| e.to_string())?;

    let mut targets = Vec::new();

    for file in &summary.files {
        // Skip deleted files
        if file.kind == egggit::diff::ChangeKind::Deleted {
            continue;
        }

        let path = PathBuf::from(&file.path);

        if should_skip_file(&path) {
            continue;
        }

        let preset = select_preset_for_file(&path);

        // Get the per-file diff and parse hunks
        let file_diff = egggit::file_diff(root, &path, base)
            .await
            .map_err(|e| e.to_string())?;

        let hunks = parse_unified_diff_hunks(&file_diff.patch);

        if hunks.is_empty() {
            // Binary or no parseable hunks — file-level target
            targets.push(SecurityReviewTarget {
                file_path: path,
                line: None,
                column: None,
                preset,
                reason: SecurityTargetReason::ChangedHunk,
            });
        } else {
            for hunk in &hunks {
                targets.push(SecurityReviewTarget {
                    file_path: hunk.file_path.clone(),
                    line: Some(hunk.new_start),
                    column: None,
                    preset: preset.clone(),
                    reason: SecurityTargetReason::ChangedHunk,
                });
            }
        }
    }

    Ok(targets)
}

// ---------------------------------------------------------------------------
// Preflight checks
// ---------------------------------------------------------------------------

const SECRET_PATTERNS: &[&str] = &[
    "API_KEY",
    "SECRET",
    "PASSWORD",
    "TOKEN",
    "PRIVATE_KEY",
    "api_key",
    "secret_key",
    "password",
    "private_key",
];

const UNSAFE_PATTERNS: &[&str] = &[
    "unsafe {",
    "unsafe fn",
    "unsafe impl",
    "transmute",
    "raw pointer",
];

/// Run deterministic preflight checks against target file contents.
///
/// For now this scans the file paths (not contents) for pattern matches.
/// In a full implementation the caller would provide file contents.
pub fn run_preflight_checks(targets: &[SecurityReviewTarget]) -> Vec<SecurityPreflightResult> {
    let mut results = Vec::new();

    // Secret pattern scan — check file names for obvious indicators
    let secret_evidence: Vec<String> = targets
        .iter()
        .filter(|t| {
            let name = t
                .file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
            SECRET_PATTERNS
                .iter()
                .any(|pat| name.contains(&pat.to_lowercase()))
        })
        .map(|t| {
            format!(
                "{}: file name matches secret pattern",
                t.file_path.display()
            )
        })
        .collect();

    if secret_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "secret_pattern_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            notes: vec!["No secret patterns detected in target file names".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "secret_pattern_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: secret_evidence,
            notes: vec!["Secret-like patterns found in target file names".to_string()],
        });
    }

    // Unsafe pattern scan — check file names for unsafe indicators
    let unsafe_evidence: Vec<String> = targets
        .iter()
        .filter(|t| {
            let name = t
                .file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
            UNSAFE_PATTERNS
                .iter()
                .any(|pat| name.contains(&pat.to_lowercase()))
        })
        .map(|t| {
            format!(
                "{}: file name matches unsafe pattern",
                t.file_path.display()
            )
        })
        .collect();

    if unsafe_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "unsafe_pattern_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            notes: vec!["No unsafe patterns detected in target file names".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "unsafe_pattern_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: unsafe_evidence,
            notes: vec!["Unsafe-like patterns found in target file names".to_string()],
        });
    }

    results
}

// ---------------------------------------------------------------------------
// Finding synthesis
// ---------------------------------------------------------------------------

/// Synthesize findings and review prompts from targets, risk markers, and
/// preflight results.
///
/// Risk markers WITHOUT additional evidence → `SecurityReviewPrompt`
/// Risk markers WITH changed code + plausible flow → `SecurityReviewFinding`
/// Preflight failures → `SecurityReviewFinding`
pub fn synthesize_findings(
    targets: &[SecurityReviewTarget],
    risk_markers: &[SecurityRiskMarkerFromWorkflow],
    preflight: &[SecurityPreflightResult],
) -> (Vec<SecurityReviewFinding>, Vec<SecurityReviewPrompt>) {
    let mut findings = Vec::new();
    let mut prompts = Vec::new();

    // Build a set of target file+line for quick lookup
    use std::collections::HashSet;
    let target_lines: HashSet<(PathBuf, u32)> = targets
        .iter()
        .filter_map(|t| {
            let line = t.line?;
            Some((t.file_path.clone(), line))
        })
        .collect();

    // Process risk markers
    for marker in risk_markers {
        let has_changed_code = if target_lines.is_empty() {
            // No target line info available — treat as changed for synthesis
            true
        } else {
            target_lines
                .iter()
                .any(|(fp, line)| *fp == marker.file_path && *line == marker.line)
        };
        let has_plausible_flow = marker.rationale.to_lowercase().contains("flow")
            || marker.rationale.to_lowercase().contains("call")
            || marker.rationale.to_lowercase().contains("reach");

        // If the marker overlaps a changed hunk and there's plausible flow,
        // emit a finding; otherwise emit a review prompt.
        if has_changed_code && has_plausible_flow {
            findings.push(SecurityReviewFinding {
                severity: "medium".to_string(),
                confidence: "medium".to_string(),
                title: format!("{}: {}", marker.category, marker.label),
                file_path: marker.file_path.clone(),
                line: Some(marker.line),
                evidence: vec![SecurityEvidence {
                    location: format!("line {}", marker.line),
                    description: marker.matched_text.clone(),
                }],
                reasoning: marker.rationale.clone(),
                recommendation: format!(
                    "Review {} at line {} for potential security implications",
                    marker.label, marker.line
                ),
                tests: Vec::new(),
            });
        } else {
            prompts.push(SecurityReviewPrompt {
                category: marker.category.clone(),
                label: marker.label.clone(),
                file_path: marker.file_path.clone(),
                line: marker.line,
                matched_text: marker.matched_text.clone(),
                rationale: marker.rationale.clone(),
            });
        }
    }

    // Process preflight failures
    for result in preflight {
        if result.status == PreflightStatus::Fail {
            for evidence_str in &result.evidence {
                findings.push(SecurityReviewFinding {
                    severity: "high".to_string(),
                    confidence: "high".to_string(),
                    title: format!("Preflight check failed: {}", result.check_name),
                    file_path: PathBuf::new(),
                    line: None,
                    evidence: vec![SecurityEvidence {
                        location: evidence_str.clone(),
                        description: result.notes.join("; "),
                    }],
                    reasoning: format!(
                        "Preflight check '{}' detected a potential issue",
                        result.check_name
                    ),
                    recommendation: format!(
                        "Investigate the {} check failure before merging",
                        result.check_name
                    ),
                    tests: Vec::new(),
                });
            }
        }
    }

    (findings, prompts)
}

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
pub use SecurityPreflightResult as PreflightResult;
#[allow(unused_imports)]
pub use SecurityReviewFinding as ReviewFinding;
#[allow(unused_imports)]
pub use SecurityReviewOutput as ReviewOutput;
#[allow(unused_imports)]
pub use SecurityReviewPrompt as ReviewPrompt;
#[allow(unused_imports)]
pub use SecurityReviewTarget as ReviewTarget;
#[allow(unused_imports)]
pub use SecurityTargetReason as TargetReason;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_unified_diff_hunks_basic() {
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

        let hunks = parse_unified_diff_hunks(patch);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, PathBuf::from("src/lib.rs"));
        assert_eq!(hunks[0].old_start, 10);
        assert_eq!(hunks[0].old_count, 6);
        assert_eq!(hunks[0].new_start, 10);
        assert_eq!(hunks[0].new_count, 8);
    }

    #[test]
    fn parse_unified_diff_hunks_multiple_files() {
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

        let hunks = parse_unified_diff_hunks(patch);
        assert_eq!(hunks.len(), 3);
        assert_eq!(hunks[0].file_path, PathBuf::from("src/a.rs"));
        assert_eq!(hunks[1].file_path, PathBuf::from("src/a.rs"));
        assert_eq!(hunks[2].file_path, PathBuf::from("src/b.rs"));
    }

    #[test]
    fn parse_unified_diff_hunks_empty() {
        let hunks = parse_unified_diff_hunks("");
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
        let hunks = parse_unified_diff_hunks(patch);
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
    fn select_preset_dependency_files() {
        assert_eq!(
            select_preset_for_file(Path::new("Cargo.toml")),
            "dependency_review"
        );
        assert_eq!(
            select_preset_for_file(Path::new("Cargo.lock")),
            "dependency_review"
        );
        assert_eq!(
            select_preset_for_file(Path::new("build.rs")),
            "dependency_review"
        );
        assert_eq!(
            select_preset_for_file(Path::new("src/package.json")),
            "dependency_review"
        );
        assert_eq!(
            select_preset_for_file(Path::new("vendor/go.sum")),
            "dependency_review"
        );
    }

    #[test]
    fn select_preset_unsafe() {
        assert_eq!(
            select_preset_for_file(Path::new("src/unsafe_ops.rs")),
            "unsafe_review"
        );
        assert_eq!(
            select_preset_for_file(Path::new("UnsafeBlock.rs")),
            "unsafe_review"
        );
    }

    #[test]
    fn select_preset_web_backend() {
        assert_eq!(
            select_preset_for_file(Path::new("src/auth/handler.rs")),
            "web_backend"
        );
        assert_eq!(
            select_preset_for_file(Path::new("src/middleware/session.rs")),
            "web_backend"
        );
        assert_eq!(
            select_preset_for_file(Path::new("src/routes/jwt.rs")),
            "web_backend"
        );
        assert_eq!(
            select_preset_for_file(Path::new("src/sql/database.rs")),
            "web_backend"
        );
    }

    #[test]
    fn select_preset_rust_cli() {
        assert_eq!(
            select_preset_for_file(Path::new("src/cli/main.rs")),
            "rust_cli"
        );
        assert_eq!(
            select_preset_for_file(Path::new("src/command/process.rs")),
            "rust_cli"
        );
        assert_eq!(
            select_preset_for_file(Path::new("src/fs/mod.rs")),
            "rust_cli"
        );
        assert_eq!(
            select_preset_for_file(Path::new("src/config.rs")),
            "rust_cli"
        );
    }

    #[test]
    fn select_preset_default() {
        assert_eq!(
            select_preset_for_file(Path::new("src/lib.rs")),
            "rust_server"
        );
        assert_eq!(
            select_preset_for_file(Path::new("src/model.rs")),
            "rust_server"
        );
    }

    #[test]
    fn should_skip_target_dir() {
        assert!(should_skip_file(Path::new("target/debug/binary")));
        assert!(should_skip_file(Path::new("node_modules/pkg/index.js")));
        assert!(should_skip_file(Path::new(".git/HEAD")));
        assert!(should_skip_file(Path::new("vendor/foo.rs")));
        assert!(should_skip_file(Path::new("dist/bundle.js")));
        assert!(should_skip_file(Path::new("build/output.rs")));
        assert!(should_skip_file(Path::new("__pycache__/mod.pyc")));
        assert!(should_skip_file(Path::new(".eggs/setup.py")));
    }

    #[test]
    fn should_skip_lock_files() {
        assert!(should_skip_file(Path::new("Cargo.lock")));
        assert!(should_skip_file(Path::new("go.sum")));
        assert!(should_skip_file(Path::new("lib.dll")));
        assert!(should_skip_file(Path::new("lib.so")));
        assert!(should_skip_file(Path::new("lib.dylib")));
        assert!(should_skip_file(Path::new("obj.o")));
        assert!(should_skip_file(Path::new("lib.a")));
        assert!(should_skip_file(Path::new("prog.exe")));
        assert!(should_skip_file(Path::new("prog.bin")));
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
    fn should_not_skip_normal_files() {
        assert!(!should_skip_file(Path::new("src/lib.rs")));
        assert!(!should_skip_file(Path::new("README.md")));
        assert!(!should_skip_file(Path::new("src/main.rs")));
    }

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
            check_name: "secret_pattern_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            notes: Vec::new(),
        }];

        let (findings, prompts) = synthesize_findings(&targets, &markers, &preflight);

        // Marker with no "flow"/"call"/"reach" in rationale → prompt, not finding
        assert!(findings.is_empty());
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].category, "unsafe_code");
        assert_eq!(prompts[0].line, 10);
    }

    #[test]
    fn synthesize_findings_marker_with_flow_produces_finding() {
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
            check_name: "secret_pattern_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            notes: Vec::new(),
        }];

        let (findings, prompts) = synthesize_findings(&targets, &markers, &preflight);

        assert_eq!(findings.len(), 1);
        assert!(prompts.is_empty());
        assert_eq!(findings[0].severity, "medium");
        assert!(findings[0].evidence.len() == 1);
    }

    #[test]
    fn synthesize_findings_preflight_failure_produces_finding() {
        let targets = vec![];
        let markers = vec![];
        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_pattern_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: vec!["api_key.rs: secret pattern in name".to_string()],
            notes: vec!["Secret-like patterns found".to_string()],
        }];

        let (findings, prompts) = synthesize_findings(&targets, &markers, &preflight);

        assert_eq!(findings.len(), 1);
        assert!(prompts.is_empty());
        assert_eq!(findings[0].severity, "high");
        assert_eq!(findings[0].confidence, "high");
        assert!(findings[0].title.contains("secret_pattern_scan"));
    }

    #[test]
    fn synthesize_findings_preflight_pass_no_findings() {
        let targets = vec![];
        let markers = vec![];
        let preflight = vec![SecurityPreflightResult {
            check_name: "secret_pattern_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            notes: Vec::new(),
        }];

        let (findings, prompts) = synthesize_findings(&targets, &markers, &preflight);

        assert!(findings.is_empty());
        assert!(prompts.is_empty());
    }

    #[test]
    fn parse_hunk_line_single_count() {
        // @@ -1 +1,2 @@
        let hunk = parse_hunk_line("@@ -1 +1,2 @@", Some(Path::new("a.rs")));
        assert!(hunk.is_some());
        let h = hunk.unwrap();
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 1);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 2);
    }

    #[test]
    fn parse_hunk_line_no_file_returns_none() {
        let hunk = parse_hunk_line("@@ -1,3 +1,4 @@", None);
        assert!(hunk.is_none());
    }

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
            .find(|r| r.check_name == "secret_pattern_scan")
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
}
