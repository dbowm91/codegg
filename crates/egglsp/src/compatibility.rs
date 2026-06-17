//! Server-specific compatibility profiles.
//!
//! Real LSP servers differ in executable names, arguments, initialization
//! options, root markers, readiness behavior, and restart requirements.
//! [`LspCompatibilityProfile`] captures these differences as explicit data
//! rather than scattered conditionals in generic client code.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How a server signals readiness after `initialize` completes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LspReadinessPolicy {
    /// Server is ready immediately after `initialized` notification.
    InitializedIsReady,
    /// Wait for the first `publishDiagnostics` or until timeout.
    WaitForDiagnosticsOrTimeout { timeout: Duration },
    /// Wait for a `$window/workDoneProgress` end notification or timeout.
    WaitForProgressEndOrTimeout { timeout: Duration },
    /// Fixed warmup delay after initialization.
    WarmupDelay { duration: Duration },
}

/// Restart behavior for a specific server.
///
/// Restart is configured exclusively via the per-server
/// compatibility profile (see
/// [`LspCompatibilityProfile::restart_policy`]). There is no
/// separate `[lsp.<server>.restart]` config schema yet — this is
/// intentional to keep restart policy close to the readiness and
/// initialization quirks that motivate it. The restart coordinator
/// in `crate::restart` is the single source of truth for
/// applying this policy on unexpected exits and explicit
/// restart requests.
///
/// `max_attempts` is the cap on consecutive restart attempts
/// before the server is transitioned to `Failed`. The counter
/// resets lazily after the client has been healthy for
/// `reset_after_healthy`. `initial_backoff` and `max_backoff`
/// define the exponential backoff curve (capped) applied
/// between attempts; the formula in
/// `crate::restart::backoff_delay` is
/// `min(initial * 2^(attempt-1), max)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspRestartPolicy {
    pub mode: LspRestartMode,
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub reset_after_healthy: Duration,
}

/// Whether restart is enabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LspRestartMode {
    Disabled,
    OnUnexpectedExit,
}

impl Default for LspRestartPolicy {
    fn default() -> Self {
        Self {
            mode: LspRestartMode::Disabled,
            max_attempts: 3,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(8),
            reset_after_healthy: Duration::from_secs(60),
        }
    }
}

/// Explicit compatibility profile for a single LSP server.
///
/// Each Tier 1 (and later Tier 2) server gets one profile. Generic
/// client code reads profile fields instead of branching on server IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspCompatibilityProfile {
    /// Stable server identifier (e.g. "rust-analyzer", "basedpyright").
    pub server_id: String,
    /// Executable names to try on PATH, in preference order.
    pub executable_candidates: Vec<String>,
    /// Default arguments passed to the server on launch.
    pub default_args: Vec<String>,
    /// Files that indicate the project root for this server.
    pub root_markers: Vec<String>,
    /// Initial `initializationOptions` sent during `initialize`.
    pub initialization_options: serde_json::Value,
    /// Configuration sent via `workspace/configuration` if requested.
    pub workspace_configuration: serde_json::Value,
    /// How the server signals readiness after initialization.
    pub readiness_policy: LspReadinessPolicy,
    /// Restart behavior for this server.
    pub restart_policy: LspRestartPolicy,
    /// Known limitations to document in compatibility reports.
    pub known_limitations: Vec<String>,
}

/// Compatibility check status in a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompatibilityCheckStatus {
    Passing,
    PassingWithKnownLimits,
    Failing,
    Skipped,
    Unsupported,
}

/// How strictly a compatibility check must pass.
///
/// Used by the real-server harness to classify checks for the final
/// assertion. A test fails when a `Required` check is not `Passing` or
/// when a `RequiredIfAdvertised` check was advertised (server reports the
/// capability) and failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompatibilityRequirement {
    /// Check must pass unconditionally; failure fails the test.
    Required,
    /// Check must pass if the server advertised the corresponding capability.
    RequiredIfAdvertised,
    /// Check is informational; failures are recorded but do not fail the test.
    Optional,
    /// Check is known to be limited or unsupported; failure is expected.
    KnownLimitation,
}

/// A single compatibility check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspCompatibilityCheck {
    pub name: String,
    pub status: CompatibilityCheckStatus,
    pub requirement: CompatibilityRequirement,
    pub detail: Option<String>,
    pub duration_ms: Option<u64>,
}

/// Server version information captured during test runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerVersion {
    pub raw: String,
    pub parsed: Option<String>,
}

/// Full compatibility report for a server test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspCompatibilityReport {
    pub server_id: String,
    pub server_version: Option<String>,
    pub platform: String,
    pub initialize_ms: u64,
    pub readiness_ms: Option<u64>,
    pub capabilities: crate::capability::LspCapabilitySnapshot,
    pub checks: Vec<LspCompatibilityCheck>,
    pub stderr_tail: Vec<String>,
    pub known_limitations: Vec<String>,
}

// ── Tier 1 Profiles ─────────────────────────────────────────────────

/// Returns the compatibility profile for `rust-analyzer`.
pub fn rust_analyzer_profile() -> LspCompatibilityProfile {
    LspCompatibilityProfile {
        server_id: "rust-analyzer".to_string(),
        executable_candidates: vec!["rust-analyzer".to_string()],
        default_args: vec![],
        root_markers: vec![
            "Cargo.toml".to_string(),
            "rust-project.json".to_string(),
            ".git".to_string(),
        ],
        initialization_options: serde_json::json!({
            "diagnostics": {
                "enable": true,
                "enableExperimental": false,
            },
            "cargo": {
                "allFeatures": false,
                "allTargets": false,
            },
            "rustc": {
                "source": "discover",
            },
        }),
        workspace_configuration: serde_json::json!({
            "rust-analyzer": {
                "checkOnSave": { "enable": false },
            },
        }),
        readiness_policy: LspReadinessPolicy::WaitForProgressEndOrTimeout {
            timeout: Duration::from_secs(30),
        },
        restart_policy: LspRestartPolicy::default(),
        known_limitations: vec![
            "First semantic requests may be incomplete while indexing".to_string(),
            "Large projects may have slow initial diagnostics".to_string(),
        ],
    }
}

/// Returns the compatibility profile for `pyright` or `basedpyright`.
pub fn pyright_profile() -> LspCompatibilityProfile {
    LspCompatibilityProfile {
        server_id: "basedpyright".to_string(),
        executable_candidates: vec![
            "basedpyright-langserver".to_string(),
            "basedpyright".to_string(),
            "pyright-langserver".to_string(),
            "pyright".to_string(),
        ],
        default_args: vec!["--stdio".to_string()],
        root_markers: vec![
            "pyproject.toml".to_string(),
            "pyrightconfig.json".to_string(),
            "setup.py".to_string(),
            ".git".to_string(),
        ],
        initialization_options: serde_json::json!({}),
        workspace_configuration: serde_json::json!({
            "pyright": {
                "typeCheckingMode": "basic",
            },
        }),
        readiness_policy: LspReadinessPolicy::WaitForDiagnosticsOrTimeout {
            timeout: Duration::from_secs(15),
        },
        restart_policy: LspRestartPolicy::default(),
        known_limitations: vec![
            "Type checking depth may vary between pyright and basedpyright".to_string(),
        ],
    }
}

/// Lookup a profile by server ID.
pub fn profile_for_server(server_id: &str) -> Option<LspCompatibilityProfile> {
    match server_id {
        "rust-analyzer" => Some(rust_analyzer_profile()),
        "basedpyright" | "pyright" => Some(pyright_profile()),
        _ => None,
    }
}

/// All known Tier 1 profiles.
pub fn tier1_profiles() -> Vec<LspCompatibilityProfile> {
    vec![rust_analyzer_profile(), pyright_profile()]
}

/// Pass 6 — Evaluate a references-result compatibility check.
///
/// The standard rule for advertised references is:
/// - Zero locations → `RequiredIfAdvertised` failure
///   (no `references (0 found)` passing report).
/// - One or more locations → pass.
///
/// Profiles that need stricter rules (e.g. Python cross-file
/// references) can use [`evaluate_references_check_with_min`]
/// directly.
///
/// `advertised` must reflect the server's actual capability
/// (typically `LspCapabilitySnapshot::supports_references`).
/// When `advertised` is `false`, the check is recorded as
/// `Unsupported` regardless of the count, so the harness never
/// reports a passing result for a server that did not advertise
/// the operation.
pub fn evaluate_references_check(
    advertised: bool,
    locations: &[lsp_types::Location],
    min_required: usize,
) -> LspCompatibilityCheck {
    evaluate_references_check_with_min(advertised, locations, min_required, 1)
}

/// Pass 6 — Variant that requires a minimum count of distinct
/// URIs in the references result. Used by the Python cross-file
/// fixture which still requires at least two distinct URIs.
pub fn evaluate_references_check_with_min(
    advertised: bool,
    locations: &[lsp_types::Location],
    min_required: usize,
    min_distinct_uris: usize,
) -> LspCompatibilityCheck {
    let name = "references";
    if !advertised {
        return LspCompatibilityCheck {
            name: name.to_string(),
            status: CompatibilityCheckStatus::Unsupported,
            requirement: CompatibilityRequirement::RequiredIfAdvertised,
            detail: Some("references not advertised by server".to_string()),
            duration_ms: None,
        };
    }
    let count = locations.len();
    let mut distinct_uris: std::collections::HashSet<String> = std::collections::HashSet::new();
    for loc in locations {
        distinct_uris.insert(loc.uri.to_string());
    }
    let distinct = distinct_uris.len();
    if count < min_required || distinct < min_distinct_uris {
        return LspCompatibilityCheck {
            name: name.to_string(),
            status: CompatibilityCheckStatus::Failing,
            requirement: CompatibilityRequirement::RequiredIfAdvertised,
            detail: Some(format!(
                "expected at least {min_required} reference(s) across {min_distinct_uris} distinct URI(s); got {count} reference(s) across {distinct} distinct URI(s)"
            )),
            duration_ms: None,
        };
    }
    LspCompatibilityCheck {
        name: name.to_string(),
        status: CompatibilityCheckStatus::Passing,
        requirement: CompatibilityRequirement::RequiredIfAdvertised,
        detail: Some(format!(
            "{count} reference(s) across {distinct} distinct URI(s)"
        )),
        duration_ms: None,
    }
}

/// Resolve a server binary from environment variable or PATH candidates.
///
/// Returns `None` if no binary is found. Does not download or install.
pub fn require_server_binary(env_var: &str, candidates: &[&str]) -> Option<std::path::PathBuf> {
    // Check explicit environment override first.
    if let Ok(path) = std::env::var(env_var) {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    // Fallback to PATH lookup.
    for name in candidates {
        if let Ok(path) = which::which(name) {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn rust_analyzer_profile_has_root_markers() {
        let p = rust_analyzer_profile();
        assert!(p.root_markers.contains(&"Cargo.toml".to_string()));
        assert!(p.root_markers.contains(&".git".to_string()));
    }

    #[test]
    fn pyright_profile_has_executable_candidates() {
        let p = pyright_profile();
        assert!(!p.executable_candidates.is_empty());
        assert!(p
            .executable_candidates
            .contains(&"basedpyright-langserver".to_string()));
    }

    #[test]
    fn profile_lookup_returns_known_servers() {
        assert!(profile_for_server("rust-analyzer").is_some());
        assert!(profile_for_server("basedpyright").is_some());
        assert!(profile_for_server("pyright").is_some());
        assert!(profile_for_server("unknown-server").is_none());
    }

    #[test]
    fn restart_policy_default_is_disabled() {
        let p = rust_analyzer_profile();
        assert_eq!(p.restart_policy.mode, LspRestartMode::Disabled);
        assert_eq!(p.restart_policy.max_attempts, 3);
    }

    #[test]
    fn tier1_profiles_count() {
        let profiles = tier1_profiles();
        assert_eq!(profiles.len(), 2);
    }

    #[test]
    fn readiness_policies_differ() {
        let ra = rust_analyzer_profile();
        let py = pyright_profile();
        // rust-analyzer uses progress-based, pyright uses diagnostics-based
        assert_ne!(ra.readiness_policy, py.readiness_policy);
    }

    // ── Pass 6 references-check tests ──────────────────────────────

    fn loc(uri: &str) -> lsp_types::Location {
        lsp_types::Location {
            uri: lsp_types::Uri::from_str(uri).expect("valid uri"),
            range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 0,
                    character: 0,
                },
                end: lsp_types::Position {
                    line: 0,
                    character: 1,
                },
            },
        }
    }

    #[test]
    fn empty_references_fail_required_if_advertised() {
        let check = evaluate_references_check(true, &[], 1);
        assert_eq!(check.status, CompatibilityCheckStatus::Failing);
        assert_eq!(
            check.requirement,
            CompatibilityRequirement::RequiredIfAdvertised
        );
        assert!(check
            .detail
            .as_deref()
            .unwrap_or("")
            .contains("0 reference"));
    }

    #[test]
    fn single_rust_reference_passes() {
        let refs = vec![loc("file:///tmp/main.rs")];
        let check = evaluate_references_check(true, &refs, 1);
        assert_eq!(check.status, CompatibilityCheckStatus::Passing);
        assert!(check
            .detail
            .as_deref()
            .unwrap_or("")
            .contains("1 reference"));
    }

    #[test]
    fn python_cross_file_references_still_require_two_uris() {
        // Two refs but same URI — must fail (only 1 distinct URI).
        let refs = vec![loc("file:///tmp/a.py"), loc("file:///tmp/a.py")];
        let check = evaluate_references_check_with_min(true, &refs, 2, 2);
        assert_eq!(check.status, CompatibilityCheckStatus::Failing);
        // Two refs across two distinct URIs — must pass.
        let refs2 = vec![loc("file:///tmp/a.py"), loc("file:///tmp/b.py")];
        let check2 = evaluate_references_check_with_min(true, &refs2, 2, 2);
        assert_eq!(check2.status, CompatibilityCheckStatus::Passing);
    }

    #[test]
    fn unadvertised_references_are_unsupported() {
        let check = evaluate_references_check(false, &[], 1);
        assert_eq!(check.status, CompatibilityCheckStatus::Unsupported);
        // Even with refs present, unadvertised stays Unsupported.
        let check2 = evaluate_references_check(false, &[loc("file:///tmp/a.rs")], 1);
        assert_eq!(check2.status, CompatibilityCheckStatus::Unsupported);
    }
}
