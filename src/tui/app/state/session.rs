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
    /// Cached git sidebar state. Rendered directly from this struct so
    /// `render_sidebar` does not touch the filesystem or spawn git
    /// commands. Refreshed asynchronously via `start_refresh_git_sidebar`
    /// on session switch, project-dir change, file events (debounced),
    /// and periodically via the event loop.
    pub git_sidebar: GitSidebarState,
}

/// Cached sidebar git metadata. Stale generations are dropped at apply
/// time so a slow git refresh cannot overwrite a newer session/project
/// state.
#[derive(Debug, Clone, Default)]
pub struct GitSidebarState {
    pub root: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub last_refreshed: Option<std::time::Instant>,
    pub loading: bool,
    pub error: Option<String>,
    pub generation: u64,
}

#[allow(dead_code)]
impl GitSidebarState {
    /// Bump the generation counter for a new refresh attempt. Returns
    /// the new generation value to embed in the spawned task.
    pub fn begin_refresh(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.loading = true;
        self.error = None;
        self.generation
    }

    /// Apply a refresh result. Returns true when the result was
    /// current (i.e. matched the active generation). Stale results
    /// are dropped.
    pub fn apply_refresh(
        &mut self,
        generation: u64,
        root: Option<String>,
        branch: Option<String>,
        dirty: bool,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.root = root;
        self.branch = branch;
        self.dirty = dirty;
        self.last_refreshed = Some(std::time::Instant::now());
        self.loading = false;
        self.error = None;
        true
    }

    /// Apply a refresh error. Returns true when the error was current.
    pub fn apply_refresh_error(&mut self, generation: u64, error: String) -> bool {
        if generation != self.generation {
            return false;
        }
        self.loading = false;
        self.error = Some(error);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_refresh_increments_generation_and_sets_loading() {
        let mut s = GitSidebarState::default();
        assert_eq!(s.generation, 0);
        let g1 = s.begin_refresh();
        assert_eq!(g1, 1);
        assert_eq!(s.generation, 1);
        assert!(s.loading);
        assert!(s.error.is_none());
        let g2 = s.begin_refresh();
        assert_eq!(g2, 2);
        assert_eq!(s.generation, 2);
    }

    #[test]
    fn apply_refresh_with_current_generation_updates_fields() {
        let mut s = GitSidebarState::default();
        let g = s.begin_refresh();
        let applied = s.apply_refresh(
            g,
            Some("/tmp/repo".to_string()),
            Some("main".to_string()),
            true,
        );
        assert!(applied);
        assert_eq!(s.root.as_deref(), Some("/tmp/repo"));
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert!(s.dirty);
        assert!(!s.loading);
        assert!(s.last_refreshed.is_some());
    }

    #[test]
    fn apply_refresh_with_stale_generation_is_dropped() {
        let mut s = GitSidebarState::default();
        // First refresh begins and completes.
        let g1 = s.begin_refresh();
        s.apply_refresh(
            g1,
            Some("/a".to_string()),
            Some("feature".to_string()),
            true,
        );

        // Second refresh begins; bumps generation.
        let g2 = s.begin_refresh();
        assert_ne!(g1, g2);

        // Stale completion with g1 is dropped.
        let applied = s.apply_refresh(g1, Some("/b".to_string()), Some("main".to_string()), false);
        assert!(!applied);
        // State still reflects the first refresh.
        assert_eq!(s.root.as_deref(), Some("/a"));
        assert_eq!(s.branch.as_deref(), Some("feature"));
        assert!(s.dirty);

        // Current completion with g2 succeeds.
        let applied = s.apply_refresh(g2, None, None, false);
        assert!(applied);
        assert!(s.root.is_none());
        assert!(s.branch.is_none());
        assert!(!s.dirty);
    }

    #[test]
    fn apply_refresh_error_with_stale_generation_is_dropped() {
        let mut s = GitSidebarState::default();
        let g1 = s.begin_refresh();
        // Stale error arrives after a newer refresh has begun.
        let g2 = s.begin_refresh();
        let applied = s.apply_refresh_error(g1, "timeout".to_string());
        assert!(!applied);
        // Newer refresh error still applies.
        let applied = s.apply_refresh_error(g2, "io".to_string());
        assert!(applied);
        assert_eq!(s.error.as_deref(), Some("io"));
        assert!(!s.loading);
    }

    #[test]
    fn begin_refresh_saturates_at_u64_max() {
        let mut s = GitSidebarState {
            generation: u64::MAX,
            ..GitSidebarState::default()
        };
        let g = s.begin_refresh();
        assert_eq!(g, u64::MAX, "must saturate, not wrap");
        assert_eq!(s.generation, u64::MAX);
    }
}
