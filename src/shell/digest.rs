use std::path::{Path, PathBuf};
use std::time::Duration;

use regex::Regex;

use super::store::{BoundedOutput, ShellOutputEntry};
use super::types::ShellStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellFailureKind {
    RustCompilerError,
    RustCompilerWarning,
    RustTestFailure,
    Panic,
    GenericNonZeroExit,
}

#[derive(Debug, Clone)]
pub struct ShellFailure {
    pub kind: ShellFailureKind,
    pub message: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TruncationReport {
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stdout_omitted_bytes: usize,
    pub stderr_omitted_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct ShellDigest {
    pub command: String,
    pub cwd: PathBuf,
    pub status: ShellStatus,
    pub exit_code: Option<i32>,
    pub elapsed: Duration,
    pub stdout_summary: Option<String>,
    pub stderr_summary: Option<String>,
    pub extracted_failures: Vec<ShellFailure>,
    pub truncation: TruncationReport,
}

impl ShellDigest {
    pub fn build(
        command: &str,
        cwd: &Path,
        status: ShellStatus,
        exit_code: Option<i32>,
        elapsed: Duration,
        stdout: &BoundedOutput,
        stderr: &BoundedOutput,
    ) -> Self {
        let stderr_text = stderr.head_str_lossy();
        let stdout_text = stdout.head_str_lossy();
        let mut failures = Vec::new();

        extract_rust_errors(&stderr_text, &mut failures);
        extract_rust_warnings(&stderr_text, &mut failures);
        extract_test_failures(&stderr_text, &mut failures);
        extract_test_failures(&stdout_text, &mut failures);
        extract_panics(&stderr_text, &mut failures);
        extract_panics(&stdout_text, &mut failures);

        match status {
            ShellStatus::Killed => {
                failures.push(ShellFailure {
                    kind: ShellFailureKind::GenericNonZeroExit,
                    message: "process killed by user".to_string(),
                    location: None,
                });
            }
            ShellStatus::TimedOut => {
                failures.push(ShellFailure {
                    kind: ShellFailureKind::GenericNonZeroExit,
                    message: "process timed out".to_string(),
                    location: None,
                });
            }
            ShellStatus::FailedToStart => {
                failures.push(ShellFailure {
                    kind: ShellFailureKind::GenericNonZeroExit,
                    message: "process failed to start".to_string(),
                    location: None,
                });
            }
            ShellStatus::Running | ShellStatus::Exited => {
                if exit_code.is_some_and(|c| c != 0) && failures.is_empty() {
                    failures.push(ShellFailure {
                        kind: ShellFailureKind::GenericNonZeroExit,
                        message: format!("process exited with code {}", exit_code.unwrap()),
                        location: None,
                    });
                }
            }
        }

        let stdout_summary = if stdout_text.trim().is_empty() {
            None
        } else {
            Some(summarize(&stdout_text, 500))
        };
        let stderr_summary = if stderr_text.trim().is_empty() {
            None
        } else {
            Some(summarize(&stderr_text, 500))
        };

        ShellDigest {
            command: command.to_string(),
            cwd: cwd.to_path_buf(),
            status,
            exit_code,
            elapsed,
            stdout_summary,
            stderr_summary,
            extracted_failures: failures,
            truncation: TruncationReport {
                stdout_truncated: stdout.omitted_bytes > 0,
                stderr_truncated: stderr.omitted_bytes > 0,
                stdout_omitted_bytes: stdout.omitted_bytes,
                stderr_omitted_bytes: stderr.omitted_bytes,
            },
        }
    }

    pub fn build_from_entry(entry: &ShellOutputEntry) -> Self {
        Self::build(
            &entry.command,
            &entry.cwd,
            entry.status,
            entry.exit_code,
            entry.elapsed.unwrap_or_default(),
            &entry.stdout,
            &entry.stderr,
        )
    }

    pub fn has_failures(&self) -> bool {
        !self.extracted_failures.is_empty()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Command: {}\n", self.command));
        out.push_str(&format!("Cwd: {}\n", self.cwd.display()));
        out.push_str(&format!(
            "Status: {}\n",
            match self.status {
                ShellStatus::Running => "running",
                ShellStatus::Exited => "exited",
                ShellStatus::TimedOut => "timed out",
                ShellStatus::FailedToStart => "failed to start",
                ShellStatus::Killed => "killed",
            }
        ));
        if let Some(code) = self.exit_code {
            out.push_str(&format!("Exit code: {}\n", code));
        }
        out.push_str(&format!("Elapsed: {:.1}s\n", self.elapsed.as_secs_f64()));

        if let Some(ref s) = self.stdout_summary {
            out.push_str(&format!("\n--- stdout ---\n{}\n", s));
        }
        if let Some(ref s) = self.stderr_summary {
            out.push_str(&format!("\n--- stderr ---\n{}\n", s));
        }

        if !self.extracted_failures.is_empty() {
            out.push_str("\n--- failures ---\n");
            for f in &self.extracted_failures {
                out.push_str(&format!("[{:?}] {}", f.kind, f.message));
                if let Some(ref loc) = f.location {
                    out.push_str(&format!(" at {}", loc));
                }
                out.push('\n');
            }
        }

        if self.truncation.stdout_truncated || self.truncation.stderr_truncated {
            out.push_str("\n--- truncation ---\n");
            if self.truncation.stdout_truncated {
                out.push_str(&format!(
                    "stdout omitted {} bytes\n",
                    self.truncation.stdout_omitted_bytes
                ));
            }
            if self.truncation.stderr_truncated {
                out.push_str(&format!(
                    "stderr omitted {} bytes\n",
                    self.truncation.stderr_omitted_bytes
                ));
            }
        }

        out
    }
}

fn summarize(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        let mut truncated = text[..max_chars].to_string();
        truncated.push_str("\n... [truncated]");
        truncated
    }
}

fn extract_rust_errors(text: &str, failures: &mut Vec<ShellFailure>) {
    let re = Regex::new(r"(?m)^error\[(E\d+)\]:?\s*(.+)$").unwrap();
    for cap in re.captures_iter(text) {
        let code = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let msg = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim();
        let location = extract_location_after(text, cap.get(0).unwrap().end());
        failures.push(ShellFailure {
            kind: ShellFailureKind::RustCompilerError,
            message: format!("[{}] {}", code, msg),
            location,
        });
    }
}

fn extract_rust_warnings(text: &str, failures: &mut Vec<ShellFailure>) {
    let re = Regex::new(r"(?m)^warning:\s*(.+)$").unwrap();
    for cap in re.captures_iter(text) {
        let msg = cap.get(1).map(|m| m.as_str()).unwrap_or("").trim();
        let location = extract_location_after(text, cap.get(0).unwrap().end());
        failures.push(ShellFailure {
            kind: ShellFailureKind::RustCompilerWarning,
            message: msg.to_string(),
            location,
        });
    }
}

fn extract_test_failures(text: &str, failures: &mut Vec<ShellFailure>) {
    let re = Regex::new(r"(?m)test result: FAILED[^;]*;\s*(\d+)\s+failed").unwrap();
    for cap in re.captures_iter(text) {
        let failed = cap.get(1).map(|m| m.as_str()).unwrap_or("0");
        failures.push(ShellFailure {
            kind: ShellFailureKind::RustTestFailure,
            message: format!("test result: FAILED ({} failed)", failed),
            location: None,
        });
    }

    let re2 = Regex::new(r"(?m)^\s*failures:\s*$").unwrap();
    for mat in re2.find_iter(text) {
        let after = &text[mat.end()..];
        let failure_block: String = after.lines().take(10).collect::<Vec<_>>().join("\n");
        if !failure_block.trim().is_empty() {
            failures.push(ShellFailure {
                kind: ShellFailureKind::RustTestFailure,
                message: format!("failures:\n{}", failure_block),
                location: None,
            });
        }
    }
}

fn extract_panics(text: &str, failures: &mut Vec<ShellFailure>) {
    let re = Regex::new(r"(?m)^thread\s+'[^']+'\s+panicked\s+at\s+'(.+)',\s+(.+):\d+").unwrap();
    for cap in re.captures_iter(text) {
        let msg = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let loc = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        failures.push(ShellFailure {
            kind: ShellFailureKind::Panic,
            message: msg.to_string(),
            location: Some(loc.to_string()),
        });
    }
}

fn extract_location_after(text: &str, offset: usize) -> Option<String> {
    let after = &text[offset..];
    let re = Regex::new(r"(?m)^\s*-->\s+(.+):\d+:\d+").unwrap();
    re.captures(after)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_output() -> BoundedOutput {
        BoundedOutput::new()
    }

    fn output_with(data: &[u8]) -> BoundedOutput {
        let mut bo = BoundedOutput::new();
        bo.append(data);
        bo
    }

    #[test]
    fn digest_basic_success() {
        let stdout = output_with(b"running 1 test\ntest ok\n");
        let digest = ShellDigest::build(
            "cargo test",
            &PathBuf::from("/tmp"),
            ShellStatus::Exited,
            Some(0),
            Duration::from_secs(2),
            &stdout,
            &empty_output(),
        );
        assert!(!digest.has_failures());
        assert!(digest.extracted_failures.is_empty());
    }

    #[test]
    fn digest_nonzero_exit_generic() {
        let digest = ShellDigest::build(
            "cargo build",
            &PathBuf::from("/tmp"),
            ShellStatus::Exited,
            Some(1),
            Duration::from_secs(1),
            &empty_output(),
            &empty_output(),
        );
        assert!(digest.has_failures());
        assert_eq!(digest.extracted_failures.len(), 1);
        assert_eq!(
            digest.extracted_failures[0].kind,
            ShellFailureKind::GenericNonZeroExit
        );
    }

    #[test]
    fn digest_rust_compiler_error() {
        let stderr = output_with(b"error[E0308]: mismatched types\n --> src/main.rs:5:9\n");
        let digest = ShellDigest::build(
            "cargo check",
            &PathBuf::from("/tmp"),
            ShellStatus::Exited,
            Some(1),
            Duration::from_secs(1),
            &empty_output(),
            &stderr,
        );
        let errors: Vec<_> = digest
            .extracted_failures
            .iter()
            .filter(|f| f.kind == ShellFailureKind::RustCompilerError)
            .collect();
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("E0308"));
    }

    #[test]
    fn digest_rust_warning() {
        let stderr = output_with(b"warning: unused variable `x`\n --> src/main.rs:3:9\n");
        let digest = ShellDigest::build(
            "cargo check",
            &PathBuf::from("/tmp"),
            ShellStatus::Exited,
            Some(0),
            Duration::from_secs(1),
            &empty_output(),
            &stderr,
        );
        let warnings: Vec<_> = digest
            .extracted_failures
            .iter()
            .filter(|f| f.kind == ShellFailureKind::RustCompilerWarning)
            .collect();
        assert!(!warnings.is_empty());
        assert!(warnings[0].message.contains("unused variable"));
    }

    #[test]
    fn digest_test_failure() {
        let stderr = output_with(
            b"test result: FAILED. 0 passed; 1 failed; 0 ignored\n\nfailures:\n\nmy_test::test_foo\n",
        );
        let digest = ShellDigest::build(
            "cargo test",
            &PathBuf::from("/tmp"),
            ShellStatus::Exited,
            Some(101),
            Duration::from_secs(5),
            &empty_output(),
            &stderr,
        );
        let test_fails: Vec<_> = digest
            .extracted_failures
            .iter()
            .filter(|f| f.kind == ShellFailureKind::RustTestFailure)
            .collect();
        assert!(!test_fails.is_empty());
        assert!(test_fails[0].message.contains("1 failed"));
    }

    #[test]
    fn digest_panic() {
        let stderr = output_with(
            b"thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: NotFound', src/main.rs:10:5\n",
        );
        let digest = ShellDigest::build(
            "cargo run",
            &PathBuf::from("/tmp"),
            ShellStatus::Exited,
            Some(101),
            Duration::from_secs(1),
            &empty_output(),
            &stderr,
        );
        let panics: Vec<_> = digest
            .extracted_failures
            .iter()
            .filter(|f| f.kind == ShellFailureKind::Panic)
            .collect();
        assert!(!panics.is_empty());
        assert!(panics[0].message.contains("unwrap"));
    }

    #[test]
    fn digest_render() {
        let stderr = output_with(b"error[E0308]: mismatched types\n --> src/main.rs:5:9\n");
        let digest = ShellDigest::build(
            "cargo check",
            &PathBuf::from("/tmp"),
            ShellStatus::Exited,
            Some(1),
            Duration::from_secs(1),
            &empty_output(),
            &stderr,
        );
        let rendered = digest.render();
        assert!(rendered.contains("Command: cargo check"));
        assert!(rendered.contains("Exit code: 1"));
        assert!(rendered.contains("E0308"));
    }

    #[test]
    fn digest_stdout_summary_none_when_empty() {
        let digest = ShellDigest::build(
            "cmd",
            &PathBuf::from("/tmp"),
            ShellStatus::Exited,
            Some(0),
            Duration::from_secs(1),
            &empty_output(),
            &empty_output(),
        );
        assert!(digest.stdout_summary.is_none());
        assert!(digest.stderr_summary.is_none());
    }

    #[test]
    fn digest_stdout_summary_present() {
        let stdout = output_with(b"some output\n");
        let digest = ShellDigest::build(
            "cmd",
            &PathBuf::from("/tmp"),
            ShellStatus::Exited,
            Some(0),
            Duration::from_secs(1),
            &stdout,
            &empty_output(),
        );
        assert!(digest.stdout_summary.is_some());
        assert!(digest.stdout_summary.unwrap().contains("some output"));
    }

    #[test]
    fn digest_truncation_report() {
        let mut stdout = BoundedOutput::new();
        stdout.append(&[b'x'; 512 * 1024]);
        let digest = ShellDigest::build(
            "cmd",
            &PathBuf::from("/tmp"),
            ShellStatus::Exited,
            Some(0),
            Duration::from_secs(1),
            &stdout,
            &empty_output(),
        );
        assert!(digest.truncation.stdout_truncated);
        assert!(!digest.truncation.stderr_truncated);
        assert!(digest.truncation.stdout_omitted_bytes > 0);
    }

    #[test]
    fn summarize_short_text() {
        let s = summarize("hello", 100);
        assert_eq!(s, "hello");
    }

    #[test]
    fn summarize_long_text() {
        let s = summarize(&"a".repeat(200), 100);
        assert!(s.len() < 120);
        assert!(s.contains("[truncated]"));
    }

    #[test]
    fn build_from_entry_matches_manual_build() {
        use crate::shell::store::ShellOutputEntry;
        use crate::shell::types::{ShellCapturePolicy, ShellCommandId, ShellStatus};
        use std::path::PathBuf;
        use std::time::SystemTime;

        let mut stdout = BoundedOutput::new();
        stdout.append(b"hello world");
        let entry = ShellOutputEntry {
            id: ShellCommandId(1),
            command: "echo hello".to_string(),
            cwd: PathBuf::from("/tmp"),
            started_at: SystemTime::now(),
            finished_at: Some(SystemTime::now()),
            status: ShellStatus::Exited,
            exit_code: Some(0),
            stdout,
            stderr: BoundedOutput::new(),
            elapsed: Some(Duration::from_secs(1)),
            promoted: false,
            promote_after: false,
            capture_policy: ShellCapturePolicy::StoreEphemeral,
        };

        let from_entry = ShellDigest::build_from_entry(&entry);
        assert_eq!(from_entry.command, "echo hello");
        assert_eq!(from_entry.exit_code, Some(0));
        assert_eq!(from_entry.elapsed, Duration::from_secs(1));
        assert!(from_entry.stdout_summary.is_some());
    }

    #[test]
    fn digest_killed_has_failure() {
        let digest = ShellDigest::build(
            "sleep 100",
            &PathBuf::from("/tmp"),
            ShellStatus::Killed,
            None,
            Duration::from_secs(5),
            &empty_output(),
            &empty_output(),
        );
        assert!(digest.has_failures());
        assert!(digest
            .extracted_failures
            .iter()
            .any(|f| f.message.contains("killed by user")));
        assert_eq!(digest.status, ShellStatus::Killed);
    }

    #[test]
    fn digest_timed_out_has_failure() {
        let digest = ShellDigest::build(
            "slow cmd",
            &PathBuf::from("/tmp"),
            ShellStatus::TimedOut,
            None,
            Duration::from_secs(300),
            &empty_output(),
            &empty_output(),
        );
        assert!(digest.has_failures());
        assert!(digest
            .extracted_failures
            .iter()
            .any(|f| f.message.contains("timed out")));
    }

    #[test]
    fn digest_failed_to_start_has_failure() {
        let digest = ShellDigest::build(
            "nonexistent",
            &PathBuf::from("/tmp"),
            ShellStatus::FailedToStart,
            None,
            Duration::ZERO,
            &empty_output(),
            &empty_output(),
        );
        assert!(digest.has_failures());
        assert!(digest
            .extracted_failures
            .iter()
            .any(|f| f.message.contains("failed to start")));
    }

    #[test]
    fn digest_killed_render_shows_status() {
        let digest = ShellDigest::build(
            "cmd",
            &PathBuf::from("/tmp"),
            ShellStatus::Killed,
            None,
            Duration::from_secs(3),
            &empty_output(),
            &empty_output(),
        );
        let rendered = digest.render();
        assert!(rendered.contains("Status: killed"));
    }
}
