use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::types::*;

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

pub(crate) fn parse_hunk_header(line: &str, current_file: Option<&Path>) -> Option<ChangedHunk> {
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

pub(crate) fn parse_range(s: &str) -> Option<(u32, u32)> {
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
pub(crate) fn infer_reason_from_preset_or_content(
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
