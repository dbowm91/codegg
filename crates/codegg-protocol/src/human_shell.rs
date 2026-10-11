//! Wire contracts for durable, explicitly authorized human-shell promotion.

use serde::{Deserialize, Serialize};

pub const HUMAN_SHELL_PROMOTION_CAPABILITY_VERSION: u32 = 1;
pub const HUMAN_SHELL_PROMOTION_MAX_BYTES: usize = 32 * 1024;
pub const HUMAN_SHELL_PROMOTION_MAX_PENDING: usize = 16;
pub const HUMAN_SHELL_PROMOTION_TTL_MS: i64 = 60 * 60 * 1000;
pub const HUMAN_SHELL_PROMOTION_RETENTION_MS: i64 = 24 * 60 * 60 * 1000;

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
