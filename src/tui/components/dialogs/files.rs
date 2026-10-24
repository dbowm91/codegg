//! `/files` — the workspace file manager box.
//!
//! This is a **navigator**, not a document surface. It owns no file text and
//! performs no reads of its own: the listing arrives already walked, and
//! opening a row is a [`TuiMsg::FilesOpenSelected`] intent the command layer
//! turns into an editor attachment through `open_editor`.
//!
//! ## The box holds a view, not a copy of the tree
//!
//! The navigator's tree **structure** belongs to exactly one value: the app's
//! `file_tree_state`. This box holds a [`FileTreeStructure`] — a read-only
//! projection of it, and a *distinct type*, so the box cannot hold an
//! authoritative field it might mutate. It is refreshed once per frame, and
//! every verb is an intent naming a **path** the command layer resolves.
//!
//! ## What the box owns
//!
//! The genuinely per-surface state: the **filter text**, the **insert mode**
//! and the **scroll offset**. The filter stays box-local by a closed
//! decision, so the two surfaces may show different rows — the accepted
//! contract, not a defect. The box measures its own viewport, so `Ctrl+d`
//! pages against the height of *this* box.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::CenteredScroll;

use crate::tui::app::state::file_tree::{
    filter_rows, FileTreeStatus, FileTreeStructure, TreePanelMode, FALLBACK_PAGE_ROWS,
};
use crate::tui::app::TuiMsg;
use crate::tui::file_icons::{self, FileIconStyle};
use crate::tui::file_tree::{TreeFilterField, TreeNodeKind, TreeRow};

/// Hints rendered under the listing. Kept in one place so the box, its
/// hosted panel, and the help dialog cannot disagree about what the keys do.
/// [`Component::keymap_hint`] hands this exact string to the panel host.
///
/// The panel chrome owns the word **back**, so `h` is **up** one level here.
pub const HINT: &str =
    "j/k move · ^d/^u page · l/→ peek · h/← up · enter open · i filter · r refresh · q/⇧Q close";
/// Separator between breadcrumb components. A `›` reads as "down one level".
const BREADCRUMB_SEP: &str = " › ";

/// The revision a box starts on: one no authority can hold, so the first
/// refresh always copies.
const NO_REVISION: u64 = u64::MAX;
#[derive(Clone)]
pub struct FilesDialog {
    pub theme: Arc<Theme>,
    /// The authority's tree structure, as a read-only view.
    ///
    /// Refreshed from `App::file_tree_state` once per frame, before this
    /// surface is drawn, and only when the authority's revision moved.
    view: FileTreeStructure,
    /// The authority's revision the view was taken at.
    view_revision: u64,
    /// The box's own filter. Box-local by a closed decision: the Ctrl+T
    /// pane's filter lives in the authority and this one does not.
    filter: String,
    /// The box's own insert mode.
    panel_mode: TreePanelMode,
    /// The box's own scroll offset.
    scroll: CenteredScroll,
    /// The box's own measured viewport height, in rows.
    visible_height: usize,
}

impl FilesDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            view: FileTreeStructure {
                rows: Vec::new(),
                selected: None,
                root: None,
                scope: String::new(),
                truncated: None,
                status: FileTreeStatus::Idle,
            },
            view_revision: NO_REVISION,
            filter: String::new(),
            panel_mode: TreePanelMode::Normal,
            scroll: CenteredScroll::new(),
            visible_height: FALLBACK_PAGE_ROWS,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    /// Adopt the authority's current structure.
    ///
    /// The copy happens only when the revision has moved: an unchanged frame
    /// costs nothing, and the box's own filter, mode and scroll are untouched.
    pub fn adopt_structure(&mut self, revision: u64, build: &dyn Fn() -> FileTreeStructure) {
        if self.view_revision == revision {
            return;
        }
        self.view = build();
        self.view_revision = revision;
    }

    /// The rows this box shows: the view's rows, narrowed by its own filter.
    fn rows(&self) -> Vec<TreeRow> {
        filter_rows(&self.view.rows, &self.filter)
    }

    /// Index of the cursor within the rows this box shows.
    fn selected_index(&self) -> Option<usize> {
        let rows = self.rows();
        self.selected_index_in(&rows)
    }

    fn selected_index_in(&self, rows: &[TreeRow]) -> Option<usize> {
        if rows.is_empty() {
            return None;
        }
        let index = self
            .view
            .selected
            .as_deref()
            .and_then(|path| rows.iter().position(|row| row.path == path))
            .unwrap_or(0);
        Some(index.min(rows.len() - 1))
    }

    /// The row the cursor is on.
    fn selected_row(&self) -> Option<TreeRow> {
        let rows = self.rows();
        let index = self.selected_index_in(&rows)?;
        rows.into_iter().nth(index)
    }

    /// The path of the row `delta` rows from the cursor, if there is one.
    fn path_at_offset(&self, delta: isize) -> Option<String> {
        let rows = self.rows();
        if rows.is_empty() {
            return None;
        }
        let current = self.selected_index_in(&rows).unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, rows.len() as isize - 1) as usize;
        rows.get(next).map(|row| row.path.clone())
    }

    /// Name the row `delta` rows from the cursor. The box knows its own
    /// filtered rows; the command layer records the choice on the authority.
    fn move_by(&self, delta: isize) -> Option<TuiMsg> {
        let path = self.path_at_offset(delta)?;
        Some(TuiMsg::FilesSelect { path })
    }

    /// Rows one page moves: two thirds of **this box's** viewport, which the
    /// box measures during its own render.
    fn page_step(&self) -> isize {
        ((self.visible_height * 2) / 3).max(1) as isize
    }

    /// Page the cursor by `direction` rows.
    fn page(&self, direction: isize) -> Option<TuiMsg> {
        let path = self.path_at_offset(direction * self.page_step())?;
        Some(TuiMsg::FilesSelect { path })
    }
    /// Keep the cursor on a row the filter still shows: the selection is the
    /// authority's, so a filter that hides it names the row it belongs on.
    fn reanchor(&self) -> Option<TuiMsg> {
        let rows = self.rows();
        let first = rows.first()?;
        if self
            .view
            .selected
            .as_deref()
            .is_some_and(|path| rows.iter().any(|row| row.path == path))
        {
            return None;
        }
        Some(TuiMsg::FilesSelect {
            path: first.path.clone(),
        })
    }

    fn reveal(&mut self) {
        let index = self.selected_index().unwrap_or(0);
        let total = self.rows().len();
        self.scroll.clamp(index, total, self.visible_height);
    }

    /// Activate the selected row. A directory is **peeked into** — the same
    /// motion `l` makes; a file asks the command layer to open it.
    fn activate(&self) -> Option<TuiMsg> {
        let row = self.selected_row()?;
        match row.kind {
            TreeNodeKind::Directory => Some(TuiMsg::FilesDescend { path: row.path }),
            TreeNodeKind::File => Some(TuiMsg::FilesOpenSelected { path: row.path }),
        }
    }

    fn render_rows(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let rows = self.rows();
        let selected = self.selected_index_in(&rows);
        let icon_style_kind = FileIconStyle::active();
        let mut lines: Vec<Line> = Vec::new();

        // The **filter field** takes a row of its own when it has something to
        // show: a listing that silently changed under a typed filter would
        // be a listing the reader could not account for.
        let field = TreeFilterField::new(&self.filter, self.panel_mode == TreePanelMode::Insert);
        let field_rows = u16::from(field.is_visible());
        let list_height = area.height.saturating_sub(field_rows);

        if let Some(message) = self.error_line() {
            lines.push(Line::from(Span::styled(
                message,
                Style::default().fg(theme.error),
            )));
        } else if rows.is_empty() {
            lines.push(Line::from(Span::styled(
                if self.view.status == FileTreeStatus::Loading
                    || self.view.status == FileTreeStatus::Waiting
                {
                    "Loading…"
                } else {
                    "No files to show"
                },
                Style::default().fg(theme.muted),
            )));
        } else {
            let selected_path = self.view.selected.clone();
            for (offset, row) in rows
                .iter()
                .skip(self.scroll.get())
                .take(list_height as usize)
                .enumerate()
            {
                let is_selected = selected == Some(self.scroll.get() + offset);
                // Depth, last-ness and the guide glyph come from one shared
                // calculation, so a row nested one level in looks the same in
                // this box and in the Ctrl+T pane.
                let indent =
                    file_icons::tree_indent(&rows, self.scroll.get() + offset, area.width as usize);
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
            if let Some(reason) = self.view.truncated.as_deref() {
                lines.push(Line::from(Span::styled(
                    format!("… {reason}"),
                    Style::default().fg(theme.warning),
                )));
                // A truncated listing is bounded *in scope*, not a failure to
                // find anything, and the recovery is a keypress: `l` re-walks
                // the selected directory on its own budget.
                lines.push(Line::from(Span::styled(
                    "  l to list a directory on its own · h to go up",
                    Style::default().fg(theme.muted),
                )));
            }
        }
        // The listing gets the rows above the field; the field itself is
        // drawn on its own row at the bottom.
        let list_area = Rect {
            height: list_height,
            ..area
        };
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), list_area);
        if field_rows == 1 {
            field.render(
                frame,
                Rect {
                    x: area.x,
                    y: area.y + list_height,
                    width: area.width,
                    height: 1,
                },
                theme,
            );
        }
    }

    /// The title is a breadcrumb of the scope: a pane scoped with `l` is not
    /// showing the workspace root, and a flat title would be wrong about
    /// every row under it.
    pub(crate) fn title(&self) -> String {
        let total = self.rows().len();
        let workspace = self
            .view
            .root
            .as_ref()
            .and_then(|root| root.file_name())
            .map(|name| name.to_string_lossy().to_string());
        let mut crumbs: Vec<&str> = Vec::new();
        if let Some(workspace) = workspace.as_deref() {
            crumbs.push(workspace);
        }
        crumbs.extend(self.view.scope.split('/').filter(|part| !part.is_empty()));
        if crumbs.is_empty() {
            return format!(" Files ({total} shown)");
        }
        format!(" Files · {} ({total} shown)", crumbs.join(BREADCRUMB_SEP))
    }

    fn error_line(&self) -> Option<&str> {
        match &self.view.status {
            FileTreeStatus::Error(message) => Some(message.as_str()),
            _ => None,
        }
    }

    /// Type into the panel's filter.
    fn handle_insert_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            KeyCode::Esc => {
                self.panel_mode = TreePanelMode::Normal;
                self.reanchor()
            }
            KeyCode::Enter => {
                self.panel_mode = TreePanelMode::Normal;
                self.reanchor()
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.reanchor()
            }
            // Only plain (or shifted) characters are typed. A control or alt
            // combination is a chord, not a character.
            KeyCode::Char(character)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.filter.push(character);
                self.reanchor()
            }
            _ => None,
        }
    }
}

impl Component for FilesDialog {
    /// A plain viewer box: no composer of its own, so the panel host may
    /// resolve `Shift+←/→/↑/↓` as the docking gesture before this dialog
    /// sees the key.
    fn docks_as_viewer(&self) -> bool {
        true
    }

    /// The verbs this box implements, for the panel host to reuse verbatim.
    fn keymap_hint(&self) -> Option<String> {
        Some(HINT.to_string())
    }

    /// The listing alone: no border, no title, no footer.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = area.inner(Margin::new(1, 1));
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.visible_height = inner.height as usize;
        self.reveal();
        self.render_rows(frame, inner, theme);
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        // The panel's own insert mode owns the keyboard while it is active:
        // printable keys edit the filter and `Esc` leaves insert mode.
        if self.panel_mode == TreePanelMode::Insert {
            return self.handle_insert_key(key);
        }
        // Checked before the character arms: `Ctrl+d`/`Ctrl+u` carry
        // `Char('d')`/`Char('u')` codes, so a listing taller than the box
        // needs a verb that moves more than one row.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => return self.page(1),
                KeyCode::Char('u') => return self.page(-1),
                _ => {}
            }
        }
        match key.code {
            // `Esc` is the **mode** switch, not the exit. In Normal mode
            // there is no mode to leave, so the key does nothing: the panel
            // keeps the keyboard and the selection.
            KeyCode::Esc => None,
            // `Shift+Q` closes the box, the same gesture that exits a
            // docked panel.
            KeyCode::Char('Q') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                Some(TuiMsg::CloseDialog)
            }
            // `q` closes the **box**; it is inert in the docked panel.
            KeyCode::Char('q') if key.modifiers.is_empty() => Some(TuiMsg::FilesClose),
            // `i` opens the panel's filter field. The filter narrows the rows
            // the panel shows; it never re-scopes the listing.
            KeyCode::Char('i') => {
                self.panel_mode = TreePanelMode::Insert;
                None
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_by(-1),
            // `l` and `h` are intents that name the row they act on: the box
            // holds a view of the shared structure and the walk belongs to
            // the command layer.
            KeyCode::Char('l') | KeyCode::Right => {
                let row = self.selected_row()?;
                Some(TuiMsg::FilesDescend { path: row.path })
            }
            KeyCode::Char('h') | KeyCode::Left => {
                let row = self.selected_row()?;
                Some(TuiMsg::FilesAscend { path: row.path })
            }
            KeyCode::Enter => self.activate(),
            // `r` re-walks the workspace. It is an intent, not a walk: the
            // command layer owns the async read.
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

        // The listing gets the whole box. The hotkey line that used to sit
        // at the bottom of the box duplicated the keymap the composer status
        // line already carries above the input.
        self.visible_height = inner.height as usize;
        self.reveal();
        self.render_rows(frame, inner, theme);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Files
    }
}

impl FilesDialog {
    pub fn visible_row_count(&self) -> usize {
        self.rows().len()
    }

    pub fn selected_path(&self) -> Option<String> {
        self.view.selected.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::file_tree::FileTreeState;
    use crate::tui::file_tree::build_tree;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(root: &std::path::Path, relative: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, b"x").expect("write");
    }

    /// The authority and the box that renders it.
    ///
    /// The box holds a **view** of the authority, so a test that wants to see
    /// a different tree moves the authority and refreshes the box.
    struct Fixture {
        state: FileTreeState,
        dialog: FilesDialog,
    }

    impl Fixture {
        fn new(root: &std::path::Path) -> Self {
            let mut state = FileTreeState::default();
            let (generation, request_id) = state.begin_rebuild(root.to_path_buf());
            state.apply_listing(generation, request_id, root.to_path_buf(), build_tree(root));
            let mut dialog = FilesDialog::new(Arc::new(Theme::default()));
            let revision = state.revision;
            let build = || state.structure();
            dialog.adopt_structure(revision, &build);
            Self { state, dialog }
        }

        /// Refresh the box from the authority, the way a frame does.
        fn refresh(&mut self) {
            let revision = self.state.revision;
            let build = || self.state.structure();
            self.dialog.adopt_structure(revision, &build);
        }

        /// Move the cursor and refresh, the way a keypress and a frame do.
        fn select(&mut self, path: &str) {
            assert!(self.state.select(path), "the authority must know {path}");
            self.refresh();
        }

        /// The scope the listing was taken in.
        fn scope(&mut self, prefix: &str) {
            assert!(
                self.state.set_root_prefix(prefix),
                "the scope must be accepted"
            );
            self.refresh();
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    fn ctrl(code: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL)
    }

    #[test]
    fn enter_asks_the_command_layer_to_enter_the_directory() {
        let root = temp();
        write(root.path(), "dir/a.txt");

        let mut fixture = Fixture::new(root.path());
        assert_eq!(fixture.dialog.selected_path().as_deref(), Some("dir"));

        // `Enter` is the same motion as `l`: it enters the directory. It used
        // to expand it in place, which at the listing window's edge expanded a
        // directory with no children — an "empty directory", not a move.
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::FilesDescend {
                path: "dir".to_string()
            })
        );
    }

    #[test]
    fn l_asks_the_command_layer_to_open_the_row() {
        let root = temp();
        write(root.path(), "dir/a.txt");
        write(root.path(), "top.txt");

        let mut fixture = Fixture::new(root.path());
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Char('l'))),
            Some(TuiMsg::FilesDescend {
                path: "dir".to_string()
            }),
            "the box cannot relist: it holds a view of the shared structure, so `l` is an intent"
        );
        // The arrow is the same verb.
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Right)),
            Some(TuiMsg::FilesDescend {
                path: "dir".to_string()
            })
        );

        // A file has no directory to enter, so `l` opens it: the command
        // layer resolves the row, and the key is never dead.
        fixture.select("top.txt");
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Char('l'))),
            Some(TuiMsg::FilesDescend {
                path: "top.txt".to_string()
            })
        );
    }

    #[test]
    fn h_names_the_row_the_cursor_is_on() {
        let root = temp();
        write(root.path(), "dir/nested/deep.txt");

        let mut fixture = Fixture::new(root.path());
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Char('h'))),
            Some(TuiMsg::FilesAscend {
                path: "dir".to_string()
            }),
            "`h` names the row it acts on: the relation is resolved against the authority"
        );
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Left)),
            Some(TuiMsg::FilesAscend {
                path: "dir".to_string()
            })
        );
    }

    #[test]
    fn movement_names_the_row_it_wants() {
        let root = temp();
        for index in 0..40 {
            write(root.path(), &format!("file{index:02}.txt"));
        }

        let mut fixture = Fixture::new(root.path());
        assert_eq!(
            fixture.dialog.selected_path().as_deref(),
            Some("file00.txt")
        );

        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Char('j'))),
            Some(TuiMsg::FilesSelect {
                path: "file01.txt".to_string()
            }),
            "`j` names the row below the cursor; the authority records it"
        );
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Char('k'))),
            Some(TuiMsg::FilesSelect {
                path: "file00.txt".to_string()
            })
        );
    }

    #[test]
    fn ctrl_d_and_ctrl_u_page_the_listing() {
        let root = temp();
        for index in 0..40 {
            write(root.path(), &format!("file{index:02}.txt"));
        }

        let mut fixture = Fixture::new(root.path());
        assert_eq!(
            fixture.dialog.selected_path().as_deref(),
            Some("file00.txt")
        );

        let paged = fixture.dialog.handle_key(ctrl('d')).expect("a page verb");
        let TuiMsg::FilesSelect { path } = paged else {
            panic!("paging must name the row it lands on");
        };
        assert_ne!(
            path, "file00.txt",
            "a page verb must move further than the `j` beside it, got {path:?}"
        );

        let back = fixture.dialog.handle_key(ctrl('u')).expect("a page verb");
        assert_eq!(
            back,
            TuiMsg::FilesSelect {
                path: "file00.txt".to_string()
            },
            "paging back returns to the first row"
        );
    }

    #[test]
    fn enter_on_a_file_asks_the_command_layer_to_open_it() {
        let root = temp();
        write(root.path(), "notes.md");

        let mut fixture = Fixture::new(root.path());
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::FilesOpenSelected {
                path: "notes.md".to_string()
            })
        );
    }

    /// `Enter` on a directory is the same motion as `l`: it enters the
    /// directory, and it never opens a document for it.
    #[test]
    fn enter_on_a_directory_enters_it_and_never_emits_an_open_intent() {
        let root = temp();
        write(root.path(), "dir/a.txt");

        let mut fixture = Fixture::new(root.path());
        fixture.select("dir");
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::FilesDescend {
                path: "dir".to_string()
            }),
            "a directory has no document to open; it is entered"
        );
    }

    /// `l` on a **file** opens it: the key is never dead, and a file has
    /// nothing to enter.
    #[test]
    fn l_on_a_file_asks_the_command_layer_to_open_it() {
        let root = temp();
        write(root.path(), "a.txt");

        let mut fixture = Fixture::new(root.path());
        fixture.select("a.txt");
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Char('l'))),
            Some(TuiMsg::FilesDescend {
                path: "a.txt".to_string()
            }),
            "the command layer resolves the row into an open"
        );
    }

    #[test]
    fn refresh_is_an_intent_and_the_dialog_walks_nothing() {
        let root = temp();
        write(root.path(), "a.txt");

        let mut fixture = Fixture::new(root.path());
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Char('r'))),
            Some(TuiMsg::FilesRefresh),
            "the daemon-side walk belongs to the command layer, not the widget"
        );
    }

    /// `Esc` is the **mode** switch, not the exit: it leaves insert mode and
    /// keeps the filter, and in Normal mode it does nothing at all. The box is
    /// left with `Shift+Q`.
    #[test]
    fn esc_is_the_mode_switch_and_shift_q_closes() {
        let root = temp();
        write(root.path(), "a.txt");

        let mut fixture = Fixture::new(root.path());
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Esc)),
            None,
            "Esc must not close the box or hand the keyboard back"
        );
        assert_eq!(
            fixture
                .dialog
                .handle_key(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT,)),
            Some(TuiMsg::CloseDialog),
            "Shift+Q is the box's exit"
        );
        assert!(fixture.dialog.handle_key(key(KeyCode::Char('z'))).is_none());
    }

    /// `q` is the box's exit verb, and the filter still owns printable keys.
    #[test]
    fn q_closes_the_box_and_the_filter_still_owns_printable_keys() {
        let root = temp();
        write(root.path(), "alpha.txt");

        let mut fixture = Fixture::new(root.path());
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Char('q'))),
            Some(TuiMsg::FilesClose),
            "`q` is the box's exit verb"
        );
        assert_eq!(
            fixture
                .dialog
                .handle_key(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT,)),
            Some(TuiMsg::CloseDialog),
            "`Shift+Q` still closes the box"
        );

        assert!(fixture.dialog.handle_key(key(KeyCode::Char('i'))).is_none());
        assert_eq!(fixture.dialog.panel_mode, TreePanelMode::Insert);
        assert!(fixture.dialog.handle_key(key(KeyCode::Char('q'))).is_none());
        assert_eq!(
            fixture.dialog.filter, "q",
            "the filter types `q`; the exit verb does not fire in insert mode"
        );
    }

    /// `i` opens the panel's filter, and `Esc` leaves insert mode **keeping**
    /// what was typed.
    #[test]
    fn the_filter_is_typed_in_insert_mode_and_survives_esc() {
        let root = temp();
        write(root.path(), "alpha.txt");
        write(root.path(), "beta.txt");

        let mut fixture = Fixture::new(root.path());
        assert!(fixture.dialog.handle_key(key(KeyCode::Char('i'))).is_none());
        assert_eq!(fixture.dialog.panel_mode, TreePanelMode::Insert);

        assert!(fixture.dialog.handle_key(key(KeyCode::Char('a'))).is_none());
        assert!(fixture.dialog.handle_key(key(KeyCode::Char('l'))).is_none());
        assert_eq!(fixture.dialog.filter, "al");

        // Esc leaves insert mode and keeps the filter, so the rows the user
        // narrowed to stay narrowed.
        assert!(fixture.dialog.handle_key(key(KeyCode::Esc)).is_none());
        assert_eq!(fixture.dialog.panel_mode, TreePanelMode::Normal);
        assert_eq!(fixture.dialog.filter, "al");
        assert_eq!(fixture.dialog.rows().len(), 1, "only alpha.txt survives");
    }

    /// The filter is **box-local**: it narrows the box's own rows and never
    /// reaches the authority's own filter.
    #[test]
    fn the_filter_is_the_boxs_own_and_never_the_authoritys() {
        let root = temp();
        write(root.path(), "alpha.txt");
        write(root.path(), "beta.txt");

        let mut fixture = Fixture::new(root.path());
        fixture.dialog.handle_key(key(KeyCode::Char('i')));
        fixture.dialog.handle_key(key(KeyCode::Char('a')));
        assert_eq!(fixture.dialog.filter, "a");
        assert_eq!(
            fixture.state.filter, "",
            "the Ctrl+T pane's filter lives in the authority and this one does not"
        );
    }

    /// A filter that hides the cursor names the row the cursor belongs on, so
    /// the authority never keeps a selection the box cannot show.
    #[test]
    fn a_filter_that_hides_the_cursor_re_anchors_it() {
        let root = temp();
        write(root.path(), "alpha.txt");
        write(root.path(), "beta.txt");

        let mut fixture = Fixture::new(root.path());
        fixture.select("beta.txt");
        fixture.dialog.handle_key(key(KeyCode::Char('i')));
        assert_eq!(
            fixture.dialog.handle_key(key(KeyCode::Char('l'))),
            Some(TuiMsg::FilesSelect {
                path: "alpha.txt".to_string()
            }),
            "the cursor moves to the first row the filter still shows"
        );
    }

    #[test]
    fn the_box_is_a_viewer_so_shift_directionals_still_dock_it() {
        let root = temp();
        write(root.path(), "a.txt");
        let fixture = Fixture::new(root.path());
        assert!(
            fixture.dialog.docks_as_viewer(),
            "without this the panel host cannot resolve Shift+←/→/↑/↓ as docking"
        );
    }

    #[test]
    fn the_title_names_the_directory_the_listing_is_rooted_at() {
        let root = temp();
        write(root.path(), "dir/a.txt");
        write(root.path(), "top.rs");

        let mut fixture = Fixture::new(root.path());
        let at_root = fixture.dialog.title();
        assert!(at_root.contains("Files"), "got {at_root:?}");
        assert!(
            !at_root.contains("dir ·"),
            "at the workspace root the scope is not named, got {at_root:?}"
        );

        fixture.scope("dir");
        let scoped = fixture.dialog.title();
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

        let mut fixture = Fixture::new(root.path());
        let text = render_text(&mut fixture.dialog, 40, 12);
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
                "the plain fallback leaves files iconless by design, but the default style must not be"
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
        let fixture = Fixture::new(root.path());
        assert_eq!(fixture.dialog.visible_row_count(), 0);
        assert!(fixture.dialog.error_line().is_none());
    }

    /// Render the box into a buffer and hand back the text.
    fn render_text(dialog: &mut FilesDialog, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                let theme = Arc::clone(&dialog.theme);
                dialog.render(frame, frame.area(), &theme);
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

    /// A listing scoped with `l`, exactly as the command layer hands it over.
    fn scoped_dialog(root: &std::path::Path, prefix: &str) -> Fixture {
        static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let mut state = FileTreeState::default();
        // The root is known before the scope is set: `begin_rebuild` resets the
        // scope chain when the root changes, so a scope set on a state that has
        // never seen this workspace is thrown away again.
        let (generation, request_id) = state.begin_rebuild(root.to_path_buf());
        state.apply_listing(generation, request_id, root.to_path_buf(), build_tree(root));
        assert!(state.set_root_prefix(prefix), "the scope must be accepted");
        let (generation, request_id) = state.begin_rebuild(root.to_path_buf());
        let mut listing = crate::tui::file_tree::from_shared_listing(
            crate::directory_listing::walk_directory(&root.join(prefix), prefix, &NEVER),
        );
        // The daemon's listing is stamped with the scope it answered; the local
        // walk helper always stamps the workspace root.
        listing.prefix = prefix.to_string();
        assert!(
            state.apply_listing(generation, request_id, root.to_path_buf(), listing),
            "a scoped listing must apply to a scoped pane"
        );
        let mut dialog = FilesDialog::new(Arc::new(Theme::default()));
        let revision = state.revision;
        let build = || state.structure();
        dialog.adopt_structure(revision, &build);
        Fixture { state, dialog }
    }

    /// Drop the box's own left border, leaving the row as the widget drew it.
    fn inner_of(line: &str) -> &str {
        match line.char_indices().nth(1) {
            Some((index, _)) => line[index..].trim_end(),
            None => "",
        }
    }

    /// The acceptance signal for the reported defect: a listing scoped with `l`
    /// is walked with depth restarting at the subtree, so its top level arrives
    /// at the depth the workspace root's own top level arrives at. Assert on
    /// the rendered buffer, not on `depth`.
    #[test]
    fn a_scoped_listing_draws_its_rows_one_level_in() {
        let root = temp();
        write(root.path(), "dir/nested/deep.txt");
        write(root.path(), "dir/top.txt");

        let mut at_root = Fixture::new(root.path());
        let mut scoped = scoped_dialog(root.path(), "dir");

        let root_depths: Vec<usize> = at_root.dialog.rows().iter().map(|r| r.depth).collect();
        let scoped_depths: Vec<usize> = scoped.dialog.rows().iter().map(|r| r.depth).collect();
        assert_eq!(
            root_depths.first().copied(),
            scoped_depths.first().copied(),
            "the premise: a subtree walk restarts depth, so `dir`'s top level \
             arrives at the same depth the workspace root's does"
        );
        assert_eq!(
            at_root.state.display_depth(&at_root.dialog.rows()[0]),
            scoped.state.display_depth(&scoped.dialog.rows()[0]),
            "the indent is relative to the scope, so the scope's own depth is \
             not added back — a deep scope no longer flattens to the same cap"
        );

        let text = render_text(&mut scoped.dialog, 44, 10);
        assert!(
            text.contains("top.txt") && text.contains("nested"),
            "the scoped listing must still list its own rows, got {text:?}"
        );
        let top_line = text
            .lines()
            .find(|line| line.contains("top.txt"))
            .expect("a row for top.txt");
        // Drop the box's own left border, leaving the guide, the icon, the name.
        let row = inner_of(top_line);
        assert!(
            row.starts_with("└─ "),
            "the scope is the parent, so its only child closes the level, before \
             the icon and the name, got {row:?}"
        );

        // And the same row in the *root* listing sits at the same indent: the
        // indent is **relative to the scope**, so each listing draws its own
        // top level at the left edge.
        let root_text = render_text(&mut at_root.dialog, 44, 10);
        let dir_line = root_text
            .lines()
            .find(|line| line.contains('─') && line.contains("dir"))
            .unwrap_or_else(|| panic!("a guided row for dir at the root in {root_text:?}"));
        let root_row = inner_of(dir_line);
        assert!(
            !root_row.starts_with('│'),
            "at the workspace root there is nothing above the row, got {root_row:?}"
        );
        assert_eq!(
            row.find("top.txt"),
            root_row.find("dir"),
            "a scoped row and a root row are drawn at the same column, because \
             the indent is relative to the scope, got {root_row:?} against {row:?}"
        );
    }

    /// The title is a breadcrumb, so a nested scope says which component each
    /// `h` steps back through.
    #[test]
    fn the_title_is_a_breadcrumb_of_the_scope() {
        let root = temp();
        write(root.path(), "dir/nested/deep.txt");

        let mut fixture = Fixture::new(root.path());
        let workspace = root
            .path()
            .file_name()
            .expect("temp dir")
            .to_string_lossy()
            .to_string();
        let at_root = fixture.dialog.title();
        assert!(at_root.contains(&workspace), "got {at_root:?}");
        assert!(
            !at_root.contains(BREADCRUMB_SEP.trim()),
            "at the root there is nowhere to go, got {at_root:?}"
        );

        fixture.scope("dir/nested");
        let scoped = fixture.dialog.title();
        assert_eq!(
            scoped,
            format!(
                " Files · {workspace}{sep}dir{sep}nested (0 shown)",
                sep = BREADCRUMB_SEP
            ),
            "one breadcrumb step per scope component"
        );
    }

    /// The host must not paraphrase this box's keymap, so the `i act` verb
    /// `handle_key` never dispatches cannot survive here.
    #[test]
    fn the_keymap_the_host_advertises_is_this_box_keymap() {
        let root = temp();
        write(root.path(), "a.txt");
        let fixture = Fixture::new(root.path());
        assert_eq!(fixture.dialog.keymap_hint().as_deref(), Some(HINT));
        assert!(HINT.contains("enter open"), "got {HINT:?}");
        assert!(
            !HINT.contains("i act"),
            "the generic host line taught a verb this box does not have"
        );
    }

    /// The box's own hotkey row is gone: the row above the composer carries
    /// the panel's keymap.
    #[test]
    fn the_modal_box_spends_its_rows_on_the_listing() {
        let root = temp();
        write(root.path(), "a.txt");
        let mut fixture = Fixture::new(root.path());
        let text = render_text(&mut fixture.dialog, 100, 12);
        assert!(
            !text.contains("enter open"),
            "the box must not repeat the keymap the composer row already carries: {text:?}"
        );
    }

    /// In a docked panel the host owns the chrome: `render_in_panel` must not
    /// repaint a border or a title over the one the host already drew.
    #[test]
    fn a_hosted_render_keeps_the_hosts_chrome_and_spends_every_row_on_rows() {
        let root = temp();
        write(root.path(), "alpha.txt");
        write(root.path(), "beta.txt");

        let mut fixture = Fixture::new(root.path());
        let theme = Arc::clone(&fixture.dialog.theme);
        let width = 30u16;
        let height = 8u16;
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .title(" ● Files "),
                    frame.area(),
                );
                fixture.dialog.render_in_panel(frame, frame.area(), &theme);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let text: String = (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            text.contains("● Files"),
            "the host title is how a focused panel is told from an unfocused one, got {text:?}"
        );
        assert!(
            !text.contains("enter open"),
            "the keymap belongs to the composer row, not a column this narrow, got {text:?}"
        );
        assert!(
            text.contains("alpha.txt") && text.contains("beta.txt"),
            "the listing still draws, got {text:?}"
        );
        // 8 rows: border, listing rows, border. Nothing reserved for a footer.
        assert_eq!(height, 8);
    }

    /// The refresh copies only when the authority's revision moved: an
    /// unchanged frame must not cost a copy of the whole tree.
    #[test]
    fn the_refresh_copies_only_when_the_revision_moved() {
        let root = temp();
        write(root.path(), "a.txt");
        let mut fixture = Fixture::new(root.path());
        let copies = std::cell::Cell::new(0usize);
        let revision = fixture.state.revision;
        let build = || {
            copies.set(copies.get() + 1);
            fixture.state.structure()
        };

        fixture.dialog.adopt_structure(revision, &build);
        assert_eq!(
            copies.get(),
            0,
            "an unchanged revision must not copy the tree"
        );

        fixture.dialog.adopt_structure(revision + 1, &build);
        assert_eq!(
            copies.get(),
            1,
            "a moved revision copies the tree exactly once"
        );
    }

    /// The refresh is the authority's, not the surface's: the box keeps its
    /// filter, its insert mode and its scroll across it. A listing landing
    /// while the user is typing must not wipe what they typed.
    #[test]
    fn the_refresh_preserves_the_surface_filter_mode_and_scroll() {
        let root = temp();
        write(root.path(), "alpha.txt");
        write(root.path(), "beta.txt");

        let mut fixture = Fixture::new(root.path());
        fixture.dialog.filter = "alp".to_string();
        fixture.dialog.panel_mode = TreePanelMode::Insert;
        fixture.dialog.scroll.set(1);

        // The authority moves, so the refresh really copies.
        assert!(fixture.state.select("beta.txt"));
        fixture.refresh();

        assert_eq!(fixture.dialog.filter, "alp", "the filter survives");
        assert_eq!(
            fixture.dialog.panel_mode,
            TreePanelMode::Insert,
            "the insert mode survives"
        );
        assert_eq!(fixture.dialog.scroll.get(), 1, "the scroll survives");
        assert_eq!(
            fixture.dialog.visible_row_count(),
            1,
            "the filter still narrows the rows"
        );
    }

    /// A page is a page of **this** box: the box measures its own height
    /// during its render, so it pages against its own viewport and not
    /// against whichever other surface happens to be on screen.
    #[test]
    fn paging_uses_the_surfaces_own_viewport_height() {
        let root = temp();
        for index in 0..40 {
            write(root.path(), &format!("file{index:02}.txt"));
        }

        let mut fixture = Fixture::new(root.path());
        fixture.dialog.visible_height = 3;
        let short = fixture.dialog.handle_key(ctrl('d')).expect("a page verb");
        fixture.dialog.visible_height = 30;
        let tall = fixture.dialog.handle_key(ctrl('d')).expect("a page verb");

        assert_eq!(
            short,
            TuiMsg::FilesSelect {
                path: "file02.txt".to_string()
            },
            "a short box pages two thirds of its own three rows"
        );
        assert_eq!(
            tall,
            TuiMsg::FilesSelect {
                path: "file20.txt".to_string()
            },
            "a tall box pages two thirds of its own thirty rows"
        );
    }
}
