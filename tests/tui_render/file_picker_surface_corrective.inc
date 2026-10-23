//! The `/files` surface corrective: one authority for the navigator's tree.
//!
//! The defect lives in the **seam between two surfaces**, so this drives the
//! real path: open `/files`, dock it into the column, press the verbs, and
//! read what the column shows.

use codegg::tui::app::state::project_tabs::{ProjectTabId, ProjectTabState};
use codegg::tui::app::{App, Dialog};
use codegg::tui::components::component::DialogType;
use codegg::tui::file_tree::build_tree;
use codegg::tui::panels::{PanelPlacement, SidePanelId};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

fn plain(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(root.path().join("dir")).expect("mkdir");
    std::fs::write(root.path().join("dir/child.txt"), b"x").expect("write");
    std::fs::write(root.path().join("top.txt"), b"x").expect("write");
    root
}

/// A workspace with enough rows for a page to land somewhere else.
fn workspace_with_files(count: usize) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("tempdir");
    for index in 0..count {
        std::fs::write(root.path().join(format!("file{index:02}.txt")), b"x").expect("write");
    }
    root
}

fn app_with_listing(root: &std::path::Path) -> App {
    let mut app = App::new_for_testing(root.to_string_lossy().to_string());
    deliver_listing(&mut app, root);
    app
}

fn deliver_listing(app: &mut App, root: &std::path::Path) {
    let (generation, request_id) = app.file_tree_state.begin_rebuild(root.to_path_buf());
    assert!(
        app.file_tree_state.apply_listing(
            generation,
            request_id,
            root.to_path_buf(),
            build_tree(root)
        ),
        "the listing must apply"
    );
}

/// Give the app a canonical project/workspace binding, so the refresh verb
/// reaches the walk.
fn bind_workspace(app: &mut App, root: &std::path::Path) {
    let mut tab = ProjectTabState::empty(ProjectTabId::new(), "project".to_string());
    tab.project_id = Some("project-1".to_string());
    tab.workspace_id = Some("workspace-1".to_string());
    tab.workspace_root = Some(root.to_path_buf());
    app.project_tabs.add_and_activate(tab);
}

fn frame(app: &mut App) -> Buffer {
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    terminal.backend().buffer().clone()
}

/// Press a key the way the event loop does: it draws a frame after every key.
fn press(app: &mut App, event: KeyEvent) {
    let _ = frame(app);
    app.on_key(event);
    let _ = frame(app);
}

fn panel_text(app: &mut App) -> String {
    let buf = frame(app);
    let Some((_, rect)) = app
        .panel_areas
        .iter()
        .find(|(placement, _)| *placement == app.panel_host.placement())
        .copied()
    else {
        return String::new();
    };
    // The panel column and the toast block must not intersect.
    let toast = ratatui::layout::Rect {
        x: 120 - 60,
        y: 2,
        width: 60,
        height: 10,
    };
    assert!(
        rect.x + rect.width <= toast.x || toast.x + toast.width <= rect.x,
        "the toast overlay covers the column under test ({rect:?} vs {toast:?})"
    );
    (rect.y..rect.y + rect.height)
        .map(|y| {
            (rect.x..rect.x + rect.width)
                .map(|x| {
                    buf.cell((x, y))
                        .map(|cell| cell.symbol().to_string())
                        .unwrap_or_default()
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn frame_text(app: &mut App) -> String {
    let buf = frame(app);
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| {
                    buf.cell((x, y))
                        .map(|cell| cell.symbol().to_string())
                        .unwrap_or_default()
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Open `/files` and dock it into the **left** column: the refresh raises a
/// toast in the top right, and a toast that covers the column under test
/// would be measuring the overlay.
fn open_and_dock(app: &mut App) {
    app.open_dialog(Dialog::Files);
    assert!(
        app.focus_manager.has_dialog(DialogType::Files),
        "the box is open"
    );
    press(app, plain('H'));
    assert!(
        !app.focus_manager.has_dialog(DialogType::Files),
        "docking unmounts the box: the panel host owns it now"
    );
    assert_eq!(
        app.panel_host.placement(),
        PanelPlacement::Left,
        "the box was docked into the left column"
    );
    assert!(app.panel_host.is_focused(), "the docked panel takes focus");
}

#[test]
fn a_docked_files_panel_expands_collapses_and_enters_through_the_real_key_path() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    open_and_dock(&mut app);

    let before = panel_text(&mut app);
    assert!(
        before.contains("dir"),
        "the panel lists the directory: {before:?}"
    );
    assert!(
        !before.contains("child.txt"),
        "the directory starts collapsed: {before:?}"
    );

    press(&mut app, plain('l'));
    let expanded = panel_text(&mut app);
    assert!(
        expanded.contains("child.txt"),
        "`l` expands the directory in the panel: {expanded:?}"
    );

    press(&mut app, plain('h'));
    let collapsed = panel_text(&mut app);
    assert!(
        !collapsed.contains("child.txt"),
        "`h` collapses the directory in the panel: {collapsed:?}"
    );

    press(&mut app, key(KeyCode::Enter));
    let entered = panel_text(&mut app);
    assert!(
        entered.contains("child.txt"),
        "`Enter` enters the directory in the panel: {entered:?}"
    );
}

#[test]
fn the_docked_panels_cursor_moves_with_j_and_k() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    open_and_dock(&mut app);

    assert_eq!(
        app.file_tree_state.selected.as_deref(),
        Some("dir"),
        "the cursor starts on the first row"
    );

    press(&mut app, plain('j'));
    assert_eq!(
        app.file_tree_state.selected.as_deref(),
        Some("top.txt"),
        "`j` moves the authority's selection, and the panel's cursor is that selection"
    );

    press(&mut app, plain('k'));
    assert_eq!(
        app.file_tree_state.selected.as_deref(),
        Some("dir"),
        "`k` moves it back"
    );
}

/// The page verbs move the **authority's** selection, not a private cursor.
#[test]
fn the_page_verbs_move_the_authorities_selection() {
    let root = workspace_with_files(40);
    let mut app = app_with_listing(root.path());
    open_and_dock(&mut app);

    let first = app.file_tree_state.selected.clone();
    assert!(first.is_some(), "the listing has a selection");

    press(&mut app, ctrl(KeyCode::Char('d')));
    let paged = app.file_tree_state.selected.clone();
    assert_ne!(
        paged, first,
        "`Ctrl+d` moves the authority's selection, not a cursor in the panel"
    );

    press(&mut app, ctrl(KeyCode::Char('u')));
    assert_eq!(
        app.file_tree_state.selected, first,
        "`Ctrl+u` moves the authority's selection back"
    );
}

#[test]
fn the_docked_panel_is_painted_from_the_authority_not_from_a_frozen_copy() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    open_and_dock(&mut app);

    let before = panel_text(&mut app);
    assert!(
        before.contains("dir"),
        "the panel lists the directory: {before:?}"
    );

    std::fs::write(root.path().join("extra.txt"), b"x").expect("write");
    deliver_listing(&mut app, root.path());

    let after = panel_text(&mut app);
    assert!(
        after.contains("extra.txt"),
        "the panel is painted from the authority's rows: {after:?}"
    );
    assert!(
        !before.contains("extra.txt"),
        "the panel had not shown the row before the listing landed"
    );
}

#[test]
fn the_docked_panel_repaints_after_r_refreshes_the_authority() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    bind_workspace(&mut app, root.path());
    open_and_dock(&mut app);

    let before = panel_text(&mut app);
    assert!(
        before.contains("dir"),
        "the panel lists the directory: {before:?}"
    );

    // `r` re-walks. There is no daemon behind this app, so the walk fails
    // and says so — but it fails *in the authority*.
    press(&mut app, plain('r'));
    let after = panel_text(&mut app);
    assert!(
        after.contains("file-tree refresh"),
        "`r` repaints the panel from the authority's new state: {after:?}"
    );
    assert!(
        !after.contains("child.txt"),
        "the authority's rows are gone, so the panel's are too: {after:?}"
    );
}

#[test]
fn the_modal_boxs_h_collapses_an_entered_directory() {
    let root = workspace();
    let mut app = app_with_listing(root.path());

    app.open_dialog(Dialog::Files);
    assert!(app.focus_manager.has_dialog(DialogType::Files));

    press(&mut app, plain('l'));
    assert_eq!(
        app.file_tree_state.selected.as_deref(),
        Some("dir/child.txt"),
        "`l` enters the directory"
    );
    let entered = frame_text(&mut app);
    assert!(
        entered.contains("child.txt"),
        "the box shows the directory's child: {entered:?}"
    );

    press(&mut app, plain('h'));
    let collapsed = frame_text(&mut app);
    assert!(
        !collapsed.contains("child.txt"),
        "`h` collapses the entered directory in the box: {collapsed:?}"
    );
    assert_eq!(
        app.file_tree_state.selected.as_deref(),
        Some("dir"),
        "the cursor is back on the directory"
    );
}

#[test]
fn the_two_surfaces_show_the_same_cursor_and_the_same_rows() {
    let root = workspace();
    let mut app = app_with_listing(root.path());

    app.open_dialog(Dialog::Files);
    press(&mut app, plain('j'));
    assert_eq!(
        app.file_tree_state.selected.as_deref(),
        Some("top.txt"),
        "the box's `j` moves the one selection"
    );
    let box_text = frame_text(&mut app);

    press(&mut app, plain('H'));
    press(&mut app, plain('j'));
    assert_eq!(
        app.file_tree_state.selected.as_deref(),
        Some("top.txt"),
        "the panel's `j` moves the same selection"
    );
    let panel = panel_text(&mut app);

    for name in ["dir", "top.txt"] {
        assert!(
            box_text.contains(name),
            "the box shows {name}: {box_text:?}"
        );
        assert!(panel.contains(name), "the panel shows {name}: {panel:?}");
    }
}

/// The filter is the one thing the two surfaces do not share.
#[test]
fn the_box_filter_is_box_local_and_the_pane_filter_stays_in_the_authority() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    app.open_dialog(Dialog::Files);

    press(&mut app, plain('i'));
    for character in "top".chars() {
        press(&mut app, plain(character));
    }
    let narrowed = frame_text(&mut app);
    assert!(
        narrowed.contains("top.txt"),
        "the box's filter keeps the matching row: {narrowed:?}"
    );
    assert!(
        !narrowed.contains("dir"),
        "the box's filter narrows the box's rows: {narrowed:?}"
    );
    assert!(
        app.file_tree_state.filter.is_empty(),
        "the box's filter is not the authority's filter: {:?}",
        app.file_tree_state.filter
    );

    // Clear the box's filter: the backspace is the filter's own, in the
    // filter's own insert mode.
    for _ in 0..3 {
        press(&mut app, key(KeyCode::Backspace));
    }
    let cleared = frame_text(&mut app);
    assert!(
        cleared.contains("dir") && cleared.contains("top.txt"),
        "the box shows every row again: {cleared:?}"
    );

    // The Ctrl+T pane's filter lives in the authority, and it does not
    // narrow the box.
    for character in "top".chars() {
        app.file_tree_state.filter_push(character);
    }
    let after = frame_text(&mut app);
    assert_eq!(
        app.file_tree_state.filter, "top",
        "the authority holds the tree pane's filter"
    );
    assert!(
        after.contains("dir") && after.contains("top.txt"),
        "the authority's filter does not reach the box: {after:?}"
    );
}

/// A completion that answers a walk nobody is waiting for must not paint.
#[test]
fn a_stale_listing_completion_does_not_refresh_a_surface() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    open_and_dock(&mut app);

    let before = panel_text(&mut app);

    let stale_generation = app.file_tree_state.generation.wrapping_sub(1);
    let (generation, request_id) = app.file_tree_state.begin_rebuild(root.path().to_path_buf());
    let after_the_walk_started = app.file_tree_state.revision;
    assert!(
        !app.file_tree_state.apply_listing(
            stale_generation,
            request_id,
            root.path().to_path_buf(),
            build_tree(root.path())
        ),
        "a stale completion is refused by the authority"
    );

    assert_eq!(
        app.file_tree_state.revision, after_the_walk_started,
        "a refused completion changes nothing, so nothing is refreshed"
    );
    assert_eq!(
        panel_text(&mut app),
        before,
        "the panel is untouched by a completion nobody asked for"
    );
    assert!(
        generation > stale_generation,
        "the walk in flight is the current one"
    );
}

/// A surface the refresh must not reach: it is not a `/files` box.
struct OtherSurface;
impl codegg::tui::components::component::Component for OtherSurface {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<codegg::tui::app::TuiMsg> {
        None
    }
    fn update(&mut self, _msg: codegg::tui::app::TuiMsg) -> Option<codegg::tui::app::TuiMsg> {
        None
    }
    fn render(
        &mut self,
        _frame: &mut ratatui::Frame,
        _area: ratatui::layout::Rect,
        _theme: &std::sync::Arc<codegg::tui::theme::Theme>,
    ) {
    }
    fn dialog_type(&self) -> codegg::tui::components::component::DialogType {
        codegg::tui::components::component::DialogType::Help
    }
}

/// A surface that was unmounted before the refresh is not reached.
#[test]
fn a_surface_unmounted_before_the_refresh_is_not_reached() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    open_and_dock(&mut app);

    // The panel's box is replaced by a surface of another type.
    app.panel_host
        .adopt(SidePanelId::Files, Box::new(OtherSurface));

    // The authority moves, and the refresh runs with no `/files` surface
    // mounted.
    deliver_listing(&mut app, root.path());
    let text = panel_text(&mut app);
    assert!(
        !text.contains("child.txt"),
        "the refresh did not paint the listing into a surface it does not own: {text:?}"
    );
}
