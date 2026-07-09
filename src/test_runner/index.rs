use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::test_runner::types::{FailureClass, TestReport, TestStatus};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const INDEX_VERSION: u32 = 1;
const MAX_INDEX_ENTRIES: usize = 100;
const MAX_FAILURE_ENTRIES_PER_RUN: usize = 10;
const MAX_MESSAGE_PREVIEW_BYTES: usize = 500;
const MAX_SUMMARY_BYTES: usize = 1000;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Error, Debug)]
pub enum TestIndexError {
    #[error("previous failures index not found at {0}")]
    IndexMissing(PathBuf),

    #[error("failed to read previous failures index at {0}: {1}")]
    IndexUnreadable(PathBuf, #[source] std::io::Error),

    #[error("malformed previous failures index at {0}: {1}")]
    IndexMalformed(PathBuf, #[source] serde_json::Error),

    #[error("no previous supervised test failure available to rerun. Run `/test`, `/test workspace`, or the `test` tool first")]
    NoPreviousFailures,

    #[error("indexed rerun command invalid: {0}")]
    CommandInvalid(String),
}

// ---------------------------------------------------------------------------
// Index types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunIndex {
    pub version: u32,
    pub updated_at: String,
    pub runs: Vec<TestRunIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunIndexEntry {
    pub run_id: String,
    pub created_at: String,
    pub status: TestStatus,
    pub failure_class: FailureClass,
    pub language: String,
    pub scope_label: String,
    pub cwd: PathBuf,
    pub argv: Vec<String>,
    pub summary: String,
    pub failures: Vec<TestFailureIndexEntry>,
    pub log_dir: PathBuf,
    pub stdout_log: Option<PathBuf>,
    pub stderr_log: Option<PathBuf>,
    pub report_json: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFailureIndexEntry {
    pub name: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message_preview: String,
    pub failure_class: FailureClass,
}

// ---------------------------------------------------------------------------
// Index file path helper
// ---------------------------------------------------------------------------

pub fn index_path(workdir: &Path) -> PathBuf {
    workdir.join(".codegg").join("test-runs").join("index.json")
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

pub fn load_index(workdir: &Path) -> Result<TestRunIndex, TestIndexError> {
    let path = index_path(workdir);
    if !path.exists() {
        return Err(TestIndexError::IndexMissing(path));
    }
    let data = std::fs::read_to_string(&path)
        .map_err(|e| TestIndexError::IndexUnreadable(path.clone(), e))?;
    let index: TestRunIndex =
        serde_json::from_str(&data).map_err(|e| TestIndexError::IndexMalformed(path, e))?;
    Ok(index)
}

// ---------------------------------------------------------------------------
// Writing (atomic with static lock)
// ---------------------------------------------------------------------------

static INDEX_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn index_lock() -> &'static tokio::sync::Mutex<()> {
    INDEX_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub async fn append_to_index(report: &TestReport, workdir: &Path) {
    let _guard = index_lock().lock().await;

    let mut index = match load_index(workdir) {
        Ok(idx) => idx,
        Err(_) => TestRunIndex {
            version: INDEX_VERSION,
            updated_at: String::new(),
            runs: Vec::new(),
        },
    };

    let entry = entry_from_report(report, workdir);
    index.runs.push(entry);

    // Bound: keep newest entries
    if index.runs.len() > MAX_INDEX_ENTRIES {
        let excess = index.runs.len() - MAX_INDEX_ENTRIES;
        index.runs.drain(..excess);
    }

    index.updated_at = chrono::Utc::now().to_rfc3339();

    let _ = write_index_atomic(workdir, &index);
}

fn write_index_atomic(workdir: &Path, index: &TestRunIndex) -> Result<(), std::io::Error> {
    let path = index_path(workdir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("json.tmp");

    let data = serde_json::to_string_pretty(index).unwrap_or_default();
    std::fs::write(&tmp_path, data)?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Entry construction from report
// ---------------------------------------------------------------------------

fn entry_from_report(report: &TestReport, _workdir: &Path) -> TestRunIndexEntry {
    let run_id = report
        .log_dir
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string());

    let created_at = chrono::Utc::now().to_rfc3339();

    let failure_class = compute_failure_class(report);

    let language = detect_language_from_argv(&report.argv);

    let failures: Vec<TestFailureIndexEntry> = report
        .failures
        .iter()
        .take(MAX_FAILURE_ENTRIES_PER_RUN)
        .map(|f| TestFailureIndexEntry {
            name: f.name.clone(),
            file: f.file.clone(),
            line: f.line,
            message_preview: truncate_utf8(&f.message, MAX_MESSAGE_PREVIEW_BYTES),
            failure_class: f.failure_class,
        })
        .collect();

    let summary = truncate_utf8(&report.summary, MAX_SUMMARY_BYTES);

    TestRunIndexEntry {
        run_id,
        created_at,
        status: report.status,
        failure_class,
        language,
        scope_label: report
            .scope_label
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        cwd: report.cwd.clone(),
        argv: report.argv.clone(),
        summary,
        failures,
        log_dir: report.log_dir.clone().unwrap_or_default(),
        stdout_log: report.stdout_log.clone(),
        stderr_log: report.stderr_log.clone(),
        report_json: report.log_dir.as_ref().map(|p| p.join("report.json")),
    }
}

fn compute_failure_class(report: &TestReport) -> FailureClass {
    if report.status == TestStatus::Passed {
        return FailureClass::Passed;
    }
    if let Some(ref timeout) = report.timeout {
        return match timeout.kind {
            crate::test_runner::types::TimeoutKind::WallClock => FailureClass::TimeoutWallClock,
            crate::test_runner::types::TimeoutKind::NoOutput => FailureClass::TimeoutNoOutput,
            crate::test_runner::types::TimeoutKind::NoProgress => FailureClass::TimeoutNoOutput,
        };
    }
    if !report.failures.is_empty() {
        return report.failures[0].failure_class;
    }
    match report.status {
        TestStatus::Failed => FailureClass::NonzeroExit,
        TestStatus::Error => FailureClass::SpawnError,
        _ => FailureClass::UnknownFailure,
    }
}

fn detect_language_from_argv(argv: &[String]) -> String {
    if argv.is_empty() {
        return "unknown".to_string();
    }
    match argv[0].as_str() {
        "cargo" => "rust".to_string(),
        "pytest" | "python" => "python".to_string(),
        "go" => "go".to_string(),
        "make" => "make".to_string(),
        "npm" | "pnpm" | "yarn" | "bun" => "node".to_string(),
        "zig" => "zig".to_string(),
        _ => "generic".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Resolution helpers
// ---------------------------------------------------------------------------

/// Check whether an index entry represents an actionable failure
/// (i.e. worth rerunning).
pub fn is_actionable_failure(entry: &TestRunIndexEntry) -> bool {
    matches!(entry.status, TestStatus::Failed | TestStatus::TimedOut)
}

/// Find the newest actionable failure entry whose cwd still exists
/// and whose argv is non-empty.
pub fn find_newest_actionable_failure<'a>(
    index: &'a TestRunIndex,
    request_workdir: &Path,
) -> Option<&'a TestRunIndexEntry> {
    index.runs.iter().rev().find(|e| {
        is_actionable_failure(e)
            && !e.argv.is_empty()
            && e.cwd.exists()
            && is_cwd_within_workdir(&e.cwd, request_workdir)
    })
}

/// Validate that indexed rerun command argv is safe to execute.
pub fn validate_indexed_rerun_command(
    argv: &[String],
    request_workdir: &Path,
    cwd: &Path,
) -> Result<(), TestIndexError> {
    if argv.is_empty() {
        return Err(TestIndexError::CommandInvalid(
            "empty argv in index entry".to_string(),
        ));
    }

    // Reject empty tokens
    for token in argv {
        if token.is_empty() {
            return Err(TestIndexError::CommandInvalid(
                "empty argv token in index entry".to_string(),
            ));
        }
    }

    // Validate cwd is under or equal to request workdir
    if !is_cwd_within_workdir(cwd, request_workdir) {
        return Err(TestIndexError::CommandInvalid(format!(
            "cwd '{}' is outside request workdir '{}'",
            cwd.display(),
            request_workdir.display(),
        )));
    }

    // Validate argv[0] is from known test executables generated by the resolver
    let valid_prefixes: &[&str] = &[
        "cargo", "pytest", "python", "go", "zig", "make", "npm", "pnpm", "yarn", "bun", "uv",
        "node",
    ];
    let first = &argv[0];
    let is_known = valid_prefixes.iter().any(|p| first == *p);
    if !is_known {
        return Err(TestIndexError::CommandInvalid(format!(
            "argv[0] '{}' is not a known test executable",
            first
        )));
    }

    Ok(())
}

fn is_cwd_within_workdir(cwd: &Path, workdir: &Path) -> bool {
    match (cwd.canonicalize(), workdir.canonicalize()) {
        (Ok(c), Ok(w)) => c == w || c.starts_with(&w),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        // Reserve 3 bytes for "..." suffix
        let target = max_bytes.saturating_sub(3);
        if target == 0 {
            return "...".to_string();
        }
        let mut end = target;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::types::{TestFailure, TestReport, TestStatus, TimeoutKind};
    use std::path::PathBuf;

    fn base_report(status: TestStatus) -> TestReport {
        TestReport {
            status,
            argv: vec!["cargo".into(), "test".into()],
            cwd: PathBuf::from("/workspace"),
            duration_ms: 12345,
            exit_code: Some(1),
            summary: "2 passed, 1 failed".into(),
            failures: vec![],
            timeout: None,
            log_dir: Some(PathBuf::from(
                "/workspace/.codegg/test-runs/20260708T123456Z-abc12345",
            )),
            stdout_log: Some(PathBuf::from(
                "/workspace/.codegg/test-runs/20260708T123456Z-abc12345/stdout.log",
            )),
            stderr_log: Some(PathBuf::from(
                "/workspace/.codegg/test-runs/20260708T123456Z-abc12345/stderr.log",
            )),
            output_truncated: false,
            scope_label: Some("auto-rust".to_string()),
            previous_run_id: None,
        }
    }

    #[test]
    fn index_load_missing_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_index(dir.path());
        assert!(matches!(result, Err(TestIndexError::IndexMissing(_))));
    }

    #[test]
    fn index_load_malformed_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let runs_dir = dir.path().join(".codegg").join("test-runs");
        std::fs::create_dir_all(&runs_dir).unwrap();
        std::fs::write(runs_dir.join("index.json"), "not json").unwrap();
        let result = load_index(dir.path());
        assert!(matches!(result, Err(TestIndexError::IndexMalformed(_, _))));
    }

    #[test]
    fn index_append_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let report = base_report(TestStatus::Failed);

        // We can't call append_to_index (async + needs tokio), so test the sync path
        let mut index = TestRunIndex {
            version: INDEX_VERSION,
            updated_at: String::new(),
            runs: Vec::new(),
        };
        let entry = entry_from_report(&report, dir.path());
        index.runs.push(entry);
        index.updated_at = chrono::Utc::now().to_rfc3339();

        let result = write_index_atomic(dir.path(), &index);
        assert!(result.is_ok());
        assert!(index_path(dir.path()).exists());
    }

    #[test]
    fn index_append_bounds_entries_to_max() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = TestRunIndex {
            version: INDEX_VERSION,
            updated_at: String::new(),
            runs: Vec::new(),
        };

        // Add more than MAX_INDEX_ENTRIES
        for _ in 0..MAX_INDEX_ENTRIES + 10 {
            let report = base_report(TestStatus::Failed);
            let entry = entry_from_report(&report, dir.path());
            index.runs.push(entry);
        }

        // Bound check
        if index.runs.len() > MAX_INDEX_ENTRIES {
            let excess = index.runs.len() - MAX_INDEX_ENTRIES;
            index.runs.drain(..excess);
        }

        assert_eq!(index.runs.len(), MAX_INDEX_ENTRIES);
    }

    #[test]
    fn index_entry_truncates_summary_and_failure_messages() {
        let dir = tempfile::tempdir().unwrap();
        let mut report = base_report(TestStatus::Failed);
        report.summary = "x".repeat(MAX_SUMMARY_BYTES + 100);
        report.failures.push(TestFailure {
            name: Some("test_long".into()),
            file: None,
            line: None,
            message: "y".repeat(MAX_MESSAGE_PREVIEW_BYTES + 100),
            failure_class: FailureClass::RustTestFailure,
        });

        let entry = entry_from_report(&report, dir.path());
        assert!(entry.summary.len() <= MAX_SUMMARY_BYTES);
        assert!(entry.summary.ends_with("..."));
        assert!(entry.failures[0].message_preview.len() <= MAX_MESSAGE_PREVIEW_BYTES);
        assert!(entry.failures[0].message_preview.ends_with("..."));
    }

    #[test]
    fn index_newest_actionable_failure_selected() {
        let dir = tempfile::tempdir().unwrap();

        // Create entries: first passed, second failed, third timed out
        let mut index = TestRunIndex {
            version: INDEX_VERSION,
            updated_at: String::new(),
            runs: vec![],
        };

        let mut passed = base_report(TestStatus::Passed);
        passed.argv = vec!["cargo".into(), "test".into()];
        passed.cwd = dir.path().to_path_buf();
        passed.log_dir = Some(dir.path().join("run1"));
        index.runs.push(entry_from_report(&passed, dir.path()));

        let mut failed = base_report(TestStatus::Failed);
        failed.argv = vec!["cargo".into(), "test".into()];
        failed.cwd = dir.path().to_path_buf();
        failed.log_dir = Some(dir.path().join("run2"));
        index.runs.push(entry_from_report(&failed, dir.path()));

        let mut timed_out = base_report(TestStatus::TimedOut);
        timed_out.exit_code = None;
        timed_out.argv = vec!["cargo".into(), "test".into()];
        timed_out.cwd = dir.path().to_path_buf();
        timed_out.log_dir = Some(dir.path().join("run3"));
        timed_out.timeout = Some(crate::test_runner::types::TestTimeout {
            kind: TimeoutKind::WallClock,
            elapsed_ms: 300000,
            last_output: None,
        });
        index.runs.push(entry_from_report(&timed_out, dir.path()));

        let found = find_newest_actionable_failure(&index, dir.path());
        assert!(found.is_some());
        let entry = found.unwrap();
        assert_eq!(entry.run_id, "run3");
        assert_eq!(entry.status, TestStatus::TimedOut);
    }

    #[test]
    fn index_skips_passed_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = TestRunIndex {
            version: INDEX_VERSION,
            updated_at: String::new(),
            runs: vec![],
        };

        let mut passed = base_report(TestStatus::Passed);
        passed.cwd = dir.path().to_path_buf();
        passed.log_dir = Some(dir.path().join("run1"));
        index.runs.push(entry_from_report(&passed, dir.path()));

        let found = find_newest_actionable_failure(&index, dir.path());
        assert!(found.is_none());
    }

    #[test]
    fn index_skips_invalid_empty_argv() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = TestRunIndex {
            version: INDEX_VERSION,
            updated_at: String::new(),
            runs: vec![],
        };

        let mut failed = base_report(TestStatus::Failed);
        failed.argv = vec![];
        failed.cwd = dir.path().to_path_buf();
        failed.log_dir = Some(dir.path().join("run1"));
        index.runs.push(entry_from_report(&failed, dir.path()));

        let found = find_newest_actionable_failure(&index, dir.path());
        assert!(found.is_none());
    }

    #[test]
    fn index_skips_missing_cwd() {
        let mut index = TestRunIndex {
            version: INDEX_VERSION,
            updated_at: String::new(),
            runs: vec![],
        };

        let mut failed = base_report(TestStatus::Failed);
        failed.argv = vec!["cargo".into(), "test".into()];
        failed.cwd = PathBuf::from("/nonexistent/path");
        failed.log_dir = Some(PathBuf::from("/nonexistent/path/.codegg/test-runs/run1"));
        index
            .runs
            .push(entry_from_report(&failed, Path::new("/nonexistent")));

        let found = find_newest_actionable_failure(&index, Path::new("/nonexistent"));
        assert!(found.is_none());
    }

    #[test]
    fn validate_indexed_rerun_command_rejects_empty_argv() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_indexed_rerun_command(&[], dir.path(), dir.path());
        assert!(matches!(result, Err(TestIndexError::CommandInvalid(_))));
    }

    #[test]
    fn validate_indexed_rerun_command_rejects_unknown_executable() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_indexed_rerun_command(
            &["curl".into(), "http://evil.com".into()],
            dir.path(),
            dir.path(),
        );
        assert!(matches!(result, Err(TestIndexError::CommandInvalid(_))));
    }

    #[test]
    fn validate_indexed_rerun_command_rejects_cwd_outside_workdir() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_indexed_rerun_command(
            &["cargo".into(), "test".into()],
            dir.path(),
            &PathBuf::from("/outside"),
        );
        assert!(matches!(result, Err(TestIndexError::CommandInvalid(_))));
    }

    #[test]
    fn validate_indexed_rerun_command_accepts_valid_command() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_indexed_rerun_command(
            &["cargo".into(), "test".into()],
            dir.path(),
            dir.path(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn is_actionable_failure_classifies_correctly() {
        let mut entry = TestRunIndexEntry {
            run_id: "test".into(),
            created_at: String::new(),
            status: TestStatus::Passed,
            failure_class: FailureClass::Passed,
            language: "rust".into(),
            scope_label: "auto-rust".into(),
            cwd: PathBuf::from("."),
            argv: vec!["cargo".into(), "test".into()],
            summary: String::new(),
            failures: vec![],
            log_dir: PathBuf::from("."),
            stdout_log: None,
            stderr_log: None,
            report_json: None,
        };

        entry.status = TestStatus::Failed;
        assert!(is_actionable_failure(&entry));

        entry.status = TestStatus::TimedOut;
        assert!(is_actionable_failure(&entry));

        entry.status = TestStatus::Passed;
        assert!(!is_actionable_failure(&entry));

        entry.status = TestStatus::Error;
        assert!(!is_actionable_failure(&entry));

        entry.status = TestStatus::Cancelled;
        assert!(!is_actionable_failure(&entry));
    }

    #[test]
    fn truncate_utf8_does_not_split_char_boundary() {
        assert_eq!(truncate_utf8("hello", 3), "...");
        assert_eq!(truncate_utf8("hello", 5), "hello");
        assert_eq!(truncate_utf8("hello", 10), "hello");
        assert_eq!(truncate_utf8("αβγ", 2), "...");
        assert_eq!(truncate_utf8("αβγ", 4), "...");
        assert_eq!(truncate_utf8("αβγ", 5), "α...");
        assert_eq!(truncate_utf8("αβγ", 7), "αβγ");
    }

    #[test]
    fn detect_language_from_argv_works() {
        assert_eq!(detect_language_from_argv(&["cargo".into()]), "rust");
        assert_eq!(detect_language_from_argv(&["pytest".into()]), "python");
        assert_eq!(detect_language_from_argv(&["go".into()]), "go");
        assert_eq!(detect_language_from_argv(&["make".into()]), "make");
        assert_eq!(detect_language_from_argv(&["npm".into()]), "node");
        assert_eq!(detect_language_from_argv(&["zig".into()]), "zig");
        assert_eq!(detect_language_from_argv(&[]), "unknown");
    }

    #[test]
    fn index_path_ends_with_correct_name() {
        let path = index_path(Path::new("/workspace"));
        assert_eq!(
            path,
            PathBuf::from("/workspace/.codegg/test-runs/index.json")
        );
    }
}
