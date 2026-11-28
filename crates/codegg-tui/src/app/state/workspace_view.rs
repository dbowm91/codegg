//! M007-M011: the Workspace whole view's own state.
//!
//! The dashboard state next door is the bounded project projection the
//! daemon already answers. This module is the *view* on top of it: the
//! route-owned NOR/INS mode and filter, the expansion caches that turn the
//! flat project list into a project -> workspace -> worktree tree, the
//! inline detail, and the new-session launch sheet.
//!
//! It performs no I/O and owns no durable object. Every cache here is a
//! projection of a daemon answer, keyed by the identity that answer named,
//! so a refresh can drop it without inventing anything.

use std::collections::{HashMap, HashSet};

use codegg::protocol::dto::{ManagedWorktreeDto, ProjectWorkspaceSummaryDto};

/// Maximum tree rows the view will paint or navigate.
pub const MAX_TREE_ROWS: usize = 512;

/// The route's keyboard owner. The shared prompt is the only text buffer;
/// this value only decides whether keys navigate the tree, reach that prompt,
/// drive a panel, or edit the explicit tree filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceInputFocus {
    #[default]
    Tree,
    Composer,
    Panel,
    Filter,
    MemberForm,
}

/// The tree filter's own input mode. It is deliberately separate from the
/// shared session/chat composer's mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceMode {
    #[default]
    Normal,
    Insert,
}

/// What one tree row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeKind {
    Project,
    Workspace,
    Worktree,
}

/// One navigable row of the Workspace tree.
#[derive(Debug, Clone)]
pub struct TreeRow {
    pub kind: TreeKind,
    /// Project id for a project row, workspace id for a workspace row,
    /// worktree id for a worktree row.
    pub id: String,
    /// Owning project. Equal to `id` on a project row.
    pub project_id: String,
    /// Owning workspace id, when the row is a worktree.
    pub workspace_id: Option<String>,
    pub depth: u8,
    pub label: String,
    /// Right-hand note: state, branch, counts.
    pub note: String,
    pub expandable: bool,
    pub expanded: bool,
}

/// One step of the new-session sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchStep {
    /// Choose an existing worktree, or start a new checkout.
    #[default]
    Placement,
    /// Choose the base branch for a new checkout.
    Base,
    /// Accept the deterministic auto name or type one.
    Branch,
}

/// The new-session sheet for one workspace.
#[derive(Debug, Default)]
pub struct LaunchSheet {
    pub project_id: String,
    pub workspace_id: String,
    /// Ready worktrees under the workspace, from `ManagedWorktreeList`.
    pub worktrees: Vec<ManagedWorktreeDto>,
    pub worktrees_loading: bool,
    pub worktree_selected: usize,
    /// Index 0 is "new checkout"; the rest are the ready worktrees.
    pub selected: usize,
    pub step: LaunchStep,
    /// Local branches offered as `base_ref`.
    pub branches: Vec<String>,
    pub branches_loading: bool,
    /// Request whose branch result may populate this sheet.
    pub branches_request_id: Option<u64>,
    pub base_selected: usize,
    /// The typed branch name. Empty means the deterministic auto name.
    pub branch_input: String,
    /// One key per submit: reused on retry, replaced after a refusal.
    pub idempotency_key: String,
    pub submitting: bool,
    /// Request whose launch result may complete this sheet.
    pub launch_request_id: Option<u64>,
    pub stage: String,
    pub error: Option<String>,
}

impl LaunchSheet {
    pub fn new(project_id: String, workspace_id: String) -> Self {
        Self {
            project_id,
            workspace_id,
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            ..Self::default()
        }
    }

    /// The ready worktrees the sheet offers, in daemon order.
    pub fn ready_worktrees(&self) -> impl Iterator<Item = &ManagedWorktreeDto> {
        self.worktrees.iter().filter(|wt| wt.state == "ready")
    }

    /// Number of selectable placement rows (new checkout + ready worktrees).
    pub fn placement_len(&self) -> usize {
        1 + self.ready_worktrees().count()
    }
}

/// An archive or restore armed by its first keypress; the second performs it.
/// Destroy never arms here — it opens the picker's counted confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleArm {
    pub project_id: String,
    pub restore: bool,
}

/// The Workspace view's own state.
#[derive(Debug, Default)]
pub struct WorkspaceViewState {
    pub mode: WorkspaceMode,
    pub input_focus: WorkspaceInputFocus,
    /// Selected position within the visible tree rows.
    pub selected: usize,
    /// Filter text. The view's own; never the composer's.
    pub filter: String,
    /// Expanded project and workspace ids.
    pub expanded: HashSet<String>,
    /// Project id -> its workspaces, from `ProjectGet`.
    pub workspaces: HashMap<String, Vec<ProjectWorkspaceSummaryDto>>,
    pub workspaces_loading: HashSet<String>,
    /// Workspace id -> its managed worktrees, from `ManagedWorktreeList`.
    pub worktrees: HashMap<String, Vec<ManagedWorktreeDto>>,
    pub worktrees_loading: HashSet<String>,
    /// When set, the detail replaces the tree in the same window.
    pub detail: Option<String>,
    pub detail_data: Option<codegg::protocol::dto::ProjectDetailsDto>,
    pub detail_loading: bool,
    pub detail_error: Option<String>,
    /// Monotonic id for the view's own requests, so a late answer cannot
    /// overwrite a newer selection.
    pub next_request: u64,
    /// The launch sheet, when one is open.
    pub launch: Option<LaunchSheet>,
    /// An archive/restore awaiting its second keypress.
    pub lifecycle_arm: Option<LifecycleArm>,
    pub reveal_workspace_id: Option<String>,
    pub team_project_id: Option<String>,
    pub team_memberships: Option<Vec<codegg::protocol::core::TeamMembershipDto>>,
    pub team_loading: bool,
    pub team_error: Option<String>,
    pub team_notice: Option<String>,
    pub member_form: bool,
    pub member_principal: String,
    pub member_role: String,
    pub pending_selector: Option<String>,
    pub selector_project_index: usize,
    pub selector_worktree_probes: usize,
    /// A transient line the view owns (a refusal, a launch stage).
    pub notice: Option<String>,
}

impl WorkspaceViewState {
    /// Enter Normal and clear the view-owned filter.
    pub fn reset_modes(&mut self) {
        self.mode = WorkspaceMode::Normal;
        self.input_focus = WorkspaceInputFocus::Tree;
        self.filter.clear();
        self.selected = 0;
        self.detail = None;
        self.launch = None;
        self.lifecycle_arm = None;
        self.reveal_workspace_id = None;
        self.team_project_id = None;
        self.team_memberships = None;
        self.team_loading = false;
        self.team_error = None;
        self.team_notice = None;
        self.member_form = false;
        self.member_principal.clear();
        self.pending_selector = None;
        self.selector_project_index = 0;
        self.selector_worktree_probes = 0;
        self.notice = None;
    }

    /// Push one printable character onto the filter (bounded).
    pub fn push_filter(&mut self, ch: char) {
        if self.filter.chars().count() >= crate::app::state::MAX_DASHBOARD_FILTER_LEN {
            return;
        }
        self.filter.push(ch);
        self.selected = 0;
    }

    /// Remove the last character, Unicode-safe.
    pub fn pop_filter(&mut self) {
        self.filter.pop();
        self.selected = 0;
    }
}
