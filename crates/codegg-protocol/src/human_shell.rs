//! Wire contracts for durable, explicitly authorized human-shell promotion.

use serde::{Deserialize, Serialize};

pub const HUMAN_SHELL_PROMOTION_CAPABILITY_VERSION: u32 = 1;
pub const HUMAN_SHELL_PROMOTION_MAX_BYTES: usize = 32 * 1024;
pub const HUMAN_SHELL_PROMOTION_MAX_PENDING: usize = 16;
pub const HUMAN_SHELL_PROMOTION_TTL_MS: i64 = 60 * 60 * 1000;
pub const HUMAN_SHELL_PROMOTION_RETENTION_MS: i64 = 24 * 60 * 60 * 1000;

pub const HUMAN_SHELL_RUN_PROTOCOL_VERSION: u32 = 1;
pub const HUMAN_SHELL_RUN_MAX_COMMAND_BYTES: usize = 64 * 1024;
pub const HUMAN_SHELL_RUN_MAX_CWD_BYTES: usize = 4096;
pub const HUMAN_SHELL_RUN_MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub const HUMAN_SHELL_RUN_MAX_READ_BYTES: usize = 64 * 1024;
pub const HUMAN_SHELL_RUN_MAX_TIMEOUT_SECS: u64 = 300;
pub const HUMAN_SHELL_RUN_MAX_ACTIVE: usize = 4;
pub const HUMAN_SHELL_RUN_MAX_RETAINED: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellRunCapabilitiesDto {
    pub version: u32,
    pub max_command_bytes: usize,
    pub max_output_bytes: usize,
    pub max_read_bytes: usize,
    pub max_timeout_secs: u64,
}

impl HumanShellRunCapabilitiesDto {
    pub fn current() -> Self {
        Self {
            version: HUMAN_SHELL_RUN_PROTOCOL_VERSION,
            max_command_bytes: HUMAN_SHELL_RUN_MAX_COMMAND_BYTES,
            max_output_bytes: HUMAN_SHELL_RUN_MAX_OUTPUT_BYTES,
            max_read_bytes: HUMAN_SHELL_RUN_MAX_READ_BYTES,
            max_timeout_secs: HUMAN_SHELL_RUN_MAX_TIMEOUT_SECS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellRunCreateDto {
    pub protocol_version: u32,
    pub idempotency_key: String,
    pub workspace_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Relative to the registered workspace root. Absolute paths are refused.
    #[serde(default)]
    pub cwd_relative: Option<String>,
    pub command: String,
    pub timeout_secs: u64,
}

impl HumanShellRunCreateDto {
    pub fn validate(&self) -> bool {
        self.protocol_version == HUMAN_SHELL_RUN_PROTOCOL_VERSION
            && !self.idempotency_key.is_empty()
            && self.idempotency_key.len() <= 128
            && !self.workspace_id.is_empty()
            && self.workspace_id.len() <= 128
            && self
                .session_id
                .as_ref()
                .is_none_or(|id| !id.is_empty() && id.len() <= 128)
            && self.cwd_relative.as_ref().is_none_or(|cwd| {
                !cwd.is_empty()
                    && cwd.len() <= HUMAN_SHELL_RUN_MAX_CWD_BYTES
                    && !std::path::Path::new(cwd).is_absolute()
                    && !cwd.contains('\0')
            })
            && !self.command.trim().is_empty()
            && self.command.len() <= HUMAN_SHELL_RUN_MAX_COMMAND_BYTES
            && !self.command.contains('\0')
            && (1..=HUMAN_SHELL_RUN_MAX_TIMEOUT_SECS).contains(&self.timeout_secs)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HumanShellRunStreamDto {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellRunOutputDto {
    pub sequence: u64,
    pub stream: HumanShellRunStreamDto,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HumanShellRunStatusDto {
    Starting,
    Running,
    Exited,
    TimedOut,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellRunSnapshotDto {
    pub run_id: String,
    pub workspace_id: String,
    pub status: HumanShellRunStatusDto,
    pub exit_code: Option<i32>,
    pub next_cursor: u64,
    pub oldest_cursor: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellPromotionCapabilitiesDto {
    pub version: u32,
    pub max_projection_bytes: usize,
    pub max_pending_per_session: usize,
    pub ttl_ms: i64,
    pub retention_ms: i64,
}

impl HumanShellPromotionCapabilitiesDto {
    pub fn current() -> Self {
        Self {
            version: HUMAN_SHELL_PROMOTION_CAPABILITY_VERSION,
            max_projection_bytes: HUMAN_SHELL_PROMOTION_MAX_BYTES,
            max_pending_per_session: HUMAN_SHELL_PROMOTION_MAX_PENDING,
            ttl_ms: HUMAN_SHELL_PROMOTION_TTL_MS,
            retention_ms: HUMAN_SHELL_PROMOTION_RETENTION_MS,
        }
    }
}

/// Client-produced projection after the mandatory redaction hook. Session,
/// project, workspace, and principal bindings are derived by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellPromotionStageDto {
    pub promotion_id: String,
    pub source_command_id: String,
    pub projection: String,
    pub source_exit_code: Option<i32>,
    pub source_truncated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HumanShellPromotionStateDto {
    Pending,
    Reserved,
    Consumed,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanShellPromotionDto {
    pub promotion_id: String,
    pub source_command_id: String,
    /// Expired/consumed projections are cleared and represented as `None`.
    pub projection: Option<String>,
    pub projection_bytes: usize,
    pub source_exit_code: Option<i32>,
    pub source_truncated: bool,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    pub state: HumanShellPromotionStateDto,
    pub consumed_turn_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HumanShellPromotionStageOutcomeDto {
    Staged,
    Duplicate,
}
