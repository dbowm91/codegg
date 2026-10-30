//! Daemon-owned security review protocol contracts.

use serde::{Deserialize, Serialize};

/// The security review surface's own protocol version.
pub const SECURITY_REVIEW_PROTOCOL_VERSION: u32 = 1;

/// The bound on one run's rendered report, in bytes.
pub const SECURITY_REVIEW_MAX_REPORT_BYTES: usize = 64 * 1024;

/// Start one daemon-owned security review run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewStartDto {
    pub protocol_version: u32,
    pub project_id: String,
    pub workspace_id: String,
    pub session_id: String,
    /// The command's own argument; the daemon parses it.
    pub argument: String,
}

/// Read one security review run's daemon-owned state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewReadDto {
    pub protocol_version: u32,
    pub project_id: String,
    pub workspace_id: String,
    pub session_id: String,
    /// The run to read; the session's newest run when absent.
    #[serde(default)]
    pub run_id: Option<String>,
}

/// Where one security review run stands.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityReviewRunStatusDto {
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// One security review run, as the daemon holds it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewRunDto {
    pub run_id: String,
    pub turn_id: String,
    pub session_id: String,
    pub status: SecurityReviewRunStatusDto,
    #[serde(default)]
    pub report: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    pub finding_count: usize,
    pub prompt_count: usize,
    pub note_count: usize,
    pub enriched: bool,
    pub lsp_available: bool,
    #[serde(default)]
    pub completed_at_ms: Option<i64>,
}
