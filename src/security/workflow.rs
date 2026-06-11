//! Security review vertical slice: diff parsing, preset selection, target
//! building, securityContext request construction, and review-prompt
//! generation.
//!
//! This module is intentionally decoupled from the LSP layer so it can run
//! without a language server.  Risk markers become review prompts — never
//! confirmed findings — in this pass.  Finding synthesis is deferred to a
//! later phase that requires concrete evidence.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Why a file/location was selected as a security review target.
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityTargetReason {
    ChangedHunk,
    DependencyMetadata,
    RiskMarker,
    PublicBoundary,
    UnsafeCode,
    ProcessExecution,
    FilesystemAccess,
    NetworkBoundary,
    AuthOrSecretHandling,
    Unknown,
}

/// A file/location selected for security review.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewTarget {
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub preset: String,
    pub reason: SecurityTargetReason,
}

/// A parsed hunk from a unified diff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

/// Reserved for future evidence-based synthesis. This vertical slice does not
/// emit this type — risk markers become [`SecurityReviewPrompt`]s, never
/// findings. Full finding synthesis will require concrete evidence, severity/
/// confidence enums, and affected-code grounding.
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

/// A review prompt derived from risk markers (not a confirmed finding).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewPrompt {
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub preset: String,
    pub category: Option<String>,
    pub title: String,
    pub rationale: String,
    pub evidence: Vec<String>,
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

/// Placeholder for future finding synthesis.  Empty in this vertical slice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewFindingStub {
    pub title: String,
    pub note: String,
}

/// Stable output shape for the security review workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewReport {
    pub targets: Vec<SecurityReviewTarget>,
    pub review_prompts: Vec<SecurityReviewPrompt>,
    pub findings: Vec<SecurityReviewFindingStub>,
    pub notes: Vec<String>,
}

/// Complete output from the full security review workflow (includes
/// preflight results and structured findings for future synthesis).
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
/// `diff --git a/... b/...` lines.  Deleted files (`--- /dev/null`),
/// binary markers, and files outside normal paths are skipped.
pub fn parse_changed_hunks(diff: &str) -> Vec<ChangedHunk> {
    let mut hunks = Vec::new();
    let mut current_file: Option<PathBuf> = None;
    let mut skip_file = false;

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // Extract the "b/path" side of "a/path b/path".
            if let Some((a_part, b_part)) = rest.split_once(" b/") {
                let a_path = a_part.trim();
                let b_path = b_part.trim();

                // Skip deleted files (b is /dev/null)
                if b_path == "/dev/null" || a_path == "/dev/null" {
                    skip_file = true;
                    current_file = None;
                    continue;
                }

                // Skip binary files
                if b_path == "/dev/null" {
                    skip_file = true;
                    current_file = None;
                    continue;
                }

                skip_file = false;
                current_file = Some(PathBuf::from(b_path));
            }
            continue;
        }

        // Skip binary diff markers
        if line.starts_with("Binary files ") || line.starts_with("GIT binary") {
            skip_file = true;
            continue;
        }

        // Skip deleted files (+++ /dev/null indicates deletion)
        if line == "+++ /dev/null" || line == "--- /dev/null" {
            skip_file = true;
            continue;
        }

        if skip_file {
            continue;
        }

        if let Some(hunk) = parse_hunk_header(line, current_file.as_deref()) {
            hunks.push(hunk);
        }
    }

    hunks
}

fn parse_hunk_header(line: &str, current_file: Option<&Path>) -> Option<ChangedHunk> {
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

/// Parse a per-file unified diff that may lack a `diff --git` header.
///
/// If `parse_changed_hunks(diff)` returns non-empty results, those are used
/// directly.  Otherwise, hunk headers (`@@ ... @@`) are parsed using
/// `file_path` as the associated file.  Binary and deleted markers are still
/// skipped where visible.
pub fn parse_changed_hunks_for_file(diff: &str, file_path: &Path) -> Vec<ChangedHunk> {
    let hunks = parse_changed_hunks(diff);
    if !hunks.is_empty() {
        return hunks;
    }

    // Fallback: parse hunk headers directly, associating them with `file_path`.
    let mut result = Vec::new();
    for line in diff.lines() {
        if line.starts_with("Binary files ") || line.starts_with("GIT binary") {
            return Vec::new();
        }
        if line == "+++ /dev/null" || line == "--- /dev/null" {
            return Vec::new();
        }
        if let Some(hunk) = parse_hunk_header(line, Some(file_path)) {
            result.push(hunk);
        }
    }
    result
}

/// Build a file-level security review target for files where no parseable
/// hunks are available.
///
/// Returns `None` for excluded paths.  When present, the target is
/// unpositioned (`line=None`, `column=None`) and uses content-based preset
/// selection when a content hint is available.
pub fn build_file_level_security_review_target(
    path: &Path,
    content_hint: Option<&str>,
) -> Option<SecurityReviewTarget> {
    if is_security_review_excluded_path(path) {
        return None;
    }
    let preset = select_security_preset(path, content_hint);
    let reason = infer_reason_from_preset_or_content(&preset, content_hint);
    Some(SecurityReviewTarget {
        file_path: path.to_path_buf(),
        line: None,
        column: None,
        preset,
        reason,
    })
}

// ---------------------------------------------------------------------------
// Exclusion rules
// ---------------------------------------------------------------------------

/// Returns true if a path should be excluded from security review.
///
/// Excludes generated/vendor directories and minified JS.  Cargo manifests,
/// lockfiles, and `build.rs` are intentionally kept reviewable.
pub fn is_security_review_excluded_path(path: &Path) -> bool {
    let full = path.to_string_lossy();

    // Skip known generated/vendor directories
    let skip_dirs = [
        "vendor/",
        "third_party/",
        "target/",
        "dist/",
        "build/",
        "node_modules/",
        "__pycache__/",
        ".eggs/",
        ".git/",
    ];
    for dir in &skip_dirs {
        if full.contains(dir) {
            return true;
        }
    }

    // Skip minified JS
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.ends_with(".min.js") {
            return true;
        }
    }

    // Skip binary/generated extensions (but NOT .lock — keep for dependency_review)
    let skip_exts = [".bin", ".exe", ".dll", ".so", ".dylib", ".o", ".a"];
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_with_dot = format!(".{ext}");
        if skip_exts.contains(&ext_with_dot.as_str()) {
            return true;
        }
    }

    // Skip hidden files (except .env patterns)
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.starts_with('.') && name != ".env" && !name.starts_with(".env.") {
            return true;
        }
    }

    false
}

/// Returns true if a file should be skipped from security review.
///
/// Alias for [`is_security_review_excluded_path`] kept for backward
/// compatibility with existing call sites.
pub fn should_skip_file(file_path: &Path) -> bool {
    is_security_review_excluded_path(file_path)
}

// ---------------------------------------------------------------------------
// Preset selection heuristics
// ---------------------------------------------------------------------------

/// Deterministic preset selection from a file path and optional content
/// snippet.  Rules are evaluated in order; first match wins.
///
/// Order:
/// 1. `dependency_review` — manifest/lockfile/build scripts
/// 2. `unsafe_review` — content or path signals unsafe/FFI
/// 3. `web_backend` — content or path signals web/auth/handler
/// 4. `rust_cli` — content or path signals CLI/process/fs
/// 5. `rust_server` — default for Rust files and unknown service code
pub fn select_security_preset(path: &Path, content_hint: Option<&str>) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let full = path.to_string_lossy().to_lowercase();
    let content_lower = content_hint.map(|c| c.to_lowercase());

    // 1. Dependency files
    if name == "Cargo.toml"
        || name == "Cargo.lock"
        || name == "build.rs"
        || full.contains("package.json")
        || full.contains("pnpm-lock.yaml")
        || full.contains("yarn.lock")
        || full.contains("package-lock.json")
        || full.contains("go.sum")
    {
        return "dependency_review".to_string();
    }

    // 2. Unsafe / FFI
    let unsafe_path_segs = ["unsafe", "ffi", "extern"];
    for seg in &unsafe_path_segs {
        if full.contains(seg) {
            return "unsafe_review".to_string();
        }
    }
    if let Some(ref content) = content_lower {
        let unsafe_content_hints = [
            "unsafe",
            "atomic",
            "unsafecell",
            "raw pointer",
            "ffi",
            "extern \"c\"",
        ];
        for hint in &unsafe_content_hints {
            if content.contains(hint) {
                return "unsafe_review".to_string();
            }
        }
    }

    // 3. Web backend
    let web_path_segs = [
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
        "request",
        "response",
    ];
    for seg in &web_path_segs {
        if full.contains(seg) {
            return "web_backend".to_string();
        }
    }
    if let Some(ref content) = content_lower {
        let web_content_hints = [
            "handler",
            "route",
            "router",
            "auth",
            "session",
            "jwt",
            "cookie",
            "middleware",
            "sql",
            "request",
            "response",
        ];
        for hint in &web_content_hints {
            if content.contains(hint) {
                return "web_backend".to_string();
            }
        }
    }

    // 4. CLI
    let cli_path_segs = ["cli", "command", "process", "fs", "config", "main"];
    for seg in &cli_path_segs {
        if full.contains(seg) {
            return "rust_cli".to_string();
        }
    }
    if let Some(ref content) = content_lower {
        let cli_content_hints = [
            "cli",
            "command",
            "args",
            "process",
            "command::new",
            "std::fs",
            "fs::",
            "config",
        ];
        for hint in &cli_content_hints {
            if content.contains(hint) {
                return "rust_cli".to_string();
            }
        }
    }

    // 5. Default
    "rust_server".to_string()
}

/// Legacy wrapper — selects preset from file path only (no content hint).
pub fn select_preset_for_file(file_path: &Path) -> String {
    select_security_preset(file_path, None)
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
// Build security review targets
// ---------------------------------------------------------------------------

/// Deduplication key for security review targets.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct SecurityReviewTargetKey {
    file_path: PathBuf,
    line: Option<u32>,
    preset: String,
    reason: SecurityTargetReason,
}

/// Build security review targets from parsed changed hunks.
///
/// For each hunk:
/// - excluded paths are skipped;
/// - `new_start` is used as the target line when `new_count > 0`;
/// - preset is selected from path + optional content hint;
/// - reason is inferred from the selected preset and content.
///
/// Targets are deduplicated by `(file_path, line, preset, reason)`.
pub fn build_security_review_targets(
    hunks: &[ChangedHunk],
    load_content: impl Fn(&Path) -> Option<String>,
) -> Vec<SecurityReviewTarget> {
    let mut seen = HashSet::new();
    let mut targets = Vec::new();

    for hunk in hunks {
        if is_security_review_excluded_path(&hunk.file_path) {
            continue;
        }

        let line = if hunk.new_count > 0 {
            Some(hunk.new_start)
        } else {
            None
        };
        let column = if line.is_some() { Some(1) } else { None };

        let content_hint = load_content(&hunk.file_path);
        let preset = select_security_preset(&hunk.file_path, content_hint.as_deref());
        let reason = infer_reason_from_preset_or_content(&preset, content_hint.as_deref());

        let key = SecurityReviewTargetKey {
            file_path: hunk.file_path.clone(),
            line,
            preset: preset.clone(),
            reason: reason.clone(),
        };

        if seen.insert(key) {
            targets.push(SecurityReviewTarget {
                file_path: hunk.file_path.clone(),
                line,
                column,
                preset,
                reason,
            });
        }
    }

    targets
}

/// Infer the [`SecurityTargetReason`] from a selected preset and optional
/// content hint.
fn infer_reason_from_preset_or_content(
    preset: &str,
    content_hint: Option<&str>,
) -> SecurityTargetReason {
    match preset {
        "dependency_review" => SecurityTargetReason::DependencyMetadata,
        "unsafe_review" => SecurityTargetReason::UnsafeCode,
        "web_backend" => {
            if let Some(content) = content_hint {
                let c = content.to_lowercase();
                if c.contains("auth") || c.contains("session") || c.contains("jwt") {
                    return SecurityTargetReason::AuthOrSecretHandling;
                }
                if c.contains("sql") || c.contains("database") {
                    return SecurityTargetReason::NetworkBoundary;
                }
            }
            SecurityTargetReason::ChangedHunk
        }
        "rust_cli" => {
            if let Some(content) = content_hint {
                let c = content.to_lowercase();
                if c.contains("process") || c.contains("command::new") {
                    return SecurityTargetReason::ProcessExecution;
                }
                if c.contains("std::fs") || c.contains("fs::") {
                    return SecurityTargetReason::FilesystemAccess;
                }
            }
            SecurityTargetReason::ChangedHunk
        }
        _ => SecurityTargetReason::ChangedHunk,
    }
}

// ---------------------------------------------------------------------------
// Build securityContext request payloads
// ---------------------------------------------------------------------------

/// Build a `securityContext` request payload for a security review target.
///
/// Returns a JSON value matching the `securityContext` operation input
/// schema.  `call_depth` is always 0 in this vertical slice.
pub fn build_security_context_request(target: &SecurityReviewTarget) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "operation".to_string(),
        serde_json::Value::String("securityContext".to_string()),
    );
    map.insert(
        "file_path".to_string(),
        serde_json::Value::String(target.file_path.to_string_lossy().to_string()),
    );
    map.insert(
        "security_preset".to_string(),
        serde_json::Value::String(target.preset.clone()),
    );
    map.insert("max_risk_markers".to_string(), serde_json::json!(80));
    map.insert("call_depth".to_string(), serde_json::json!(0));

    if let Some(line) = target.line {
        map.insert("line".to_string(), serde_json::json!(line));
    }
    if let Some(column) = target.column {
        map.insert("column".to_string(), serde_json::json!(column));
    }

    serde_json::Value::Object(map)
}

// ---------------------------------------------------------------------------
// Convert securityContext packets to review prompts
// ---------------------------------------------------------------------------

/// Convert parsed `securityContext` JSON output into review prompts.
///
/// Each risk marker becomes a [`SecurityReviewPrompt`].  No markers are
/// turned into findings in this vertical slice.  Malformed or missing
/// fields fail soft with empty prompts or notes.
pub fn prompts_from_security_context(
    target: &SecurityReviewTarget,
    context_json: &serde_json::Value,
) -> Vec<SecurityReviewPrompt> {
    let mut prompts = Vec::new();

    let risk_markers = match context_json.get("risk_markers") {
        Some(serde_json::Value::Array(arr)) => arr,
        _ => return prompts,
    };

    let truncated = context_json
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    for marker in risk_markers {
        let category = marker
            .get("category")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let label = marker
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let marker_line = marker
            .get("line")
            .and_then(|v| v.as_u64())
            .map(|l| l as u32);
        let matched_text = marker
            .get("matched_text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let rationale = marker
            .get("rationale")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let file_path = marker
            .get("file")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| target.file_path.clone());

        let line = marker_line.or(target.line);

        let title = format!(
            "Review {}: {}",
            category.as_deref().unwrap_or("unknown"),
            label
        );

        let mut evidence = vec![
            "source: securityContext.risk_marker".to_string(),
            category.unwrap_or_default(),
            matched_text.to_string(),
            rationale.to_string(),
            format!("target reason: {:?}", target.reason),
            format!("preset: {}", target.preset),
        ];

        if truncated {
            evidence.push("context was truncated".to_string());
        }

        prompts.push(SecurityReviewPrompt {
            file_path,
            line,
            preset: target.preset.clone(),
            category: marker
                .get("category")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            title,
            rationale: rationale.to_string(),
            evidence,
        });
    }

    prompts
}

// ---------------------------------------------------------------------------
// Report assembly
// ---------------------------------------------------------------------------

/// Assemble a [`SecurityReviewReport`] from targets, prompts, and notes.
///
/// Always includes the note that risk markers are review prompts, not
/// confirmed findings.  `findings` is always empty in this vertical slice.
pub fn assemble_security_review_report(
    targets: Vec<SecurityReviewTarget>,
    prompts: Vec<SecurityReviewPrompt>,
    mut notes: Vec<String>,
) -> SecurityReviewReport {
    notes.push("risk markers are review prompts, not confirmed findings".to_string());

    SecurityReviewReport {
        targets,
        review_prompts: prompts,
        findings: Vec::new(),
        notes,
    }
}

// ---------------------------------------------------------------------------
// Minimal invocation surface
// ---------------------------------------------------------------------------

/// Plan a security review from a unified diff string.
///
/// Creates targets and request payloads but does **not** execute LSP.
/// Returns a [`SecurityReviewReport`] with targets, review prompts, and
/// empty findings.
pub fn plan_security_review_from_diff(diff: &str, _repo_root: &Path) -> SecurityReviewReport {
    let hunks = parse_changed_hunks(diff);
    let targets = build_security_review_targets(&hunks, |_| None);
    let prompts: Vec<SecurityReviewPrompt> = targets
        .iter()
        .map(|t| SecurityReviewPrompt {
            file_path: t.file_path.clone(),
            line: t.line,
            preset: t.preset.clone(),
            category: None,
            title: format!("Review changed hunk: {}", t.file_path.display()),
            rationale: format!("Changed hunk detected (reason: {:?})", t.reason),
            evidence: vec![
                "source: changed_hunk".to_string(),
                format!("preset: {}", t.preset),
                format!("reason: {:?}", t.reason),
                "no securityContext executed in this planning step".to_string(),
            ],
        })
        .collect();

    assemble_security_review_report(
        targets,
        prompts,
        vec!["planned from diff — no LSP execution".to_string()],
    )
}

// ---------------------------------------------------------------------------
// Target discovery from diff (async, egggit-backed)
// ---------------------------------------------------------------------------

/// Discover security review targets from a git diff.
///
/// Uses `egggit::diff_summary` and `egggit::file_diff` to get changed
/// files, parse hunks, and create targets with the appropriate preset.
///
/// This is a read-only operation — it does not mutate the worktree.
pub async fn discover_targets_from_diff(
    root: &Path,
    base: Option<&str>,
) -> Result<Vec<SecurityReviewTarget>, String> {
    let summary = egggit::diff_summary(root, base)
        .await
        .map_err(|e| e.to_string())?;

    let mut all_hunks = Vec::new();
    let mut file_level_paths: Vec<(PathBuf, Option<String>)> = Vec::new();

    for file in &summary.files {
        if file.kind == egggit::diff::ChangeKind::Deleted {
            continue;
        }

        let path = PathBuf::from(&file.path);

        if should_skip_file(&path) {
            continue;
        }

        let content_hint = std::fs::read_to_string(root.join(&path)).ok();

        let file_diff = egggit::file_diff(root, &path, base)
            .await
            .map_err(|e| e.to_string())?;

        let hunks = parse_changed_hunks_for_file(&file_diff.patch, &path);

        if hunks.is_empty() {
            file_level_paths.push((path, content_hint));
        } else {
            all_hunks.extend(hunks);
        }
    }

    let mut targets =
        build_security_review_targets(&all_hunks, |p| std::fs::read_to_string(root.join(p)).ok());

    for (path, content_hint) in file_level_paths {
        if let Some(target) =
            build_file_level_security_review_target(&path, content_hint.as_deref())
        {
            targets.push(target);
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

/// Run deterministic preflight checks against target file paths.
///
/// These checks inspect **file names only**, not file contents.  Check names
/// and notes reflect this limitation explicitly.
pub fn run_preflight_checks(targets: &[SecurityReviewTarget]) -> Vec<SecurityPreflightResult> {
    let mut results = Vec::new();

    // Secret filename-hint scan — check file names for obvious indicators
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
        .map(|t| format!("{}: file name matches secret hint", t.file_path.display()))
        .collect();

    if secret_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            notes: vec!["No secret filename hints detected in target file names".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: secret_evidence,
            notes: vec!["Secret-like filename hints found in target file names".to_string()],
        });
    }

    // Unsafe filename-hint scan — check file names for unsafe indicators
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
        .map(|t| format!("{}: file name matches unsafe hint", t.file_path.display()))
        .collect();

    if unsafe_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "unsafe_filename_hint_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            notes: vec!["No unsafe filename hints detected in target file names".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "unsafe_filename_hint_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: unsafe_evidence,
            notes: vec!["Unsafe-like filename hints found in target file names".to_string()],
        });
    }

    results
}

// ---------------------------------------------------------------------------
// Finding synthesis (produces only prompts in the vertical slice)
// ---------------------------------------------------------------------------

/// Synthesize review prompts from targets, risk markers, and preflight
/// results.
///
/// In the vertical slice, risk markers **always** become
/// [`SecurityReviewPrompt`]s — never findings.  The returned
/// `Vec<SecurityReviewFinding>` is always empty by design.  Preflight
/// failures are also surfaced as prompts.
pub fn synthesize_findings(
    _targets: &[SecurityReviewTarget],
    risk_markers: &[SecurityRiskMarkerFromWorkflow],
    preflight: &[SecurityPreflightResult],
) -> (Vec<SecurityReviewFinding>, Vec<SecurityReviewPrompt>) {
    let mut prompts = Vec::new();

    // Process risk markers — always prompts, never findings
    for marker in risk_markers {
        prompts.push(SecurityReviewPrompt {
            file_path: marker.file_path.clone(),
            line: Some(marker.line),
            preset: String::new(),
            category: Some(marker.category.clone()),
            title: format!("Review {}: {}", marker.category, marker.label),
            rationale: marker.rationale.clone(),
            evidence: vec![
                marker.category.clone(),
                marker.matched_text.clone(),
                marker.rationale.clone(),
            ],
        });
    }

    // Process preflight failures — also prompts
    for result in preflight {
        if result.status == PreflightStatus::Fail {
            for evidence_str in &result.evidence {
                prompts.push(SecurityReviewPrompt {
                    file_path: PathBuf::new(),
                    line: None,
                    preset: String::new(),
                    category: Some(result.check_name.clone()),
                    title: format!("Preflight check failed: {}", result.check_name),
                    rationale: result.notes.join("; "),
                    evidence: vec![evidence_str.clone()],
                });
            }
        }
    }

    // Findings are always empty in the vertical slice
    (Vec::new(), prompts)
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
}
