use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::session::Session;
use crate::tui::app::types::{HistoryEntry, SessionStatus};

#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub action: String,
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
