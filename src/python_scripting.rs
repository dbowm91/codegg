use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::process::Command;

const DEFAULT_PYTHON_TIMEOUT_SECS: u64 = 60;
const MAX_SCRIPT_LENGTH: usize = 500_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PythonScriptMode {
    Analyze,
    Transform,
    Verify,
}

impl PythonScriptMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Analyze => "analyze",
            Self::Transform => "transform",
            Self::Verify => "verify",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Analyze => "Read-only analysis of data or code",
            Self::Transform => "Mutating script that may change files",
            Self::Verify => "Test/verification script (e.g. pytest)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PythonScriptSource {
    Inline(String),
    FilePath(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonScript {
    pub mode: PythonScriptMode,
    pub source: PythonScriptSource,
    pub timeout_secs: u64,
    pub cwd: Option<PathBuf>,
    pub env: Option<Vec<(String, String)>>,
}

impl PythonScript {
    pub fn analyze_inline(code: &str) -> Self {
        Self {
            mode: PythonScriptMode::Analyze,
            source: PythonScriptSource::Inline(code.to_string()),
            timeout_secs: DEFAULT_PYTHON_TIMEOUT_SECS,
            cwd: None,
            env: None,
        }
    }

    pub fn transform_inline(code: &str) -> Self {
        Self {
            mode: PythonScriptMode::Transform,
            source: PythonScriptSource::Inline(code.to_string()),
            timeout_secs: DEFAULT_PYTHON_TIMEOUT_SECS,
            cwd: None,
            env: None,
        }
    }

    pub fn verify_inline(code: &str) -> Self {
        Self {
            mode: PythonScriptMode::Verify,
            source: PythonScriptSource::Inline(code.to_string()),
            timeout_secs: 300,
            cwd: None,
            env: None,
        }
    }

    pub fn from_file(path: PathBuf, mode: PythonScriptMode) -> Self {
        let timeout_secs = match mode {
            PythonScriptMode::Verify => 300,
            _ => DEFAULT_PYTHON_TIMEOUT_SECS,
        };
        Self {
            mode,
            source: PythonScriptSource::FilePath(path),
            timeout_secs,
            cwd: None,
            env: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PythonRiskLevel {
    Safe,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonRiskAnalysis {
    pub level: PythonRiskLevel,
    pub reasons: Vec<String>,
    pub has_file_io: bool,
    pub has_subprocess: bool,
    pub has_network: bool,
    pub has_destructive_ops: bool,
}

impl PythonRiskAnalysis {
    pub fn safe() -> Self {
        Self {
            level: PythonRiskLevel::Safe,
            reasons: vec![],
            has_file_io: false,
            has_subprocess: false,
            has_network: false,
            has_destructive_ops: false,
        }
    }

    pub fn requires_permission(&self) -> bool {
        matches!(self.level, PythonRiskLevel::Medium | PythonRiskLevel::High)
    }
}

pub fn analyze_python_risk(code: &str) -> PythonRiskAnalysis {
    let mut reasons = Vec::new();
    let has_file_io =
        code.contains("open(") || code.contains("write(") || code.contains("os.remove");
    let has_subprocess =
        code.contains("subprocess") || code.contains("os.system") || code.contains("os.popen");
    let has_network = code.contains("requests.")
        || code.contains("urllib")
        || code.contains("http.client")
        || code.contains("socket.");
    let has_destructive_ops =
        code.contains("shutil.rmtree") || code.contains("os.unlink") || code.contains("os.rmdir");

    if has_file_io {
        reasons.push("file I/O operations detected".to_string());
    }
    if has_subprocess {
        reasons.push("subprocess calls detected".to_string());
    }
    if has_network {
        reasons.push("network access detected".to_string());
    }
    if has_destructive_ops {
        reasons.push("destructive file operations detected".to_string());
    }

    let level = if has_destructive_ops {
        PythonRiskLevel::High
    } else if has_subprocess || has_network {
        PythonRiskLevel::Medium
    } else if has_file_io {
        PythonRiskLevel::Low
    } else {
        PythonRiskLevel::Safe
    };

    PythonRiskAnalysis {
        level,
        reasons,
        has_file_io,
        has_subprocess,
        has_network,
        has_destructive_ops,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PythonRunStatus {
    Success,
    Failed(i32),
    TimedOut,
    SpawnError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonRunResult {
    pub status: PythonRunStatus,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub mode: PythonScriptMode,
    pub script_length: usize,
}

impl PythonRunResult {
    pub fn exit_code(&self) -> Option<i32> {
        match self.status {
            PythonRunStatus::Success => Some(0),
            PythonRunStatus::Failed(code) => Some(code),
            _ => None,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self.status, PythonRunStatus::Success)
    }
}

pub async fn run_python_script(script: &PythonScript) -> PythonRunResult {
    if script.source.code().len() > MAX_SCRIPT_LENGTH {
        return PythonRunResult {
            status: PythonRunStatus::SpawnError,
            stdout: String::new(),
            stderr: format!(
                "script exceeds maximum length of {} bytes",
                MAX_SCRIPT_LENGTH
            ),
            duration: Duration::ZERO,
            mode: script.mode,
            script_length: script.source.code().len(),
        };
    }

    let start = Instant::now();

    let python_cmd = find_python_command();
    let mut args: Vec<String> = Vec::new();

    match &script.source {
        PythonScriptSource::Inline(code) => {
            args.push("-c".to_string());
            args.push(code.clone());
        }
        PythonScriptSource::FilePath(path) => {
            args.push(path.to_string_lossy().to_string());
        }
    }

    let mut cmd = Command::new(&python_cmd);
    cmd.args(&args);

    if let Some(cwd) = &script.cwd {
        cmd.current_dir(cwd);
    }

    if let Some(env_vars) = &script.env {
        for (key, value) in env_vars {
            cmd.env(key, value);
        }
    }

    let timeout = Duration::from_secs(script.timeout_secs);

    cmd.kill_on_drop(true);

    let output = match tokio::time::timeout(timeout, cmd.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return PythonRunResult {
                status: PythonRunStatus::SpawnError,
                stdout: String::new(),
                stderr: format!("failed to spawn python: {e}"),
                duration: start.elapsed(),
                mode: script.mode,
                script_length: script.source.code().len(),
            };
        }
        Err(_) => {
            return PythonRunResult {
                status: PythonRunStatus::TimedOut,
                stdout: String::new(),
                stderr: format!("python script timed out after {}s", script.timeout_secs),
                duration: start.elapsed(),
                mode: script.mode,
                script_length: script.source.code().len(),
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let status = match output.status.code() {
        Some(0) => PythonRunStatus::Success,
        Some(code) => PythonRunStatus::Failed(code),
        None => PythonRunStatus::Failed(-1),
    };

    PythonRunResult {
        status,
        stdout,
        stderr,
        duration: start.elapsed(),
        mode: script.mode,
        script_length: script.source.code().len(),
    }
}

fn find_python_command() -> String {
    if cfg!(target_os = "windows") {
        "python".to_string()
    } else {
        "python3".to_string()
    }
}

impl PythonScriptSource {
    pub fn code(&self) -> &str {
        match self {
            Self::Inline(code) => code,
            Self::FilePath(_) => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_analysis_safe_code() {
        let analysis = analyze_python_risk("print('hello')");
        assert_eq!(analysis.level, PythonRiskLevel::Safe);
        assert!(!analysis.requires_permission());
    }

    #[test]
    fn risk_analysis_file_io() {
        let analysis = analyze_python_risk("f = open('file.txt', 'w')");
        assert_eq!(analysis.level, PythonRiskLevel::Low);
        assert!(!analysis.requires_permission());
    }

    #[test]
    fn risk_analysis_subprocess() {
        let analysis = analyze_python_risk("import subprocess; subprocess.run(['ls'])");
        assert_eq!(analysis.level, PythonRiskLevel::Medium);
        assert!(analysis.requires_permission());
    }

    #[test]
    fn risk_analysis_destructive() {
        let analysis = analyze_python_risk("import shutil; shutil.rmtree('/tmp/dir')");
        assert_eq!(analysis.level, PythonRiskLevel::High);
        assert!(analysis.requires_permission());
    }

    #[test]
    fn risk_analysis_network() {
        let analysis = analyze_python_risk("import requests; requests.get('http://example.com')");
        assert_eq!(analysis.level, PythonRiskLevel::Medium);
        assert!(analysis.requires_permission());
    }

    #[test]
    fn script_modes() {
        let s = PythonScript::analyze_inline("x = 1");
        assert_eq!(s.mode, PythonScriptMode::Analyze);
        let s = PythonScript::transform_inline("open('f','w')");
        assert_eq!(s.mode, PythonScriptMode::Transform);
        let s = PythonScript::verify_inline("assert True");
        assert_eq!(s.mode, PythonScriptMode::Verify);
    }

    #[test]
    fn run_result_helpers() {
        let r = PythonRunResult {
            status: PythonRunStatus::Success,
            stdout: "ok".into(),
            stderr: String::new(),
            duration: Duration::from_millis(10),
            mode: PythonScriptMode::Analyze,
            script_length: 10,
        };
        assert!(r.is_success());
        assert_eq!(r.exit_code(), Some(0));

        let r = PythonRunResult {
            status: PythonRunStatus::Failed(1),
            stdout: String::new(),
            stderr: "error".into(),
            duration: Duration::from_millis(10),
            mode: PythonScriptMode::Verify,
            script_length: 10,
        };
        assert!(!r.is_success());
        assert_eq!(r.exit_code(), Some(1));
    }

    #[tokio::test]
    async fn run_simple_python() {
        let script = PythonScript::analyze_inline("print('hello from python')");
        let result = run_python_script(&script).await;
        assert!(result.is_success());
        assert!(result.stdout.contains("hello from python"));
    }

    #[tokio::test]
    async fn run_python_with_error() {
        let script = PythonScript::analyze_inline("import sys; sys.exit(1)");
        let result = run_python_script(&script).await;
        assert!(!result.is_success());
        assert_eq!(result.exit_code(), Some(1));
    }

    #[tokio::test]
    async fn run_python_timeout() {
        let script = PythonScript {
            mode: PythonScriptMode::Analyze,
            source: PythonScriptSource::Inline("import time; time.sleep(30)".into()),
            timeout_secs: 1,
            cwd: None,
            env: None,
        };
        let result = run_python_script(&script).await;
        assert!(matches!(result.status, PythonRunStatus::TimedOut));
    }
}
