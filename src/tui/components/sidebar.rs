use crate::session::events::AgentPlan;
use crate::session::Session;
use crate::tui::app::state::session::{DiffStatsState, GitSidebarInfo};
use codegg_protocol::projection::dto::{AgentTreeNodeProjection, AgentTreeStatus};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
    Widget,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::super::app::TodoEntry;
use super::super::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarSection {
    Goal,
    Plan,
    Todos,
    FileChanges,
    ToolPrograms,
    AgentRuns,
    Convergences,
}

/// Stable identity for one logical sidebar row. It is independent of the
/// rendered line number, scroll offset, and collapse state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SidebarFocusTarget {
    Section(SidebarSection),
    Todo(usize),
    McpServer(usize),
    FileChange(usize),
    ToolProgram(String),
    AgentRun(String),
    AgentTreeNode(u64),
    Convergence(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarActivation {
    None,
    ToggleSection,
    ToggleAgentNode(u64),
    InspectRun(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HoveredElement {
    Section(SidebarSection),
    Todo(usize),
    McpServer(usize),
    FileChange(usize),
    ToolProgram(String),
    AgentRun(String),
    AgentTreeNode(u64),
    Convergence(String),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarFileChange {
    pub path: String,
    pub action: String,
    pub diff_preview: Vec<String>,
    pub diff_state: DiffStatsState,
}

/// Compact representation of a tool program for the sidebar display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarToolProgram {
    pub program_id: String,
    pub state: String,
    pub language: String,
    pub calls_completed: u32,
    pub summary: Option<String>,
}

/// Compact durable delegated-run entry for the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarAgentRun {
    pub run_id: String,
    pub task_id: Option<String>,
    pub agent: String,
    pub status: String,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub result_commit: Option<String>,
    pub attention_required: bool,
    pub progress: Option<String>,
}

/// Compact bounded convergence state for the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarConvergence {
    pub convergence_id: String,
    pub status: String,
    pub cycle_ordinal: u8,
    pub max_cycles: u8,
    pub remaining_cycles: u8,
    pub producer_completed: usize,
    pub producer_active: usize,
    pub verifier_run_id: Option<String>,
    pub verdict_kind: Option<String>,
    pub awaiting_decision: bool,
    pub selected_run_id: Option<String>,
    pub selected_result_commit: Option<String>,
    pub last_finding_count: usize,
}

pub struct SidebarWidget {
    pub theme: Arc<Theme>,
    pub session: Option<Session>,
    pub agent: String,
    pub provider: String,
    pub model: String,
    pub todos: Vec<TodoEntry>,
    pub mcp_servers: Vec<(String, String)>,
    pub file_changes: Vec<SidebarFileChange>,
    pub git_branch: Option<String>,
    pub git_dirty: bool,
    pub git_staged_count: usize,
    pub git_unstaged_count: usize,
    pub git_untracked_count: usize,
    pub git_conflicted_count: usize,
    pub git_ahead: Option<i32>,
    pub git_behind: Option<i32>,
    pub project_root: Option<String>,
    pub goal: Option<String>,
    pub plan: Option<AgentPlan>,
    /// Active and recently completed background tool programs.
    pub tool_programs: Vec<SidebarToolProgram>,
    pub agent_runs: Vec<SidebarAgentRun>,
    /// Canonical bounded agent hierarchy from the active session projection.
    pub agent_tree: Vec<AgentTreeNodeProjection>,
    pub convergences: Vec<SidebarConvergence>,
    scroll_offset: usize,
    goal_collapsed: bool,
    plan_collapsed: bool,
    todos_collapsed: bool,
    file_changes_collapsed: bool,
    tool_programs_collapsed: bool,
    agent_runs_collapsed: bool,
    convergences_collapsed: bool,
    hovered_element: HoveredElement,
    tooltip_text: String,
    focused_target: Option<SidebarFocusTarget>,
    focused_position: Option<usize>,
    scope_project_id: Option<String>,
    collapsed_agent_nodes: HashSet<u64>,
}

const MAX_SIDEBAR_AGENT_TREE_NODES: usize = 64;

#[derive(Debug, Clone)]
struct SidebarAgentTreeRow {
    node: AgentTreeNodeProjection,
    depth: usize,
    has_children: bool,
    collapsed: bool,
}

impl SidebarWidget {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            session: None,
            agent: String::new(),
            provider: String::new(),
            model: String::new(),
            todos: Vec::new(),
            mcp_servers: Vec::new(),
            file_changes: Vec::new(),
            git_branch: None,
            git_dirty: false,
            git_staged_count: 0,
            git_unstaged_count: 0,
            git_untracked_count: 0,
            git_conflicted_count: 0,
            git_ahead: None,
            git_behind: None,
            project_root: None,
            goal: None,
            plan: None,
            tool_programs: Vec::new(),
            agent_runs: Vec::new(),
            agent_tree: Vec::new(),
            convergences: Vec::new(),
            scroll_offset: 0,
            goal_collapsed: false,
            plan_collapsed: false,
            todos_collapsed: false,
            file_changes_collapsed: false,
            tool_programs_collapsed: false,
            agent_runs_collapsed: false,
            convergences_collapsed: false,
            hovered_element: HoveredElement::None,
            tooltip_text: String::new(),
            focused_target: None,
            focused_position: None,
            scope_project_id: None,
            collapsed_agent_nodes: HashSet::new(),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_session(&mut self, session: &Session) {
        self.session = Some(session.clone());
    }

    pub fn set_agent(&mut self, agent: &str) {
        self.agent = agent.to_string();
    }

    pub fn set_model(&mut self, model: &str) {
        self.model = model.to_string();
    }

    pub fn set_provider(&mut self, provider: &str) {
        self.provider = provider.to_string();
    }

    pub fn set_todos(&mut self, todos: Vec<TodoEntry>) {
        self.todos = todos;
    }

    pub fn set_mcp_servers(&mut self, servers: Vec<(String, String)>) {
        self.mcp_servers = servers;
    }

    pub fn set_file_changes(&mut self, changes: Vec<SidebarFileChange>) {
        self.file_changes = changes;
    }

    pub fn set_git_info(&mut self, info: GitSidebarInfo) {
        self.git_branch = info.branch;
        self.git_dirty = info.dirty;
        self.project_root = info.root;
        self.git_staged_count = info.staged_count;
        self.git_unstaged_count = info.unstaged_count;
        self.git_untracked_count = info.untracked_count;
        self.git_conflicted_count = info.conflicted_count;
        self.git_ahead = info.ahead;
        self.git_behind = info.behind;
    }

    pub fn set_goal(&mut self, goal: Option<String>) {
        self.goal = goal;
    }

    pub fn set_plan(&mut self, plan: Option<AgentPlan>) {
        self.plan = plan;
    }

    pub fn set_tool_programs(&mut self, programs: Vec<SidebarToolProgram>) {
        self.tool_programs = programs;
    }

    pub fn set_agent_runs(&mut self, runs: Vec<SidebarAgentRun>) {
        self.agent_runs = runs;
        self.reconcile_focus();
    }

    pub fn set_agent_tree(&mut self, nodes: Vec<AgentTreeNodeProjection>) {
        self.agent_tree = nodes
            .into_iter()
            .take(MAX_SIDEBAR_AGENT_TREE_NODES)
            .collect();
        let present: HashSet<u64> = self.agent_tree.iter().map(|node| node.task_id).collect();
        self.collapsed_agent_nodes.retain(|id| present.contains(id));
        self.reconcile_focus();
    }

    /// Keep presentation-only selection scoped to the active project.
    pub fn set_project_scope(&mut self, project_id: Option<&str>) {
        let project_id = project_id.map(str::to_string);
        if self.scope_project_id != project_id {
            self.scope_project_id = project_id;
            self.focused_target = None;
            self.focused_position = None;
            self.scroll_offset = 0;
        }
    }

    pub fn is_focused(&self) -> bool {
        self.focused_target.is_some()
    }

    pub fn focus_sidebar(&mut self) -> bool {
        self.reconcile_focus();
        if self.focused_target.is_none() {
            self.set_focused_target(self.focus_targets().into_iter().next());
        }
        self.focused_target.is_some()
    }

    pub fn blur_sidebar(&mut self) {
        self.focused_target = None;
        self.focused_position = None;
    }

    pub fn focused_target(&self) -> Option<&SidebarFocusTarget> {
        self.focused_target.as_ref()
    }

    /// Keep the selected logical row inside the rendered viewport.
    pub fn ensure_focused_visible(&mut self, area: Rect) {
        let Some(target) = self.focused_target.as_ref() else {
            return;
        };
        let target = match target {
            SidebarFocusTarget::Section(section) => HoveredElement::Section(*section),
            SidebarFocusTarget::Todo(index) => HoveredElement::Todo(*index),
            SidebarFocusTarget::McpServer(index) => HoveredElement::McpServer(*index),
            SidebarFocusTarget::FileChange(index) => HoveredElement::FileChange(*index),
            SidebarFocusTarget::ToolProgram(id) => HoveredElement::ToolProgram(id.clone()),
            SidebarFocusTarget::AgentRun(id) => HoveredElement::AgentRun(id.clone()),
            SidebarFocusTarget::AgentTreeNode(id) => HoveredElement::AgentTreeNode(*id),
            SidebarFocusTarget::Convergence(id) => HoveredElement::Convergence(id.clone()),
        };
        let Some(line) = self
            .line_targets()
            .iter()
            .position(|candidate| candidate == &target)
        else {
            return;
        };
        let viewport = sidebar_content_height(area) as usize;
        if line < self.scroll_offset {
            self.scroll_offset = line;
        } else if viewport > 0 && line >= self.scroll_offset + viewport {
            self.scroll_offset = line + 1 - viewport;
        }
        self.scroll_offset = self.scroll_offset.min(self.max_scroll(area));
    }

    pub fn set_convergences(&mut self, convergences: Vec<SidebarConvergence>) {
        self.convergences = convergences;
    }

    pub fn toggle_focused(&mut self) {
        match self.activate_focused() {
            SidebarActivation::ToggleSection => {
                if let Some(SidebarFocusTarget::Section(section)) = self.focused_target.clone() {
                    self.toggle_section(section);
                }
            }
            SidebarActivation::ToggleAgentNode(task_id) => {
                if !self.collapsed_agent_nodes.insert(task_id) {
                    self.collapsed_agent_nodes.remove(&task_id);
                }
            }
            SidebarActivation::None | SidebarActivation::InspectRun(_) => {}
        }
        self.reconcile_focus();
    }

    pub fn focus_next(&mut self) {
        self.move_focus(1);
    }

    pub fn focus_prev(&mut self) {
        self.move_focus(-1);
    }

    pub fn focus_page_next(&mut self, area: Rect) {
        self.move_focus(scroll_step(area) as isize);
    }

    pub fn focus_page_prev(&mut self, area: Rect) {
        self.move_focus(-(scroll_step(area) as isize));
    }

    pub fn collapse_or_parent(&mut self) {
        let Some(target) = self.focused_target.clone() else {
            return;
        };
        match target {
            SidebarFocusTarget::Section(section) => self.set_section_collapsed(section, true),
            SidebarFocusTarget::AgentTreeNode(task_id) => {
                let parent = self
                    .agent_tree
                    .iter()
                    .find(|node| node.task_id == task_id)
                    .and_then(|node| node.parent_task_id);
                if let Some(parent) = parent {
                    let target = SidebarFocusTarget::AgentTreeNode(parent);
                    if self.focus_targets().contains(&target) {
                        self.focused_target = Some(target.clone());
                        self.focused_position =
                            self.focus_targets().iter().position(|item| item == &target);
                    }
                }
            }
            _ => {}
        }
        self.reconcile_focus();
    }

    pub fn expand_focused(&mut self) {
        let Some(target) = self.focused_target.clone() else {
            return;
        };
        match target {
            SidebarFocusTarget::Section(section) => self.set_section_collapsed(section, false),
            SidebarFocusTarget::AgentTreeNode(task_id) => {
                self.collapsed_agent_nodes.remove(&task_id);
            }
            _ => {}
        }
        self.reconcile_focus();
    }

    pub fn focus_hovered(&mut self) {
        self.focused_target = Self::hovered_to_focus(&self.hovered_element);
        self.focused_position = self
            .focused_target
            .as_ref()
            .and_then(|target| self.focus_targets().iter().position(|item| item == target));
        self.reconcile_focus();
    }

    pub fn activate_focused(&mut self) -> SidebarActivation {
        match self.focused_target.clone() {
            Some(SidebarFocusTarget::Section(_)) => SidebarActivation::ToggleSection,
            Some(SidebarFocusTarget::AgentTreeNode(task_id)) => {
                let has_children = self
                    .agent_tree
                    .iter()
                    .any(|node| node.parent_task_id == Some(task_id));
                if has_children {
                    SidebarActivation::ToggleAgentNode(task_id)
                } else {
                    self.run_id_for_task(task_id)
                        .map(SidebarActivation::InspectRun)
                        .unwrap_or(SidebarActivation::None)
                }
            }
            Some(SidebarFocusTarget::AgentRun(run_id)) => SidebarActivation::InspectRun(run_id),
            _ => SidebarActivation::None,
        }
    }

    pub fn focused_name(&self) -> Option<&str> {
        match self.focused_target.as_ref() {
            Some(SidebarFocusTarget::AgentRun(run_id)) => Some(run_id),
            Some(SidebarFocusTarget::ToolProgram(program_id)) => Some(program_id),
            Some(SidebarFocusTarget::Convergence(id)) => Some(id),
            _ => None,
        }
    }

    fn focus_targets(&self) -> Vec<SidebarFocusTarget> {
        let mut targets = Vec::new();
        if self.goal.is_some() {
            targets.push(SidebarFocusTarget::Section(SidebarSection::Goal));
        }
        if self
            .plan
            .as_ref()
            .is_some_and(|plan| !plan.items.is_empty())
        {
            targets.push(SidebarFocusTarget::Section(SidebarSection::Plan));
        }
        if !self.todos.is_empty() {
            targets.push(SidebarFocusTarget::Section(SidebarSection::Todos));
            if !self.todos_collapsed {
                targets.extend((0..self.todos.len()).map(SidebarFocusTarget::Todo));
            }
        }
        if !self.mcp_servers.is_empty() {
            targets.extend((0..self.mcp_servers.len()).map(SidebarFocusTarget::McpServer));
        }
        if !self.file_changes.is_empty() {
            targets.push(SidebarFocusTarget::Section(SidebarSection::FileChanges));
            if !self.file_changes_collapsed {
                targets.extend((0..self.file_changes.len()).map(SidebarFocusTarget::FileChange));
            }
        }
        if !self.tool_programs.is_empty() {
            targets.push(SidebarFocusTarget::Section(SidebarSection::ToolPrograms));
            if !self.tool_programs_collapsed {
                targets.extend(
                    self.tool_programs
                        .iter()
                        .map(|program| SidebarFocusTarget::ToolProgram(program.program_id.clone())),
                );
            }
        }
        if !self.agent_tree.is_empty() || !self.agent_runs.is_empty() {
            targets.push(SidebarFocusTarget::Section(SidebarSection::AgentRuns));
            if !self.agent_runs_collapsed {
                if !self.agent_tree.is_empty() {
                    targets.extend(
                        self.visible_agent_tree_rows()
                            .into_iter()
                            .map(|row| SidebarFocusTarget::AgentTreeNode(row.node.task_id)),
                    );
                }
                targets.extend(
                    self.detached_runs()
                        .map(|run| SidebarFocusTarget::AgentRun(run.run_id.clone())),
                );
            }
        }
        if !self.convergences.is_empty() {
            targets.push(SidebarFocusTarget::Section(SidebarSection::Convergences));
            if !self.convergences_collapsed {
                targets.extend(
                    self.convergences
                        .iter()
                        .map(|item| SidebarFocusTarget::Convergence(item.convergence_id.clone())),
                );
            }
        }
        targets
    }

    fn move_focus(&mut self, delta: isize) {
        let targets = self.focus_targets();
        if targets.is_empty() {
            self.focused_target = None;
            return;
        }
        let current = self
            .focused_target
            .as_ref()
            .and_then(|target| targets.iter().position(|candidate| candidate == target));
        let next = match current {
            Some(index) => (index as isize + delta).clamp(0, targets.len() as isize - 1) as usize,
            None if delta < 0 => 0,
            None => 0,
        };
        self.focused_target = targets.get(next).cloned();
        self.focused_position = Some(next);
    }

    fn reconcile_focus(&mut self) {
        let targets = self.focus_targets();
        let Some(current) = self.focused_target.clone() else {
            return;
        };
        if targets.contains(&current) {
            self.focused_position = targets.iter().position(|target| target == &current);
            return;
        }
        self.focused_target = self
            .focused_position
            .and_then(|index| targets.get(index.min(targets.len().saturating_sub(1))))
            .cloned();
        self.focused_position = self
            .focused_target
            .as_ref()
            .and_then(|target| targets.iter().position(|candidate| candidate == target));
    }

    fn set_focused_target(&mut self, target: Option<SidebarFocusTarget>) {
        self.focused_target = target;
        self.focused_position = self.focused_target.as_ref().and_then(|current| {
            self.focus_targets()
                .iter()
                .position(|candidate| candidate == current)
        });
    }

    fn hovered_to_focus(hovered: &HoveredElement) -> Option<SidebarFocusTarget> {
        match hovered {
            HoveredElement::Section(section) => Some(SidebarFocusTarget::Section(*section)),
            HoveredElement::Todo(index) => Some(SidebarFocusTarget::Todo(*index)),
            HoveredElement::McpServer(index) => Some(SidebarFocusTarget::McpServer(*index)),
            HoveredElement::FileChange(index) => Some(SidebarFocusTarget::FileChange(*index)),
            HoveredElement::ToolProgram(id) => Some(SidebarFocusTarget::ToolProgram(id.clone())),
            HoveredElement::AgentRun(id) => Some(SidebarFocusTarget::AgentRun(id.clone())),
            HoveredElement::AgentTreeNode(id) => Some(SidebarFocusTarget::AgentTreeNode(*id)),
            HoveredElement::Convergence(id) => Some(SidebarFocusTarget::Convergence(id.clone())),
            HoveredElement::None => None,
        }
    }

    fn toggle_section(&mut self, section: SidebarSection) {
        match section {
            SidebarSection::Goal => self.goal_collapsed = !self.goal_collapsed,
            SidebarSection::Plan => self.plan_collapsed = !self.plan_collapsed,
            SidebarSection::Todos => self.todos_collapsed = !self.todos_collapsed,
            SidebarSection::FileChanges => {
                self.file_changes_collapsed = !self.file_changes_collapsed
            }
            SidebarSection::ToolPrograms => {
                self.tool_programs_collapsed = !self.tool_programs_collapsed
            }
            SidebarSection::AgentRuns => self.agent_runs_collapsed = !self.agent_runs_collapsed,
            SidebarSection::Convergences => {
                self.convergences_collapsed = !self.convergences_collapsed
            }
        }
    }

    fn set_section_collapsed(&mut self, section: SidebarSection, collapsed: bool) {
        match section {
            SidebarSection::Goal => self.goal_collapsed = collapsed,
            SidebarSection::Plan => self.plan_collapsed = collapsed,
            SidebarSection::Todos => self.todos_collapsed = collapsed,
            SidebarSection::FileChanges => self.file_changes_collapsed = collapsed,
            SidebarSection::ToolPrograms => self.tool_programs_collapsed = collapsed,
            SidebarSection::AgentRuns => self.agent_runs_collapsed = collapsed,
            SidebarSection::Convergences => self.convergences_collapsed = collapsed,
        }
    }

    fn run_id_for_task(&self, task_id: u64) -> Option<String> {
        self.agent_runs
            .iter()
            .find(|run| run.task_id.as_deref().and_then(|s| s.parse::<u64>().ok()) == Some(task_id))
            .map(|run| run.run_id.clone())
    }

    fn detached_runs(&self) -> impl Iterator<Item = &SidebarAgentRun> {
        self.agent_runs.iter().filter(|run| {
            match run.task_id.as_deref().and_then(|s| s.parse::<u64>().ok()) {
                // Integer comparison: no per-comparison `to_string()` alloc.
                Some(id) => !self.agent_tree.iter().any(|node| node.task_id == id),
                // No task id never matches a tree node, so it is detached
                // (preserves the previous `None == Some(..)` behavior).
                None => true,
            }
        })
    }

    fn visible_agent_tree_rows(&self) -> Vec<SidebarAgentTreeRow> {
        let mut by_id = HashMap::new();
        for node in self.agent_tree.iter().cloned() {
            by_id.insert(node.task_id, node);
        }
        let mut children: HashMap<Option<u64>, Vec<u64>> = HashMap::new();
        for node in &self.agent_tree {
            let parent = node.parent_task_id.filter(|id| by_id.contains_key(id));
            children.entry(parent).or_default().push(node.task_id);
        }
        let mut output = Vec::new();
        let mut visited = HashSet::new();
        for id in children.get(&None).into_iter().flatten().copied() {
            self.append_agent_tree_row(id, 0, &by_id, &children, &mut visited, &mut output);
        }
        // A malformed cycle must not hide all remaining bounded nodes.
        for node in &self.agent_tree {
            if !visited.contains(&node.task_id)
                && !self.hidden_by_collapsed_ancestor(node.task_id, &by_id)
            {
                self.append_agent_tree_row(
                    node.task_id,
                    0,
                    &by_id,
                    &children,
                    &mut visited,
                    &mut output,
                );
            }
        }
        output
    }

    fn append_agent_tree_row(
        &self,
        id: u64,
        depth: usize,
        by_id: &HashMap<u64, AgentTreeNodeProjection>,
        children: &HashMap<Option<u64>, Vec<u64>>,
        visited: &mut HashSet<u64>,
        output: &mut Vec<SidebarAgentTreeRow>,
    ) {
        if !visited.insert(id) {
            return;
        }
        let Some(node) = by_id.get(&id).cloned() else {
            return;
        };
        let has_children = children
            .get(&Some(id))
            .is_some_and(|items| !items.is_empty());
        output.push(SidebarAgentTreeRow {
            node,
            depth,
            has_children,
            collapsed: self.collapsed_agent_nodes.contains(&id),
        });
        if !self.collapsed_agent_nodes.contains(&id) {
            for child in children.get(&Some(id)).into_iter().flatten().copied() {
                self.append_agent_tree_row(child, depth + 1, by_id, children, visited, output);
            }
        }
    }

    fn hidden_by_collapsed_ancestor(
        &self,
        id: u64,
        by_id: &HashMap<u64, AgentTreeNodeProjection>,
    ) -> bool {
        let mut current = id;
        let mut seen = HashSet::new();
        while let Some(parent) = by_id.get(&current).and_then(|node| node.parent_task_id) {
            if !seen.insert(parent) {
                return false;
            }
            if self.collapsed_agent_nodes.contains(&parent) {
                return true;
            }
            if !by_id.contains_key(&parent) {
                return false;
            }
            current = parent;
        }
        false
    }

    pub fn scroll_up(&mut self, area: Rect) {
        self.scroll_offset = self.scroll_offset.saturating_sub(scroll_step(area));
    }

    pub fn scroll_down(&mut self, area: Rect) {
        self.scroll_offset = (self.scroll_offset + scroll_step(area)).min(self.max_scroll(area));
    }

    pub fn toggle_hovered_section(&mut self) -> bool {
        let HoveredElement::Section(section) = self.hovered_element else {
            return false;
        };

        self.focused_target = Some(SidebarFocusTarget::Section(section));
        self.toggle_section(section);
        self.reconcile_focus();
        true
    }

    pub fn max_scroll(&self, area: Rect) -> usize {
        let viewport_height = sidebar_content_height(area) as usize;
        self.content_lines(area.width)
            .len()
            .saturating_sub(viewport_height)
    }

    pub fn set_hover_position(&mut self, x: u16, y: u16, area: Option<Rect>) {
        if let Some(area) = area {
            let rel_y = y.saturating_sub(area.y);
            let rel_x = x.saturating_sub(area.x);

            if rel_x > 0 && rel_x < area.width && rel_y > 0 && rel_y < area.height {
                self.hovered_element = self.element_at(rel_x, rel_y);
                self.tooltip_text = self.get_tooltip_for_hover();
            } else {
                self.hovered_element = HoveredElement::None;
                self.tooltip_text.clear();
            }
        }
    }

    pub fn clear_hover(&mut self) {
        self.hovered_element = HoveredElement::None;
        self.tooltip_text.clear();
    }

    pub fn get_tooltip(&self) -> Option<&str> {
        if self.tooltip_text.is_empty() {
            None
        } else {
            Some(&self.tooltip_text)
        }
    }

    fn element_at(&self, _x: u16, y: u16) -> HoveredElement {
        let visible_y = y.saturating_sub(1) as usize;
        self.line_targets()
            .get(self.scroll_offset + visible_y)
            .cloned()
            .unwrap_or(HoveredElement::None)
    }

    fn line_targets(&self) -> Vec<HoveredElement> {
        let mut targets = Vec::new();

        targets.push(HoveredElement::None);
        if self.session.is_some() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::None);
            if self
                .session
                .as_ref()
                .and_then(|s| s.share_url.as_ref())
                .is_some()
            {
                targets.push(HoveredElement::None);
            }
        } else {
            targets.push(HoveredElement::None);
        }

        targets.push(HoveredElement::None);
        targets.push(HoveredElement::None);
        targets.push(HoveredElement::None);
        if self.git_branch.is_some() && self.git_dirty {
            targets.push(HoveredElement::None);
        }
        if self.git_branch.is_some()
            && (self.git_staged_count > 0
                || self.git_unstaged_count > 0
                || self.git_untracked_count > 0
                || self.git_conflicted_count > 0)
        {
            targets.push(HoveredElement::None);
        }
        if self.git_branch.is_some() && (self.git_ahead.is_some() || self.git_behind.is_some()) {
            targets.push(HoveredElement::None);
        }

        targets.push(HoveredElement::None);
        targets.push(HoveredElement::None);
        targets.push(HoveredElement::None);
        if !self.provider.is_empty() {
            targets.push(HoveredElement::None);
        }
        targets.push(HoveredElement::None);

        if self.goal.is_some() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::Section(SidebarSection::Goal));
            if !self.goal_collapsed {
                targets.push(HoveredElement::None);
            }
        }

        if let Some(ref plan) = self.plan {
            if !plan.items.is_empty() {
                targets.push(HoveredElement::None);
                targets.push(HoveredElement::Section(SidebarSection::Plan));
                if !self.plan_collapsed {
                    for _ in &plan.items {
                        targets.push(HoveredElement::None);
                    }
                }
            }
        }

        if !self.todos.is_empty() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::Section(SidebarSection::Todos));
            if !self.todos_collapsed {
                for i in 0..self.todos.len() {
                    targets.push(HoveredElement::Todo(i));
                }
            }
        }

        if !self.mcp_servers.is_empty() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::None);
            for i in 0..self.mcp_servers.len() {
                targets.push(HoveredElement::McpServer(i));
            }
        }

        if !self.file_changes.is_empty() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::Section(SidebarSection::FileChanges));
            if !self.file_changes_collapsed {
                for i in 0..self.file_changes.len() {
                    targets.push(HoveredElement::FileChange(i));
                }
            }
        }

        if !self.tool_programs.is_empty() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::Section(SidebarSection::ToolPrograms));
            if !self.tool_programs_collapsed {
                targets.extend(
                    self.tool_programs
                        .iter()
                        .map(|program| HoveredElement::ToolProgram(program.program_id.clone())),
                );
            }
        }

        if !self.agent_tree.is_empty() || !self.agent_runs.is_empty() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::Section(SidebarSection::AgentRuns));
            if !self.agent_runs_collapsed {
                if !self.agent_tree.is_empty() {
                    targets.extend(
                        self.visible_agent_tree_rows()
                            .into_iter()
                            .map(|row| HoveredElement::AgentTreeNode(row.node.task_id)),
                    );
                }
                targets.extend(
                    self.detached_runs()
                        .map(|run| HoveredElement::AgentRun(run.run_id.clone())),
                );
            }
        }

        if !self.convergences.is_empty() {
            targets.push(HoveredElement::None);
            targets.push(HoveredElement::Section(SidebarSection::Convergences));
            if !self.convergences_collapsed {
                for convergence in &self.convergences {
                    targets.push(HoveredElement::Convergence(
                        convergence.convergence_id.clone(),
                    ));
                }
            }
        }

        targets
    }

    fn content_lines(&self, area_width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let width = area_width as usize;

        if let Some(sess) = &self.session {
            let title = clean_inline_text(&sess.title, width);
            lines.push(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(vec![
                Span::styled("id: ", Style::default().fg(self.theme.muted)),
                Span::raw(sess.id.chars().take(8).collect::<String>()),
            ]));
            if let Some(ref url) = sess.share_url {
                lines.push(Line::from(vec![
                    Span::styled("shared: ", Style::default().fg(self.theme.success)),
                    Span::raw(clean_inline_text(url, width.saturating_sub(10))),
                ]));
            }
        } else {
            lines.push(self.section_header(" Session "));
            lines.push(Line::from(Span::styled(
                "no session",
                Style::default().fg(self.theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(self.section_header(" Git "));
        if let Some(ref branch) = self.git_branch {
            lines.push(Line::from(vec![
                Span::styled("branch: ", Style::default().fg(self.theme.muted)),
                Span::raw(clean_inline_text(branch, width.saturating_sub(10))),
            ]));
            if self.git_dirty {
                let mut status_spans = vec![Span::styled(
                    "status: ",
                    Style::default().fg(self.theme.warning),
                )];
                let mut parts = Vec::new();
                if self.git_staged_count > 0 {
                    parts.push(format!("{} staged", self.git_staged_count));
                }
                if self.git_unstaged_count > 0 {
                    parts.push(format!("{} unstaged", self.git_unstaged_count));
                }
                if self.git_untracked_count > 0 {
                    parts.push(format!("{} untracked", self.git_untracked_count));
                }
                if self.git_conflicted_count > 0 {
                    parts.push(format!("{} conflicted", self.git_conflicted_count));
                }
                if parts.is_empty() {
                    parts.push("dirty".to_string());
                }
                status_spans.push(Span::raw(parts.join(", ")));
                lines.push(Line::from(status_spans));
            }
            if self.git_ahead.is_some() || self.git_behind.is_some() {
                let mut sync_spans = vec![Span::styled(
                    "sync: ",
                    Style::default().fg(self.theme.muted),
                )];
                let mut sync_parts = Vec::new();
                if let Some(ahead) = self.git_ahead {
                    sync_parts.push(format!("↑{ahead}"));
                }
                if let Some(behind) = self.git_behind {
                    sync_parts.push(format!("↓{behind}"));
                }
                sync_spans.push(Span::raw(sync_parts.join(" ")));
                lines.push(Line::from(sync_spans));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "not a git repo",
                Style::default().fg(self.theme.muted),
            )));
        }

        lines.push(Line::from(""));
        lines.push(self.section_header(" Config "));
        lines.push(Line::from(vec![
            Span::styled("agent: ", Style::default().fg(self.theme.muted)),
            Span::raw(clean_inline_text(&self.agent, width.saturating_sub(9))),
        ]));
        if !self.provider.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("provider: ", Style::default().fg(self.theme.muted)),
                Span::raw(clean_inline_text(&self.provider, width.saturating_sub(12))),
            ]));
        }
        let model_short = self.model.split('/').next_back().unwrap_or(&self.model);
        lines.push(Line::from(vec![
            Span::styled("model: ", Style::default().fg(self.theme.muted)),
            Span::raw(clean_inline_text(model_short, width.saturating_sub(9))),
        ]));

        if self.goal.is_some() {
            lines.push(Line::from(""));
            lines.push(self.collapsible_header_for(
                " Goal ",
                self.goal_collapsed,
                SidebarSection::Goal,
            ));
            if let Some(ref goal) = self.goal {
                if !self.goal_collapsed {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", clean_inline_text(goal, width.saturating_sub(4))),
                        Style::default().fg(self.theme.foreground),
                    )));
                }
            }
        }

        if let Some(ref plan) = self.plan {
            if !plan.items.is_empty() {
                lines.push(Line::from(""));
                lines.push(self.collapsible_header_for(
                    &format!(" Plan ({}) ", plan.items.len()),
                    self.plan_collapsed,
                    SidebarSection::Plan,
                ));
                if !self.plan_collapsed {
                    for item in &plan.items {
                        let (icon, style) = match item.status {
                            crate::session::events::PlanItemStatus::Done => {
                                ("[x]", Style::default().fg(self.theme.success))
                            }
                            crate::session::events::PlanItemStatus::InProgress => {
                                ("[>]", Style::default().fg(self.theme.warning))
                            }
                            crate::session::events::PlanItemStatus::Skipped => {
                                ("[-]", Style::default().fg(self.theme.muted))
                            }
                            crate::session::events::PlanItemStatus::Blocked => {
                                ("[?]", Style::default().fg(self.theme.error))
                            }
                            crate::session::events::PlanItemStatus::Pending => {
                                ("[ ]", Style::default().fg(self.theme.muted))
                            }
                        };
                        lines.push(Line::from(vec![
                            Span::styled(format!("  {} ", icon), style),
                            Span::styled(
                                clean_inline_text(&item.text, width.saturating_sub(8)),
                                Style::default().fg(self.theme.foreground),
                            ),
                        ]));
                    }
                }
            }
        }

        if !self.todos.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.collapsible_header_for(
                &format!(" Todos ({}) ", self.todos.len()),
                self.todos_collapsed,
                SidebarSection::Todos,
            ));
            if !self.todos_collapsed {
                for todo in &self.todos {
                    let status_icon = match todo.status.as_str() {
                        "completed" => "[x]",
                        "in_progress" => "[>]",
                        _ => "[ ]",
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{status_icon} "),
                            Style::default().fg(self.theme.muted),
                        ),
                        Span::raw(clean_inline_text(&todo.content, width.saturating_sub(6))),
                    ]));
                }
            }
        }

        if !self.mcp_servers.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.section_header(" MCP Servers "));
            for (name, status) in &self.mcp_servers {
                let dot = match status.as_str() {
                    "connected" => "*",
                    "connecting" => "~",
                    "error" => "!",
                    _ => "-",
                };
                let dot_style = match status.as_str() {
                    "connected" => Style::default().fg(self.theme.success),
                    "connecting" => Style::default().fg(self.theme.warning),
                    "error" => Style::default().fg(self.theme.error),
                    _ => Style::default().fg(self.theme.muted),
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{dot} "), dot_style),
                    Span::raw(clean_inline_text(name, width.saturating_sub(4))),
                ]));
            }
        }

        if !self.file_changes.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.collapsible_header_for(
                &format!(" Modified Files ({}) ", self.file_changes.len()),
                self.file_changes_collapsed,
                SidebarSection::FileChanges,
            ));
            if !self.file_changes_collapsed {
                for change in &self.file_changes {
                    let (stats_text, stats_style) = match &change.diff_state {
                        DiffStatsState::Ready {
                            additions,
                            deletions,
                            ..
                        } => (format!("+{additions} -{deletions}"), Style::default()),
                        DiffStatsState::Pending { .. } => {
                            ("diff...".to_string(), Style::default().fg(self.theme.muted))
                        }
                        DiffStatsState::Skipped { reason, .. } => {
                            (reason.to_string(), Style::default().fg(self.theme.muted))
                        }
                        DiffStatsState::Error { .. } => (
                            "diff err".to_string(),
                            Style::default().fg(self.theme.error),
                        ),
                    };
                    let stats_width = stats_text.len() + 1;
                    let mut spans = vec![
                        Span::styled(
                            format!("  {} ", clean_inline_text(&change.action, 1)),
                            Style::default().fg(self.theme.warning),
                        ),
                        Span::raw(clean_inline_text(
                            &change.path,
                            width.saturating_sub(6 + stats_width),
                        )),
                        Span::raw(" "),
                    ];
                    // For Ready state, split into colored +/- spans.
                    if let DiffStatsState::Ready {
                        additions,
                        deletions,
                        ..
                    } = &change.diff_state
                    {
                        spans.push(Span::styled(
                            format!("+{additions}"),
                            Style::default().fg(self.theme.success),
                        ));
                        spans.push(Span::raw(" "));
                        spans.push(Span::styled(
                            format!("-{deletions}"),
                            Style::default().fg(self.theme.error),
                        ));
                    } else {
                        spans.push(Span::styled(stats_text, stats_style));
                    }
                    lines.push(Line::from(spans));
                }
            }
        }

        if !self.tool_programs.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.collapsible_header_for(
                &format!(" Tool Programs ({}) ", self.tool_programs.len()),
                self.tool_programs_collapsed,
                SidebarSection::ToolPrograms,
            ));
            if !self.tool_programs_collapsed {
                for prog in &self.tool_programs {
                    let state_icon = match prog.state.as_str() {
                        "completed" => ("✓", Style::default().fg(self.theme.success)),
                        "failed" => ("✗", Style::default().fg(self.theme.error)),
                        "running" => ("●", Style::default().fg(self.theme.warning)),
                        "admitted" => ("○", Style::default().fg(self.theme.muted)),
                        _ => ("~", Style::default().fg(self.theme.muted)),
                    };
                    let short_id: String = prog.program_id.chars().take(8).collect();
                    let summary_part = prog
                        .summary
                        .as_deref()
                        .map(|s| format!(" {}", clean_inline_text(s, width.saturating_sub(20))))
                        .unwrap_or_default();
                    let target = SidebarFocusTarget::ToolProgram(prog.program_id.clone());
                    lines.push(self.with_row_style(
                        Line::from(vec![
                            Span::styled(format!("  {} ", state_icon.0), state_icon.1),
                            Span::styled(
                                clean_inline_text(&short_id, 8),
                                Style::default().fg(self.theme.muted),
                            ),
                            Span::raw(format!(" ({})", prog.state)),
                            Span::raw(summary_part),
                        ]),
                        &target,
                    ));
                }
            }
        }

        if !self.agent_tree.is_empty() || !self.agent_runs.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.collapsible_header_for(
                &format!(
                    " Agent Runs ({}) ",
                    self.agent_tree.len().max(self.agent_runs.len())
                ),
                self.agent_runs_collapsed,
                SidebarSection::AgentRuns,
            ));
            if !self.agent_runs_collapsed {
                if !self.agent_tree.is_empty() {
                    for row in self.visible_agent_tree_rows() {
                        let node = &row.node;
                        let run = self.agent_runs.iter().find(|run| {
                            run.task_id.as_deref() == Some(node.task_id.to_string().as_str())
                        });
                        let (icon, style) = match node.status {
                            AgentTreeStatus::Completed => {
                                ("✓", Style::default().fg(self.theme.success))
                            }
                            AgentTreeStatus::Failed => ("✗", Style::default().fg(self.theme.error)),
                            AgentTreeStatus::Running => {
                                ("●", Style::default().fg(self.theme.warning))
                            }
                        };
                        let marker = if row.has_children {
                            if row.collapsed {
                                "[+]"
                            } else {
                                "[-]"
                            }
                        } else {
                            "  "
                        };
                        let indent = "  ".repeat(row.depth.min(8));
                        let identity = run
                            .map(|run| run.run_id.chars().take(8).collect::<String>())
                            .unwrap_or_else(|| format!("task-{}", node.task_id));
                        let location = run
                            .and_then(|run| run.branch.as_deref().or(run.worktree.as_deref()))
                            .map(|value| format!(" {value}"))
                            .unwrap_or_default();
                        let commit = run
                            .and_then(|run| run.result_commit.as_deref())
                            .map(|value| {
                                format!(" -> {}", value.chars().take(8).collect::<String>())
                            })
                            .unwrap_or_default();
                        let progress = run
                            .and_then(|run| run.progress.as_deref())
                            .or(node.result_summary.as_deref())
                            .map(|value| {
                                format!(" {}", clean_inline_text(value, width.saturating_sub(25)))
                            })
                            .unwrap_or_default();
                        let target = SidebarFocusTarget::AgentTreeNode(node.task_id);
                        lines.push(self.with_row_style(
                            Line::from(vec![
                                Span::raw(indent),
                                Span::styled(format!("{marker} {icon} "), style),
                                Span::styled(identity, Style::default().fg(self.theme.muted)),
                                Span::raw(format!(
                                    " {}{}{}{}",
                                    node.agent, location, commit, progress
                                )),
                            ]),
                            &target,
                        ));
                    }
                }
                for run in self.detached_runs() {
                    let (icon, style) = if run.attention_required {
                        ("!", Style::default().fg(self.theme.error))
                    } else {
                        match run.status.as_str() {
                            "completed" => ("✓", Style::default().fg(self.theme.success)),
                            "failed" | "cancelled" | "interrupted" => {
                                ("✗", Style::default().fg(self.theme.error))
                            }
                            "running" | "waiting" => ("●", Style::default().fg(self.theme.warning)),
                            _ => ("○", Style::default().fg(self.theme.muted)),
                        }
                    };
                    let id: String = run.run_id.chars().take(8).collect();
                    let location = run
                        .branch
                        .as_deref()
                        .or(run.worktree.as_deref())
                        .map(|value| format!(" {value}"))
                        .unwrap_or_default();
                    let commit = run
                        .result_commit
                        .as_deref()
                        .map(|value| format!(" -> {}", value.chars().take(8).collect::<String>()))
                        .unwrap_or_default();
                    let target = SidebarFocusTarget::AgentRun(run.run_id.clone());
                    lines.push(self.with_row_style(
                        Line::from(vec![
                            Span::styled(format!("  {icon} "), style),
                            Span::styled(id, Style::default().fg(self.theme.muted)),
                            Span::raw(format!(" {}{}{}", run.agent, location, commit)),
                        ]),
                        &target,
                    ));
                }
            }
        }

        if !self.convergences.is_empty() {
            lines.push(Line::from(""));
            lines.push(self.collapsible_header_for(
                &format!(" Convergences ({}) ", self.convergences.len()),
                self.convergences_collapsed,
                SidebarSection::Convergences,
            ));
            if !self.convergences_collapsed {
                for convergence in &self.convergences {
                    let awaiting = convergence.awaiting_decision;
                    let terminal = matches!(
                        convergence.status.as_str(),
                        "completed" | "failed" | "cancelled" | "exhausted"
                    );
                    let icon = if awaiting {
                        "?"
                    } else if terminal {
                        "✓"
                    } else {
                        "●"
                    };
                    let style = if awaiting {
                        Style::default().fg(self.theme.warning)
                    } else {
                        Style::default().fg(self.theme.muted)
                    };
                    let id: String = convergence.convergence_id.chars().take(8).collect();
                    let verdict = convergence
                        .verdict_kind
                        .as_deref()
                        .or(convergence.verifier_run_id.as_deref())
                        .unwrap_or("pending");
                    let target =
                        SidebarFocusTarget::Convergence(convergence.convergence_id.clone());
                    lines.push(self.with_row_style(
                        Line::from(vec![
                            Span::styled(format!("  {icon} "), style),
                            Span::styled(id, Style::default().fg(self.theme.muted)),
                            Span::raw(format!(
                                " {} (cycle {}/{}, {} left) P:{}/{} V:{} F:{}",
                                convergence.status,
                                convergence.cycle_ordinal + 1,
                                convergence.max_cycles,
                                convergence.remaining_cycles,
                                convergence.producer_completed,
                                convergence.producer_completed + convergence.producer_active,
                                clean_inline_text(verdict, width.saturating_sub(35)),
                                convergence.last_finding_count,
                            )),
                        ]),
                        &target,
                    ));
                }
            }
        }

        lines
    }

    fn get_tooltip_for_hover(&self) -> String {
        match &self.hovered_element {
            HoveredElement::Section(section) => match section {
                SidebarSection::Goal => "Click to collapse/expand goal".to_string(),
                SidebarSection::Plan => "Click to collapse/expand plan".to_string(),
                SidebarSection::Todos => "Click to collapse/expand todos".to_string(),
                SidebarSection::FileChanges => {
                    "Click to collapse/expand modified files".to_string()
                }
                SidebarSection::ToolPrograms => {
                    "Click to collapse/expand tool programs".to_string()
                }
                SidebarSection::AgentRuns => "Click to collapse/expand agent runs".to_string(),
                SidebarSection::Convergences => {
                    "Click to collapse/expand convergence runs".to_string()
                }
            },
            HoveredElement::Todo(idx) => self
                .todos
                .get(*idx)
                .map(|todo| format!("Todo: {} [{}]", todo.content, todo.status))
                .unwrap_or_default(),
            HoveredElement::McpServer(idx) => self
                .mcp_servers
                .get(*idx)
                .map(|(name, status)| format!("MCP Server: {} ({})", name, status))
                .unwrap_or_default(),
            HoveredElement::FileChange(idx) => self
                .file_changes
                .get(*idx)
                .map(|change| {
                    let stats_str = match &change.diff_state {
                        DiffStatsState::Ready {
                            additions,
                            deletions,
                            ..
                        } => {
                            format!("+{additions} -{deletions}")
                        }
                        DiffStatsState::Pending { .. } => "computing diff...".to_string(),
                        DiffStatsState::Skipped { reason, .. } => format!("skipped: {reason}"),
                        DiffStatsState::Error { message, .. } => format!("error: {message}"),
                    };
                    format!(
                        "Modified: {} ({}, {})",
                        change.path, change.action, stats_str
                    )
                })
                .unwrap_or_default(),
            HoveredElement::ToolProgram(id) => self
                .tool_programs
                .iter()
                .find(|program| &program.program_id == id)
                .map(|program| format!("Tool program: {} ({})", id, program.state))
                .unwrap_or_default(),
            HoveredElement::AgentRun(id) => self
                .agent_runs
                .iter()
                .find(|run| &run.run_id == id)
                .map(|run| format!("Agent run: {} ({}) — Enter to inspect", id, run.status))
                .unwrap_or_default(),
            HoveredElement::AgentTreeNode(task_id) => self
                .agent_tree
                .iter()
                .find(|node| node.task_id == *task_id)
                .map(|node| {
                    format!(
                        "Agent {} ({:?}) — Enter to inspect",
                        node.agent, node.status
                    )
                })
                .unwrap_or_default(),
            HoveredElement::Convergence(id) => self
                .convergences
                .iter()
                .find(|item| &item.convergence_id == id)
                .map(|item| format!("Convergence: {} ({})", id, item.status))
                .unwrap_or_default(),
            HoveredElement::None => String::new(),
        }
    }

    fn section_header(&self, label: &str) -> Line<'static> {
        // Section titles share the muted (placeholder) text color so the
        // sidebar reads with a single text hue. `theme.primary` is often
        // a near-background accent in Halloy themes (e.g. Cyber Red's
        // #230202) which renders the title invisible.
        let style = Style::default()
            .fg(self.theme.muted)
            .add_modifier(Modifier::BOLD);
        Line::from(Span::styled(label.to_string(), style))
    }

    fn collapsible_header(&self, label: &str, collapsed: bool) -> Line<'static> {
        let marker = if collapsed { "[+]" } else { "[-]" };
        let style = Style::default()
            .fg(self.theme.muted)
            .add_modifier(Modifier::BOLD);
        Line::from(vec![
            Span::styled(format!("{marker} "), Style::default().fg(self.theme.muted)),
            Span::styled(label.to_string(), style),
        ])
    }

    fn collapsible_header_for(
        &self,
        label: &str,
        collapsed: bool,
        section: SidebarSection,
    ) -> Line<'static> {
        self.with_row_style(
            self.collapsible_header(label, collapsed),
            &SidebarFocusTarget::Section(section),
        )
    }

    fn with_row_style(&self, line: Line<'static>, target: &SidebarFocusTarget) -> Line<'static> {
        if self.focused_target.as_ref() == Some(target) {
            line.style(self.theme.selection_style())
        } else {
            line
        }
    }
}

impl Default for SidebarWidget {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &SidebarWidget {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let lines = self.content_lines(area.width);
        let scroll_offset = self.scroll_offset.min(self.max_scroll(area));

        let block = Block::default()
            .title(" Sidebar ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .scroll((scroll_offset as u16, 0));
        paragraph.render(area, buf);

        if self.max_scroll(area) > 0 {
            let mut state =
                ScrollbarState::new(self.content_lines(area.width).len()).position(scroll_offset);
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(self.theme.foreground))
                .track_style(Style::default().fg(self.theme.border))
                .begin_symbol(None)
                .end_symbol(None)
                .render(area, buf, &mut state);
        }

        if let Some(tooltip) = self.get_tooltip() {
            let tooltip_width = (tooltip.len() as u16 + 4).min(area.width.saturating_sub(3));
            let tooltip_x = area.x + 1;
            let tooltip_y = area.y + area.height.saturating_sub(2);

            if tooltip_y > area.y {
                let tooltip_area = Rect::new(tooltip_x, tooltip_y, tooltip_width, 2);
                let tooltip_block = Block::default()
                    .border_style(Style::default().fg(self.theme.border))
                    .borders(Borders::ALL)
                    .style(Style::default().bg(self.theme.background));

                let tooltip_text =
                    Paragraph::new(tooltip).style(Style::default().fg(self.theme.foreground));

                tooltip_block.render(tooltip_area, buf);
                tooltip_text.render(
                    Rect::new(
                        tooltip_area.x + 1,
                        tooltip_area.y + 1,
                        tooltip_area.width.saturating_sub(2),
                        1,
                    ),
                    buf,
                );
            }
        }
    }
}

fn scroll_step(area: Rect) -> usize {
    ((sidebar_content_height(area) as usize) / 3).max(1)
}

fn sidebar_content_height(area: Rect) -> u16 {
    area.height.saturating_sub(2)
}

pub fn clean_inline_text(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch == '\n' || ch == '\r' || ch.is_control() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        out.push(ch);
    }

    if max_chars == 0 {
        return String::new();
    }

    let count = out.chars().count();
    if count <= max_chars {
        return out;
    }

    if max_chars <= 1 {
        return "...".chars().take(max_chars).collect();
    }

    let keep = max_chars.saturating_sub(1);
    format!("{}…", out.chars().take(keep).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(
        task_id: u64,
        parent_task_id: Option<u64>,
        status: AgentTreeStatus,
    ) -> AgentTreeNodeProjection {
        AgentTreeNodeProjection {
            task_id,
            agent: format!("agent-{task_id}"),
            description: format!("description-{task_id}"),
            status,
            parent_task_id,
            created_at: task_id as i64,
            completed_at: None,
            result_summary: None,
        }
    }

    fn run(run_id: &str, task_id: &str) -> SidebarAgentRun {
        SidebarAgentRun {
            run_id: run_id.to_string(),
            task_id: Some(task_id.to_string()),
            agent: "agent".to_string(),
            status: "completed".to_string(),
            worktree: None,
            branch: None,
            result_commit: None,
            attention_required: false,
            progress: None,
        }
    }

    #[test]
    fn logical_focus_targets_follow_nested_collapse() {
        let mut sidebar = SidebarWidget::default();
        sidebar.set_agent_tree(vec![
            node(1, None, AgentTreeStatus::Running),
            node(2, Some(1), AgentTreeStatus::Completed),
            node(3, Some(2), AgentTreeStatus::Failed),
        ]);

        assert_eq!(sidebar.focus_targets().len(), 4);
        sidebar.focus_sidebar();
        sidebar.focus_next();
        assert_eq!(
            sidebar.focused_target(),
            Some(&SidebarFocusTarget::AgentTreeNode(1))
        );
        sidebar.toggle_focused();
        assert_eq!(sidebar.focus_targets().len(), 2);
        assert_eq!(
            sidebar.focused_target(),
            Some(&SidebarFocusTarget::AgentTreeNode(1))
        );
    }

    #[test]
    fn nested_agent_rows_are_parent_first_and_bounded() {
        let mut sidebar = SidebarWidget::default();
        sidebar.set_agent_tree(vec![
            node(3, Some(2), AgentTreeStatus::Failed),
            node(1, None, AgentTreeStatus::Running),
            node(2, Some(1), AgentTreeStatus::Completed),
        ]);
        let rows = sidebar.visible_agent_tree_rows();
        assert_eq!(
            rows.iter().map(|row| row.node.task_id).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(
            rows.iter().map(|row| row.depth).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(rows[2].node.status, AgentTreeStatus::Failed);
    }

    #[test]
    fn selected_node_disappearance_clamps_without_rebinding_by_index() {
        let mut sidebar = SidebarWidget::default();
        sidebar.set_agent_tree(vec![
            node(1, None, AgentTreeStatus::Running),
            node(2, Some(1), AgentTreeStatus::Running),
        ]);
        sidebar.focus_sidebar();
        sidebar.focus_next();
        sidebar.focus_next();
        assert_eq!(
            sidebar.focused_target(),
            Some(&SidebarFocusTarget::AgentTreeNode(2))
        );

        sidebar.set_agent_tree(vec![node(1, None, AgentTreeStatus::Completed)]);
        assert_eq!(
            sidebar.focused_target(),
            Some(&SidebarFocusTarget::AgentTreeNode(1))
        );
    }

    #[test]
    fn leaf_tree_node_joins_exact_durable_task_id_for_inspection() {
        let mut sidebar = SidebarWidget::default();
        sidebar.set_agent_runs(vec![run("run-7", "7")]);
        sidebar.set_agent_tree(vec![node(7, None, AgentTreeStatus::Completed)]);
        sidebar.focus_sidebar();
        sidebar.focus_next();
        assert_eq!(
            sidebar.activate_focused(),
            SidebarActivation::InspectRun("run-7".to_string())
        );
    }
}
