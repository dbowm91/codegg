use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tokio::process::Command;

use super::sandbox::{check_compatibility, derive_envelope};
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

    // Compute script body hash for reproducibility tracking
    let script_body_hash = Some(format!("{:x}", Sha256::digest(request.code.as_bytes())));

    // Validate script length
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
            diff: None,
            script_body_hash,
            stdout_label: None,
            stderr_label: None,
            diff_label: None,
        };
    }

    // Validate and canonicalize cwd
    let cwd = match validate_cwd(&request.cwd, request.workspace_root.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            return PythonRunResult {
                status: PythonRunStatus::SpawnError,
                stdout: String::new(),
                stderr: e,
                duration: start.elapsed(),
                mode: request.mode,
                script_length: request.code.len(),
                risk: PythonRiskAssessment::safe(),
                capabilities: PythonCapabilityEnvelope::analyze(),
                changed_files: vec![],
                interpreter: String::new(),
                diff: None,
                script_body_hash,
                stdout_label: None,
                stderr_label: None,
                diff_label: None,
            };
        }
    };

    // Static risk analysis
    let (capabilities, risk) = derive_envelope(request.mode, &request.code);

    // Pre-execution capability check: deny if risk analysis flags capabilities
    // that the mode does not permit (e.g., network in Analyze, subprocess in
    // Analyze/Transform, destructive_fs anywhere, file writes in Analyze).
    // This blocks execution BEFORE any child process is spawned.
    let violations = check_compatibility(request.mode, &request.code);
    if !violations.is_empty() {
        return PythonRunResult {
            status: PythonRunStatus::Failed(-3),
            stdout: String::new(),
            stderr: format!(
                "[python_script] denied: capability check failed for {}",
                violations.join(", ")
            ),
            duration: start.elapsed(),
            mode: request.mode,
            script_length: request.code.len(),
            risk,
            capabilities,
            changed_files: vec![],
            interpreter: String::new(),
            diff: None,
            script_body_hash,
            stdout_label: None,
            stderr_label: None,
            diff_label: None,
        };
    }

    // Materialize script to temp file BEFORE snapshot so the script file
    // itself is not detected as a workspace change.
    let tmp_dir = cwd.join(".codegg").join("python_runs");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let script_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let script_file = tmp_dir.join(format!("script_{script_id}.py"));

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
            diff: None,
            script_body_hash,
            stdout_label: None,
            stderr_label: None,
            diff_label: None,
        };
    }

    // Pre-execution snapshot for ALL modes (Analyze, Transform, Verify).
    // Used post-execution to detect unauthorized file changes.
    // Taken after script materialization so the temp script is included.
    let pre_snapshot = Some(WorkspaceSnapshot::capture(&cwd));

    // Pre-execution content capture for diff generation (Transform mode).
    // Stores the content of all files under cwd so we can compute textual diffs
    // after execution.
    let pre_contents = capture_file_contents(&cwd);

    // Find python interpreter
    let interpreter = find_python_interpreter();
    let timeout = Duration::from_secs(request.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS));

    // Execute with timeout and minimal environment isolation.
    // Preserved env vars:
    //   PATH       - required to find the interpreter and shared libraries
    //   HOME       - Python user site-packages, ~/.pythonhistory
    //   LANG/LC_ALL - locale for consistent string handling
    //   VIRTUAL_ENV - virtualenv activation (if present)
    //   PYTHONPATH  - explicit module search path (if set by caller)
    //   DYLD_LIBRARY_PATH (macOS only) - dynamic linker search path
    let original_dyld = std::env::var("DYLD_LIBRARY_PATH").ok();
    let run_result = match tokio::time::timeout(
        timeout,
        Command::new(&interpreter)
            .arg(script_file.to_string_lossy().to_string())
            .current_dir(&cwd)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env("LANG", std::env::var("LANG").unwrap_or_default())
            .env("LC_ALL", std::env::var("LC_ALL").unwrap_or_default())
            .env(
                "VIRTUAL_ENV",
                std::env::var("VIRTUAL_ENV").unwrap_or_default(),
            )
            .env(
                "PYTHONPATH",
                std::env::var("PYTHONPATH").unwrap_or_default(),
            )
            .env("DYLD_LIBRARY_PATH", original_dyld.unwrap_or_default())
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

    // Post-execution snapshot and diff for ALL modes.
    // If files changed in a read-only mode (Analyze, Verify), that is a
    // policy violation and the run is failed even if the script exited 0.
    let (changed_files, status, stderr) = if let Some(pre) = &pre_snapshot {
        let post = WorkspaceSnapshot::capture(&cwd);
        let changed = pre.diff(&post);
        if !changed.is_empty() {
            let count = changed.len();
            match request.mode {
                PythonExecutionMode::Analyze => {
                    // Analyze is strictly read-only: any file change is a violation
                    (
                        changed,
                        PythonRunStatus::Failed(-2),
                        format!(
                            "{stderr}\n[python_script] policy violation: analyze mode produced {count} file change(s)",
                        ),
                    )
                }
                PythonExecutionMode::Verify => {
                    // Verify is read-only by default: file changes are a violation
                    (
                        changed,
                        PythonRunStatus::Failed(-2),
                        format!(
                            "{stderr}\n[python_script] policy violation: verify mode produced {count} file change(s)",
                        ),
                    )
                }
                PythonExecutionMode::Transform => {
                    // Transform mode is allowed to change files; report them
                    (changed, status, stderr)
                }
            }
        } else {
            (changed, status, stderr)
        }
    } else {
        (vec![], status, stderr)
    };

    // Generate textual diff for Transform mode changed files.
    let diff = if request.mode == PythonExecutionMode::Transform && !changed_files.is_empty() {
        Some(generate_textual_diff(&cwd, &changed_files, &pre_contents))
    } else {
        None
    };

    // Generate pseudo-local run labels (not registered in any artifact store)
    let run_id = script_id.to_string();
    let stdout_label = Some(format!("python_run://{run_id}/stdout"));
    let stderr_label = Some(format!("python_run://{run_id}/stderr"));
    let diff_label = if diff.is_some() {
        Some(format!("python_run://{run_id}/diff"))
    } else {
        None
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
        diff,
        script_body_hash,
        stdout_label,
        stderr_label,
        diff_label,
    }
}

/// Validate that `cwd` exists, is a directory, and is inside the workspace.
/// When `workspace_root` is provided, uses it for containment checks.
/// Falls back to `env::current_dir()` when no explicit root is given.
fn validate_cwd(cwd: &Path, workspace_root: Option<&Path>) -> Result<PathBuf, String> {
    let candidate = if cwd.as_os_str().is_empty() {
        std::env::current_dir().map_err(|e| format!("cannot determine current directory: {e}"))?
    } else {
        cwd.to_path_buf()
    };

    if !candidate.exists() {
        return Err(format!("cwd does not exist: {}", candidate.display()));
    }

    if !candidate.is_dir() {
        return Err(format!("cwd is not a directory: {}", candidate.display()));
    }

    let canonical_cwd = candidate
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize cwd: {e}"))?;

    // Use explicit workspace root when provided, else fall back to process cwd
    let effective_root = workspace_root
        .map(|r| r.to_path_buf())
        .or_else(|| std::env::current_dir().ok());

    if let Some(root) = effective_root {
        let canonical_root = root
            .canonicalize()
            .map_err(|e| format!("cannot canonicalize workspace root: {e}"))?;

        if !canonical_cwd.starts_with(&canonical_root) {
            return Err(format!(
                "cwd is outside workspace: {} (workspace: {})",
                canonical_cwd.display(),
                canonical_root.display()
            ));
        }
    }

    Ok(canonical_cwd)
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

/// Capture the contents of all files under `root` for diff generation.
/// Returns a map from relative path to file content bytes.
/// Only captures text-readable files up to a per-file size limit.
fn capture_file_contents(root: &Path) -> HashMap<PathBuf, Vec<u8>> {
    const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB per file
    let mut contents = HashMap::new();
    if let Ok(entries) = collect_workspace_files(root) {
        for entry in entries {
            if let Ok(meta) = std::fs::metadata(&entry) {
                if meta.len() <= MAX_FILE_BYTES {
                    if let Ok(data) = std::fs::read(&entry) {
                        let rel = entry.strip_prefix(root).unwrap_or(&entry).to_path_buf();
                        contents.insert(rel, data);
                    }
                }
            }
        }
    }
    contents
}

/// Collect all files under `root` (same walk logic as snapshot).
fn collect_workspace_files(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut results = Vec::new();
    if !root.exists() {
        return Ok(results);
    }
    collect_dir(root, &mut results, 10)?;
    Ok(results)
}

fn collect_dir(dir: &Path, results: &mut Vec<PathBuf>, depth: usize) -> Result<(), std::io::Error> {
    if depth == 0 {
        return Ok(());
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default();
                let name_str = name.to_string_lossy();
                if name_str == "."
                    || name_str == ".."
                    || name_str.starts_with('.')
                    || name_str == "target"
                    || name_str == "node_modules"
                    || name_str == ".codegg"
                {
                    continue;
                }
                collect_dir(&path, results, depth - 1)?;
            } else {
                let name = path.file_name().unwrap_or_default();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') || name_str == "Thumbs.db" {
                    continue;
                }
                results.push(path);
            }
        }
    }
    Ok(())
}

/// Generate a simple textual diff for changed files.
///
/// Produces a human-readable (non-unified-diff) format:
/// - For modified files: file path + truncated old/new content
/// - For new files: file path + "new file" + truncated content
/// - For deleted files: file path + "deleted" + truncated old content
fn generate_textual_diff(
    root: &Path,
    changed_files: &[PathBuf],
    pre_contents: &HashMap<PathBuf, Vec<u8>>,
) -> String {
    const MAX_DIFF_CONTENT: usize = 4000;
    let mut lines = Vec::new();

    for file in changed_files {
        let rel = file.strip_prefix(root).unwrap_or(file);
        let existed_before = pre_contents.contains_key(rel);
        let post_content = std::fs::read(root.join(rel)).ok();

        match (existed_before, &post_content) {
            (true, Some(new_bytes)) => {
                // Modified file
                if let Some(old_bytes) = pre_contents.get(rel) {
                    let old_text = String::from_utf8_lossy(old_bytes);
                    let new_text = String::from_utf8_lossy(new_bytes);
                    lines.push(format!("--- a/{}", rel.display()));
                    lines.push(format!("+++ b/{}", rel.display()));
                    if old_text == new_text {
                        lines.push("(content unchanged — metadata-only change)".to_string());
                    } else {
                        let old_preview = truncate_for_diff(&old_text, MAX_DIFF_CONTENT);
                        let new_preview = truncate_for_diff(&new_text, MAX_DIFF_CONTENT);
                        lines.push(format!("-{old_preview}"));
                        lines.push(format!("+{new_preview}"));
                    }
                    lines.push(String::new());
                }
            }
            (true, None) => {
                // Deleted file
                if let Some(old_bytes) = pre_contents.get(rel) {
                    let old_text = String::from_utf8_lossy(old_bytes);
                    lines.push(format!("--- a/{}", rel.display()));
                    lines.push("+++ /dev/null".to_string());
                    let old_preview = truncate_for_diff(&old_text, MAX_DIFF_CONTENT);
                    lines.push(format!("-{old_preview}"));
                    lines.push(String::new());
                }
            }
            (false, Some(new_bytes)) => {
                // New file
                let new_text = String::from_utf8_lossy(new_bytes);
                lines.push("--- /dev/null".to_string());
                lines.push(format!("+++ b/{}", rel.display()));
                let new_preview = truncate_for_diff(&new_text, MAX_DIFF_CONTENT);
                lines.push(format!("+{new_preview}"));
                lines.push(String::new());
            }
            (false, None) => {
                // Edge case: file appeared and disappeared in the same run
                lines.push(format!("{}: appeared and disappeared", rel.display()));
                lines.push(String::new());
            }
        }
    }

    lines.join("\n")
}

/// Truncate text for diff display, adding a notice if truncated.
fn truncate_for_diff(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!(
            "{truncated}\n[truncated at {max_chars} chars, total {} chars]",
            text.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to get a unique workspace-relative temp dir for each test.
    fn test_cwd(suffix: &str) -> PathBuf {
        let cwd = std::env::current_dir().unwrap();
        let dir = cwd.join(".codegg").join("test_tmp").join(format!(
            "t_{}_{}",
            suffix,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[tokio::test]
    async fn execute_simple_analyze() {
        let dir = test_cwd("simple");
        let request = PythonScriptRequest {
            code: "print('hello from python')".to_string(),
            mode: PythonExecutionMode::Analyze,
            cwd: dir.clone(),
            workspace_root: None,
            timeout_secs: Some(10),
            session_id: None,
            intent: None,
        };
        let result = execute_python_script(&request).await;
        assert!(result.is_success(), "stderr: {}", result.stderr);
        assert!(result.stdout.contains("hello from python"));
    }

    #[tokio::test]
    async fn execute_analyze_with_write_detected() {
        let dir = test_cwd("write_detect");
        let request = PythonScriptRequest {
            code: "open('should_not_exist.txt', 'w').write('nope')".to_string(),
            mode: PythonExecutionMode::Analyze,
            cwd: dir.clone(),
            workspace_root: None,
            timeout_secs: Some(10),
            session_id: None,
            intent: None,
        };
        let result = execute_python_script(&request).await;
        // Risk analysis should flag file I/O
        assert!(result.risk.has_file_io);
        // Analyze mode envelope should deny write_workspace
        assert!(!result.capabilities.write_workspace);
        // Cleanup
        let _ = std::fs::remove_file(dir.join("should_not_exist.txt"));
    }

    #[tokio::test]
    async fn denied_capability_blocks_execution() {
        // Script with subprocess usage in Analyze mode should be blocked
        let dir = test_cwd("denied");
        let request = PythonScriptRequest {
            code: "import subprocess\nsubprocess.run(['echo', 'should not run'])".to_string(),
            mode: PythonExecutionMode::Analyze,
            cwd: dir.clone(),
            workspace_root: None,
            timeout_secs: Some(10),
            session_id: None,
            intent: None,
        };
        let result = execute_python_script(&request).await;
        assert!(!result.is_success(), "expected failure, got success");
        assert!(
            result.stderr.contains("denied"),
            "stderr: {}",
            result.stderr
        );
        assert!(
            result.stderr.contains("subprocess"),
            "stderr: {}",
            result.stderr
        );
    }

    #[tokio::test]
    async fn analyze_mode_detects_file_write() {
        let dir = test_cwd("analyze_write");
        // Script that writes a file using pathlib.
        // The AST scanner now correctly detects Path.write_text() as a file write,
        // so the pre-execution check catches it before execution.
        let request = PythonScriptRequest {
            code: "from pathlib import Path\nPath('_anomaly_test.txt').write_text('nope')"
                .to_string(),
            mode: PythonExecutionMode::Analyze,
            cwd: dir.clone(),
            workspace_root: None,
            timeout_secs: Some(10),
            session_id: None,
            intent: None,
        };
        let result = execute_python_script(&request).await;
        // Analyze mode should fail when file write is detected by static analysis
        assert!(
            !result.is_success(),
            "expected policy violation, got success; stderr: {}",
            result.stderr
        );
        assert!(
            result.stderr.contains("policy violation") || result.stderr.contains("denied"),
            "stderr: {}",
            result.stderr
        );
        // Cleanup
        let _ = std::fs::remove_file(dir.join("_anomaly_test.txt"));
    }

    #[tokio::test]
    async fn verify_mode_detects_file_write() {
        let dir = test_cwd("verify_write");
        // Script that writes a file using pathlib.
        // The AST scanner now correctly detects Path.write_text() as a file write,
        // so the pre-execution check catches it before execution.
        let request = PythonScriptRequest {
            code: "from pathlib import Path\nPath('_verify_test.txt').write_text('nope')"
                .to_string(),
            mode: PythonExecutionMode::Verify,
            cwd: dir.clone(),
            workspace_root: None,
            timeout_secs: Some(10),
            session_id: None,
            intent: None,
        };
        let result = execute_python_script(&request).await;
        // Verify mode should fail when file write is detected by static analysis
        assert!(
            !result.is_success(),
            "expected policy violation, got success; stderr: {}",
            result.stderr
        );
        assert!(
            result.stderr.contains("policy violation") || result.stderr.contains("denied"),
            "stderr: {}",
            result.stderr
        );
        // Cleanup
        let _ = std::fs::remove_file(dir.join("_verify_test.txt"));
    }

    #[tokio::test]
    async fn transform_mode_allows_file_write() {
        let dir = test_cwd("transform_write");
        let request = PythonScriptRequest {
            code: "with open('_transform_test.txt', 'w') as f: f.write('allowed')".to_string(),
            mode: PythonExecutionMode::Transform,
            cwd: dir.clone(),
            workspace_root: None,
            timeout_secs: Some(10),
            session_id: None,
            intent: None,
        };
        let result = execute_python_script(&request).await;
        // Transform mode succeeds when files change
        assert!(result.is_success(), "stderr: {}", result.stderr);
        assert!(!result.changed_files.is_empty());
        // Cleanup
        let _ = std::fs::remove_file(dir.join("_transform_test.txt"));
    }

    #[test]
    fn validate_cwd_rejects_nonexistent() {
        let result = validate_cwd(Path::new("/nonexistent_path_xyz_12345"), None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    #[test]
    fn validate_cwd_rejects_file_not_dir() {
        let result = validate_cwd(std::env::current_exe().unwrap().as_path(), None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a directory"));
    }

    #[test]
    fn validate_cwd_empty_falls_back_to_current_dir() {
        let result = validate_cwd(Path::new(""), None);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_cwd_accepts_current_dir() {
        let cwd = std::env::current_dir().unwrap();
        let result = validate_cwd(&cwd, None);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_cwd_rejects_outside_workspace() {
        let result = validate_cwd(Path::new("/"), None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("outside workspace"));
    }

    #[test]
    fn find_python_returns_string() {
        let interp = find_python_interpreter();
        assert!(!interp.is_empty());
    }
}
