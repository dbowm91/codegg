//! M006-D: project-scoped file-tree widget.
//!
//! Renders one row per visible tree entry: an indent guide by depth, a
//! directory/file marker, an expand caret for directories, and the selected
//! row highlighted. A truncation notice occupies the last row when the walk
//! hit a bound, so a partial tree never presents itself as complete.
//!
//! ## Not a content path
//!
//! This widget draws directory *entries* only. It has no API for reading a
//! file, and opening an entry is the caller's job through `open_editor`.
//! Document text stays owned by the M005 controller
//! (`scripts/check_tui_editor_text_authority.py`).
//!
//! ## Bounded work
//!
//! Only the rows that fit the pane are materialized, so per-frame cost is a
//! function of the viewport rather than of the workspace.

use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::app::state::file_tree::{FileTreeState, FileTreeStatus};
use crate::tui::file_tree::{TreeNodeKind, TreeRow};
use crate::tui::theme::Theme;

/// Minimum width for the tree pane. Below this the caller renders the editor
/// full width instead of a degenerate pane.
pub const MIN_TREE_PANE_WIDTH: u16 = 22;

/// Fraction of the editor viewport given to the tree pane.
pub const TREE_PANE_RATIO: u16 = 2;

/// Split `area` into `(tree, editor)`, or `(None, area)` when the pane does
/// not fit.
///
/// Degrading to the full-width editor rather than producing a zero-width pane
/// is deliberate: a one-column tree would be worse than no tree.
pub fn split_pane(area: Rect, visible: bool) -> (Option<Rect>, Rect) {
    if !visible || area.width < MIN_TREE_PANE_WIDTH.saturating_mul(2) {
        return (None, area);
    }
    let width = (area.width / TREE_PANE_RATIO).max(MIN_TREE_PANE_WIDTH);
    if width >= area.width {
        return (None, area);
    }
    let tree = Rect {
        x: area.x,
        y: area.y,
        width,
        height: area.height,
    };
    let editor = Rect {
        x: area.x + width,
        y: area.y,
        width: area.width - width,
        height: area.height,
    };
    (Some(tree), editor)
}

pub struct FileTreeView<'a> {
    pub state: &'a FileTreeState,
    pub focused: bool,
    pub theme: Arc<Theme>,
}

impl<'a> FileTreeView<'a> {
    pub fn new(state: &'a FileTreeState, focused: bool, theme: Arc<Theme>) -> Self {
        Self {
            state,
            focused,
            theme,
        }
    }

    pub fn render(self, frame: &mut Frame, area: Rect) {
        let theme = Arc::clone(&self.theme);
        let title = format!(
            " {} · {}{} ",
            self.root_label(),
            self.status_label(),
            if self.focused { " [tree]" } else { "" },
        );
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if self.focused {
                theme.primary
            } else {
                theme.border
            }))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let rows = self.state.rows();
        // One row is reserved for the truncation notice when there is one, so
        // the listing is never silently cropped by the pane height.
        let notice = self.state.truncated.as_deref();
        let notice_rows = u16::from(notice.is_some());
        let visible_height = inner.height.saturating_sub(notice_rows) as usize;

        let total = rows.len();
        let scroll = self.state.scroll.min(total.saturating_sub(1));
        let end = (scroll + visible_height).min(total);

        let mut lines: Vec<Line> = Vec::with_capacity(visible_height + 1);
        if total == 0 {
            lines.push(Line::from(Span::styled(
                self.empty_label(),
                Style::default().fg(theme.muted),
            )));
        }
        for row in &rows[scroll..end] {
            lines.push(self.row_line(row));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        if let Some(notice) = notice {
            let notice_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(1),
                width: inner.width,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("… {notice}"),
                    Style::default().fg(theme.warning),
                )),
                notice_area,
            );
        }
    }

    fn row_line(&self, row: &TreeRow) -> Line<'static> {
        let theme = Arc::clone(&self.theme);
        let selected = self.state.selected.as_deref() == Some(row.path.as_str());
        let marker = match row.kind {
            TreeNodeKind::Directory => {
                if row.expanded {
                    "▾ "
                } else {
                    "▸ "
                }
            }
            TreeNodeKind::File => "  ",
        };
        let indent = "  ".repeat(row.depth);
        let name_style = if selected {
            Style::default()
                .fg(theme.background)
                .add_modifier(Modifier::BOLD)
        } else {
            match row.kind {
                TreeNodeKind::Directory => Style::default().fg(theme.primary),
                TreeNodeKind::File => Style::default().fg(theme.foreground),
            }
        };
        let mut spans = vec![
            Span::raw(indent),
            Span::styled(marker, Style::default().fg(theme.muted)),
            Span::styled(row.name.clone(), name_style),
        ];
        if selected {
            spans.push(Span::raw(""));
        }
        Line::from(spans)
    }

    fn root_label(&self) -> String {
        self.state
            .root
            .as_ref()
            .and_then(|root| root.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_string()
    }

    fn status_label(&self) -> String {
        match &self.state.status {
            FileTreeStatus::Idle => String::new(),
            FileTreeStatus::Loading => "· loading…".to_string(),
            FileTreeStatus::Ready => String::new(),
            FileTreeStatus::Error(message) => format!("· {message}"),
        }
    }

    fn empty_label(&self) -> String {
        match &self.state.status {
            FileTreeStatus::Error(message) => message.clone(),
            FileTreeStatus::Loading => "Scanning workspace…".to_string(),
            _ => "Empty workspace".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::file_tree::FileTreeState;
    use crate::tui::file_tree::build_tree;
    use crate::tui::theme::Theme;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn theme() -> Arc<Theme> {
        Arc::new(Theme::default())
    }

    fn populated() -> (tempfile::TempDir, FileTreeState) {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("src")).expect("mkdir");
        std::fs::write(root.path().join("src/lib.rs"), b"x").expect("write");
        std::fs::write(root.path().join("README.md"), b"x").expect("write");

        let mut state = FileTreeState {
            visible: true,
            ..FileTreeState::default()
        };
        let (generation, request_id) = state.begin_rebuild(root.path().to_path_buf());
        state.apply_listing(
            generation,
            request_id,
            root.path().to_path_buf(),
            build_tree(root.path()),
        );
        (root, state)
    }

    fn render_to_lines(state: &FileTreeState, focused: bool, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                FileTreeView::new(state, focused, theme()).render(frame, frame.area());
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn pane_splits_only_when_there_is_room() {
        let wide = Rect::new(0, 0, 120, 40);
        let (tree, editor) = split_pane(wide, true);
        let tree = tree.expect("wide area splits");
        assert!(tree.width >= MIN_TREE_PANE_WIDTH);
        assert_eq!(tree.width + editor.width, wide.width);
        assert_eq!(editor.x, tree.x + tree.width);

        let narrow = Rect::new(0, 0, MIN_TREE_PANE_WIDTH, 40);
        let (tree, editor) = split_pane(narrow, true);
        assert!(
            tree.is_none(),
            "a narrow terminal keeps the full-width editor"
        );
        assert_eq!(editor, narrow);
    }

    #[test]
    fn hidden_pane_never_splits() {
        let wide = Rect::new(0, 0, 120, 40);
        let (tree, editor) = split_pane(wide, false);
        assert!(tree.is_none());
        assert_eq!(editor, wide);
    }

    #[test]
    fn renders_entries_and_the_selected_row() {
        let (_root, state) = populated();
        let text = render_to_lines(&state, true, 40, 20);
        assert!(text.contains("src"), "the directory should render");
        assert!(text.contains("README.md"), "the file should render");
        assert!(
            text.contains("[tree]"),
            "focus should be visible in the title"
        );
    }

    #[test]
    fn expanding_reveals_descendants_and_collapsing_hides_them() {
        let (_root, mut state) = populated();
        // The tree starts fully collapsed, so the child is hidden.
        let collapsed = render_to_lines(&state, true, 40, 20);
        assert!(collapsed.contains("src"), "the directory row stays");
        assert!(!collapsed.contains("lib.rs"), "a collapsed child is hidden");

        assert_eq!(state.toggle_selected().as_deref(), Some("src"));
        let expanded = render_to_lines(&state, true, 40, 20);
        assert!(expanded.contains("lib.rs"), "expanding reveals the child");

        assert_eq!(state.toggle_selected().as_deref(), Some("src"));
        let recollapsed = render_to_lines(&state, true, 40, 20);
        assert!(
            recollapsed.contains("src"),
            "the directory row stays reachable"
        );
        assert!(
            !recollapsed.contains("lib.rs"),
            "collapsing hides the child again"
        );
    }

    #[test]
    fn an_empty_workspace_renders_a_label_not_a_blank_pane() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut state = FileTreeState {
            visible: true,
            ..FileTreeState::default()
        };
        let (generation, request_id) = state.begin_rebuild(root.path().to_path_buf());
        state.apply_listing(
            generation,
            request_id,
            root.path().to_path_buf(),
            build_tree(root.path()),
        );
        assert!(render_to_lines(&state, true, 40, 20).contains("Empty workspace"));
    }

    #[test]
    fn a_truncation_notice_is_rendered() {
        let root = tempfile::tempdir().expect("tempdir");
        for i in 0..(crate::tui::file_tree::MAX_TREE_NODES + 5) {
            std::fs::write(root.path().join(format!("f{i:06}.txt")), b"x").expect("write");
        }
        let mut state = FileTreeState {
            visible: true,
            ..FileTreeState::default()
        };
        let (generation, request_id) = state.begin_rebuild(root.path().to_path_buf());
        state.apply_listing(
            generation,
            request_id,
            root.path().to_path_buf(),
            build_tree(root.path()),
        );
        let text = render_to_lines(&state, true, 60, 20);
        assert!(
            text.contains("stopped at the maximum"),
            "a truncated tree must say so, got:\n{text}"
        );
    }

    #[test]
    fn a_zero_area_pane_does_not_panic() {
        let (_root, state) = populated();
        let _ = render_to_lines(&state, true, 0, 0);
        let _ = render_to_lines(&state, true, 1, 1);
    }

    #[test]
    fn a_loading_tree_says_it_is_scanning() {
        let state = FileTreeState {
            visible: true,
            status: FileTreeStatus::Loading,
            ..FileTreeState::default()
        };
        assert!(render_to_lines(&state, true, 40, 20).contains("Scanning workspace"));
    }
}
