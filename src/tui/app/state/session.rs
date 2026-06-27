use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::session::Session;
use crate::tui::app::types::{HistoryEntry, SessionStatus};
use crate::tui::file_diff::FileDiffStatsResult;

/// State of the diff computation for a changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffStatsState {
    Pending {
        generation: u64,
    },
    Ready {
        generation: u64,
        additions: usize,
        deletions: usize,
    },
    Skipped {
        generation: u64,
        reason: &'static str,
    },
    Error {
        generation: u64,
        message: String,
    },
}

impl DiffStatsState {
    pub fn generation(&self) -> u64 {
        match self {
            DiffStatsState::Pending { generation }
            | DiffStatsState::Ready { generation, .. }
            | DiffStatsState::Skipped { generation, .. }
            | DiffStatsState::Error { generation, .. } => *generation,
        }
    }

    pub fn from_result(generation: u64, result: FileDiffStatsResult) -> Self {
        match result {
            FileDiffStatsResult::Ready {
                additions,
                deletions,
            } => DiffStatsState::Ready {
                generation,
                additions,
                deletions,
            },
            FileDiffStatsResult::Skipped { reason } => {
                DiffStatsState::Skipped { generation, reason }
            }
            FileDiffStatsResult::Error { message } => DiffStatsState::Error {
                generation,
                message,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub action: String,
    pub diff_preview: Vec<String>,
    pub diff_state: DiffStatsState,
}

pub struct SessionState {
    pub session: Option<Session>,
    pub session_status: SessionStatus,
    pub token_in: u64,
    pub token_out: u64,
    pub live_output_tokens: u64,
    pub live_output_text: String,
    pub reasoning_tokens: usize,
    pub cached_tokens: u64,
    pub history: VecDeque<HistoryEntry>,
    pub history_pos: Option<usize>,
    pub indexed_files: Arc<RwLock<Vec<String>>>,
    pub project_dir: String,
    pub last_edited_file: Option<String>,
    pub changed_files: Vec<ChangedFile>,
    pub mcp_servers: Vec<(String, String)>,
    pub context_tokens: usize,
    pub context_limit: usize,
    pub compaction_count: usize,
    pub rpm_limit: Option<u64>,
    pub tpm_limit: Option<u64>,
    pub rpm_remaining: Option<u64>,
    pub tpm_remaining: Option<u64>,
    pub permission_pending: bool,
    pub subagent_count: usize,
}
