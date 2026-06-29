#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    None,
    Model,
    Agent,
    Session,
    Help,
    Tree,
    Theme,
    Question,
    Permission,
    Mcp,
    Keybind,
    Share,
    Import,
    Template,
    Connect,
    Context,
    Cost,
    Usage,
    Stats,
    Goto,
    Plan,
    Diff,
    Confirm,
    Review,
    ResearchBrowser,
    SecurityReview,
    SourcePreview,
    ShellShow,
    TaskList,
    WorktreeList,
    GoalShow,
    MemoryResults,
    DoctorReport,
}

impl Dialog {
    pub fn is_open(&self) -> bool {
        matches!(
            self,
            Self::Model
                | Self::Agent
                | Self::Session
                | Self::Help
                | Self::Tree
                | Self::Theme
                | Self::Question
                | Self::Permission
                | Self::Mcp
                | Self::Keybind
                | Self::Share
                | Self::Import
                | Self::Template
                | Self::Connect
                | Self::Context
                | Self::Cost
                | Self::Usage
                | Self::Stats
                | Self::Goto
                | Self::Plan
                | Self::Diff
                | Self::Confirm
                | Self::Review
                | Self::ResearchBrowser
                | Self::SecurityReview
                | Self::SourcePreview
                | Self::ShellShow
                | Self::TaskList
                | Self::WorktreeList
                | Self::GoalShow
                | Self::MemoryResults
                | Self::DoctorReport
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TuiMsg {
    SubmitPrompt,
    NavigateUp,
    NavigateDown,
    NavigateLeft,
    NavigateRight,
    CycleAgent,
    OpenModelDialog,
    OpenAgentDialog,
    OpenSessionDialog,
    OpenHelpDialog,
    OpenTreeDialog,
    OpenThemeDialog,
    OpenShareDialog,
    OpenImportDialog,
    OpenDiffDialog {
        old_content: Box<str>,
        new_content: Box<str>,
        title: Box<str>,
    },
    SelectModel {
        model: String,
    },
    SelectAgent {
        agent_name: String,
    },
    SelectSession(Box<Session>),
    SubmitConnect,
    ConnectConfigured {
        provider_name: String,
        env_var: Option<String>,
        api_key: Option<String>,
    },
    CloseDialog,
    ToggleSidebar,
    ToggleFullscreen,
    ToggleReasoning,
    ToggleTts,
    CycleModelForward,
    CycleModelBackward,
    ClearSession,
    NewSession,
    CloseSession,
    CharInput(char),
    Backspace,
    Delete,
    CursorLeft,
    CursorRight,
    CursorHome,
    CursorEnd,
    PageUp,
    PageDown,
    Search,
    SearchNext,
    SearchPrev,
    ClearSearch,
    FocusPrompt,
    StashPrompt,
    RestorePrompt,
    CopyMessage,
    Quit,
    ExternalEditor,
    UndoDelete,
    /// Legacy alias for `ThemeCommit`. Kept so any in-tree call sites
    /// that still send `SelectTheme` continue to work; new code should
    /// prefer the `ThemePreviewChanged` / `ThemeCommit` / `ThemeRevert`
    /// trio.
    SelectTheme {
        theme_name: String,
    },
    /// Live-preview a theme in the running UI without persisting it. The
    /// app updates its current `Theme` to match; nothing is written to
    /// the database. Used while the user navigates the theme picker.
    ThemePreviewChanged {
        theme_id: String,
    },
    /// Persist a theme (DB row + config mirror) and close the picker.
    /// Distinct from `SelectTheme` so live preview can keep emitting
    /// previews until the user explicitly commits.
    ThemeCommit {
        theme_id: String,
    },
    /// Revert the running UI to the last committed theme. Used by Esc
    /// and by the dialog-close revert path.
    ThemeRevert,
    SubmitPermission {
        choice_index: usize,
    },
    SubmitQuestionAnswers {
        answers_json: String,
    },
    SelectTreeSession {
        session_id: String,
    },
    ForkTreeSession {
        session_id: String,
    },
    ForkSession {
        session_id: String,
    },
    SubmitImportPreview,
    ConfirmImport,
    SelectTemplate {
        key: String,
        template: Box<SessionTemplate>,
    },
    GotoMessage {
        index: usize,
    },
    CopyShareUrl,
    McpAction {
        server_name: String,
        action: String,
    },
    KeybindChanged {
        action: String,
        binding: String,
    },
    ConfirmDeleteSession {
        session_id: String,
    },
    ConfirmArchiveSession {
        session_id: String,
        unarchive: bool,
    },
    ConfirmBulkDelete {
        count: usize,
        session_ids: Vec<String>,
    },
    ConfirmBulkArchive {
        count: usize,
        unarchive: bool,
        session_ids: Vec<String>,
    },
    ConfirmResult(Option<bool>),
    ReviewOpenDiff {
        path: String,
    },
    ResearchOpenRun {
        run_id: String,
    },
    ResearchRefreshRuns,
    ResearchLoadSection {
        run_id: String,
        section: String,
    },
    /// Copy a file path to the clipboard and surface a toast.
    /// Emitted by the security review panel's "jump to file" action;
    /// the file is never opened or mutated.
    SecurityReviewJump {
        path: String,
        line: Option<u32>,
    },
    OpenSourcePreview {
        path: PathBuf,
        line: Option<u32>,
        origin_label: Option<String>,
    },
    ShellInclude {
        id: String,
        mode: String,
    },
    ShellAsk {
        id: String,
    },
    ShellRerun {
        id: String,
    },
    ShellKill {
        id: String,
    },
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub text: String,
    pub score: f64,
    pub last_used: i64,
    pub frequency: u32,
}

impl HistoryEntry {
    pub fn new(text: String) -> Self {
        let now = now_ms();
        Self {
            text,
            score: 1.0,
            last_used: now,
            frequency: 1,
        }
    }

    pub fn touch(&mut self) {
        self.frequency += 1;
        self.last_used = now_ms();
        self.recalc_score();
    }

    fn recalc_score(&mut self) {
        let now = now_ms();
        let age_hours = ((now - self.last_used) as f64) / 3600000.0;
        let recency = 1.0 / (1.0 + age_hours);
        let freq = (self.frequency as f64).ln() + 1.0;
        self.score = recency * freq;
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

use std::path::PathBuf;

use crate::config::schema::SessionTemplate;
use crate::session::models::Session;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct TodoEntry {
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Idle,
    Working,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompletionType {
    Slash,
    File,
    Agent,
}
