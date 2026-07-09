use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::process::Command;

use super::sandbox::derive_envelope;
use super::snapshot::WorkspaceSnapshot;
use super::types::{
    PythonCapabilityEnvelope, PythonExecutionMode, PythonRiskAssessment, PythonRunResult,
    PythonRunStatus, PythonScriptRequest,
};

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_SCRIPT_LENGTH: usize = 500_000;

/// Execute a Python script request, returning a structured result.
pub async fn execute_python_script(request: &PythonScriptRequest) -> PythonRunResult {
    let start = Instant::now();

    // Validate
    if request.code.len() > MAX_SCRIPT_LENGTH {
        return PythonRunResult {
            status: PythonRunStatus::SpawnError,
            stdout: String::new(),
            stderr: format!("script exceeds maximum length of {MAX_SCRIPT_LENGTH} bytes"),
            duration: Duration::ZERO,
            mode: request.mode,
            script_length: request.code.len(),
            risk: PythonRiskAssessment::safe(),
            capabilities: PythonCapabilityEnvelope::analyze(),
            changed_files: vec![],
            interpreter: String::new(),
        };
    }

    // Static risk analysis
    let (capabilities, risk) = derive_envelope(request.mode, &request.code);

    // Pre-execution snapshot for Transform mode
    let pre_snapshot = if request.mode == PythonExecutionMode::Transform {
        Some(WorkspaceSnapshot::capture(&request.cwd))
    } else {
        None
    };

    // Materialize script to temp file
    let tmp_dir = request.cwd.join(".codegg").join("python_runs");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let script_file = tmp_dir.join(format!(
        "script_{}.py",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));

    if let Err(e) = std::fs::write(&script_file, &request.code) {
        return PythonRunResult {
            status: PythonRunStatus::SpawnError,
            stdout: String::new(),
            stderr: format!("failed to write script: {e}"),
            duration: start.elapsed(),
            mode: request.mode,
            script_length: request.code.len(),
            risk,
            capabilities,
            changed_files: vec![],
            interpreter: String::new(),
        };
    }

    // Find python interpreter
    let interpreter = find_python_interpreter();
    let timeout = Duration::from_secs(request.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS));

    // Execute with timeout
    let run_result = match tokio::time::timeout(
        timeout,
        Command::new(&interpreter)
            .arg(script_file.to_string_lossy().to_string())
            .current_dir(&request.cwd)
            .kill_on_drop(true)
            .output(),
    )
    .await
    {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let status = match output.status.code() {
                Some(0) => PythonRunStatus::Success,
                Some(code) => PythonRunStatus::Failed(code),
                None => PythonRunStatus::Failed(-1),
            };
            (status, stdout, stderr)
        }
        Ok(Err(e)) => (
            PythonRunStatus::SpawnError,
            String::new(),
            format!("failed to spawn python: {e}"),
        ),
        Err(_) => (
            PythonRunStatus::TimedOut,
            String::new(),
            format!("python script timed out after {}s", timeout.as_secs()),
        ),
    };

    let (status, stdout, stderr) = run_result;

    // Post-execution snapshot for Transform mode
    let changed_files = if let Some(pre) = &pre_snapshot {
        let post = WorkspaceSnapshot::capture(&request.cwd);
        pre.diff(&post)
    } else {
        vec![]
    };

    // For Analyze mode, if files changed that's a policy violation
    let (status, stderr) = if request.mode == PythonExecutionMode::Analyze
        && !changed_files.is_empty()
        && status.is_success()
    {
        (
            PythonRunStatus::Failed(-2),
            format!(
                "{stderr}\n[python_script] policy violation: analyze mode produced {} file changes",
                changed_files.len()
            ),
        )
    } else {
        (status, stderr)
    };

    // Cleanup temp script
    let _ = std::fs::remove_file(&script_file);

    PythonRunResult {
        status,
        stdout,
        stderr,
        duration: start.elapsed(),
        mode: request.mode,
        script_length: request.code.len(),
        risk,
        capabilities,
        changed_files,
        interpreter,
    }
}

/// Find the Python interpreter to use.
/// Priority: VIRTUAL_ENV > python3 > python
fn find_python_interpreter() -> String {
    // Check VIRTUAL_ENV
    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        let venv_path = PathBuf::from(&venv);
        let python = if cfg!(target_os = "windows") {
            venv_path.join("Scripts").join("python.exe")
        } else {
            venv_path.join("bin").join("python3")
        };
        if python.exists() {
            return python.to_string_lossy().to_string();
        }
    }

    // Fall back to python3 then python
    if cfg!(target_os = "windows") {
        "python".to_string()
    } else {
        "python3".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn execute_simple_analyze() {
        let request = PythonScriptRequest {
            code: "print('hello from python')".to_string(),
            mode: PythonExecutionMode::Analyze,
            cwd: std::env::temp_dir(),
            timeout_secs: Some(10),
            session_id: None,
            intent: None,
        };
        let result = execute_python_script(&request).await;
        assert!(result.is_success());
        assert!(result.stdout.contains("hello from python"));
    }

    #[tokio::test]
    async fn execute_analyze_with_write_detected() {
        let dir = std::env::temp_dir().join("python_exec_test_write");
        let _ = std::fs::create_dir_all(&dir);
        let request = PythonScriptRequest {
            code: "open('should_not_exist.txt', 'w').write('nope')".to_string(),
            mode: PythonExecutionMode::Analyze,
            cwd: dir.clone(),
            timeout_secs: Some(10),
            session_id: None,
            intent: None,
        };
        let result = execute_python_script(&request).await;
        // Risk analysis should flag file I/O
        assert!(result.risk.has_file_io);
        // Analyze mode envelope should deny write_workspace
        assert!(!result.capabilities.write_workspace);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_python_returns_string() {
        let interp = find_python_interpreter();
        assert!(!interp.is_empty());
    }
}
