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
    /// Profile-level capability overrides for fields the protocol does
    /// not advertise on the server side (notably type-hierarchy on
    /// `lsp-types` 0.97). Generic client code merges these into the
    /// snapshot derived from `ServerCapabilities` via
    /// [`crate::capability::LspCapabilitySnapshot::from_capabilities_with_override`].
    #[serde(default)]
    pub observed_capabilities: crate::capability::ObservedCapabilitiesOverride,
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

/// Per-operation compatibility detail distinguishing protocol
/// success, semantic success, skipped checks, and known
/// limitations. "Passing" means an exercised semantic assertion
/// passed — not merely that the server advertised the capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspOperationCompatibility {
    /// Operation name (e.g. "implementation", "typeHierarchy/prepare").
    pub operation: String,
    /// True when the server advertised this capability.
    pub advertised: bool,
    /// True when the test actually sent a request for this operation.
    pub exercised: bool,
    /// True when the LSP request succeeded (no protocol error).
    pub request_succeeded: bool,
    /// True when the semantic assertion (e.g. expected file, label
    /// substring, minimum count) passed.
    pub semantic_assertion_passed: bool,
    /// How strictly this check must pass.
    pub requirement: CompatibilityRequirement,
    /// Optional reason when the check is a known limitation.
    pub known_limit: Option<String>,
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
    /// Pass 6 — per-operation compatibility records. Each entry
    /// distinguishes protocol success, semantic success, and known
    /// limitations on a per-operation basis (not by parsing check
    /// names). `serde(default)` keeps backward compatibility with
    /// older report JSON that lacks this field.
    #[serde(default)]
    pub operation_support: Vec<LspOperationCompatibility>,
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
        readiness_policy: LspReadinessPolicy::WaitForDiagnosticsOrTimeout {
            timeout: Duration::from_secs(30),
        },
        restart_policy: LspRestartPolicy::default(),
        known_limitations: vec![
            "First semantic requests may be incomplete while indexing".to_string(),
            "Large projects may have slow initial diagnostics".to_string(),
        ],
        // Phase 4: rust-analyzer supports type hierarchy, but
        // `lsp-types` 0.97 does not expose a server-side
        // `type_hierarchy_provider` field. The override flips the
        // normalized flag on so `semanticContext` / `typeHierarchy`
        // callers see the correct capability.
        observed_capabilities: crate::capability::ObservedCapabilitiesOverride {
            type_hierarchy: Some(true),
            type_hierarchy_tested_version: Some("2024-11-25".to_string()),
        },
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
        observed_capabilities: crate::capability::ObservedCapabilitiesOverride::default(),
    }
}

// ── Tier 2 Profiles ─────────────────────────────────────────────────

/// Returns the compatibility profile for `gopls` (the official Go
/// language server).
///
/// gopls supports type hierarchy, but `lsp-types` 0.97 only models
/// type hierarchy as a CLIENT capability, so the server-side
/// advertised state is not visible in `ServerCapabilities`. The
/// profile's [`observed_capabilities`] field flips
/// `supports_type_hierarchy` on so generic client code can route
/// `typeHierarchy` requests to gopls.
pub fn gopls_profile() -> LspCompatibilityProfile {
    LspCompatibilityProfile {
        server_id: "gopls".to_string(),
        executable_candidates: vec!["gopls".to_string()],
        default_args: vec![],
        root_markers: vec![
            "go.work".to_string(),
            "go.mod".to_string(),
            ".git".to_string(),
        ],
        initialization_options: serde_json::json!({
            "usePlaceholders": true,
            "completeUnimported": true,
            "semanticTokens": true,
        }),
        workspace_configuration: serde_json::json!({
            "gopls": {
                "analyses": {
                    "unusedparams": true,
                },
            },
        }),
        // gopls emits diagnostics eagerly once a package is loaded.
        // Diagnostics-based readiness is the most reliable signal
        // for the production code path.
        readiness_policy: LspReadinessPolicy::WaitForDiagnosticsOrTimeout {
            timeout: Duration::from_secs(15),
        },
        restart_policy: LspRestartPolicy::default(),
        known_limitations: vec![
            "gopls requires a go.mod (or go.work) in the workspace root".to_string(),
            "Workspace symbols need go.work for multi-module workspaces".to_string(),
        ],
        observed_capabilities: crate::capability::ObservedCapabilitiesOverride {
            type_hierarchy: Some(true),
            type_hierarchy_tested_version: Some("v0.16.1".to_string()),
        },
    }
}

/// Returns the compatibility profile for `typescript-language-server`.
///
/// `typescript-language-server` reports `$/progress` notifications
/// while loading the project, so progress-end is the most reliable
/// readiness signal.
pub fn typescript_language_server_profile() -> LspCompatibilityProfile {
    LspCompatibilityProfile {
        server_id: "typescript-language-server".to_string(),
        executable_candidates: vec!["typescript-language-server".to_string()],
        default_args: vec!["--stdio".to_string()],
        root_markers: vec![
            "tsconfig.json".to_string(),
            "jsconfig.json".to_string(),
            "package.json".to_string(),
            ".git".to_string(),
        ],
        initialization_options: serde_json::json!({}),
        workspace_configuration: serde_json::json!({
            "typescript": {
                "preferences": {
                    "includeCompletionsForModuleExports": true,
                },
            },
        }),
        // typescript-language-server sends diagnostics once the project
        // is loaded. Use diagnostics as the readiness signal with a
        // generous timeout for large projects.
        readiness_policy: LspReadinessPolicy::WaitForDiagnosticsOrTimeout {
            timeout: Duration::from_secs(30),
        },
        restart_policy: LspRestartPolicy::default(),
        known_limitations: vec![
            "Requires node_modules installed locally (CI installs pinned versions)".to_string(),
            "Single-language server (handles TS/JS but no JSX/TSX-specific quirks)".to_string(),
        ],
        observed_capabilities: crate::capability::ObservedCapabilitiesOverride::default(),
    }
}

/// Returns the compatibility profile for `clangd`.
///
/// clangd does not reliably emit progress notifications and may not
/// push diagnostics immediately on small projects, so a fixed
/// warmup delay is the most deterministic readiness signal for
/// tests. The `--background-index=false` and `--clang-tidy=0`
/// arguments keep test runs deterministic.
pub fn clangd_profile() -> LspCompatibilityProfile {
    LspCompatibilityProfile {
        server_id: "clangd".to_string(),
        executable_candidates: vec!["clangd".to_string()],
        // --background-index=false keeps the index off the cold
        // path so test fixtures do not race background indexing.
        // --clang-tidy=0 disables clang-tidy diagnostics, which
        // would otherwise require clang-tidy configuration in
        // every fixture.
        default_args: vec![
            "--background-index=false".to_string(),
            "--clang-tidy=0".to_string(),
        ],
        root_markers: vec![
            "compile_commands.json".to_string(),
            "compile_flags.txt".to_string(),
            "CMakeLists.txt".to_string(),
            ".git".to_string(),
        ],
        initialization_options: serde_json::json!({
            "compilationDatabasePath": "compile_commands.json",
        }),
        workspace_configuration: serde_json::json!({}),
        // clangd does not reliably emit progress; a short warmup
        // delay is the most deterministic readiness signal for
        // test runs.
        readiness_policy: LspReadinessPolicy::WarmupDelay {
            duration: Duration::from_secs(2),
        },
        restart_policy: LspRestartPolicy::default(),
        known_limitations: vec![
            "Requires compile_commands.json or compile_flags.txt in workspace root".to_string(),
            "Background indexing disabled for test determinism".to_string(),
            "textDocument/prepareTypeHierarchy not supported".to_string(),
        ],
        observed_capabilities: crate::capability::ObservedCapabilitiesOverride::default(),
    }
}

/// Lookup a profile by server ID.
pub fn profile_for_server(server_id: &str) -> Option<LspCompatibilityProfile> {
    match server_id {
        "rust-analyzer" => Some(rust_analyzer_profile()),
        "basedpyright" | "pyright" => Some(pyright_profile()),
        "gopls" => Some(gopls_profile()),
        "typescript-language-server" => Some(typescript_language_server_profile()),
        "clangd" => Some(clangd_profile()),
        _ => None,
    }
}

/// All known Tier 1 profiles.
pub fn tier1_profiles() -> Vec<LspCompatibilityProfile> {
    vec![rust_analyzer_profile(), pyright_profile()]
}

/// All known Tier 2 profiles.
pub fn tier2_profiles() -> Vec<LspCompatibilityProfile> {
    vec![
        gopls_profile(),
        typescript_language_server_profile(),
        clangd_profile(),
    ]
}

/// Every profile registered in this module, across tiers. The order
/// matches the per-tier ordering so callers iterating this list
/// observe a stable, deterministic sequence.
pub fn all_profiles() -> Vec<LspCompatibilityProfile> {
    let mut all = Vec::with_capacity(tier1_profiles().len() + tier2_profiles().len());
    all.extend(tier1_profiles());
    all.extend(tier2_profiles());
    all
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

    // ── Tier 2 profile tests ────────────────────────────────────────

    #[test]
    fn gopls_profile_shape() {
        let p = gopls_profile();
        assert_eq!(p.server_id, "gopls");
        assert_eq!(p.executable_candidates, vec!["gopls".to_string()]);
        assert!(p.default_args.is_empty());
        assert!(p.root_markers.contains(&"go.mod".to_string()));
        assert!(p.root_markers.contains(&"go.work".to_string()));
        assert!(p.root_markers.contains(&".git".to_string()));
        assert_eq!(
            p.readiness_policy,
            LspReadinessPolicy::WaitForDiagnosticsOrTimeout {
                timeout: Duration::from_secs(15)
            }
        );
        assert_eq!(p.restart_policy, LspRestartPolicy::default());
        // Type hierarchy is observed on gopls.
        assert_eq!(p.observed_capabilities.type_hierarchy, Some(true));
        assert!(!p.known_limitations.is_empty());
    }

    #[test]
    fn typescript_language_server_profile_shape() {
        let p = typescript_language_server_profile();
        assert_eq!(p.server_id, "typescript-language-server");
        assert_eq!(
            p.executable_candidates,
            vec!["typescript-language-server".to_string()]
        );
        assert_eq!(p.default_args, vec!["--stdio".to_string()]);
        assert!(p.root_markers.contains(&"tsconfig.json".to_string()));
        assert!(p.root_markers.contains(&"jsconfig.json".to_string()));
        assert!(p.root_markers.contains(&"package.json".to_string()));
        assert!(p.root_markers.contains(&".git".to_string()));
        assert_eq!(
            p.readiness_policy,
            LspReadinessPolicy::WaitForDiagnosticsOrTimeout {
                timeout: Duration::from_secs(30)
            }
        );
        assert_eq!(p.restart_policy, LspRestartPolicy::default());
        // No observed overrides for typescript-language-server.
        assert_eq!(p.observed_capabilities.type_hierarchy, None);
        assert!(!p.known_limitations.is_empty());
    }

    #[test]
    fn clangd_profile_shape() {
        let p = clangd_profile();
        assert_eq!(p.server_id, "clangd");
        assert_eq!(p.executable_candidates, vec!["clangd".to_string()]);
        assert!(p
            .default_args
            .contains(&"--background-index=false".to_string()));
        assert!(p.default_args.contains(&"--clang-tidy=0".to_string()));
        assert!(p
            .root_markers
            .contains(&"compile_commands.json".to_string()));
        assert!(p.root_markers.contains(&"compile_flags.txt".to_string()));
        assert!(p.root_markers.contains(&"CMakeLists.txt".to_string()));
        assert!(p.root_markers.contains(&".git".to_string()));
        assert_eq!(
            p.readiness_policy,
            LspReadinessPolicy::WarmupDelay {
                duration: Duration::from_secs(2)
            }
        );
        assert_eq!(p.restart_policy, LspRestartPolicy::default());
        // clangd does not support type hierarchy.
        assert_eq!(p.observed_capabilities.type_hierarchy, None);
        assert!(!p.known_limitations.is_empty());
    }

    #[test]
    fn gopls_profile_has_correct_root_markers() {
        // Focused root-marker check separate from the broader
        // `gopls_profile_shape` test so a regression in the root
        // marker list is reported independently from readiness or
        // observed-capability regressions.
        let p = gopls_profile();
        assert!(
            p.root_markers.contains(&"go.mod".to_string()),
            "gopls root_markers missing go.mod: {:?}",
            p.root_markers
        );
        assert!(
            p.root_markers.contains(&"go.work".to_string()),
            "gopls root_markers missing go.work: {:?}",
            p.root_markers
        );
        assert!(
            p.root_markers.contains(&".git".to_string()),
            "gopls root_markers missing .git fallback: {:?}",
            p.root_markers
        );
        // gopls should NOT advertise TypeScript- or Python-specific
        // root markers.
        assert!(!p.root_markers.iter().any(|m| m == "tsconfig.json"));
        assert!(!p.root_markers.iter().any(|m| m == "pyproject.toml"));
    }

    #[test]
    fn typescript_profile_supports_stdio() {
        // Focused assertion that typescript-language-server uses
        // the `--stdio` transport argument. A regression to the
        // transport flag will be reported here independently of
        // readiness/root-marker regressions.
        let p = typescript_language_server_profile();
        assert!(
            p.default_args.contains(&"--stdio".to_string()),
            "typescript-language-server default_args missing --stdio: {:?}",
            p.default_args
        );
        // Root markers must include at least one TypeScript-specific
        // and one Node-specific marker so the project root can be
        // resolved for either layout.
        assert!(p.root_markers.contains(&"tsconfig.json".to_string()));
        assert!(p.root_markers.contains(&"package.json".to_string()));
    }

    #[test]
    fn clangd_profile_uses_warmup_delay() {
        // Focused assertion that clangd uses a warmup-delay
        // readiness policy. clangd does not reliably emit progress
        // notifications on small fixtures, so a fixed warmup delay
        // is the deterministic readiness signal. A regression here
        // will surface as a CI flake on real-server runs.
        let p = clangd_profile();
        match &p.readiness_policy {
            LspReadinessPolicy::WarmupDelay { duration } => {
                assert!(
                    !duration.is_zero(),
                    "clangd warmup delay must be > 0 to allow background indexing settle"
                );
                assert!(
                    *duration <= Duration::from_secs(10),
                    "clangd warmup delay should stay bounded for CI; got {:?}",
                    duration
                );
            }
            other => panic!(
                "expected clangd to use WarmupDelay readiness; got {:?}",
                other
            ),
        }
        // clangd-specific root markers must be present.
        assert!(p
            .root_markers
            .contains(&"compile_commands.json".to_string()));
        // clangd must disable clang-tidy by default to avoid
        // requiring clang-tidy configuration in every fixture.
        assert!(
            p.default_args.contains(&"--clang-tidy=0".to_string()),
            "clangd default_args missing --clang-tidy=0: {:?}",
            p.default_args
        );
    }

    #[test]
    fn profile_lookup_returns_tier2_servers() {
        assert!(profile_for_server("gopls").is_some());
        assert!(profile_for_server("typescript-language-server").is_some());
        assert!(profile_for_server("clangd").is_some());
        // Server IDs not in the catalog still return None.
        assert!(profile_for_server("solargraph").is_none());
    }

    #[test]
    fn tier2_profiles_count() {
        let profiles = tier2_profiles();
        assert_eq!(profiles.len(), 3);
    }

    #[test]
    fn all_profiles_includes_tier1_and_tier2() {
        let all = all_profiles();
        assert_eq!(all.len(), tier1_profiles().len() + tier2_profiles().len());
        let ids: std::collections::HashSet<String> =
            all.iter().map(|p| p.server_id.clone()).collect();
        assert!(ids.contains("rust-analyzer"));
        assert!(ids.contains("basedpyright"));
        assert!(ids.contains("gopls"));
        assert!(ids.contains("typescript-language-server"));
        assert!(ids.contains("clangd"));
    }

    #[test]
    fn all_profile_server_ids_are_unique() {
        let all = all_profiles();
        let mut ids: Vec<String> = all.iter().map(|p| p.server_id.clone()).collect();
        ids.sort();
        let original_len = ids.len();
        ids.dedup();
        assert_eq!(
            ids.len(),
            original_len,
            "duplicate server_id across profiles: {all:?}"
        );
    }

    #[test]
    fn observed_capabilities_default_for_tier1() {
        // pyright has no override; rust-analyzer advertises type
        // hierarchy via the override (lsp-types 0.97 has no
        // server-side field for it).
        let ra = rust_analyzer_profile();
        assert_eq!(
            ra.observed_capabilities.type_hierarchy,
            Some(true),
            "rust-analyzer profile must declare type hierarchy via the override"
        );
        let py = pyright_profile();
        assert_eq!(py.observed_capabilities.type_hierarchy, None);
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
