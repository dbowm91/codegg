//! M006-D: project-scoped file-tree widget.
//!
//! Renders one row per visible tree entry: an indent guide by depth, a
//! per-type icon, and the selected row highlighted. A truncation notice
//! occupies the last row when the walk hit a bound, so a partial tree never
//! presents itself as complete.
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
//! function of the viewport rather than of the workspace. That same viewport
//! is returned to the caller rather than written into the state from here: a
//! widget borrows the state it is handed, and `FileTreeState::viewport_rows`
//! is layout published by the renderer, not navigation decided in a draw.

use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::app::state::file_tree::{FileTreeState, FileTreeStatus};
use crate::tui::file_icons::{self, FileIconStyle};
use crate::tui::file_tree::TreeRow;
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

/// What the pane measured while drawing itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaneLayout {
    /// Rows the listing area could show. `0` when the pane did not fit.
    pub viewport_rows: usize,
}

impl<'a> FileTreeView<'a> {
    pub fn new(state: &'a FileTreeState, focused: bool, theme: Arc<Theme>) -> Self {
        Self {
            state,
            focused,
            theme,
        }
    }

    /// Draw the pane, returning how many rows the listing area could show.
    ///
    /// The caller publishes that as `FileTreeState::viewport_rows` so the
    /// paging verbs move a page of *this* pane. Returning it keeps the
    /// measurement next to the layout that produced it.
    pub fn render(self, frame: &mut Frame, area: Rect) -> PaneLayout {
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
            return PaneLayout { viewport_rows: 0 };
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
        for (offset, row) in rows[scroll..end].iter().enumerate() {
            lines.push(self.row_line(row, &rows, scroll + offset, inner.width));
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
        PaneLayout {
            viewport_rows: visible_height,
        }
    }

    /// `index` addresses `rows`, which is what the shared indent needs to know
    /// whether this row closes its level. `inner_width` is the pane's usable
    /// columns: the guide is sized against it so that at the 22-column minimum
    /// the name is what survives a narrow column.
    fn row_line(
        &self,
        row: &TreeRow,
        rows: &[TreeRow],
        index: usize,
        inner_width: u16,
    ) -> Line<'static> {
        let theme = Arc::clone(&self.theme);
        let selected = self.state.selected.as_deref() == Some(row.path.as_str());
        // Shared with the `/files` box, including the scope a `l`-scoped
        // listing needs added to `row.depth`: both renderers once computed
        // their own indent and drew different ladders for the same tree.
        let indent = file_icons::tree_indent(self.state, rows, index, inner_width as usize);
        // The folder icon carries the expansion state, so there is no second
        // caret column doing the same job in a pane 22 columns wide.
        let (glyph, icon_style) = file_icons::icon_style(
            row.kind,
            &row.name,
            row.expanded,
            FileIconStyle::active(),
            &self.theme,
        );
        // Selection inverts the row rather than recoloring it: the type
        // colour is the only thing distinguishing a `.rs` from a `.md` at a
        // glance, and dropping it on the row the cursor is standing on would
        // hide it exactly when the user is deciding.
        let name_style = if selected {
            Style::default()
                .bg(theme.selection)
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD)
        } else {
            icon_style
        };
        Line::from(vec![
            Span::raw(indent),
            Span::styled(format!("{glyph} "), icon_style),
            Span::styled(row.name.clone(), name_style),
        ])
    }

    /// The pane's title names the directory it is listing.
    ///
    /// A pane scoped with `l` shows a subtree, and a title that still read
    /// the workspace name would misdescribe every row under it — so the scope
    /// leads and the workspace it hangs off is the quieter half.
    fn root_label(&self) -> String {
        let workspace = self
            .state
            .root
            .as_ref()
            .and_then(|root| root.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("workspace");
        match self.state.root_prefix() {
            "" => workspace.to_string(),
            scope => format!("{scope} · {workspace}"),
        }
    }

    fn status_label(&self) -> String {
        match &self.state.status {
            FileTreeStatus::Idle => String::new(),
            FileTreeStatus::Loading => "· loading…".to_string(),
            // C004: explicit busy state while a prior blocking scan drains.
            FileTreeStatus::Waiting => "· waiting for prior scan…".to_string(),
            FileTreeStatus::Ready => String::new(),
            FileTreeStatus::Error(message) => format!("· {message}"),
        }
    }

    fn empty_label(&self) -> String {
        match &self.state.status {
            FileTreeStatus::Error(message) => message.clone(),
            FileTreeStatus::Loading => "Scanning workspace…".to_string(),
            FileTreeStatus::Waiting => "Waiting for prior scan to stop…".to_string(),
            _ => "Empty workspace".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::file_tree::FileTreeState;
    use crate::tui::file_tree::{build_tree, TreeNodeKind};
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
    fn rows_carry_icons_rather_than_a_bare_caret() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("nested")).expect("mkdir");
        std::fs::write(root.path().join("main.rs"), b"x").expect("write");
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

        let style = FileIconStyle::active();
        let text = render_to_lines(&state, true, 40, 20);
        let folder = crate::tui::file_icons::glyph(TreeNodeKind::Directory, "nested", false, style);
        let rust = crate::tui::file_icons::glyph(TreeNodeKind::File, "main.rs", false, style);
        let markdown = crate::tui::file_icons::glyph(TreeNodeKind::File, "README.md", false, style);
        for icon in [folder, rust, markdown] {
            assert!(
                text.contains(icon),
                "the {icon:?} glyph should be on screen, got:\n{text}"
            );
        }
        assert_ne!(
            rust, markdown,
            "a .rs and a .md must not draw the same glyph"
        );

        // Expansion is drawn in the icon, not in a second caret column, so a
        // pane 22 columns wide still has room for the name.
        assert!(state.set_root_prefix("nested"));
        let collapsed = render_to_lines(&state, true, 40, 20);
        assert!(collapsed.contains("nested"));
    }

    #[test]
    fn the_title_names_the_scoped_root() {
        let (_root, mut state) = populated();
        let workspace = state
            .root
            .as_ref()
            .and_then(|root| root.file_name())
            .unwrap()
            .to_string_lossy()
            .to_string();
        let at_root = render_to_lines(&state, true, 60, 20);
        assert!(
            at_root.contains(&workspace),
            "an unscoped pane names the workspace, got:\n{at_root}"
        );

        assert!(state.set_root_prefix("src"));
        let scoped = render_to_lines(&state, true, 60, 20);
        assert!(
            scoped.contains("src"),
            "a pane scoped with `l` must say so in its title, got:\n{scoped}"
        );
    }

    #[test]
    fn the_rendered_pane_reports_the_viewport_it_can_show() {
        let (_root, state) = populated();
        let measure = |width: u16, height: u16| {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("terminal");
            let mut layout = PaneLayout::default();
            terminal
                .draw(|frame| {
                    layout = FileTreeView::new(&state, true, theme()).render(frame, frame.area());
                })
                .expect("draw");
            layout
        };
        // 20 rows of pane, 2 of border: the paging verbs need this number to
        // move a page instead of guessing.
        assert_eq!(measure(40, 20).viewport_rows, 18);
        assert_eq!(
            measure(40, 0).viewport_rows,
            0,
            "a pane that did not fit shows nothing"
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
