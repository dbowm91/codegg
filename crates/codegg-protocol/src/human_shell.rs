//! Bounded daemon-owned finite human shell protocol.

use serde::{Deserialize, Serialize};

pub const HUMAN_SHELL_PROTOCOL_VERSION: u32 = 1;
pub const HUMAN_SHELL_CAPABILITY: &str = "human_shell.v1";
pub const HUMAN_SHELL_MAX_COMMAND_BYTES: usize = 16 * 1024;
pub const HUMAN_SHELL_MAX_READ_BYTES: usize = 64 * 1024;
pub const HUMAN_SHELL_MAX_TIMEOUT_SECS: u64 = 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellRunCapabilitiesDto {
    pub supported: bool,
    pub protocol_version: u32,
    pub max_command_bytes: usize,
    pub max_read_bytes: usize,
    pub max_timeout_secs: u64,
    pub max_runs_per_principal: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HumanShellOriginDto {
    Bang,
    BangBang,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellRunCreateRequestDto {
    pub protocol_version: u32,
    /// Client-generated idempotency identity. It has no authority by itself.
    pub submission_id: String,
    pub project_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub origin: HumanShellOriginDto,
    pub confirmed_dangerous: bool,
    pub command: String,
    /// Bounded timeout selected from the human-shell configuration.
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HumanShellRunStatusDto {
    Running,
    Exited,
    TimedOut,
    Cancelled,
    Failed,
    Lost,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellOutputChunkDto {
    pub sequence: u64,
    pub stderr: bool,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellRunMetadataDto {
    pub run_id: String,
    pub project_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub execution_node: String,
    pub cwd: String,
    pub command: String,
    pub origin: HumanShellOriginDto,
    pub status: HumanShellRunStatusDto,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellRunReadDto {
    pub run_id: String,
    pub project_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub from_sequence: u64,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellRunCancelDto {
    pub run_id: String,
    pub project_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
}
