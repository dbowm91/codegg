use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TestScope {
    Auto,
    Workspace,
    Changed,
    Package(String),
    File(PathBuf),
    PreviousFailures,
    CustomCommand(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestLanguage {
    Rust,
    Python,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TestStatus {
    Passed,
    Failed,
    TimedOut,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TimeoutKind {
    WallClock,
    NoOutput,
    NoProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FailureClass {
    Passed,
    RustTestFailure,
    RustPanic,
    RustCompileError,
    RustDoctestFailure,
    PytestFailure,
    PytestError,
    PytestCollectionError,
    NonzeroExit,
    TimeoutWallClock,
    TimeoutNoOutput,
    SpawnError,
    UnknownFailure,
}

impl fmt::Display for FailureClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passed => write!(f, "passed"),
            Self::RustTestFailure => write!(f, "rust_test_failure"),
            Self::RustPanic => write!(f, "rust_panic"),
            Self::RustCompileError => write!(f, "rust_compile_error"),
            Self::RustDoctestFailure => write!(f, "rust_doctest_failure"),
            Self::PytestFailure => write!(f, "pytest_failure"),
            Self::PytestError => write!(f, "pytest_error"),
            Self::PytestCollectionError => write!(f, "pytest_collection_error"),
            Self::NonzeroExit => write!(f, "nonzero_exit"),
            Self::TimeoutWallClock => write!(f, "timeout_wall_clock"),
            Self::TimeoutNoOutput => write!(f, "timeout_no_output"),
            Self::SpawnError => write!(f, "spawn_error"),
            Self::UnknownFailure => write!(f, "unknown_failure"),
        }
    }
}

impl FailureClass {
    pub fn from_display_str(s: &str) -> Option<Self> {
        match s {
            "passed" => Some(Self::Passed),
            "rust_test_failure" => Some(Self::RustTestFailure),
            "rust_panic" => Some(Self::RustPanic),
            "rust_compile_error" => Some(Self::RustCompileError),
            "rust_doctest_failure" => Some(Self::RustDoctestFailure),
            "pytest_failure" => Some(Self::PytestFailure),
            "pytest_error" => Some(Self::PytestError),
            "pytest_collection_error" => Some(Self::PytestCollectionError),
            "nonzero_exit" => Some(Self::NonzeroExit),
            "timeout_wall_clock" => Some(Self::TimeoutWallClock),
            "timeout_no_output" => Some(Self::TimeoutNoOutput),
            "spawn_error" => Some(Self::SpawnError),
            "unknown_failure" => Some(Self::UnknownFailure),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::RustTestFailure => "rust_test_failure",
            Self::RustPanic => "rust_panic",
            Self::RustCompileError => "rust_compile_error",
            Self::RustDoctestFailure => "rust_doctest_failure",
            Self::PytestFailure => "pytest_failure",
            Self::PytestError => "pytest_error",
            Self::PytestCollectionError => "pytest_collection_error",
            Self::NonzeroExit => "nonzero_exit",
            Self::TimeoutWallClock => "timeout_wall_clock",
            Self::TimeoutNoOutput => "timeout_no_output",
            Self::SpawnError => "spawn_error",
            Self::UnknownFailure => "unknown_failure",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestRunRequest {
    pub scope: TestScope,
    pub workdir: PathBuf,
    pub timeout_secs: Option<u64>,
    pub stall_timeout_secs: Option<u64>,
    pub max_report_bytes: Option<usize>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedTestCommand {
    pub language: TestLanguage,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub scope_label: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestFailure {
    pub name: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    #[serde(rename = "failure_class")]
    pub failure_class: FailureClass,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestTimeout {
    pub kind: TimeoutKind,
    pub elapsed_ms: u64,
    pub last_output: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestReport {
    pub status: TestStatus,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    pub summary: String,
    pub failures: Vec<TestFailure>,
    pub timeout: Option<TestTimeout>,
    pub log_dir: Option<PathBuf>,
    pub stdout_log: Option<PathBuf>,
    pub stderr_log: Option<PathBuf>,
    pub output_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_run_id: Option<String>,
}

/// Snapshot types for test lifecycle events.
#[derive(Debug, Clone)]
pub struct TestRunStartedSnapshot {
    pub session_id: String,
    pub job_id: String,
    pub command: String,
    pub cwd: String,
}

#[derive(Debug, Clone)]
pub struct TestRunProgressSnapshot {
    pub session_id: String,
    pub job_id: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct TestRunCompletedSnapshot {
    pub session_id: String,
    pub job_id: String,
    pub status: String,
    pub summary: String,
    pub log_dir: Option<String>,
}

/// Sink for test lifecycle events. Implementations can publish to an
/// event bus, log events, or aggregate progress. The runner calls these
/// methods at key lifecycle points; implementations should be cheap
/// (the runner does not await return values).
pub trait TestEventSink: Send + Sync {
    fn started(&self, snapshot: TestRunStartedSnapshot);
    fn progress(&self, snapshot: TestRunProgressSnapshot);
    fn completed(&self, snapshot: TestRunCompletedSnapshot);
}
