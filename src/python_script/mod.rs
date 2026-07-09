pub mod analyze;
pub mod executor;
pub mod projection;
pub mod sandbox;
pub mod snapshot;
pub mod tool;
pub mod types;

// Re-export key types for convenience
pub use analyze::analyze_python_risk;
pub use executor::execute_python_script;
pub use projection::project_python_run;
pub use sandbox::{check_compatibility, derive_envelope};
pub use tool::PythonScriptTool;
pub use types::{
    PythonCapabilityEnvelope, PythonExecutionMode, PythonRiskAssessment, PythonRiskLevel,
    PythonRiskScanner, PythonRunResult, PythonRunStatus, PythonScriptRequest, PythonScriptSource,
};

#[cfg(test)]
mod tests {
    use super::snapshot::WorkspaceSnapshot;
    use super::*;
    use std::time::Duration;

    // ── Type construction & serde roundtrip ──────────────────────────────

    #[test]
    fn mode_serde_roundtrip() {
        for mode in [
            PythonExecutionMode::Analyze,
            PythonExecutionMode::Transform,
            PythonExecutionMode::Verify,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: PythonExecutionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn mode_label_and_display() {
        assert_eq!(PythonExecutionMode::Analyze.label(), "analyze");
        assert_eq!(PythonExecutionMode::Transform.to_string(), "transform");
        assert_eq!(
            PythonExecutionMode::Verify.description(),
            "Test/verification script with controlled subprocess"
        );
    }

    #[test]
    fn risk_level_serde_roundtrip() {
        for level in [
            PythonRiskLevel::Safe,
            PythonRiskLevel::Low,
            PythonRiskLevel::Medium,
            PythonRiskLevel::High,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let back: PythonRiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, back);
        }
    }

    #[test]
    fn risk_assessment_serde_roundtrip() {
        let risk = PythonRiskAssessment {
            level: PythonRiskLevel::Medium,
            reasons: vec!["network access detected".into()],
            has_file_io: false,
            has_file_read: false,
            has_file_write: false,
            has_subprocess: false,
            has_network: true,
            has_destructive_ops: false,
            has_dynamic_execution: false,
            imports: vec!["requests".into()],
            scanner: PythonRiskScanner::Fallback,
        };
        let json = serde_json::to_string(&risk).unwrap();
        let back: PythonRiskAssessment = serde_json::from_str(&json).unwrap();
        assert_eq!(risk, back);
    }

    #[test]
    fn capability_envelope_serde_roundtrip() {
        let env = PythonCapabilityEnvelope::verify();
        let json = serde_json::to_string(&env).unwrap();
        let back: PythonCapabilityEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(env, back);
    }

    #[test]
    fn script_request_serde_roundtrip() {
        let req = PythonScriptRequest {
            code: "print(1)".into(),
            mode: PythonExecutionMode::Analyze,
            cwd: std::env::temp_dir(),
            timeout_secs: Some(30),
            session_id: Some("s1".into()),
            intent: Some("test".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: PythonScriptRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req.code, back.code);
        assert_eq!(req.mode, back.mode);
        assert_eq!(req.timeout_secs, back.timeout_secs);
    }

    #[test]
    fn script_source_code_accessor() {
        let inline = PythonScriptSource::Inline("x = 1".into());
        assert_eq!(inline.code(), "x = 1");
        let file = PythonScriptSource::FilePath(std::path::PathBuf::from("x.py"));
        assert_eq!(file.code(), "");
    }

    // ── RunStatus edge cases ────────────────────────────────────────────

    #[test]
    fn run_status_exit_codes() {
        assert_eq!(PythonRunStatus::Success.exit_code(), Some(0));
        assert_eq!(PythonRunStatus::Failed(42).exit_code(), Some(42));
        assert_eq!(PythonRunStatus::TimedOut.exit_code(), None);
        assert_eq!(PythonRunStatus::SpawnError.exit_code(), None);
    }

    #[test]
    fn run_status_labels() {
        assert_eq!(PythonRunStatus::Success.label(), "success");
        assert_eq!(PythonRunStatus::Failed(1).label(), "failed");
        assert_eq!(PythonRunStatus::TimedOut.label(), "timed_out");
        assert_eq!(PythonRunStatus::SpawnError.label(), "spawn_error");
    }

    #[test]
    fn run_status_is_success() {
        assert!(PythonRunStatus::Success.is_success());
        assert!(!PythonRunStatus::Failed(1).is_success());
        assert!(!PythonRunStatus::TimedOut.is_success());
        assert!(!PythonRunStatus::SpawnError.is_success());
    }

    // ── Risk assessment construction ────────────────────────────────────

    #[test]
    fn risk_assessment_safe_constructor() {
        let risk = PythonRiskAssessment::safe();
        assert_eq!(risk.level, PythonRiskLevel::Safe);
        assert!(risk.reasons.is_empty());
        assert!(!risk.has_file_io);
        assert!(!risk.has_subprocess);
        assert!(!risk.has_network);
        assert!(!risk.has_destructive_ops);
        assert!(!risk.has_dynamic_execution);
        assert!(risk.imports.is_empty());
    }

    #[test]
    fn requires_permission_for_medium_and_high() {
        let mut risk = PythonRiskAssessment::safe();
        risk.level = PythonRiskLevel::Safe;
        assert!(!risk.requires_permission());
        risk.level = PythonRiskLevel::Low;
        assert!(!risk.requires_permission());
        risk.level = PythonRiskLevel::Medium;
        assert!(risk.requires_permission());
        risk.level = PythonRiskLevel::High;
        assert!(risk.requires_permission());
    }

    // ── Capability envelope defaults & edge cases ───────────────────────

    #[test]
    fn envelope_analyze_default() {
        let env = PythonCapabilityEnvelope::analyze();
        assert!(env.read_workspace);
        assert!(!env.write_workspace);
        assert!(!env.read_outside_workspace);
        assert!(!env.write_outside_workspace);
        assert!(!env.subprocess);
        assert!(!env.network);
        assert!(!env.env_access);
        assert!(!env.dependency_install);
        assert!(!env.destructive_fs);
    }

    #[test]
    fn envelope_transform_default() {
        let env = PythonCapabilityEnvelope::transform();
        assert!(env.read_workspace);
        assert!(env.write_workspace);
        assert!(!env.subprocess);
        assert!(!env.network);
    }

    #[test]
    fn envelope_verify_default() {
        let env = PythonCapabilityEnvelope::verify();
        assert!(env.read_workspace);
        assert!(!env.write_workspace);
        assert!(env.subprocess);
        assert!(!env.network);
    }

    #[test]
    fn envelope_from_mode_and_risk_denies_network() {
        let risk = PythonRiskAssessment {
            level: PythonRiskLevel::Medium,
            reasons: vec![],
            has_file_io: false,
            has_file_read: false,
            has_file_write: false,
            has_subprocess: false,
            has_network: true,
            has_destructive_ops: false,
            has_dynamic_execution: false,
            imports: vec![],
            scanner: PythonRiskScanner::Fallback,
        };
        let env =
            PythonCapabilityEnvelope::from_mode_and_risk(PythonExecutionMode::Transform, &risk);
        assert!(!env.network);
    }

    #[test]
    fn envelope_from_mode_and_risk_denies_destructive() {
        let risk = PythonRiskAssessment {
            level: PythonRiskLevel::High,
            reasons: vec![],
            has_file_io: false,
            has_file_read: false,
            has_file_write: false,
            has_subprocess: false,
            has_network: false,
            has_destructive_ops: true,
            has_dynamic_execution: false,
            imports: vec![],
            scanner: PythonRiskScanner::Fallback,
        };
        let env =
            PythonCapabilityEnvelope::from_mode_and_risk(PythonExecutionMode::Transform, &risk);
        assert!(!env.destructive_fs);
    }

    #[test]
    fn envelope_verify_keeps_subprocess_when_risk_detected() {
        let risk = PythonRiskAssessment {
            level: PythonRiskLevel::Medium,
            reasons: vec![],
            has_file_io: false,
            has_file_read: false,
            has_file_write: false,
            has_subprocess: true,
            has_network: false,
            has_destructive_ops: false,
            has_dynamic_execution: false,
            imports: vec![],
            scanner: PythonRiskScanner::Fallback,
        };
        // Verify mode allows subprocess even when risk detected
        let env = PythonCapabilityEnvelope::from_mode_and_risk(PythonExecutionMode::Verify, &risk);
        assert!(env.subprocess);
    }

    #[test]
    fn envelope_analyze_denies_subprocess_when_risk_detected() {
        let risk = PythonRiskAssessment {
            level: PythonRiskLevel::Medium,
            reasons: vec![],
            has_file_io: false,
            has_file_read: false,
            has_file_write: false,
            has_subprocess: true,
            has_network: false,
            has_destructive_ops: false,
            has_dynamic_execution: false,
            imports: vec![],
            scanner: PythonRiskScanner::Fallback,
        };
        let env = PythonCapabilityEnvelope::from_mode_and_risk(PythonExecutionMode::Analyze, &risk);
        assert!(!env.subprocess);
    }

    #[test]
    fn has_denied_capabilities_empty_for_clean() {
        let risk = PythonRiskAssessment::safe();
        let env = PythonCapabilityEnvelope::analyze();
        assert!(env.has_denied_capabilities(&risk).is_empty());
    }

    #[test]
    fn has_denied_capabilities_reports_network() {
        let risk = PythonRiskAssessment {
            level: PythonRiskLevel::Medium,
            reasons: vec![],
            has_file_io: false,
            has_file_read: false,
            has_file_write: false,
            has_subprocess: false,
            has_network: true,
            has_destructive_ops: false,
            has_dynamic_execution: false,
            imports: vec![],
            scanner: PythonRiskScanner::Fallback,
        };
        let env = PythonCapabilityEnvelope::analyze();
        let denied = env.has_denied_capabilities(&risk);
        assert!(denied.contains(&"network".to_string()));
    }

    #[test]
    fn has_denied_capabilities_reports_write() {
        let risk = PythonRiskAssessment {
            level: PythonRiskLevel::Low,
            reasons: vec![],
            has_file_io: true,
            has_file_read: false,
            has_file_write: true,
            has_subprocess: false,
            has_network: false,
            has_destructive_ops: false,
            has_dynamic_execution: false,
            imports: vec![],
            scanner: PythonRiskScanner::Fallback,
        };
        let env = PythonCapabilityEnvelope::analyze();
        let denied = env.has_denied_capabilities(&risk);
        assert!(denied.contains(&"write_workspace".to_string()));
    }

    // ── Analyze risk: edge cases beyond existing tests ──────────────────

    #[test]
    fn subprocess_pip_install_returns_medium() {
        let result = analyze_python_risk("subprocess.run(['pip', 'install', 'foo'])");
        assert_eq!(result.level, PythonRiskLevel::Medium);
    }

    #[test]
    fn conda_install_detected() {
        let result = analyze_python_risk("import os\nos.system('conda install numpy')");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result
            .reasons
            .iter()
            .any(|r| r.contains("dependency installation")));
    }

    #[test]
    fn os_popen_returns_medium() {
        let result = analyze_python_risk("os.popen('ls')");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_subprocess);
    }

    #[test]
    fn urllib_returns_medium() {
        let result =
            analyze_python_risk("import urllib.request\nurllib.request.urlopen('http://x')");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_network);
    }

    #[test]
    fn socket_returns_medium() {
        let result = analyze_python_risk("import socket\nsocket.socket()");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_network);
    }

    #[test]
    fn os_rmdir_returns_high() {
        let result = analyze_python_risk("os.rmdir('/tmp/dir')");
        assert_eq!(result.level, PythonRiskLevel::High);
    }

    #[test]
    fn os_unlink_returns_high() {
        let result = analyze_python_risk("os.unlink('file.txt')");
        assert_eq!(result.level, PythonRiskLevel::High);
    }

    #[test]
    fn pathlib_unlink_detected_by_ast_scanner() {
        let result = analyze_python_risk("from pathlib import Path\nPath('x.txt').unlink()");
        assert_eq!(result.level, PythonRiskLevel::High);
        assert!(result.has_destructive_ops);
        assert_eq!(result.scanner, PythonRiskScanner::Ast);
    }

    #[test]
    fn exec_returns_low() {
        let result = analyze_python_risk("exec('x = 1')");
        assert!(result.has_dynamic_execution);
        assert_eq!(result.level, PythonRiskLevel::Low);
    }

    #[test]
    fn compile_returns_low() {
        let result = analyze_python_risk("compile('1+1', '<string>', 'eval')");
        assert!(result.has_dynamic_execution);
        assert_eq!(result.level, PythonRiskLevel::Low);
    }

    #[test]
    fn __import___returns_low() {
        let result = analyze_python_risk("__import__('os')");
        assert!(result.has_dynamic_execution);
        assert_eq!(result.level, PythonRiskLevel::Low);
    }

    #[test]
    fn mixed_subprocess_and_file_io_stays_medium() {
        let result = analyze_python_risk("subprocess.run(['ls'])\nf = open('x', 'w')");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_subprocess);
        assert!(result.has_file_io);
    }

    #[test]
    fn import_comma_separated_takes_first() {
        let result = analyze_python_risk("import os, sys, json");
        assert!(result.imports.contains(&"os".to_string()));
    }

    #[test]
    fn import_from_statement() {
        let result = analyze_python_risk("from os import path");
        assert!(result.imports.contains(&"os".to_string()));
    }

    #[test]
    fn empty_code_is_safe() {
        let result = analyze_python_risk("");
        assert_eq!(result.level, PythonRiskLevel::Safe);
        assert!(result.reasons.is_empty());
    }

    #[test]
    fn code_with_only_comments_is_safe() {
        let result = analyze_python_risk("# this is a comment\n# another comment");
        assert_eq!(result.level, PythonRiskLevel::Safe);
    }

    // ── Sandbox: edge cases ─────────────────────────────────────────────

    #[test]
    fn sandbox_analyze_with_network_code() {
        let violations = check_compatibility(
            PythonExecutionMode::Analyze,
            "import requests\nrequests.get('http://x')",
        );
        assert!(violations.contains(&"network".to_string()));
    }

    #[test]
    fn sandbox_transform_with_destructive_code() {
        let violations = check_compatibility(
            PythonExecutionMode::Transform,
            "import shutil\nshutil.rmtree('/tmp/x')",
        );
        assert!(violations.contains(&"destructive_fs".to_string()));
    }

    #[test]
    fn sandbox_verify_with_subprocess_no_violation() {
        let violations = check_compatibility(
            PythonExecutionMode::Verify,
            "import subprocess\nsubprocess.run(['ls'])",
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn sandbox_analyze_clean_code() {
        let violations = check_compatibility(PythonExecutionMode::Analyze, "x = 1 + 2");
        assert!(violations.is_empty());
    }

    #[test]
    fn derive_envelope_returns_both_parts() {
        let (env, risk) = derive_envelope(PythonExecutionMode::Analyze, "print('hi')");
        assert_eq!(risk.level, PythonRiskLevel::Safe);
        assert!(env.read_workspace);
    }

    // ── Snapshot: edge cases ────────────────────────────────────────────

    #[test]
    fn snapshot_diff_same_state_no_changes() {
        let dir = std::env::temp_dir().join("python_snap_test_same");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("a.txt"), "hello").unwrap();
        let before = WorkspaceSnapshot::capture(&dir);
        let after = WorkspaceSnapshot::capture(&dir);
        assert!(before.diff(&after).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_nonexistent_root() {
        let snap = WorkspaceSnapshot::capture(std::path::Path::new("/nonexistent_path_xyz"));
        assert_eq!(snap.file_count(), 0);
    }

    #[test]
    fn snapshot_multiple_files_changed() {
        let dir = std::env::temp_dir().join("python_snap_test_multi");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("a.txt"), "v1").unwrap();
        std::fs::write(dir.join("b.txt"), "v1").unwrap();
        let before = WorkspaceSnapshot::capture(&dir);
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(dir.join("a.txt"), "v2 longer").unwrap();
        std::fs::write(dir.join("b.txt"), "v2 longer").unwrap();
        let after = WorkspaceSnapshot::capture(&dir);
        let changed = before.diff(&after);
        assert!(changed.len() >= 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Projection: edge cases ──────────────────────────────────────────

    #[test]
    fn projection_shows_timed_out() {
        let result = PythonRunResult {
            status: PythonRunStatus::TimedOut,
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_secs(60),
            mode: PythonExecutionMode::Analyze,
            script_length: 10,
            risk: PythonRiskAssessment::safe(),
            capabilities: PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: "python3".into(),
            diff: None,
            script_body_hash: None,
            stdout_handle: None,
            stderr_handle: None,
            diff_handle: None,
        };
        let text = project_python_run(&result);
        assert!(text.contains("Timed Out"));
    }

    #[test]
    fn projection_shows_spawn_error() {
        let result = PythonRunResult {
            status: PythonRunStatus::SpawnError,
            stdout: String::new(),
            stderr: "no such interpreter".into(),
            duration: Duration::ZERO,
            mode: PythonExecutionMode::Analyze,
            script_length: 10,
            risk: PythonRiskAssessment::safe(),
            capabilities: PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: "python3".into(),
            diff: None,
            script_body_hash: None,
            stdout_handle: None,
            stderr_handle: None,
            diff_handle: None,
        };
        let text = project_python_run(&result);
        assert!(text.contains("Spawn Error"));
        assert!(text.contains("no such interpreter"));
    }

    #[test]
    fn projection_shows_risk_reasons() {
        let result = PythonRunResult {
            status: PythonRunStatus::Success,
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_millis(50),
            mode: PythonExecutionMode::Analyze,
            script_length: 30,
            risk: PythonRiskAssessment {
                level: PythonRiskLevel::Medium,
                reasons: vec!["network access detected".into()],
                has_file_io: false,
                has_file_read: false,
                has_file_write: false,
                has_subprocess: false,
                has_network: true,
                has_destructive_ops: false,
                has_dynamic_execution: false,
                imports: vec!["requests".into()],
                scanner: PythonRiskScanner::Fallback,
            },
            capabilities: PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: "python3".into(),
            diff: None,
            script_body_hash: None,
            stdout_handle: None,
            stderr_handle: None,
            diff_handle: None,
        };
        let text = project_python_run(&result);
        assert!(text.contains("network access detected"));
    }

    #[test]
    fn projection_empty_stdout_stderr() {
        let result = PythonRunResult {
            status: PythonRunStatus::Success,
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_millis(10),
            mode: PythonExecutionMode::Analyze,
            script_length: 5,
            risk: PythonRiskAssessment::safe(),
            capabilities: PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: "python3".into(),
            diff: None,
            script_body_hash: None,
            stdout_handle: None,
            stderr_handle: None,
            diff_handle: None,
        };
        let text = project_python_run(&result);
        assert!(!text.contains("### stdout"));
        assert!(!text.contains("### stderr"));
    }

    #[test]
    fn projection_stderr_only() {
        let result = PythonRunResult {
            status: PythonRunStatus::Failed(1),
            stdout: String::new(),
            stderr: "Traceback...".into(),
            duration: Duration::from_millis(10),
            mode: PythonExecutionMode::Verify,
            script_length: 20,
            risk: PythonRiskAssessment::safe(),
            capabilities: PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: "python3".into(),
            diff: None,
            script_body_hash: None,
            stdout_handle: None,
            stderr_handle: None,
            diff_handle: None,
        };
        let text = project_python_run(&result);
        assert!(text.contains("### stderr"));
        assert!(!text.contains("### stdout"));
    }

    // ── PythonRunResult summary ─────────────────────────────────────────

    #[test]
    fn summary_includes_mode_and_status() {
        let result = PythonRunResult {
            status: PythonRunStatus::Success,
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_millis(123),
            mode: PythonExecutionMode::Transform,
            script_length: 10,
            risk: PythonRiskAssessment::safe(),
            capabilities: PythonCapabilityEnvelope::analyze(),
            changed_files: vec![std::path::PathBuf::from("a.txt")],
            interpreter: "python3".into(),
            diff: None,
            script_body_hash: None,
            stdout_handle: None,
            stderr_handle: None,
            diff_handle: None,
        };
        let s = result.summary();
        assert!(s.contains("transform"));
        assert!(s.contains("success"));
        assert!(s.contains("changed: 1 files"));
    }

    #[test]
    fn summary_failure_includes_exit_code() {
        let result = PythonRunResult {
            status: PythonRunStatus::Failed(42),
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_millis(10),
            mode: PythonExecutionMode::Analyze,
            script_length: 5,
            risk: PythonRiskAssessment::safe(),
            capabilities: PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: "python3".into(),
            diff: None,
            script_body_hash: None,
            stdout_handle: None,
            stderr_handle: None,
            diff_handle: None,
        };
        let s = result.summary();
        assert!(s.contains("exit: 42"));
    }

    #[test]
    fn summary_includes_risk_reasons() {
        let result = PythonRunResult {
            status: PythonRunStatus::Success,
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_millis(10),
            mode: PythonExecutionMode::Analyze,
            script_length: 10,
            risk: PythonRiskAssessment {
                level: PythonRiskLevel::Low,
                reasons: vec!["file I/O operations detected".into()],
                has_file_io: true,
                has_file_read: false,
                has_file_write: true,
                has_subprocess: false,
                has_network: false,
                has_destructive_ops: false,
                has_dynamic_execution: false,
                imports: vec![],
                scanner: PythonRiskScanner::Fallback,
            },
            capabilities: PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: "python3".into(),
            diff: None,
            script_body_hash: None,
            stdout_handle: None,
            stderr_handle: None,
            diff_handle: None,
        };
        let s = result.summary();
        assert!(s.contains("file I/O operations detected"));
    }

    // ── Cross-module: classify → plan → route ───────────────────────────

    #[test]
    fn classify_python_analyze_plans_python_backend() {
        use crate::command_intent::{classify_command, CommandIntentKind};
        use crate::command_planner::{plan_execution, ExecutionBackend};

        let intent = classify_command("python3 -c 'import sys; print(sys.version)'");
        assert_eq!(intent.kind, CommandIntentKind::PythonAnalyze);
        let plan = plan_execution(&intent);
        assert!(matches!(
            plan.backend,
            ExecutionBackend::PythonScript { .. }
        ));
    }

    #[test]
    fn classify_python_transform_recognized() {
        use crate::command_intent::{classify_command, CommandIntentKind};

        let intent = classify_command("python3 transform.py");
        assert_eq!(intent.kind, CommandIntentKind::PythonTransform);
    }

    #[test]
    fn classify_python_verify_recognized() {
        use crate::command_intent::{classify_command, CommandIntentKind};

        let intent = classify_command("pytest tests/");
        assert_eq!(intent.kind, CommandIntentKind::Test);
    }

    #[test]
    fn route_to_python_script_decision() {
        use crate::command_intent::classify_command;
        use crate::command_planner::plan_execution;
        use crate::command_routing::resolve_routing;

        let intent = classify_command("python3 -c 'print(1)'");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(
            decision,
            crate::command_routing::RoutingDecision::RouteToPythonScripting { .. }
        ));
    }
}
