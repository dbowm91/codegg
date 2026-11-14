//! One constructible instance per dialog surface, built from public API only.
//!
//! ## What the fixture table is for
//!
//! I1, I2 and I3a are only worth anything if they run over the surfaces the
//! frame pass actually paints. This module is that inventory: a fresh
//! component per render (no shared state, so tests stay independent under
//! one-process-per-test), named by its `DialogType`, which is the name the
//! baseline manifest records.
//!
//! ## Two tables, and which invariant reads which
//!
//! [`dialogs()`] is the table I1, I2, I3a and I5 read. It is deliberately
//! **empty of content**: a colour measurement is a property of the chrome, and
//! a fixture that carried rows would re-bank `[contrast]`, which another
//! milestone owns.
//!
//! [`dialogs_with_content()`] is the table **I4 only** reads. A key that acts
//! on a selection cannot act when there is no selection to move, so without
//! seeded rows the measurement reports correct code as broken. The two tables
//! differ in exactly the surfaces listed in [`CONTENT_MAKERS`].
//!
//! Every content fixture applies the same two rules, so a reader can predict
//! the state a key is measured on: rows exist ([`LIST_ROWS`] of them), and the
//! cursor starts mid-list ([`PARKED_ROW`]). A cursor parked on row 0 makes `↑`
//! and `k` inert by construction — an honest boundary for a user at the top,
//! but not the state the advertisement is about.
//!
//! The two scroll-only surfaces (see [`park_scrolled`]) have no cursor, so
//! their view is moved down the same way a reader moves it.
//!
//! ## What is deliberately absent, and why
//!
//! The table is a *fixture* table, not a second registry, so an absent
//! surface is a reported limitation of this harness rather than a silent
//! hole. Three surfaces cannot be built without live domain state, and
//! building that state would put a run store, a credential or a projection
//! inside a render invariant:
//!
//! * `RunDetail` — `RunDetailDialog::new` takes a `RunDetailView`, which is a
//!   whole run-store record (cell, invocation, permissions, policy,
//!   artifacts, projection, changes).
//! * `TriggerSecret` and `TeamTokenSecret` — both are minted only by
//!   `from_secret` from a one-time credential the daemon issues.
//! * `SourcePreview` — `SourcePreviewDialog::new` reads the previewed file,
//!   so rendering it would make a render invariant depend on a file on disk.
//!
//! `DialogType::None` is not a surface. `CommandPalette` reports
//! `DialogType::Context`, so it is the completion overlay rather than a
//! mountable stub and would duplicate the `Context` fixture.

use std::path::PathBuf;
use std::sync::Arc;

use codegg::agent::Agent;
use codegg::config::schema::SessionTemplate;
use codegg::permission::PermissionRequest;
use codegg::protocol::core::{ChatMessageDto, UserPromptQueueEntryDto};
use codegg::research::service::ResearchRunSummary;
use codegg::research::types::{ResearchDepth, ResearchMode, ResearchRunCounts, RunStatus};
use codegg::security::workflow::{
    SecurityConfidence, SecurityReviewCommandArgs, SecurityReviewFinding, SecurityReviewOutput,
    SecurityReviewReceipt, SecuritySeverity,
};
use codegg::session::Session;
use codegg::theme::ThemeRegistry;
use codegg::tui::app::state::chat::ChatStatus;
use codegg::tui::app::state::file_tree::{FileTreeStatus, FileTreeStructure};
use codegg::tui::app::state::panel_hotkeys::PanelHotkeys;
use codegg::tui::app::state::work_orders::TaskScheduleDraft;
use codegg::tui::components::component::{Component, DialogType};
use codegg::tui::components::dialogs::agent::AgentDialog;
use codegg::tui::components::dialogs::chat::{ChatDialog, ChatSnapshot};
use codegg::tui::components::dialogs::confirm::ConfirmDialog;
use codegg::tui::components::dialogs::connect::{ConnectDialog, ProviderInfo};
use codegg::tui::components::dialogs::connection_selection::ConnectionSelectionDialog;
use codegg::tui::components::dialogs::diff::DiffDialog;
use codegg::tui::components::dialogs::files::FilesDialog;
use codegg::tui::components::dialogs::goto::GotoDialog;
use codegg::tui::components::dialogs::help::HelpDialog;
use codegg::tui::components::dialogs::import::ImportDialog;
use codegg::tui::components::dialogs::info::{InfoDialog, InfoType};
use codegg::tui::components::dialogs::job_detail::JobDetailDialog;
use codegg::tui::components::dialogs::keybind::KeybindDialog;
use codegg::tui::components::dialogs::mcp::{McpDialog, McpServerInfo, McpToolInfo};
use codegg::tui::components::dialogs::model::ModelDialog;
use codegg::tui::components::dialogs::panel_manager::PanelManagerDialog;
use codegg::tui::components::dialogs::permission::PermissionDialog;
use codegg::tui::components::dialogs::plan::PlanDialog;
use codegg::tui::components::dialogs::plugin::PluginDialog;
use codegg::tui::components::dialogs::policy_selector::{PolicySelectorDialog, PolicySelectorKind};
use codegg::tui::components::dialogs::project_picker::ProjectPickerDialog;
use codegg::tui::components::dialogs::question::{QuestionDialog, QuestionSpec};
use codegg::tui::components::dialogs::queue::QueueDialog;
use codegg::tui::components::dialogs::research::ResearchBrowserDialog;
use codegg::tui::components::dialogs::review::{ReviewDialog, ReviewItem};
use codegg::tui::components::dialogs::security_review::SecurityReviewDialog;
use codegg::tui::components::dialogs::session::SessionDialog;
use codegg::tui::components::dialogs::share::ShareDialog;
use codegg::tui::components::dialogs::task_schedule::{TaskScheduleDialog, TaskScheduleSeed};
use codegg::tui::components::dialogs::task_view::TaskViewDialog;
use codegg::tui::components::dialogs::template::TemplateDialog;
use codegg::tui::components::dialogs::theme::ThemePickerDialog;
use codegg::tui::components::dialogs::tree::{TreeDialog, TreeNode};
use codegg::tui::components::dialogs::workspace_dashboard::WorkspaceDashboardDialog;
use codegg::tui::file_tree::{TreeNodeKind, TreeRow};
use codegg::tui::panels::{PanelPlacement, SidePanelId};
use codegg::tui::theme::Theme;

/// One mountable dialog surface.
pub struct DialogFixture {
    /// The `DialogType` variant name, which is also the name the baseline
    /// manifest records and the name the guard resolves against the source.
    pub surface: &'static str,
    pub dialog_type: DialogType,
    /// A fresh component per call, so no test shares render state.
    pub make: fn(&Arc<Theme>) -> Box<dyn Component>,
}

/// The bundled list production passes to the theme picker.
///
/// `ThemePickerDialog::new` falls back to the legacy static palette, not the
/// list the product shows: the modal builds the picker from
/// `theme_registry.all_tui_themes()` (`src/tui/app/modal.rs`, `Dialog::Theme`).
/// Loaded once because the sweep builds the fixture once per theme.
fn picked_themes() -> &'static [Theme] {
    static THEMES: std::sync::OnceLock<Vec<Theme>> = std::sync::OnceLock::new();
    THEMES.get_or_init(|| ThemeRegistry::load_builtins().all_tui_themes())
}

/// The picker in the state production opens it in.
fn theme_picker(theme: &Arc<Theme>) -> Box<dyn Component> {
    let mut picker = ThemePickerDialog::with_themes(Arc::clone(theme), picked_themes().to_vec());
    // The modal calls this on open; it moves the cursor's row into the
    // visible window, which a fixture that skipped it would not measure.
    picker.initialize_selection();
    Box::new(picker)
}

/// The body every list-like fixture carries.
///
/// Sixty rows is deliberate: these surfaces advertise `j/k scroll`, and a
/// four-line body at the harness's 100×30 measurement size never overflows,
/// so the measured frame could not show either key acting. A fixture too
/// small to scroll reports an implemented key as an unhandled one.
fn lines() -> Vec<String> {
    (1..=60).map(|n| format!("line {n:02}")).collect()
}

fn info_dialog(theme: &Arc<Theme>, info_type: InfoType) -> Box<dyn Component> {
    Box::new(InfoDialog::new(Arc::clone(theme), info_type, lines()))
}

fn seed() -> TaskScheduleSeed {
    TaskScheduleSeed {
        prompt_preview: "run the acceptance suite".to_string(),
        lanes: vec![("lane-a".to_string(), None)],
        lane_index: 0,
        models: vec!["model-a".to_string()],
        model_index: 0,
        policy_summary: "ask".to_string(),
        workspace_summary: "main".to_string(),
        queue_summary: "empty".to_string(),
        capabilities_supported: true,
        queue_orders: vec![Vec::new()],
        queue_revisions: vec![0],
        queue_pinned_heads: vec![None],
        trigger_capable: true,
    }
}

/// Every dialog surface this harness can build.
pub fn dialogs() -> Vec<DialogFixture> {
    vec![
        DialogFixture {
            surface: "Share",
            dialog_type: DialogType::Share,
            make: |theme| Box::new(ShareDialog::new(Arc::clone(theme))),
        },
        DialogFixture {
            surface: "Model",
            dialog_type: DialogType::Model,
            make: |theme| Box::new(ModelDialog::new(Arc::clone(theme))),
        },
        DialogFixture {
            surface: "Agent",
            dialog_type: DialogType::Agent,
            make: |theme| Box::new(AgentDialog::new(Arc::clone(theme))),
        },
        DialogFixture {
            surface: "Session",
            dialog_type: DialogType::Session,
            make: |theme| Box::new(SessionDialog::new(Arc::clone(theme))),
        },
        DialogFixture {
            surface: "Help",
            dialog_type: DialogType::Help,
            make: |theme| Box::new(HelpDialog::new(Arc::clone(theme), lines())),
        },
        DialogFixture {
            surface: "Tree",
            dialog_type: DialogType::Tree,
            make: |theme| Box::new(TreeDialog::new(Arc::clone(theme))),
        },
        DialogFixture {
            surface: "Files",
            dialog_type: DialogType::Files,
            make: |theme| Box::new(FilesDialog::new(Arc::clone(theme))),
        },
        DialogFixture {
            surface: "Theme",
            dialog_type: DialogType::Theme,
            make: theme_picker,
        },
        DialogFixture {
            surface: "Permission",
            dialog_type: DialogType::Permission,
            make: |theme| {
                Box::new(PermissionDialog::new(
                    PermissionRequest {
                        tool: "bash".to_string(),
                        path: Some("cargo test".to_string()),
                        args: None,
                    },
                    Arc::clone(theme),
                ))
            },
        },
        DialogFixture {
            surface: "PanelManager",
            dialog_type: DialogType::PanelManager,
            make: |theme| {
                Box::new(PanelManagerDialog::new(
                    Arc::clone(theme),
                    PanelHotkeys::default(),
                    SidePanelId::Session,
                    PanelPlacement::Right,
                    true,
                ))
            },
        },
        DialogFixture {
            surface: "Mcp",
            dialog_type: DialogType::Mcp,
            make: |_theme| Box::new(McpDialog::new()),
        },
        DialogFixture {
            surface: "Question",
            dialog_type: DialogType::Question,
            make: |_theme| {
                Box::new(QuestionDialog::new(vec![QuestionSpec {
                    question: "Continue with the sweep?".to_string(),
                    options: None,
                    initial: None,
                }]))
            },
        },
        DialogFixture {
            surface: "Diff",
            dialog_type: DialogType::Diff,
            make: |_theme| {
                Box::new(DiffDialog::new(
                    "let x = 1;\n".into(),
                    "let x = 2;\n".into(),
                    " src/x.rs ".into(),
                ))
            },
        },
        DialogFixture {
            surface: "Import",
            dialog_type: DialogType::Import,
            make: |theme| Box::new(ImportDialog::new(Arc::clone(theme))),
        },
        DialogFixture {
            surface: "Template",
            dialog_type: DialogType::Template,
            make: |theme| Box::new(TemplateDialog::new(Arc::clone(theme))),
        },
        DialogFixture {
            surface: "Connect",
            dialog_type: DialogType::Connect,
            make: |theme| Box::new(ConnectDialog::new(Vec::new(), Arc::clone(theme))),
        },
        DialogFixture {
            surface: "ConnectionSelection",
            dialog_type: DialogType::ConnectionSelection,
            make: |theme| {
                Box::new(ConnectionSelectionDialog::new(
                    "session-1".to_string(),
                    Arc::clone(theme),
                ))
            },
        },
        DialogFixture {
            surface: "Keybind",
            dialog_type: DialogType::Keybind,
            make: |theme| Box::new(KeybindDialog::new(Arc::clone(theme))),
        },
        DialogFixture {
            surface: "Goto",
            dialog_type: DialogType::Goto,
            make: |_theme| Box::new(GotoDialog::new(12)),
        },
        DialogFixture {
            surface: "Plan",
            dialog_type: DialogType::Plan,
            make: |_theme| {
                Box::new(PlanDialog::new(
                    "plan-1".to_string(),
                    "render invariant harness".to_string(),
                ))
            },
        },
        DialogFixture {
            surface: "Review",
            dialog_type: DialogType::Review,
            make: |_theme| Box::new(ReviewDialog::new()),
        },
        DialogFixture {
            surface: "Confirm",
            dialog_type: DialogType::Confirm,
            make: |_theme| {
                Box::new(ConfirmDialog::new(
                    "Confirm".to_string(),
                    "Are you sure?".to_string(),
                ))
            },
        },
        DialogFixture {
            surface: "ResearchBrowser",
            dialog_type: DialogType::ResearchBrowser,
            make: |_theme| Box::new(ResearchBrowserDialog::new()),
        },
        DialogFixture {
            surface: "SecurityReview",
            dialog_type: DialogType::SecurityReview,
            make: |_theme| Box::new(SecurityReviewDialog::new()),
        },
        DialogFixture {
            surface: "Plugin",
            dialog_type: DialogType::Plugin,
            make: |_theme| {
                Box::new(PluginDialog::new(
                    "plugin-1".to_string(),
                    " Plugin ".to_string(),
                    lines(),
                ))
            },
        },
        DialogFixture {
            surface: "JobDetail",
            dialog_type: DialogType::JobDetail,
            make: |_theme| Box::new(JobDetailDialog::loading("job-1".to_string())),
        },
        DialogFixture {
            surface: "ProjectChat",
            dialog_type: DialogType::ProjectChat,
            make: |theme| Box::new(ChatDialog::new(Arc::clone(theme), "project-1".to_string())),
        },
        DialogFixture {
            surface: "ProjectPicker",
            dialog_type: DialogType::ProjectPicker,
            make: |theme| Box::new(ProjectPickerDialog::new(Arc::clone(theme))),
        },
        DialogFixture {
            surface: "TaskSchedule",
            dialog_type: DialogType::TaskSchedule,
            make: |_theme| {
                Box::new(TaskScheduleDialog::new(
                    seed(),
                    &TaskScheduleDraft::default(),
                ))
            },
        },
        DialogFixture {
            surface: "TaskView",
            dialog_type: DialogType::TaskView,
            make: |_theme| Box::new(TaskViewDialog::new("project-1".to_string())),
        },
        DialogFixture {
            surface: "WorkspaceDashboard",
            dialog_type: DialogType::WorkspaceDashboard,
            make: |_theme| Box::new(WorkspaceDashboardDialog::new()),
        },
        DialogFixture {
            surface: "PolicySelector",
            dialog_type: DialogType::PolicySelector,
            make: |theme| {
                Box::new(PolicySelectorDialog::new(
                    PolicySelectorKind::Approval,
                    None,
                    Arc::clone(theme),
                ))
            },
        },
        DialogFixture {
            surface: "Queue",
            dialog_type: DialogType::Queue,
            make: |theme| Box::new(QueueDialog::new(Arc::clone(theme), Vec::new())),
        },
        DialogFixture {
            surface: "Context",
            dialog_type: DialogType::Context,
            make: |theme| info_dialog(theme, InfoType::Context),
        },
        DialogFixture {
            surface: "Cost",
            dialog_type: DialogType::Cost,
            make: |theme| info_dialog(theme, InfoType::Cost),
        },
        DialogFixture {
            surface: "Usage",
            dialog_type: DialogType::Usage,
            make: |theme| info_dialog(theme, InfoType::Usage),
        },
        DialogFixture {
            surface: "Stats",
            dialog_type: DialogType::Stats,
            make: |theme| info_dialog(theme, InfoType::Stats),
        },
        DialogFixture {
            surface: "ShellShow",
            dialog_type: DialogType::ShellShow,
            make: |theme| info_dialog(theme, InfoType::ShellShow),
        },
        DialogFixture {
            surface: "Terminal",
            dialog_type: DialogType::Terminal,
            make: |theme| info_dialog(theme, InfoType::TerminalShow),
        },
        DialogFixture {
            surface: "TaskList",
            dialog_type: DialogType::TaskList,
            make: |theme| info_dialog(theme, InfoType::TaskList),
        },
        DialogFixture {
            surface: "WorktreeList",
            dialog_type: DialogType::WorktreeList,
            make: |theme| info_dialog(theme, InfoType::WorktreeList),
        },
        DialogFixture {
            surface: "GoalShow",
            dialog_type: DialogType::GoalShow,
            make: |theme| info_dialog(theme, InfoType::GoalShow),
        },
        DialogFixture {
            surface: "MemoryResults",
            dialog_type: DialogType::MemoryResults,
            make: |theme| info_dialog(theme, InfoType::MemoryResults),
        },
        DialogFixture {
            surface: "DoctorReport",
            dialog_type: DialogType::DoctorReport,
            make: |theme| info_dialog(theme, InfoType::DoctorReport),
        },
        DialogFixture {
            surface: "Logs",
            dialog_type: DialogType::Logs,
            make: |theme| info_dialog(theme, InfoType::Logs),
        },
        DialogFixture {
            surface: "Collaborators",
            dialog_type: DialogType::Collaborators,
            make: |theme| info_dialog(theme, InfoType::Collaborators),
        },
        DialogFixture {
            surface: "Team",
            dialog_type: DialogType::Team,
            make: |theme| info_dialog(theme, InfoType::Team),
        },
        DialogFixture {
            surface: "Control",
            dialog_type: DialogType::Control,
            make: |theme| info_dialog(theme, InfoType::Control),
        },
    ]
}

// ── The content-bearing table (I4 only) ─────────────────────────────

/// Rows every list-shaped content fixture carries.
///
/// Eight is enough for the position prelude to land in the middle with room on
/// both sides, and enough for a page key to land somewhere else.
pub const LIST_ROWS: usize = 8;

/// How many steps a surface's position prelude moves, and so the row its
/// cursor (or its scroll offset) starts on when an up-direction is measured.
///
/// One, not a middle row: a list can be shorter than the middle of the measured
/// size — `Permission` offers four options and `Plan` three — and a prelude
/// that lands on the last row makes `j` inert in exactly the way a cursor at
/// row 0 makes `k` inert. One step leaves both directions somewhere to go on
/// every list with at least two rows.
pub const PARKED_ROW: usize = 1;

/// Surfaces whose advertised up-direction needs somewhere to go.
///
/// Measured, not guessed: every name here is a surface whose `↑` or `k` reads
/// as inert from the first row. The harness applies the prelude
/// (`baseline::mount`), pressing `↓` [`PARKED_ROW`] time on the fresh mount and
/// only when the key being measured is an up-direction. Content lives in this
/// module and position is this one list, so a reviewer reads both without
/// chasing either.
///
/// A surface absent from this list is measured from its first row, which is
/// where a user arrives.
pub const PARKED_SURFACES: &[&str] = &[
    "Agent",
    "Connect",
    "Diff",
    "Help",
    "Keybind",
    "Mcp",
    "Model",
    "Permission",
    "Plan",
    "Plugin",
    "PolicySelector",
    "Queue",
    "ResearchBrowser",
    "Review",
    "SecurityReview",
    "Session",
    "Template",
    "Tree",
    "Context",
    "Cost",
    "Usage",
    "Stats",
    "ShellShow",
    "Terminal",
    "TaskList",
    "WorktreeList",
    "GoalShow",
    "MemoryResults",
    "DoctorReport",
    "Logs",
    "Collaborators",
    "Team",
    "Control",
];

/// Whether a surface's position prelude applies.
pub fn parks_position(surface: &str) -> bool {
    PARKED_SURFACES.contains(&surface)
}

fn file_rows() -> Vec<TreeRow> {
    (0..LIST_ROWS)
        .map(|n| TreeRow {
            path: format!("src/module_{n:02}.rs"),
            name: format!("module_{n:02}.rs"),
            depth: 1,
            kind: TreeNodeKind::File,
            expanded: false,
        })
        .collect()
}

fn seeded_agent(theme: &Arc<Theme>) -> Box<dyn Component> {
    let agents: Vec<Agent> = (0..LIST_ROWS)
        .map(|n| Agent {
            name: format!("agent-{n:02}"),
            description: format!("agent {n:02}"),
            ..Agent::default()
        })
        .collect();
    let mut dialog = AgentDialog::new(Arc::clone(theme));
    dialog.set_agents(agents.iter().collect());
    Box::new(dialog)
}

fn seeded_connect(theme: &Arc<Theme>) -> Box<dyn Component> {
    let providers = (0..LIST_ROWS)
        .map(|n| {
            ProviderInfo::api_key(
                format!("provider-{n:02}"),
                format!("Provider {n:02}"),
                "catalog row",
                None,
            )
        })
        .collect();
    Box::new(ConnectDialog::new(providers, Arc::clone(theme)))
}

fn seeded_diff(_theme: &Arc<Theme>) -> Box<dyn Component> {
    // Eighty changed lines at the harness's 30-row height: without a body that
    // overflows, the page keys cannot move either.
    let old: String = (0..80).map(|n| format!("let value_{n} = {n};\n")).collect();
    let new: String = (0..80)
        .map(|n| format!("let value_{n} = {};\n", n + 1))
        .collect();
    Box::new(DiffDialog::new(old.into(), new.into(), " src/x.rs ".into()))
}

fn seeded_files(theme: &Arc<Theme>) -> Box<dyn Component> {
    let rows = file_rows();
    let selected = rows.get(PARKED_ROW).map(|row| row.path.clone());
    let structure = FileTreeStructure {
        rows,
        selected,
        root: Some(PathBuf::from("/workspace")),
        scope: String::new(),
        truncated: None,
        status: FileTreeStatus::Ready,
    };
    let mut dialog = FilesDialog::new(Arc::clone(theme));
    dialog.adopt_structure(1, &|| structure.clone());
    Box::new(dialog)
}

fn seeded_goto(_theme: &Arc<Theme>) -> Box<dyn Component> {
    // `Enter go` needs a target: with an empty field there is nothing to go to,
    // and the advertisement would read as inert.
    let mut dialog = GotoDialog::new(12);
    dialog.append_char('7');
    Box::new(dialog)
}

fn seeded_keybind(theme: &Arc<Theme>) -> Box<dyn Component> {
    let mut dialog = KeybindDialog::new(Arc::clone(theme));
    // `r reset` clears the bindings, so an empty map makes the advertised verb
    // a no-op the measurement would report as inert. No reader reaches this box
    // holding a map that was never populated.
    let bindings = KeybindDialog::actions()
        .iter()
        .take(2)
        .enumerate()
        .map(|(n, action)| (format!("ctrl+{}", n + 1), *action))
        .collect();
    dialog.set_bindings(bindings);
    Box::new(dialog)
}

fn seeded_mcp(_theme: &Arc<Theme>) -> Box<dyn Component> {
    let mut dialog = McpDialog::new();
    dialog.set_servers(
        (0..LIST_ROWS)
            .map(|n| McpServerInfo {
                name: format!("server-{n:02}"),
                status: "connected".to_string(),
                status_error: None,
                server_type: "stdio".to_string(),
                tools: vec![McpToolInfo {
                    name: "search".to_string(),
                    description: "search the workspace".to_string(),
                }],
                resources: Vec::new(),
                has_oauth: false,
            })
            .collect(),
    );
    Box::new(dialog)
}

fn seeded_model(theme: &Arc<Theme>) -> Box<dyn Component> {
    let mut dialog = ModelDialog::new(Arc::clone(theme));
    dialog.set_models(
        (0..LIST_ROWS)
            .map(|n| format!("provider-a/model-{n:02}"))
            .collect(),
    );
    Box::new(dialog)
}

fn seeded_project_chat(theme: &Arc<Theme>) -> Box<dyn Component> {
    let messages = (0..LIST_ROWS)
        .map(|n| ChatMessageDto {
            message_id: format!("message-{n:02}"),
            channel_id: "general".to_string(),
            project_id: "project-1".to_string(),
            seq: n as u64,
            author_principal: format!("member-{n:02}"),
            author_agent: None,
            body: format!("message {n:02}"),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            revision: 1,
            redacted: false,
            created_at_ms: n as i64,
            edited_at_ms: None,
        })
        .collect();
    let mut dialog = ChatDialog::new(Arc::clone(theme), "project-1".to_string());
    dialog.apply_snapshot(ChatSnapshot {
        messages,
        // `reply` and `edit` are gated on `ChatView::interactive`, which is
        // `Ready | Error`. A window that holds messages but reports `Unknown`
        // is not a state the daemon produces, and measuring the verbs against
        // it would report the gate as an unhandled key.
        status: ChatStatus::Ready,
        ..ChatSnapshot::default()
    });
    Box::new(dialog)
}

fn seeded_queue(theme: &Arc<Theme>) -> Box<dyn Component> {
    Box::new(QueueDialog::new(
        Arc::clone(theme),
        (0..LIST_ROWS)
            .map(|n| UserPromptQueueEntryDto {
                id: format!("entry-{n:02}"),
                ordinal: n as i64,
                text: format!("queued prompt {n:02}"),
                text_bytes: 18,
                status: "pending".to_string(),
                created_at_ms: n as i64,
                expires_at_ms: 0,
                run_at_ms: None,
            })
            .collect(),
    ))
}

fn seeded_research(_theme: &Arc<Theme>) -> Box<dyn Component> {
    let now = chrono::Utc::now();
    let mut dialog = ResearchBrowserDialog::new();
    dialog.set_runs(
        (0..LIST_ROWS)
            .map(|n| ResearchRunSummary {
                run_id: format!("run-{n:02}"),
                status: RunStatus::Completed,
                question: format!("question {n:02}"),
                mode: ResearchMode::Landscape,
                depth: ResearchDepth::Medium,
                started_at: now,
                finished_at: Some(now),
                artifact_dir: PathBuf::from("/workspace/.codegg/research"),
                counts: ResearchRunCounts::default(),
            })
            .collect(),
    );
    Box::new(dialog)
}

fn seeded_review(_theme: &Arc<Theme>) -> Box<dyn Component> {
    let mut dialog = ReviewDialog::new();
    dialog.set_items(
        (0..LIST_ROWS)
            .map(|n| ReviewItem {
                path: format!("src/module_{n:02}.rs"),
                kind: "Modified".to_string(),
                additions: n as u64 + 1,
                deletions: n as u64,
            })
            .collect(),
    );
    Box::new(dialog)
}

fn seeded_security_review(_theme: &Arc<Theme>) -> Box<dyn Component> {
    let path = PathBuf::from("src/commands/run.rs");
    let findings = (0..LIST_ROWS)
        .map(|n| SecurityReviewFinding {
            severity: SecuritySeverity::High,
            confidence: SecurityConfidence::High,
            title: format!("finding {n:02}"),
            file_path: path.clone(),
            line: Some(10 + n as u32),
            category: Some("command-injection".to_string()),
            evidence: Vec::new(),
            reasoning: "user input reaches a shell".to_string(),
            recommendation: "pass argv instead".to_string(),
            tests: vec!["a test that runs the argv".to_string()],
        })
        .collect();
    // No hunks. A hunk would give `h` (jump to hunk) a destination — and it
    // would also arm the advertised `H` (copy hunk), whose arm calls
    // `copy_to_clipboard`. A render measurement must not write the operator's
    // clipboard, so the receipt carries findings without hunk context and `h`
    // is a correct no-op. `SecurityReview`/`h` is recorded with that reason.
    Box::new(SecurityReviewDialog::with_receipt(SecurityReviewReceipt {
        id: "receipt-1".to_string(),
        root: PathBuf::from("/workspace"),
        args: SecurityReviewCommandArgs::default(),
        output: SecurityReviewOutput {
            targets: Vec::new(),
            findings,
            review_prompts: Vec::new(),
            preflight_results: Vec::new(),
            notes: Vec::new(),
            hunks: Vec::new(),
        },
        rendered_report: "security review".to_string(),
        completed_at_ms: 0,
        enriched: false,
        lsp_available: false,
        hunk_context_requested: false,
        hunk_context_available: false,
        hunk_context_executed: false,
        hunk_context_succeeded: false,
        hunk_context_requests_attempted: 0,
        hunk_context_requests_succeeded: 0,
    }))
}

fn seeded_session(theme: &Arc<Theme>) -> Box<dyn Component> {
    let mut dialog = SessionDialog::new(Arc::clone(theme));
    dialog.load_sessions(
        (0..LIST_ROWS)
            .map(|n| Session {
                id: format!("session-{n:02}"),
                title: format!("Session {n:02}"),
                slug: format!("session-{n:02}"),
                directory: "/workspace".to_string(),
                ..Session::default()
            })
            .collect(),
    );
    Box::new(dialog)
}

fn seeded_template(theme: &Arc<Theme>) -> Box<dyn Component> {
    let mut dialog = TemplateDialog::new(Arc::clone(theme));
    dialog.set_templates(
        (0..LIST_ROWS)
            .map(|n| {
                (
                    format!("template-{n:02}"),
                    SessionTemplate {
                        name: format!("Template {n:02}"),
                        ..SessionTemplate::default()
                    },
                )
            })
            .collect(),
    );
    Box::new(dialog)
}

fn seeded_tree(theme: &Arc<Theme>) -> Box<dyn Component> {
    let nodes = (0..LIST_ROWS)
        .map(|n| TreeNode {
            id: format!("node-{n:02}"),
            session_id: format!("session-{n:02}"),
            label: format!("session {n:02}"),
            time_updated: n as i64,
            message_count: Some(n + 1),
            is_current: n == 0,
            is_archived: false,
            // A session with forks: `e` (expand) toggles a node's children,
            // so a tree of leaves would make the advertised verb unmeasurable.
            children: (0..2)
                .map(|fork| TreeNode {
                    id: format!("node-{n:02}-{fork}"),
                    session_id: format!("session-{n:02}-{fork}"),
                    label: format!("fork {fork}"),
                    time_updated: n as i64,
                    message_count: None,
                    is_current: false,
                    is_archived: false,
                    children: Vec::new(),
                    depth: 1,
                })
                .collect(),
            depth: 0,
        })
        .collect();
    let mut dialog = TreeDialog::new(Arc::clone(theme));
    dialog.load_nodes(nodes, None);
    Box::new(dialog)
}

/// The surfaces whose empty fixture cannot exercise the keys they advertise,
/// and the constructor that gives each one the content its keys act on.
///
/// Content only: a surface absent from this table is measured on [`dialogs()`]'s
/// fixture, because its keys do not depend on rows. Position is
/// [`PARKED_SURFACES`], not this table.
type ContentMaker = fn(&Arc<Theme>) -> Box<dyn Component>;

const CONTENT_MAKERS: &[(&str, ContentMaker)] = &[
    ("Agent", seeded_agent),
    ("Connect", seeded_connect),
    ("Diff", seeded_diff),
    ("Files", seeded_files),
    ("Goto", seeded_goto),
    ("Keybind", seeded_keybind),
    ("Mcp", seeded_mcp),
    ("Model", seeded_model),
    ("ProjectChat", seeded_project_chat),
    ("Queue", seeded_queue),
    ("ResearchBrowser", seeded_research),
    ("Review", seeded_review),
    ("SecurityReview", seeded_security_review),
    ("Session", seeded_session),
    ("Template", seeded_template),
    ("Tree", seeded_tree),
];

/// The same surfaces as [`dialogs()`], with content where I4 needs it.
///
/// I4 only. I1, I2, I3a and I5 keep reading [`dialogs()`], whose empty
/// fixtures are what `[contrast]` was banked against.
pub fn dialogs_with_content() -> Vec<DialogFixture> {
    let mut fixtures = dialogs();
    for (surface, make) in CONTENT_MAKERS {
        let fixture = fixtures
            .iter_mut()
            .find(|fixture| fixture.surface == *surface)
            .unwrap_or_else(|| {
                panic!("`{surface}` is in the content table and not in `dialogs()`")
            });
        fixture.make = *make;
    }
    for surface in PARKED_SURFACES {
        assert!(
            fixtures.iter().any(|fixture| fixture.surface == *surface),
            "`{surface}` is in the position list and not in `dialogs()`"
        );
    }
    fixtures
}

/// A box adopted into a hosted panel, so the panel has content and can take
/// the keyboard the way a docked box can.
pub fn hosted_box(theme: &Arc<Theme>, panel: SidePanelId) -> Box<dyn Component> {
    let info_type = match panel {
        SidePanelId::Context => Some(InfoType::Context),
        SidePanelId::Cost => Some(InfoType::Cost),
        SidePanelId::Usage => Some(InfoType::Usage),
        SidePanelId::Stats => Some(InfoType::Stats),
        SidePanelId::ShellShow => Some(InfoType::ShellShow),
        SidePanelId::Tasks => Some(InfoType::TaskList),
        SidePanelId::Goal => Some(InfoType::GoalShow),
        SidePanelId::Memory => Some(InfoType::MemoryResults),
        SidePanelId::Doctor => Some(InfoType::DoctorReport),
        SidePanelId::Collaborators => Some(InfoType::Collaborators),
        SidePanelId::Logs => Some(InfoType::Logs),
        SidePanelId::Team => Some(InfoType::Team),
        SidePanelId::Control => Some(InfoType::Control),
        _ => None,
    };
    match info_type {
        Some(info_type) => Box::new(InfoDialog::new(Arc::clone(theme), info_type, lines())),
        // Every other hosted panel gets a box whose surface is broad enough to
        // paint at any column width, so the fixture exercises the panel rather
        // than the box's own narrow-content fallback.
        None => Box::new(FilesDialog::new(Arc::clone(theme))),
    }
}

/// Surfaces that deliberately paint nothing at 20×6, with the production
/// guard that makes them decline.
///
/// I2 says a non-degenerate size paints at least one cell. Four surfaces do
/// not, and none is a harness artefact: each opens its render body with an
/// explicit minimum-size guard (`DiffDialog` needs seven rows; the three
/// sheet layouts need 30×8), so 20×6 is below their own declared minimum.
/// Recording them here rather than skipping them in the assertion is what
/// keeps the property checkable in both directions — a *new* decliner fails
/// the comparison, and so does a recorded one that starts painting (its guard
/// was removed and this row is now stale). Whether the minimum should be one
/// uniform size belongs to M002's panel chrome, not to M001.
pub fn small_size_decliners() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Diff", "DiffDialog::render returns when area.height < 7"),
        (
            "TaskSchedule",
            "render_sheet returns when area.height < 8 or area.width < 30",
        ),
        (
            "TaskView",
            "render_view returns when area.height < 8 or area.width < 30",
        ),
        (
            "WorkspaceDashboard",
            "render_dashboard returns when area.height < 8 or area.width < 30",
        ),
    ]
}

/// An unused path, kept here so the exclusion above is a value a reader can
/// grep for rather than a claim in prose.
pub fn excluded_surfaces() -> [(&'static str, &'static str); 3] {
    [
        (
            "RunDetail",
            "RunDetailDialog::new takes a whole run-store record",
        ),
        (
            "TriggerSecret",
            "minted only from a live one-time trigger credential",
        ),
        (
            "SourcePreview",
            "SourcePreviewDialog::new reads the previewed file",
        ),
    ]
}

/// The path the excluded `SourcePreview` surface would have previewed, so the
/// exclusion names a real shape rather than nothing at all.
pub fn excluded_source_preview_path() -> PathBuf {
    PathBuf::from("src/tui/components/component.rs")
}
