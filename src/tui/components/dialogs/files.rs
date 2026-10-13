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
//! [`FileTreeState`], so `l`/`h`, Enter, and the cursor clamp are literally
//! the same code the editor-side pane runs, and a fix in one is a fix in both.
//! The box seeds itself from the app's shared state when it opens, so `/files`
//! picks up where a previous browse left off instead of resetting the cursor
//! to the workspace root every time.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
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
use crate::tui::file_tree::{TreeListing, TreeNodeKind};

/// Hints rendered under the listing. Kept in one place so the box, its
/// hosted panel, and the help dialog cannot disagree about what the keys do.
pub const HINT: &str = "j/k move · l/→ enter dir · h/← back · enter open · r refresh · esc close";

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
        let mut lines: Vec<Line> = Vec::new();

        if let Some(message) = self.error_line() {
            lines.push(Line::from(Span::styled(
                message,
                Style::default().fg(theme.error),
            )));
        } else if rows.is_empty() {
            lines.push(Line::from(Span::styled(
                if self.state.status == FileTreeStatus::Loading {
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
                let marker = match row.kind {
                    TreeNodeKind::Directory if row.expanded => "▾ ",
                    TreeNodeKind::Directory => "▸ ",
                    TreeNodeKind::File => "  ",
                };
                // Indent is capped so a deep path cannot push the name out of
                // a narrow column entirely — depth is orientation, the name
                // is content.
                let indent = "  ".repeat(row.depth.min(8));
                let style = if is_selected {
                    Style::default().bg(theme.selection).fg(theme.foreground)
                } else if row.kind == TreeNodeKind::Directory {
                    Style::default().fg(theme.primary)
                } else {
                    Style::default().fg(theme.foreground)
                };
                let mut line =
                    Line::from(Span::styled(format!("{indent}{marker}{}", row.name), style));
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
            // `l` goes *into* a directory (expand and step onto the first
            // child); `h` comes back out (collapse in place, or step to the
            // parent). Enter only ever expands, so it leaves the cursor where
            // the user put it.
            KeyCode::Char('l') | KeyCode::Right => {
                self.state.enter_selected();
                self.reveal();
                None
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.state.leave_selected();
                self.reveal();
                None
            }
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
        let total = self.state.rows().len();
        let title = match &self.state.root {
            Some(root) => format!(
                " Files ({} shown{}) ",
                total,
                root.file_name()
                    .map(|name| format!(" · {}", name.to_string_lossy()))
                    .unwrap_or_default()
            ),
            None => format!(" Files ({total} shown) "),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
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

    #[test]
    fn a_directory_opens_in_place_and_enter_takes_the_cursor_inside() {
        let root = temp();
        write(root.path(), "dir/a.txt");

        let mut dialog = dialog(root.path());
        assert_eq!(dialog.selected_path().as_deref(), Some("dir"));

        // Enter expands but does not move the cursor.
        assert!(dialog.handle_key(key(KeyCode::Enter)).is_none());
        assert_eq!(dialog.selected_path().as_deref(), Some("dir"));
        assert!(dialog.visible_row_count() >= 2);

        // `l` expands *and* steps inside.
        assert!(dialog.handle_key(key(KeyCode::Char('l'))).is_none());
        assert_eq!(dialog.selected_path().as_deref(), Some("dir/a.txt"));
    }

    #[test]
    fn h_walks_back_out_one_level_at_a_time() {
        let root = temp();
        write(root.path(), "dir/nested/deep.txt");

        let mut dialog = dialog(root.path());
        dialog.handle_key(key(KeyCode::Char('l')));
        dialog.handle_key(key(KeyCode::Char('l')));
        assert_eq!(
            dialog.selected_path().as_deref(),
            Some("dir/nested/deep.txt")
        );

        assert!(dialog.handle_key(key(KeyCode::Left)).is_none());
        assert_eq!(dialog.selected_path().as_deref(), Some("dir/nested"));
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
    fn an_empty_workspace_renders_a_message_rather_than_a_blank_box() {
        let root = temp();
        let dialog = dialog(root.path());
        assert_eq!(dialog.visible_row_count(), 0);
        assert!(dialog.error_line().is_none());
    }
}
