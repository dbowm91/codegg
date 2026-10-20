use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::protocol::core::UserPromptQueueEntryDto;
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

/// A tool call the TUI has seen start but not yet seen finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningTool {
    pub tool_id: String,
    pub name: String,
    pub started: std::time::Instant,
}

impl RunningTool {
    pub fn new(tool_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            tool_id: tool_id.into(),
            name: name.into(),
            started: std::time::Instant::now(),
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }
}

pub struct SessionState {
    pub session: Option<Session>,
    pub session_status: SessionStatus,
    /// When the current `Working` status was entered, and when the TUI last saw
    /// any turn event for it.
    ///
    /// The TUI enters `Working` optimistically when a prompt is submitted, so
    /// only a `TurnCompleted` / `TurnFailed` / `Error` event clears it. If the
    /// daemon never delivers a terminal event — a turn that stalls upstream, or
    /// a core that goes away mid-turn — nothing else ever clears the status and
    /// the spinner runs forever with no error text anywhere. This timestamp is
    /// the stall watchdog's input; it is refreshed by every incoming turn event.
    pub working_since: Option<std::time::Instant>,
    /// The tool call currently in flight, if any.
    ///
    /// A long tool call is silent by construction — `bash` delivers its
    /// output only on completion, so a multi-minute build emits no events at
    /// all. The stall watchdog measures silence, which made it kill healthy
    /// turns that were simply running several slow commands. Tracking the
    /// in-flight call lets the watchdog tell "working" apart from "wedged".
    pub running_tool: Option<RunningTool>,
    /// The daemon-issued id of the turn currently in flight.
    ///
    /// `CoreRequest::TurnCancel` addresses a turn by `(session_id, turn_id)`
    /// and the daemon *rejects* a mismatch (`turn_id_mismatch`) rather than
    /// ignoring it, so stopping a turn is impossible without this id. It
    /// arrives on two wires — `CoreEvent::TurnStarted` and the
    /// `TurnAdmissionReceipt` returned by submit — and both used to be
    /// discarded, which is why nothing in the TUI could interrupt a turn.
    ///
    /// Bound from `TurnStarted`: that is the event-level truth, so it
    /// survives a frontend reconnect that joins a turn already running in the
    /// daemon. Cleared by every terminal turn event.
    pub active_turn_id: Option<String>,
    /// The user stopped the current turn, and has not resumed or replaced it.
    ///
    /// A cancelled turn arrives as an ordinary `TurnCompleted` with an
    /// unrecognised `stop_reason`, so status alone cannot distinguish "I
    /// stopped this" from "the agent finished". Without this flag the stopped
    /// state is invisible and `/start` has nothing to act on.
    pub stopped_turn: bool,
    /// The most recent turn failure, held until the agent resumes.
    ///
    /// A toast is transient by design and a transcript row is scroll state, so
    /// before this the only record of "the turn died because the provider was
    /// overloaded" was a message that vanished in seconds and a log line the
    /// user does not know exists. This is the one durable line the main buffer
    /// shows, because it is the surface with no status row of its own.
    ///
    /// Cleared when the agent resumes rather than on a timer: a user who is
    /// still reading the reason should not have it disappear underneath them,
    /// and a user who has started a new turn has already moved past it.
    pub last_error: Option<String>,
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
    /// M002a (ADR-0035): the daemon's live prompt-queue projection for this
    /// session, in daemon-owned ordinal order.
    ///
    /// Seeded empty and always replaced wholesale from `CoreResponse::
    /// UserPromptQueue`. It is a *projection*, never an authoritative copy: the
    /// TUI owns no queue state that could drift, and there is no local edit
    /// path. A failed load leaves the previous value intact rather than
    /// clearing it, so a transient transport error does not make a real
    /// backlog look empty.
    ///
    /// Entries carry their daemon-owned `status`. A `delivering` entry appears
    /// here and is **not** editable: it is live, a turn is in flight against it,
    /// and the daemon refuses an edit to it with
    /// `user_prompt_queue_entry_delivering`.
    pub user_prompt_queue: Vec<UserPromptQueueEntryDto>,
}

/// Git sidebar metadata passed from the background probe to the
/// cached state. Grouped into a struct to keep function signatures
/// concise.
#[derive(Debug, Clone, Default)]
pub struct GitSidebarInfo {
    pub root: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub staged_count: usize,
    pub unstaged_count: usize,
    pub untracked_count: usize,
    pub conflicted_count: usize,
    pub ahead: Option<i32>,
    pub behind: Option<i32>,
    /// Active operation family (`"merge"`, `"rebase"`, `"cherry-pick"`,
    /// `"revert"`, `"bisect"`, `"apply-mailbox"`, `"sequencer"`,
    /// `"unknown"`, or `None`).
    pub operation_state_label: Option<String>,
    /// Legal recovery actions for the current operation state
    /// (e.g. `["continue", "abort"]`).
    pub available_actions: Vec<String>,
    /// Conflicted paths from the typed conflict model. Empty when not
    /// in a conflicted state.
    pub conflicted_paths: Vec<String>,
}

/// Cached sidebar git metadata. Stale generations are dropped at apply
/// time so a slow git refresh cannot overwrite a newer session/project
/// state.
#[derive(Debug, Clone, Default)]
pub struct GitSidebarState {
    pub root: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub staged_count: usize,
    pub unstaged_count: usize,
    pub untracked_count: usize,
    pub conflicted_count: usize,
    pub ahead: Option<i32>,
    pub behind: Option<i32>,
    pub last_refreshed: Option<std::time::Instant>,
    pub loading: bool,
    pub error: Option<String>,
    pub generation: u64,
    /// Active operation family (Phase F). `None` when the repo is clean.
    pub operation_state_label: Option<String>,
    /// Legal recovery actions for the active operation.
    pub available_actions: Vec<String>,
    /// Conflicted paths from the typed conflict model. Empty when not
    /// in a conflicted state.
    pub conflicted_paths: Vec<String>,
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
    pub fn apply_refresh(&mut self, generation: u64, info: GitSidebarInfo) -> bool {
        if generation != self.generation {
            return false;
        }
        self.root = info.root;
        self.branch = info.branch;
        self.dirty = info.dirty;
        self.staged_count = info.staged_count;
        self.unstaged_count = info.unstaged_count;
        self.untracked_count = info.untracked_count;
        self.conflicted_count = info.conflicted_count;
        self.operation_state_label = info.operation_state_label;
        self.available_actions = info.available_actions;
        self.conflicted_paths = info.conflicted_paths;
        self.ahead = info.ahead;
        self.behind = info.behind;
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
            GitSidebarInfo {
                root: Some("/tmp/repo".to_string()),
                branch: Some("main".to_string()),
                dirty: true,
                staged_count: 2,
                unstaged_count: 3,
                untracked_count: 1,
                conflicted_count: 0,
                ahead: Some(5),
                behind: Some(1),
                operation_state_label: None,
                available_actions: Vec::new(),
                conflicted_paths: Vec::new(),
            },
        );
        assert!(applied);
        assert_eq!(s.root.as_deref(), Some("/tmp/repo"));
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert!(s.dirty);
        assert_eq!(s.staged_count, 2);
        assert_eq!(s.unstaged_count, 3);
        assert_eq!(s.untracked_count, 1);
        assert_eq!(s.conflicted_count, 0);
        assert_eq!(s.ahead, Some(5));
        assert_eq!(s.behind, Some(1));
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
            GitSidebarInfo {
                root: Some("/a".to_string()),
                branch: Some("feature".to_string()),
                dirty: true,
                staged_count: 1,
                unstaged_count: 2,
                untracked_count: 3,
                conflicted_count: 0,
                ahead: None,
                behind: None,
                operation_state_label: None,
                available_actions: Vec::new(),
                conflicted_paths: Vec::new(),
            },
        );

        // Second refresh begins; bumps generation.
        let g2 = s.begin_refresh();
        assert_ne!(g1, g2);

        // Stale completion with g1 is dropped.
        let applied = s.apply_refresh(
            g1,
            GitSidebarInfo {
                root: Some("/b".to_string()),
                branch: Some("main".to_string()),
                dirty: false,
                staged_count: 0,
                unstaged_count: 0,
                untracked_count: 0,
                conflicted_count: 0,
                ahead: None,
                behind: None,
                operation_state_label: None,
                available_actions: Vec::new(),
                conflicted_paths: Vec::new(),
            },
        );
        assert!(!applied);
        // State still reflects the first refresh.
        assert_eq!(s.root.as_deref(), Some("/a"));
        assert_eq!(s.branch.as_deref(), Some("feature"));
        assert!(s.dirty);

        // Current completion with g2 succeeds.
        let applied = s.apply_refresh(g2, GitSidebarInfo::default());
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

    #[test]
    fn default_git_sidebar_state_has_zero_counts() {
        let s = GitSidebarState::default();
        assert_eq!(s.staged_count, 0);
        assert_eq!(s.unstaged_count, 0);
        assert_eq!(s.untracked_count, 0);
        assert_eq!(s.conflicted_count, 0);
        assert!(s.ahead.is_none());
        assert!(s.behind.is_none());
    }
}
