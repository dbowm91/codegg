//! `/lsp-doctor` diagnostic report.
//!
//! Composes root diagnosis, server status, capability summary, cache
//! status, and preview status into a single read-only diagnostic
//! surface. Does **not** start any LSP server.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::health::LspObservabilitySnapshot;

/// Comprehensive diagnostic report for a file path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDoctorReport {
    /// The input path that was diagnosed.
    pub input_path: String,
    /// Canonicalized path (if available).
    pub canonical_path: Option<String>,
    /// Whether the file is inside the allowed root.
    pub inside_allowed_root: bool,
    /// Selected workspace root.
    pub selected_root: Option<String>,
    /// Root markers found walking up from the file.
    pub root_markers_found: Vec<String>,
    /// Detected language from file extension.
    pub detected_language: Option<String>,
    /// Server profile that would be used.
    pub server_profile: Option<String>,
    /// Active server key if already running.
    pub active_server_key: Option<String>,
    /// Operational state label if running.
    pub operational_state: Option<String>,
    /// Server generation if running.
    pub generation: Option<u64>,
    /// Rendered capability summary if initialized.
    pub capability_summary: Option<String>,
    /// Bounded stderr tail from the server process.
    pub stderr_tail: Vec<String>,
    /// Semantic cache mode ("enabled" or "disabled").
    pub cache_mode: String,
    /// Semantic cache entry count.
    pub cache_entries: usize,
    /// Total preview artifacts registered.
    pub preview_count: usize,
    /// Number of stale preview artifacts.
    pub preview_stale_count: usize,
    /// Issues found during diagnosis.
    pub issues: Vec<String>,
    /// Remediation suggestions.
    pub remediation: Vec<String>,
    /// Observability snapshot (clients, cache stats, previews).
    pub observability: Option<LspObservabilitySnapshot>,
}

/// Build an [`LspDoctorReport`] for the given file path.
///
/// This is an async function because it may query the live [`LspService`]
/// for active server state. It does **not** start any server.
///
/// # Arguments
///
/// * `input_path` — The file path to diagnose.
/// * `allowed_root` — Optional allowed root boundary.
/// * `service` — Optional live LSP service for server state queries.
/// * `cache_mode` — Semantic cache mode string ("enabled" or "disabled").
/// * `cache_entries` — Number of cache entries.
/// * `preview_total_count` — Total registered preview artifacts.
/// * `preview_stale_count` — Number of stale preview artifacts.
/// * `observability` — Optional observability snapshot for rich metrics.
pub async fn build_doctor_report(
    input_path: &Path,
    allowed_root: Option<&Path>,
    service: Option<&crate::service::LspService>,
    cache_mode: &str,
    cache_entries: usize,
    preview_total_count: usize,
    preview_stale_count: usize,
    observability: Option<LspObservabilitySnapshot>,
) -> LspDoctorReport {
    let canonical_path = input_path
        .canonicalize()
        .ok()
        .map(|p| p.display().to_string());

    let root_diag = crate::root::diagnose_root(input_path, allowed_root);

    let mut issues = root_diag.issues.clone();
    let mut remediation = Vec::new();

    // Query live service for active server info.
    let mut active_server_key: Option<String> = None;
    let mut operational_state: Option<String> = None;
    let mut generation: Option<u64> = None;
    let mut capability_summary: Option<String> = None;
    let mut stderr_tail: Vec<String> = Vec::new();

    if let Some(svc) = service {
        let keys = svc.client_keys().await;
        if let Some(profile) = &root_diag.server_profile {
            // Find a key that matches this server profile.
            for key in &keys {
                if key.contains(profile.as_str()) {
                    active_server_key = Some(key.clone());
                    if let Some(state) = svc.operational_state_for_key(key).await {
                        operational_state = Some(state.label().to_string());
                        if let Some(note) = state.context_note() {
                            issues.push(note);
                        }
                    }
                    generation = Some(svc.generation_for_key(key).await);
                    if let Some(caps) = svc.effective_capabilities_for_key(key).await {
                        let summary = crate::tui_summary::ServerCapabilitySummary::from(&caps);
                        capability_summary = Some(render_caps_brief(&summary));
                    }
                    if let Some(health) = svc.operational_health_snapshot(key).await {
                        stderr_tail = health.stderr_tail;
                    }
                    break;
                }
            }
        }
    } else {
        issues.push("No LSP service available".to_string());
    }

    // Build remediation hints.
    if !root_diag.inside_allowed_root {
        remediation.push(format!(
            "Move the file inside the allowed root: {}",
            allowed_root
                .map(|r| r.display().to_string())
                .unwrap_or_default()
        ));
    }
    if root_diag.selected_root.is_none() {
        remediation.push(
            "Add a root marker (e.g. Cargo.toml, package.json) to the project directory"
                .to_string(),
        );
    }
    if root_diag.server_profile.is_none() {
        if let Some(lang) = &root_diag.detected_language {
            remediation.push(format!("No LSP server is configured for language: {lang}"));
        } else {
            remediation.push("Could not detect language from file extension".to_string());
        }
    }
    if active_server_key.is_none() && root_diag.server_profile.is_some() {
        remediation.push(format!(
            "Install {} and ensure it is on PATH, then reopen a file in this project",
            root_diag
                .server_profile
                .as_deref()
                .unwrap_or("the LSP server")
        ));
    }
    if operational_state
        .as_deref()
        .map(|s| s == "failed" || s == "degraded")
        .unwrap_or(false)
    {
        remediation
            .push("Restart the server with /lsp-restart or check stderr for errors".to_string());
    }
    if cache_mode == "disabled" {
        remediation
            .push("Enable cache via [lsp_semantic_cache] config with mode=\"memory\"".to_string());
    }
    if preview_stale_count > 0 {
        remediation.push(
            "Some previews are stale; re-run the original LSP command to refresh".to_string(),
        );
    }
    if issues.is_empty() && remediation.is_empty() {
        remediation.push("LSP is operational for this file".to_string());
    }

    LspDoctorReport {
        input_path: root_diag.input_path,
        canonical_path,
        inside_allowed_root: root_diag.inside_allowed_root,
        selected_root: root_diag.selected_root,
        root_markers_found: root_diag.root_markers_found,
        detected_language: root_diag.detected_language,
        server_profile: root_diag.server_profile,
        active_server_key,
        operational_state,
        generation,
        capability_summary,
        stderr_tail,
        cache_mode: cache_mode.to_string(),
        cache_entries,
        preview_count: preview_total_count,
        preview_stale_count,
        issues,
        remediation,
        observability,
    }
}

/// Render a [`LspDoctorReport`] as human-readable text.
pub fn render_doctor_report(report: &LspDoctorReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("LSP Doctor: {}", report.input_path));

    if let Some(ref canonical) = report.canonical_path {
        lines.push(format!("  Canonical: {canonical}"));
    }

    lines.push(format!(
        "  Inside allowed root: {}",
        if report.inside_allowed_root {
            "yes"
        } else {
            "no"
        }
    ));

    if let Some(ref root) = report.selected_root {
        lines.push(format!("  Workspace root: {root}"));
    } else {
        lines.push("  Workspace root: (none)".to_string());
    }

    if report.root_markers_found.is_empty() {
        lines.push("  Root markers: (none)".to_string());
    } else {
        lines.push(format!(
            "  Root markers: {}",
            report.root_markers_found.join(", ")
        ));
    }

    if let Some(ref lang) = report.detected_language {
        lines.push(format!("  Language: {lang}"));
    } else {
        lines.push("  Language: (unrecognized)".to_string());
    }

    if let Some(ref profile) = report.server_profile {
        lines.push(format!("  Server profile: {profile}"));
    } else {
        lines.push("  Server profile: (none)".to_string());
    }

    // Active server info.
    if let Some(ref key) = report.active_server_key {
        lines.push(format!("  Active server: {key}"));
        if let Some(ref state) = report.operational_state {
            lines.push(format!("  Operational state: {state}"));
        }
        if let Some(gen) = report.generation {
            lines.push(format!("  Generation: {gen}"));
        }
    } else {
        lines.push("  Active server: (none)".to_string());
    }

    // Capabilities.
    if let Some(ref caps) = report.capability_summary {
        lines.push(format!("  Capabilities: {caps}"));
    } else if report.active_server_key.is_some() {
        lines.push("  Capabilities: not yet initialized".to_string());
    }

    // Stderr.
    if !report.stderr_tail.is_empty() {
        let tail: Vec<&String> = report.stderr_tail.iter().rev().take(5).rev().collect();
        lines.push(format!("  Stderr (last {}):", tail.len()));
        for line in &tail {
            lines.push(format!("    {line}"));
        }
    }

    // Cache.
    lines.push(format!(
        "  Cache: mode={}, entries={}",
        report.cache_mode, report.cache_entries
    ));

    // Previews.
    lines.push(format!(
        "  Previews: total={}, stale={}",
        report.preview_count, report.preview_stale_count
    ));

    // Observability detail.
    if let Some(ref obs) = report.observability {
        lines.push("  Observability:".to_string());
        for line in obs.render_detail().lines() {
            lines.push(format!("    {line}"));
        }
    }

    // Issues.
    if report.issues.is_empty() {
        lines.push("  Issues: (none)".to_string());
    } else {
        lines.push("  Issues:".to_string());
        for issue in &report.issues {
            lines.push(format!("    - {issue}"));
        }
    }

    // Remediation.
    if report.remediation.is_empty() {
        lines.push("  Remediation: (none needed)".to_string());
    } else {
        lines.push("  Remediation:".to_string());
        for hint in &report.remediation {
            lines.push(format!("    - {hint}"));
        }
    }

    lines.join("\n")
}

/// Render a compact capability summary string.
fn render_caps_brief(caps: &crate::tui_summary::ServerCapabilitySummary) -> String {
    let mut supported = Vec::new();
    let all = [
        ("diagnostics", caps.supports_diagnostics),
        ("hover", caps.supports_hover),
        ("completion", caps.supports_completion),
        ("rename", caps.supports_rename),
        ("code_actions", caps.supports_code_actions),
        ("formatting", caps.supports_document_formatting),
        ("declaration", caps.supports_declaration),
        ("implementation", caps.supports_implementation),
        ("signature_help", caps.supports_signature_help),
        ("semantic_tokens", caps.supports_semantic_tokens),
    ];
    for (name, flag) in &all {
        if *flag {
            supported.push(*name);
        }
    }
    if supported.is_empty() {
        "none advertised".to_string()
    } else {
        supported.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn doctor_report_missing_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("missing.py");
        let report = futures::executor::block_on(build_doctor_report(
            &file, None, None, "disabled", 0, 0, 0, None,
        ));
        assert_eq!(report.detected_language.as_deref(), Some("python"));
        // No allowed root set, so inside_allowed_root is always true.
        assert!(report.inside_allowed_root);
        assert!(!report.issues.is_empty());
        assert!(!report.remediation.is_empty());
    }

    #[test]
    fn doctor_report_outside_allowed_root() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]").unwrap();
        let file = tmp.path().join("main.rs");
        fs::write(&file, "fn main() {}").unwrap();
        let allowed = Path::new("/completely/different/path");

        let report = futures::executor::block_on(build_doctor_report(
            &file,
            Some(allowed),
            None,
            "disabled",
            0,
            0,
            0,
            None,
        ));
        assert!(!report.inside_allowed_root);
        assert!(report
            .remediation
            .iter()
            .any(|r| r.contains("Move the file")));
    }

    #[test]
    fn doctor_report_unsupported_language() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".git"), "").unwrap();
        let file = tmp.path().join("data.weirdext");
        fs::write(&file, "").unwrap();

        let report = futures::executor::block_on(build_doctor_report(
            &file, None, None, "disabled", 0, 0, 0, None,
        ));
        assert!(report.detected_language.is_none());
        assert!(report.server_profile.is_none());
        assert!(report
            .issues
            .iter()
            .any(|i| i.contains("Could not detect language")));
    }

    #[test]
    fn doctor_report_no_root_marker() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("orphan.py");
        fs::write(&file, "print('hello')").unwrap();

        let report = futures::executor::block_on(build_doctor_report(
            &file, None, None, "disabled", 0, 0, 0, None,
        ));
        assert!(report.selected_root.is_none());
        assert!(report.issues.iter().any(|i| i.contains("No project root")));
        assert!(report.remediation.iter().any(|r| r.contains("root marker")));
    }

    #[test]
    fn doctor_report_happy_path_rust() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        let src = tmp.path().join("src");
        fs::create_dir(&src).unwrap();
        let file = src.join("main.rs");
        fs::write(&file, "fn main() {}").unwrap();

        let report = futures::executor::block_on(build_doctor_report(
            &file, None, None, "disabled", 0, 0, 0, None,
        ));
        assert_eq!(report.detected_language.as_deref(), Some("rust"));
        assert_eq!(report.server_profile.as_deref(), Some("rust-analyzer"));
        assert!(report.selected_root.is_some());
        assert!(report.inside_allowed_root);
    }

    #[test]
    fn doctor_report_with_cache_info() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        let file = tmp.path().join("main.rs");
        fs::write(&file, "").unwrap();

        let report = futures::executor::block_on(build_doctor_report(
            &file, None, None, "memory", 5, 2, 1, None,
        ));
        assert_eq!(report.cache_mode, "memory");
        assert_eq!(report.cache_entries, 5);
        assert_eq!(report.preview_count, 2);
        assert_eq!(report.preview_stale_count, 1);
    }

    #[test]
    fn render_doctor_report_contains_sections() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        let file = tmp.path().join("main.rs");
        fs::write(&file, "").unwrap();

        let report = futures::executor::block_on(build_doctor_report(
            &file, None, None, "disabled", 0, 0, 0, None,
        ));
        let rendered = render_doctor_report(&report);
        assert!(rendered.contains("LSP Doctor:"));
        assert!(rendered.contains("Inside allowed root:"));
        assert!(rendered.contains("Workspace root:"));
        assert!(rendered.contains("Language:"));
        assert!(rendered.contains("Server profile:"));
        assert!(rendered.contains("Active server:"));
        assert!(rendered.contains("Cache:"));
        assert!(rendered.contains("Previews:"));
        assert!(rendered.contains("Issues:"));
        assert!(rendered.contains("Remediation:"));
        // No observability section when snapshot is None.
        assert!(!rendered.contains("Observability:"));
    }

    #[test]
    fn render_doctor_report_with_observability() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        let file = tmp.path().join("main.rs");
        fs::write(&file, "").unwrap();

        let obs = LspObservabilitySnapshot {
            active_clients: 2,
            cache_mode: "memory".to_string(),
            cache_entries: 10,
            cache_bytes: 4096,
            cache_hits: 5,
            cache_misses: 2,
            cache_stale_misses: 1,
            cache_evictions: 0,
            preview_count: 3,
            preview_stale_count: 1,
            preview_applied_count: 1,
            ..Default::default()
        };
        let report = futures::executor::block_on(build_doctor_report(
            &file,
            None,
            None,
            "memory",
            10,
            3,
            1,
            Some(obs),
        ));
        let rendered = render_doctor_report(&report);
        assert!(rendered.contains("Observability:"));
        assert!(rendered.contains("Active clients: 2"));
        assert!(rendered.contains("Cache:"));
        assert!(rendered.contains("hits=5"));
        assert!(rendered.contains("Previews:"));
        assert!(rendered.contains("applied=1"));
    }
}
