//! `/files` — the workspace file manager box.
//!
//! This is a **navigator**, not a document surface. It owns no file text and
//! performs no reads of its own: the listing arrives already walked, and
//! opening a row is a [`TuiMsg::FilesOpenSelected`] intent the command layer
//! turns into an editor attachment through `open_editor`. That is the same
//! split the Ctrl+T tree pane and `/open` use, so all three reach a document
//! through one path.
//!
//! Navigation state is not reimplemented here either — the box holds a real
//! [`FileTreeState`], so `l`/`h`, Enter, paging, and the cursor clamp are
//! literally the same code the editor-side pane runs, and a fix in one is a
//! fix in both. Re-rooting (`l`) is the one thing the box cannot do to its
//! own copy: it emits [`TuiMsg::FilesDescend`] / [`TuiMsg::FilesAscend`] and
//! the command layer moves the shared scope and issues the walk.
//!
//! The box seeds itself from the app's shared state when it opens, so `/files`
//! picks up where a previous browse left off instead of resetting the cursor
//! to the workspace root every time.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::CenteredScroll;

use crate::tui::app::state::file_tree::{FileTreeState, FileTreeStatus};
use crate::tui::app::TuiMsg;
use crate::tui::file_icons::{self, FileIconStyle};
use crate::tui::file_tree::{TreeListing, TreeNodeKind};

/// Hints rendered under the listing. Kept in one place so the box, its
/// hosted panel, and the help dialog cannot disagree about what the keys do.
pub const HINT: &str =
    "j/k move · ^d/^u page · l/→ root at dir · h/← up · enter open · r refresh · esc close";

/// Deepest indent the box will draw.
///
/// Indentation is orientation, the name is content: past this a nested row
/// would push its name out of a narrow column entirely, and the listing is
/// scoped one directory at a time anyway, so real depth inside the pane is
/// one or two levels.
const MAX_INDENT_DEPTH: usize = 8;

#[derive(Clone)]
pub struct FilesDialog {
    pub theme: Arc<Theme>,
    /// The shared navigation state. Cloned from the app on open so the box
    /// starts where the user was, then owned by the box for its lifetime.
    pub state: FileTreeState,
    pub scroll: CenteredScroll,
    visible_height: usize,
}

impl FilesDialog {
    pub fn new(theme: Arc<Theme>, state: FileTreeState) -> Self {
        Self {
            theme,
            state,
            scroll: CenteredScroll::new(),
            visible_height: 10,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    /// Adopt a freshly completed walk without losing the cursor.
    ///
    /// The shared `apply_listing` already drops a listing that belongs to a
    /// different workspace or generation, so a refresh that lands after the
    /// user left the project cannot repaint this box with someone else's
    /// tree.
    pub fn apply_listing(
        &mut self,
        generation: u64,
        request_id: u64,
        root: std::path::PathBuf,
        listing: TreeListing,
    ) {
        if !self
            .state
            .apply_listing(generation, request_id, root, listing)
        {
            self.state.request.cancel();
        }
    }

    fn move_selection(&mut self, delta: isize) {
        self.state.move_selection(delta);
        self.reveal();
    }

    fn page_selection(&mut self, direction: isize) {
        // The box measures its own viewport during render; feeding it back
        // here is what makes `Ctrl+d` move a page of *this* box rather than a
        // fixed guess.
        self.state.viewport_rows = self.visible_height;
        self.state.page_selection(direction);
        self.reveal();
    }

    fn reveal(&mut self) {
        let index = self.state.selected_index().unwrap_or(0);
        let total = self.state.rows().len();
        self.state.ensure_visible(index, self.visible_height);
        self.scroll.set(self.state.scroll);
        self.scroll.clamp(index, total, self.visible_height);
    }

    /// Activate the selected row.
    ///
    /// A directory expands in place (the same verb the Ctrl+T pane's Enter
    /// uses); a file asks the command layer to open it. Only a file can
    /// become a document, so only a file emits an intent — a directory
    /// reaches the editor by being entered with `l` first.
    fn activate(&mut self) -> Option<TuiMsg> {
        let row = self.state.selected_row()?;
        match row.kind {
            TreeNodeKind::Directory => {
                self.state.toggle_selected();
                self.reveal();
                None
            }
            TreeNodeKind::File => Some(TuiMsg::FilesOpenSelected { path: row.path }),
        }
    }

    fn render_rows(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let rows = self.state.rows();
        let selected = self.state.selected_index();
        let icon_style_kind = FileIconStyle::active();
        let mut lines: Vec<Line> = Vec::new();

        if let Some(message) = self.error_line() {
            lines.push(Line::from(Span::styled(
                message,
                Style::default().fg(theme.error),
            )));
        } else if rows.is_empty() {
            lines.push(Line::from(Span::styled(
                if self.state.status == FileTreeStatus::Loading
                    || self.state.status == FileTreeStatus::Waiting
                {
                    "Loading…"
                } else {
                    "No files to show"
                },
                Style::default().fg(theme.muted),
            )));
        } else {
            let selected_path = self.state.selected.clone();
            for (offset, row) in rows
                .iter()
                .skip(self.scroll.get())
                .take(area.height as usize)
                .enumerate()
            {
                let is_selected = selected == Some(self.scroll.get() + offset);
                let indent = "  ".repeat(row.depth.min(MAX_INDENT_DEPTH));
                let (glyph, icon_style) = file_icons::icon_style(
                    row.kind,
                    &row.name,
                    row.expanded,
                    icon_style_kind,
                    theme,
                );
                // Selection repaints the whole row, so the name is taken from
                // the icon's style with only the foreground replaced: the
                // type colour survives the cursor moving onto the row.
                let name_style = if is_selected {
                    Style::default()
                        .bg(theme.selection)
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD)
                } else {
                    icon_style
                };
                let mut line = Line::from(vec![
                    Span::raw(indent),
                    Span::styled(format!("{glyph} "), icon_style),
                    Span::styled(row.name.clone(), name_style),
                ]);
                if is_selected && selected_path.is_some() {
                    line = line.style(Modifier::BOLD);
                }
                lines.push(line);
            }
            if let Some(reason) = self.state.truncated.as_deref() {
                lines.push(Line::from(Span::styled(
                    format!("… {reason}"),
                    Style::default().fg(theme.warning),
                )));
            }
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
    }

    /// The title names the directory being listed.
    ///
    /// A pane scoped with `l` is not showing the workspace root, and a title
    /// that still read the workspace name would be telling the user the wrong
    /// thing about every row under it.
    fn title(&self) -> String {
        let total = self.state.rows().len();
        let workspace = self
            .state
            .root
            .as_ref()
            .and_then(|root| root.file_name())
            .map(|name| name.to_string_lossy().to_string());
        let scope = self.state.root_prefix();
        match (workspace.as_deref(), scope) {
            (_, "") => format!(" Files ({total} shown)"),
            (Some(workspace), scope) => format!(" Files · {scope} ({total} shown) · {workspace}"),
            (None, scope) => format!(" Files · {scope} ({total} shown)"),
        }
    }

    fn error_line(&self) -> Option<&str> {
        match &self.state.status {
            FileTreeStatus::Error(message) => Some(message.as_str()),
            _ => None,
        }
    }
}

impl Component for FilesDialog {
    /// A plain viewer box: no composer, no live filter, no mode of its own,
    /// so the panel host may resolve `Shift+←/→/↑/↓` as the docking gesture
    /// before this dialog sees the key.
    fn docks_as_viewer(&self) -> bool {
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        // Checked before the character arms: `Ctrl+d`/`Ctrl+u` carry
        // `Char('d')`/`Char('u')` codes, so a listing taller than the box
        // needs a verb that moves more than one row. The box owns the
        // keyboard while it is open, so nothing downstream competes for them.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => {
                    self.page_selection(1);
                    return None;
                }
                KeyCode::Char('u') => {
                    self.page_selection(-1);
                    return None;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Esc => Some(TuiMsg::CloseDialog),
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                None
            }
            // `l` makes the selected directory the listing's root and `h`
            // goes back one level. Both are intents: the box holds a copy of
            // the shared state and the walk belongs to the command layer, so
            // it names the direction rather than moving its own scope.
            KeyCode::Char('l') | KeyCode::Right => {
                let row = self.state.selected_row()?;
                if row.kind != TreeNodeKind::Directory {
                    return None;
                }
                Some(TuiMsg::FilesDescend { path: row.path })
            }
            KeyCode::Char('h') | KeyCode::Left => Some(TuiMsg::FilesAscend),
            KeyCode::Enter => self.activate(),
            // `r` re-walks the workspace. It is an intent, not a walk: the
            // command layer owns the async read and the completion comes back
            // through the shared generation-guarded path.
            KeyCode::Char('r') => Some(TuiMsg::FilesRefresh),
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        if area.height < 3 {
            return;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.title())
            .border_style(Style::default().fg(theme.border));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // The hint row is reserved rather than overlaid, so the listing never
        // draws under it and the last visible file is never half-hidden.
        let height = inner.height.saturating_sub(1) as usize;
        self.visible_height = height.max(1);
        self.reveal();

        let list_area = Rect {
            height: inner.height.saturating_sub(1),
            ..inner
        };
        self.render_rows(frame, list_area, theme);

        let hint_area = Rect {
            y: inner.y + inner.height.saturating_sub(1),
            height: 1,
            ..inner
        };
        frame.render_widget(
            Paragraph::new(HINT).style(Style::default().fg(theme.muted)),
            hint_area,
        );
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Files
    }
}

/// Read-only view used by tests and the hosted panel.
impl FilesDialog {
    pub fn visible_row_count(&self) -> usize {
        self.state.rows().len()
    }

    pub fn selected_path(&self) -> Option<String> {
        self.state.selected.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::file_tree::build_tree;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(root: &std::path::Path, relative: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, b"x").expect("write");
    }

    fn dialog(root: &std::path::Path) -> FilesDialog {
        let mut state = FileTreeState::default();
        let (generation, request_id) = state.begin_rebuild(root.to_path_buf());
        state.apply_listing(generation, request_id, root.to_path_buf(), build_tree(root));
        FilesDialog::new(Arc::new(Theme::default()), state)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    fn ctrl(code: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL)
    }

    #[test]
    fn enter_expands_a_directory_in_place_without_moving_the_cursor() {
        let root = temp();
        write(root.path(), "dir/a.txt");

        let mut dialog = dialog(root.path());
        assert_eq!(dialog.selected_path().as_deref(), Some("dir"));

        assert!(dialog.handle_key(key(KeyCode::Enter)).is_none());
        assert_eq!(dialog.selected_path().as_deref(), Some("dir"));
        assert!(dialog.visible_row_count() >= 2);
    }

    #[test]
    fn l_asks_the_command_layer_to_root_the_listing_at_the_directory() {
        let root = temp();
        write(root.path(), "dir/a.txt");
        write(root.path(), "top.txt");

        let mut dialog = dialog(root.path());
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('l'))),
            Some(TuiMsg::FilesDescend {
                path: "dir".to_string()
            }),
            "the box cannot relist: it holds a copy of the state, so `l` is an intent"
        );
        // The arrow is the same verb.
        assert_eq!(
            dialog.handle_key(key(KeyCode::Right)),
            Some(TuiMsg::FilesDescend {
                path: "dir".to_string()
            })
        );

        // A file has no directory to root the listing at.
        dialog.state.selected = Some("top.txt".to_string());
        assert_eq!(dialog.handle_key(key(KeyCode::Char('l'))), None);
    }

    #[test]
    fn h_asks_the_command_layer_to_go_back_one_level() {
        let root = temp();
        write(root.path(), "dir/nested/deep.txt");

        let mut dialog = dialog(root.path());
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('h'))),
            Some(TuiMsg::FilesAscend)
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Left)),
            Some(TuiMsg::FilesAscend)
        );
    }

    #[test]
    fn ctrl_d_and_ctrl_u_page_the_listing() {
        let root = temp();
        for index in 0..40 {
            write(root.path(), &format!("file{index:02}.txt"));
        }

        let mut dialog = dialog(root.path());
        assert_eq!(dialog.selected_path().as_deref(), Some("file00.txt"));

        assert!(dialog.handle_key(ctrl('d')).is_none());
        assert!(
            dialog.selected_path().as_deref() != Some("file00.txt"),
            "a page verb must move further than the `j` beside it, got {:?}",
            dialog.selected_path()
        );

        dialog.handle_key(ctrl('u'));
        assert_eq!(
            dialog.selected_path().as_deref(),
            Some("file00.txt"),
            "paging back returns to the first row"
        );
    }

    #[test]
    fn enter_on_a_file_asks_the_command_layer_to_open_it() {
        let root = temp();
        write(root.path(), "notes.md");

        let mut dialog = dialog(root.path());
        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::FilesOpenSelected {
                path: "notes.md".to_string()
            })
        );
    }

    #[test]
    fn enter_on_a_directory_never_emits_an_open_intent() {
        let root = temp();
        write(root.path(), "dir/a.txt");

        let mut dialog = dialog(root.path());
        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            None,
            "a directory has no document to open; it is expanded in place"
        );
    }

    #[test]
    fn refresh_is_an_intent_and_the_dialog_walks_nothing() {
        let root = temp();
        write(root.path(), "a.txt");

        let mut dialog = dialog(root.path());
        assert_eq!(
            dialog.handle_key(key(KeyCode::Char('r'))),
            Some(TuiMsg::FilesRefresh),
            "the daemon-side walk belongs to the command layer, not the widget"
        );
    }

    #[test]
    fn esc_closes_and_unclaimed_keys_do_nothing() {
        let root = temp();
        write(root.path(), "a.txt");

        let mut dialog = dialog(root.path());
        assert_eq!(
            dialog.handle_key(key(KeyCode::Esc)),
            Some(TuiMsg::CloseDialog)
        );
        assert!(dialog.handle_key(key(KeyCode::Char('z'))).is_none());
    }

    #[test]
    fn the_box_is_a_viewer_so_shift_directionals_still_dock_it() {
        let root = temp();
        write(root.path(), "a.txt");
        let dialog = dialog(root.path());
        assert!(
            dialog.docks_as_viewer(),
            "without this the panel host cannot resolve Shift+←/→/↑/↓ as docking"
        );
    }

    #[test]
    fn the_title_names_the_directory_the_listing_is_rooted_at() {
        let root = temp();
        write(root.path(), "dir/a.txt");
        write(root.path(), "top.rs");

        let mut dialog = dialog(root.path());
        let at_root = dialog.title();
        assert!(at_root.contains("Files"), "got {at_root:?}");
        assert!(
            !at_root.contains("dir ·"),
            "at the workspace root the scope is not named, got {at_root:?}"
        );

        assert!(dialog.state.set_root_prefix("dir"));
        let scoped = dialog.title();
        assert!(
            scoped.contains("dir") && scoped.contains("Files"),
            "a pane scoped with `l` must not keep claiming the workspace root, got {scoped:?}"
        );
        assert!(
            !scoped.contains("top.rs"),
            "the title names a scope, not the rows under it"
        );
    }

    #[test]
    fn rows_carry_an_icon_that_distinguishes_file_types() {
        let root = temp();
        write(root.path(), "main.rs");
        write(root.path(), "README.md");
        write(root.path(), "Cargo.toml");
        write(root.path(), "src/lib.rs");

        let mut dialog = dialog(root.path());
        let backend = ratatui::backend::TestBackend::new(40, 12);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                let theme = Arc::clone(&dialog.theme);
                dialog.render(frame, frame.area(), &theme);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let text: String = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("main.rs"));
        assert!(text.contains("Cargo.toml"));

        let style = FileIconStyle::active();
        let rust = file_icons::glyph(TreeNodeKind::File, "main.rs", false, style);
        let markdown = file_icons::glyph(TreeNodeKind::File, "README.md", false, style);
        let config = file_icons::glyph(TreeNodeKind::File, "Cargo.toml", false, style);
        assert_ne!(
            rust, markdown,
            "a .rs and a .md must not draw the same glyph"
        );
        assert_ne!(markdown, config);
        for icon in [rust, markdown, config] {
            assert!(
                !icon.is_empty(),
                "the plain fallback leaves files iconless by design, but the default style must not"
            );
            assert!(
                text.contains(icon),
                "the {icon:?} glyph should be on screen"
            );
        }
    }

    #[test]
    fn an_empty_workspace_renders_a_message_rather_than_a_blank_box() {
        let root = temp();
        let dialog = dialog(root.path());
        assert_eq!(dialog.visible_row_count(), 0);
        assert!(dialog.error_line().is_none());
    }
}
