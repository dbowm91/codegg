use super::analyze::analyze_python_risk;
use super::types::{PythonCapabilityEnvelope, PythonExecutionMode, PythonRiskAssessment};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
