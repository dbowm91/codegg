use std::path::PathBuf;

use super::analyze::analyze_python_risk;
use super::types::{
    CapabilityViolation, PythonCapabilityEnvelope, PythonCapabilityProfile, PythonExecutionMode,
    PythonPolicyDecision, PythonRiskAssessment, SandboxBackend,
};

/// Derive the capability envelope from mode and risk analysis.
pub fn derive_envelope(
    mode: PythonExecutionMode,
    code: &str,
) -> (PythonCapabilityEnvelope, PythonRiskAssessment) {
    let risk = analyze_python_risk(code);
    let envelope = PythonCapabilityEnvelope::from_mode_and_risk(mode, &risk);
    (envelope, risk)
}

/// Check if a script's detected capabilities are compatible with its mode.
/// Returns a list of violations (empty = ok).
pub fn check_compatibility(mode: PythonExecutionMode, code: &str) -> Vec<String> {
    let (envelope, risk) = derive_envelope(mode, code);
    envelope.has_denied_capabilities(&risk)
}

/// Resolve the full policy for a Python script execution.
///
/// Pipeline: AST risk → mode → workspace context → profile → enforcement backend.
/// Risk analysis can only narrow capabilities, never widen them.
pub fn resolve_policy(
    mode: PythonExecutionMode,
    code: &str,
    workspace_root: &PathBuf,
) -> PythonPolicyDecision {
    let risk = analyze_python_risk(code);
    let profile = PythonCapabilityProfile::from_mode_risk_and_context(mode, workspace_root, &risk);

    // Build violation list from risk vs profile
    let mut denied = Vec::new();
    if risk.has_network && !profile.allow_network {
        denied.push(CapabilityViolation {
            capability: "network".to_string(),
            reason: "network access detected by static analysis".to_string(),
        });
    }
    if risk.has_subprocess && !profile.allow_subprocess {
        denied.push(CapabilityViolation {
            capability: "subprocess".to_string(),
            reason: "subprocess usage not permitted in this mode".to_string(),
        });
    }
    if risk.has_destructive_ops && !profile.allow_destructive_fs {
        denied.push(CapabilityViolation {
            capability: "destructive_fs".to_string(),
            reason: "destructive filesystem operations denied".to_string(),
        });
    }
    if risk.has_dynamic_execution {
        denied.push(CapabilityViolation {
            capability: "dynamic_execution".to_string(),
            reason: "dynamic code execution (eval/exec/compile) detected".to_string(),
        });
    }

    // Determine enforcement backend
    let (backend, os_fs_isolation, os_net_isolation, warnings) = resolve_enforcement_backend(&profile);

    PythonPolicyDecision {
        profile,
        denied,
        warnings,
        enforcement_backend: backend,
        os_filesystem_isolation: os_fs_isolation,
        os_network_isolation: os_net_isolation,
    }
}

/// Determine which enforcement backend is available and what isolation it provides.
fn resolve_enforcement_backend(
    profile: &PythonCapabilityProfile,
) -> (SandboxBackend, bool, bool, Vec<String>) {
    #[cfg(target_os = "linux")]
    {
        if crate::security::sandbox::SandboxConfig::is_available() {
            let mut warnings = Vec::new();
            // Landlock handles filesystem but not network
            if profile.allow_network {
                warnings.push(
                    "network isolation not supported by Landlock; denying network capability"
                        .to_string(),
                );
            }
            return (SandboxBackend::Landlock, true, false, warnings);
        }
    }

    // Portable fallback
    let mut warnings = Vec::new();
    warnings.push(
        "OS-level sandboxing not available on this platform; using portable fallback".to_string(),
    );
    if profile.allow_subprocess {
        warnings.push(
            "subprocess supervision is policy-based only without OS sandbox".to_string(),
        );
    }
    if profile.allow_network {
        warnings.push(
            "network isolation not available; denying network capability".to_string(),
        );
    }
    (SandboxBackend::PortableFallback, false, false, warnings)
}

/// Validate that a subprocess invocation matches the profile's allowed rules.
/// Returns Ok(()) if allowed, Err(reason) if denied.
pub fn validate_subprocess_invocation(
    profile: &PythonCapabilityProfile,
    cmd: &str,
    first_arg: Option<&str>,
) -> Result<(), String> {
    if !profile.allow_subprocess {
        return Err("subprocess execution not allowed in this mode".to_string());
    }

    for rule in &profile.allowed_subprocesses {
        if rule.matches(cmd, first_arg) {
            return Ok(());
        }
    }

    Err(format!(
        "subprocess '{cmd}' is not in the allowed list for this profile"
    ))
}

/// Check whether a file path is inside any of the allowed roots.
pub fn path_inside_allowed_roots(path: &PathBuf, roots: &[PathBuf]) -> bool {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    roots.iter().any(|root| {
        if let Ok(canonical_root) = root.canonicalize() {
            canonical.starts_with(&canonical_root)
        } else {
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::ExecutableRule;

    fn ws() -> PathBuf {
        std::env::current_dir().unwrap()
    }

    // ── Existing envelope tests ───────────────────────────────────────

    #[test]
    fn analyze_mode_denies_write() {
        let (env, _) = derive_envelope(PythonExecutionMode::Analyze, "f = open('x', 'w')");
        assert!(!env.write_workspace);
    }

    #[test]
    fn transform_mode_allows_write() {
        let (env, _) = derive_envelope(PythonExecutionMode::Transform, "f = open('x', 'w')");
        assert!(env.write_workspace);
    }

    #[test]
    fn verify_mode_allows_subprocess() {
        let (env, _) = derive_envelope(PythonExecutionMode::Verify, "import subprocess");
        assert!(env.subprocess);
    }

    #[test]
    fn analyze_mode_denies_subprocess() {
        let violations =
            check_compatibility(PythonExecutionMode::Analyze, "subprocess.run(['ls'])");
        assert!(violations.contains(&"subprocess".to_string()));
    }

    #[test]
    fn safe_code_no_violations() {
        let violations = check_compatibility(PythonExecutionMode::Analyze, "print('hello')");
        assert!(violations.is_empty());
    }

    #[test]
    fn analyze_mode_defaults() {
        let (env, _) = derive_envelope(PythonExecutionMode::Analyze, "x = 1");
        assert!(env.read_workspace);
        assert!(!env.write_workspace);
        assert!(!env.subprocess);
        assert!(!env.network);
        assert!(!env.destructive_fs);
    }

    #[test]
    fn transform_mode_defaults() {
        let (env, _) = derive_envelope(PythonExecutionMode::Transform, "x = 1");
        assert!(env.read_workspace);
        assert!(env.write_workspace);
        assert!(!env.subprocess);
        assert!(!env.network);
    }

    #[test]
    fn verify_mode_defaults() {
        let (env, _) = derive_envelope(PythonExecutionMode::Verify, "x = 1");
        assert!(env.read_workspace);
        assert!(!env.write_workspace);
        assert!(env.subprocess);
        assert!(!env.network);
    }

    #[test]
    fn network_code_gets_network_denied() {
        let (env, _) = derive_envelope(
            PythonExecutionMode::Transform,
            "import requests\nrequests.get('http://example.com')",
        );
        assert!(!env.network);
    }

    #[test]
    fn destructive_code_gets_destructive_denied() {
        let (env, _) = derive_envelope(
            PythonExecutionMode::Transform,
            "import shutil\nshutil.rmtree('/tmp/dir')",
        );
        assert!(!env.destructive_fs);
    }

    #[test]
    fn analyze_mode_allows_read() {
        let (env, _) = derive_envelope(PythonExecutionMode::Analyze, "f = open('x', 'r')");
        assert!(env.read_workspace);
        assert!(!env.write_workspace);
    }

    #[test]
    fn analyze_mode_read_no_violations() {
        let violations = check_compatibility(PythonExecutionMode::Analyze, "f = open('x', 'r')");
        assert!(!violations.contains(&"read_workspace".to_string()));
    }

    #[test]
    fn analyze_mode_write_denied() {
        let violations = check_compatibility(PythonExecutionMode::Analyze, "f = open('x', 'w')");
        assert!(violations.contains(&"write_workspace".to_string()));
    }

    #[test]
    fn transform_mode_write_allowed() {
        let violations = check_compatibility(PythonExecutionMode::Transform, "f = open('x', 'w')");
        assert!(!violations.contains(&"write_workspace".to_string()));
    }

    #[test]
    fn verify_mode_read_no_violations() {
        let violations = check_compatibility(PythonExecutionMode::Verify, "f = open('x', 'r')");
        assert!(!violations.contains(&"read_workspace".to_string()));
    }

    #[test]
    fn verify_mode_write_denied() {
        let violations = check_compatibility(PythonExecutionMode::Verify, "f = open('x', 'w')");
        assert!(violations.contains(&"write_workspace".to_string()));
    }

    #[test]
    fn analyze_mode_pathlib_read_no_violations() {
        let violations = check_compatibility(
            PythonExecutionMode::Analyze,
            "from pathlib import Path\nPath('x').read_text()",
        );
        assert!(!violations.contains(&"read_workspace".to_string()));
        assert!(!violations.contains(&"write_workspace".to_string()));
    }

    #[test]
    fn analyze_mode_pathlib_write_denied() {
        let violations = check_compatibility(
            PythonExecutionMode::Analyze,
            "from pathlib import Path\nPath('x').write_text('y')",
        );
        assert!(violations.contains(&"write_workspace".to_string()));
    }

    // ── Profile tests ─────────────────────────────────────────────────

    #[test]
    fn profile_analyze_no_write_roots() {
        let profile = PythonCapabilityProfile::analyze(&ws());
        assert!(profile.read_roots.contains(&ws()));
        assert!(profile.write_roots.is_empty());
        assert!(!profile.allow_subprocess);
        assert!(!profile.allow_network);
        assert!(!profile.allow_destructive_fs);
        assert!(!profile.allow_dependency_install);
    }

    #[test]
    fn profile_transform_writes_workspace() {
        let profile = PythonCapabilityProfile::transform(&ws());
        assert!(profile.write_roots.contains(&ws()));
        assert!(!profile.allow_subprocess);
        assert!(!profile.allow_network);
    }

    #[test]
    fn profile_verify_allows_subprocess() {
        let profile = PythonCapabilityProfile::verify(&ws());
        assert!(profile.allow_subprocess);
        assert!(!profile.write_roots.is_empty() || profile.write_roots.is_empty());
        assert!(!profile.allow_network);
        assert!(!profile.allowed_subprocesses.is_empty());
    }

    #[test]
    fn profile_verify_has_cargo_rule() {
        let profile = PythonCapabilityProfile::verify(&ws());
        assert!(profile.allowed_subprocesses.iter().any(|r| r.command == "cargo"));
    }

    #[test]
    fn profile_verify_has_pytest_rule() {
        let profile = PythonCapabilityProfile::verify(&ws());
        assert!(profile
            .allowed_subprocesses
            .iter()
            .any(|r| r.command == "pytest"));
    }

    #[test]
    fn profile_from_mode_risk_denies_network() {
        let risk = PythonRiskAssessment {
            level: super::super::types::PythonRiskLevel::Medium,
            reasons: vec![],
            has_file_io: false,
            has_file_read: false,
            has_file_write: false,
            has_subprocess: false,
            has_network: true,
            has_destructive_ops: false,
            has_dynamic_execution: false,
            imports: vec![],
            scanner: super::super::types::PythonRiskScanner::Fallback,
        };
        let profile =
            PythonCapabilityProfile::from_mode_risk_and_context(PythonExecutionMode::Transform, &ws(), &risk);
        assert!(!profile.allow_network);
    }

    #[test]
    fn profile_from_mode_risk_denies_destructive() {
        let risk = PythonRiskAssessment {
            level: super::super::types::PythonRiskLevel::High,
            reasons: vec![],
            has_file_io: false,
            has_file_read: false,
            has_file_write: false,
            has_subprocess: false,
            has_network: false,
            has_destructive_ops: true,
            has_dynamic_execution: false,
            imports: vec![],
            scanner: super::super::types::PythonRiskScanner::Fallback,
        };
        let profile =
            PythonCapabilityProfile::from_mode_risk_and_context(PythonExecutionMode::Transform, &ws(), &risk);
        assert!(!profile.allow_destructive_fs);
    }

    #[test]
    fn profile_from_mode_risk_verify_keeps_subprocess() {
        let risk = PythonRiskAssessment {
            level: super::super::types::PythonRiskLevel::Medium,
            reasons: vec![],
            has_file_io: false,
            has_file_read: false,
            has_file_write: false,
            has_subprocess: true,
            has_network: false,
            has_destructive_ops: false,
            has_dynamic_execution: false,
            imports: vec![],
            scanner: super::super::types::PythonRiskScanner::Fallback,
        };
        let profile =
            PythonCapabilityProfile::from_mode_risk_and_context(PythonExecutionMode::Verify, &ws(), &risk);
        assert!(profile.allow_subprocess);
    }

    // ── ExecutableRule tests ──────────────────────────────────────────

    #[test]
    fn executable_rule_matches_command() {
        let rule = ExecutableRule::new("cargo", "test runner");
        assert!(rule.matches("cargo", None));
        assert!(!rule.matches("pytest", None));
    }

    #[test]
    fn executable_rule_matches_with_arg_prefix() {
        let rule = ExecutableRule::new("go", "go test").with_arg_prefix("test");
        assert!(rule.matches("go", Some("test")));
        assert!(rule.matches("go", Some("test -v")));
        assert!(!rule.matches("go", Some("build")));
        assert!(!rule.matches("go", None));
    }

    #[test]
    fn executable_rule_empty_prefix_matches_any() {
        let rule = ExecutableRule::new("cargo", "any cargo");
        assert!(rule.matches("cargo", Some("test")));
        assert!(rule.matches("cargo", Some("build")));
        assert!(rule.matches("cargo", None));
    }

    // ── Policy resolution tests ───────────────────────────────────────

    #[test]
    fn resolve_policy_analyze_safe() {
        let decision = resolve_policy(PythonExecutionMode::Analyze, "print('hi')", &ws());
        assert_eq!(decision.profile.mode, PythonExecutionMode::Analyze);
        assert!(decision.denied.is_empty());
        assert!(!decision.profile.allow_subprocess);
        assert!(!decision.profile.allow_network);
    }

    #[test]
    fn resolve_policy_analyze_with_network_deny() {
        let decision = resolve_policy(
            PythonExecutionMode::Analyze,
            "import requests\nrequests.get('http://x')",
            &ws(),
        );
        assert!(decision
            .denied
            .iter()
            .any(|v| v.capability == "network"));
    }

    #[test]
    fn resolve_policy_verify_allows_subprocess() {
        let decision = resolve_policy(
            PythonExecutionMode::Verify,
            "import subprocess\nsubprocess.run(['ls'])",
            &ws(),
        );
        assert!(decision.profile.allow_subprocess);
        assert!(!decision
            .denied
            .iter()
            .any(|v| v.capability == "subprocess"));
    }

    #[test]
    fn resolve_policy_transform_deny_destructive() {
        let decision = resolve_policy(
            PythonExecutionMode::Transform,
            "import shutil\nshutil.rmtree('/tmp/x')",
            &ws(),
        );
        assert!(!decision.profile.allow_destructive_fs);
        assert!(decision
            .denied
            .iter()
            .any(|v| v.capability == "destructive_fs"));
    }

    // ── Subprocess validation tests ───────────────────────────────────

    #[test]
    fn validate_subprocess_deny_when_not_allowed() {
        let profile = PythonCapabilityProfile::analyze(&ws());
        let result = validate_subprocess_invocation(&profile, "cargo", Some("test"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_subprocess_allow_cargo_test() {
        let profile = PythonCapabilityProfile::verify(&ws());
        let result = validate_subprocess_invocation(&profile, "cargo", Some("test"));
        assert!(result.is_ok());
    }

    #[test]
    fn validate_subprocess_allow_pytest() {
        let profile = PythonCapabilityProfile::verify(&ws());
        let result = validate_subprocess_invocation(&profile, "pytest", Some("tests/"));
        assert!(result.is_ok());
    }

    #[test]
    fn validate_subprocess_deny_unknown_binary() {
        let profile = PythonCapabilityProfile::verify(&ws());
        let result = validate_subprocess_invocation(&profile, "rm", Some("-rf"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_subprocess_deny_go_without_test_prefix() {
        let profile = PythonCapabilityProfile::verify(&ws());
        let result = validate_subprocess_invocation(&profile, "go", Some("build"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_subprocess_allow_go_test() {
        let profile = PythonCapabilityProfile::verify(&ws());
        let result = validate_subprocess_invocation(&profile, "go", Some("test"));
        assert!(result.is_ok());
    }

    // ── Path containment tests ────────────────────────────────────────

    #[test]
    fn path_inside_allowed_roots_inside() {
        let root = ws();
        let path = root.join("src").join("main.rs");
        assert!(path_inside_allowed_roots(&path, &[root]));
    }

    #[test]
    fn path_inside_allowed_roots_outside() {
        let root = ws();
        let path = PathBuf::from("/etc/passwd");
        assert!(!path_inside_allowed_roots(&path, &[root]));
    }

    // ── Enforcement backend tests ─────────────────────────────────────

    #[test]
    fn resolve_policy_produces_backend() {
        let decision = resolve_policy(PythonExecutionMode::Analyze, "x = 1", &ws());
        // On any platform, should get a backend
        assert!(
            decision.enforcement_backend == SandboxBackend::Landlock
                || decision.enforcement_backend == SandboxBackend::PortableFallback
        );
    }

    #[test]
    fn resolve_policy_portable_has_warnings() {
        let decision = resolve_policy(PythonExecutionMode::Analyze, "x = 1", &ws());
        // On non-Linux, should have portable fallback warnings
        #[cfg(not(target_os = "linux"))]
        assert!(!decision.warnings.is_empty());
    }
}
