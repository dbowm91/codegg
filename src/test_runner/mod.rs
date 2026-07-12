pub mod bus_sink;
pub mod custom;
pub mod index;
pub mod parse;
pub mod projection;
pub mod report;
pub mod resolve;
pub mod runner;
pub mod types;

pub use bus_sink::BusEventSink;
pub use index::{TestIndexError, TestRunIndex, TestRunIndexEntry};
pub use parse::{failure_class_summary, ingest_stderr_line, ingest_stdout_line, TestParseState};
pub use projection::test_report_to_projection;
pub use report::{format_test_report, format_test_report_with_cap, DEFAULT_MAX_REPORT_BYTES};
pub use resolve::{resolve_test_command, TestResolveError};
pub use runner::{resolve_and_run_test, run_resolved_test, DelegatedTestRun, TestRunError};
pub use types::{
    FailureClass, ResolvedTestCommand, TestEventSink, TestReport, TestRunCompletedSnapshot,
    TestRunProgressSnapshot, TestRunRequest, TestRunStartedSnapshot, TestScope, TestStatus,
    TestTimeout, TimeoutKind,
};
