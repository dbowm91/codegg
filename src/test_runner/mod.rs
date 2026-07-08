pub mod parse;
pub mod report;
pub mod resolve;
pub mod runner;
pub mod types;

pub mod custom;

pub use parse::{failure_class_summary, ingest_stderr_line, ingest_stdout_line, TestParseState};
pub use report::format_test_report;
pub use resolve::{resolve_test_command, TestResolveError};
pub use runner::{resolve_and_run_test, run_resolved_test, TestRunError};
pub use types::{
    FailureClass, ResolvedTestCommand, TestEventSink, TestReport, TestRunCompletedSnapshot,
    TestRunProgressSnapshot, TestRunRequest, TestRunStartedSnapshot, TestScope, TestStatus,
    TestTimeout, TimeoutKind,
};
