//! M002: the file tree's row model, counted.
//!
//! `tui_render_cost.inc` measures the content caches C006 introduced and
//! `tui_frame_cost.inc` measures the per-frame cost M001 removed from the
//! frame composition. This member measures the **file tree**: the `Ctrl+T`
//! pane and the `/files` box
//! flatten and filter a listing at most once per frame, per surface, behind a
//! cache whose invalidation is proven against a stale path.
//!
//! Two counters do the accounting, and they are deliberately a pair:
//!
//! * `file_tree_row_builds` — one per flatten of the listing into the row
//!   list a surface draws;
//! * `file_tree_filter_passes` — one per application of a surface's filter to
//!   a row list, whether it narrowed the list or copied it whole.
//!
//! A frame can filter rows it did not flatten (the `/files` box filters the
//! structure it adopted), so neither count implies the other.
//!
//! The **cost** tests are written against the *after* state: on the baseline
//! they fail, and the failure output is the before-count — that is WP1's
//! evidence. The equivalence and stale-path tests pass on the baseline too,
//! which is exactly what makes them guard rails rather than evidence.
//!
//! The stale-path tests are the milestone's load-bearing ones: a cache that
//! survives a listing change displays a path that is gone, and C006 stopped
//! short of this milestone for exactly that reason.

use std::path::Path;
use std::sync::Arc;

use codegg::directory_listing::MAX_LISTING_NODES;
use codegg::tui::app::state::file_tree::{
    filter_rows, FileTreeState, FileTreeStatus, FileTreeStructure,
};
use codegg::tui::app::{App, Dialog};
use codegg::tui::components::component::DialogType;
use codegg::tui::components::dialogs::files::FilesDialog;
use codegg::tui::components::file_tree::FileTreeView;
use codegg::tui::file_tree::{
    build_tree, visible_rows, TreeListing, TreeNode, TreeNodeKind, TreeRow,
};
use codegg::tui::panels::SidePanelId;
use codegg::tui::render_cost::{self, RenderCost};
use codegg::tui::route::Route;
use codegg::tui::theme::Theme;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

// ── Fixtures ────────────────────────────────────────────────────────────

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(root.path().join("dir")).expect("mkdir");
    std::fs::write(root.path().join("dir/child.txt"), b"x").expect("write");
    std::fs::write(root.path().join("top.txt"), b"x").expect("write");
    root
}

fn file(path: &str) -> TreeNode {
    TreeNode {
        name: path.rsplit('/').next().unwrap_or(path).to_string(),
        path: path.to_string(),
        depth: 1,
        kind: TreeNodeKind::File,
        children: None,
    }
}

fn directory(path: &str, depth: usize, children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        name: path.rsplit('/').next().unwrap_or(path).to_string(),
        path: path.to_string(),
        depth,
        kind: TreeNodeKind::Directory,
        children: Some(children),
    }
}

fn listing(nodes: Vec<TreeNode>) -> TreeListing {
    TreeListing {
        prefix: String::new(),
        nodes,
        truncated: false,
        truncated_reason: None,
        examined: 0,
    }
}

fn app_with_listing(root: &Path) -> App {
    let mut app = App::new_for_testing(root.to_string_lossy().to_string());
    deliver(&mut app, root, build_tree(root));
    app
}

/// Apply one complete walk, the way the daemon's completion does.
fn deliver(app: &mut App, root: &Path, listing: TreeListing) {
    let (generation, request_id) = app.file_tree_state.begin_rebuild(root.to_path_buf());
    assert!(
        app.file_tree_state
            .apply_listing(generation, request_id, root.to_path_buf(), listing),
        "the listing must apply to the state that asked for it"
    );
}

fn open_files_box(app: &mut App) {
    app.open_dialog(Dialog::Files);
    assert!(
        app.focus_manager.has_dialog(DialogType::Files),
        "the box is mounted"
    );
}

/// Press a key the way the event loop does: a frame after every key.
fn press(app: &mut App, event: crossterm::event::KeyEvent) {
    let _ = render(app, 140, 40);
    app.on_key(event);
    let _ = render(app, 140, 40);
}

fn show_tree_pane(app: &mut App) {
    app.file_tree_state.visible = true;
    app.ui_state.tree_focused = true;
    app.ui_state.routes.navigate_to(Route::Editor);
}

// ── Measurement ─────────────────────────────────────────────────────────

fn render(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("a test terminal");
    terminal
        .draw(|frame| app.render(frame))
        .expect("the frame draws");
    terminal.backend().buffer().clone()
}

/// One drawn frame's operation count.
fn frame_cost(app: &mut App) -> RenderCost {
    let before = render_cost::snapshot();
    let _buffer = render(app, 120, 40);
    render_cost::snapshot().since(&before)
}

fn buffer_text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── WP2 / WP6: the pane reads one row slice per frame ────────────────────

/// The `Ctrl+T` pane drew the same tree twice: the second frame flattens and
/// filters nothing, because the listing and the filter are unchanged.
#[test]
fn the_tree_pane_rebuilds_nothing_on_an_unchanged_frame() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    show_tree_pane(&mut app);
    // The pane publishes its measured viewport on the first frame, which is a
    // real state change; the frame under test is the one after it.
    let _warm = render(&mut app, 120, 40);

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.file_tree_row_builds, 0,
        "the pane flattened the whole listing again on a frame where nothing changed"
    );
    assert_eq!(
        cost.file_tree_filter_passes, 0,
        "the pane filtered the whole listing again on a frame where nothing changed"
    );
}

/// A listing that arrives is built when it lands, not by the frame that
/// draws it — and the frame after it draws the arrival, not the previous
/// listing.
#[test]
fn a_listing_arrival_builds_the_rows_once_and_the_next_frame_reads_them() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    show_tree_pane(&mut app);
    let _warm = render(&mut app, 120, 40);

    let before = render_cost::snapshot();
    deliver(
        &mut app,
        root.path(),
        listing(vec![file("after.txt"), file("second.txt")]),
    );
    let arrival = render_cost::snapshot().since(&before);

    assert!(
        arrival.file_tree_row_builds >= 1,
        "a listing arrival must build the row model it will draw, got {}",
        arrival.file_tree_row_builds
    );
    let cost = frame_cost(&mut app);
    assert_eq!(
        cost.file_tree_row_builds, 0,
        "the frame after an arrival re-flattened the listing the arrival had \
         already built"
    );
}

/// The pane's own filter is part of the row model: typing narrows it once,
/// and the next frame reads the narrowed rows.
#[test]
fn a_pane_filter_change_rebuilds_once_and_the_next_frame_reads_it() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    show_tree_pane(&mut app);
    let _warm = render(&mut app, 120, 40);

    let before = render_cost::snapshot();
    app.file_tree_state.filter_push('t');
    let filter = render_cost::snapshot().since(&before);
    assert_eq!(
        filter.file_tree_row_builds, 1,
        "a filter change rebuilds the filtered row list from the listing, once"
    );
    assert_eq!(
        filter.file_tree_filter_passes, 1,
        "a filter change is one pass over the row list"
    );

    // The first frame after typing reveals the pane's filter field, which
    // costs a row of layout and therefore re-measures the viewport — a real
    // state change that rebuilds the rows once. It is reported rather than
    // hidden: the honest worst frame after a keystroke is one build.
    let measuring = frame_cost(&mut app);
    assert!(
        measuring.file_tree_row_builds <= 1,
        "the frame that re-measured the viewport built more than once, got {}",
        measuring.file_tree_row_builds
    );

    // With the new viewport published, the pane is unchanged again.
    let cost = frame_cost(&mut app);
    assert_eq!(
        cost.file_tree_row_builds, 0,
        "the pane rebuilt rows on a frame where its layout and listing were unchanged"
    );
    assert_eq!(
        cost.file_tree_filter_passes, 0,
        "the pane filtered rows on a frame where its layout and listing were unchanged"
    );
}

// ── WP4 / WP5: the `/files` box reads one row slice per frame ────────────

/// A modal `/files` frame that changed nothing performs neither a row build
/// nor a filter pass: the box draws the rows it cached when it adopted the
/// structure and when its filter last moved.
#[test]
fn a_files_frame_that_changed_nothing_builds_and_filters_nothing() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    open_files_box(&mut app);
    let _warm = render(&mut app, 120, 40);

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.file_tree_row_builds, 0,
        "a `/files` frame that changed nothing flattened the listing"
    );
    assert_eq!(
        cost.file_tree_filter_passes, 0,
        "a `/files` frame that changed nothing filtered the row list"
    );
}

/// The worst `/files` frame — the one that adopts a listing that just landed
/// — still performs at most one filter pass and no row build: the box
/// filters the adopted structure, which the authority already flattened.
#[test]
fn a_files_frame_after_a_listing_arrival_filters_at_most_once() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    open_files_box(&mut app);
    let _warm = render(&mut app, 120, 40);

    deliver(&mut app, root.path(), listing(vec![file("fresh.txt")]));

    let cost = frame_cost(&mut app);
    assert_eq!(
        cost.file_tree_row_builds, 0,
        "the box re-flattened a structure the authority had already built"
    );
    assert!(
        cost.file_tree_filter_passes <= 1,
        "one frame must not filter the adopted listing more than once, got {}",
        cost.file_tree_filter_passes
    );
}

/// The docked panel is the same surface as the modal box: it draws the same
/// cached rows, so an unchanged frame performs neither a build nor a pass.
#[test]
fn a_docked_files_panel_that_changed_nothing_builds_and_filters_nothing() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    app.open_dialog(Dialog::Files);
    press(
        &mut app,
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('H'),
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert!(
        !app.focus_manager.has_dialog(DialogType::Files),
        "docking unmounts the box: the panel host owns it now"
    );
    assert!(
        app.panel_host
            .with_hosted_mut::<FilesDialog, bool>(SidePanelId::Files, |_| true)
            .unwrap_or(false),
        "the panel host holds the box"
    );
    let _warm = render(&mut app, 140, 40);

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.file_tree_row_builds, 0,
        "a docked `/files` frame that changed nothing flattened the listing"
    );
    assert_eq!(
        cost.file_tree_filter_passes, 0,
        "a docked `/files` frame that changed nothing filtered the row list"
    );
}

// ── WP3: a stale path is never displayed ────────────────────────────────

/// The C006 stop condition, at the frame: a listing that no longer holds a
/// path must not draw it on the next frame.
#[test]
fn a_removed_path_never_reaches_a_drawn_frame() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    open_files_box(&mut app);
    let before = buffer_text(&render(&mut app, 120, 40));
    assert!(before.contains("top.txt"), "the premise: it draws it");

    deliver(&mut app, root.path(), listing(vec![file("other.txt")]));
    let after = buffer_text(&render(&mut app, 120, 40));

    assert!(
        !after.contains("top.txt"),
        "a removed path was drawn on the frame after the listing that dropped it: {after:?}"
    );
    assert!(
        after.contains("other.txt"),
        "the replacement listing is drawn: {after:?}"
    );
}

/// A renamed path shows its new name and never the old one.
#[test]
fn a_renamed_path_is_drawn_under_its_new_name() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    show_tree_pane(&mut app);
    let before = buffer_text(&render(&mut app, 120, 40));
    assert!(before.contains("top.txt"), "the premise: it draws it");

    deliver(&mut app, root.path(), listing(vec![file("renamed.txt")]));
    let after = buffer_text(&render(&mut app, 120, 40));

    assert!(
        !after.contains("top.txt"),
        "the pane drew the old name after the rename: {after:?}"
    );
    assert!(
        after.contains("renamed.txt"),
        "the pane drew the new name: {after:?}"
    );
}

// ── WP7: the rendered output is unchanged ──────────────────────────────

/// One row-model fixture: the listing, the expansion set and the filter.
struct Fixture {
    name: &'static str,
    nodes: Vec<TreeNode>,
    expanded: &'static [&'static str],
    filter: &'static str,
    truncated: bool,
}

fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "empty",
            nodes: Vec::new(),
            expanded: &[],
            filter: "",
            truncated: false,
        },
        Fixture {
            name: "flat",
            nodes: vec![file("alpha.txt"), file("beta.txt"), file("gamma.txt")],
            expanded: &[],
            filter: "",
            truncated: false,
        },
        Fixture {
            name: "nested",
            nodes: vec![
                directory(
                    "src",
                    1,
                    vec![
                        directory("src/tui", 2, vec![file("src/tui/editor.rs")]),
                        file("src/lib.rs"),
                    ],
                ),
                file("Cargo.toml"),
            ],
            expanded: &["src", "src/tui"],
            filter: "",
            truncated: false,
        },
        Fixture {
            name: "truncated",
            nodes: vec![file("a.txt"), file("b.txt")],
            expanded: &[],
            filter: "",
            truncated: true,
        },
        Fixture {
            name: "filtered",
            nodes: vec![file("alpha.txt"), file("beta.txt"), file("gamma.txt")],
            expanded: &[],
            filter: "a",
            truncated: false,
        },
        Fixture {
            name: "filtered with no matches",
            nodes: vec![file("alpha.txt"), file("beta.txt")],
            expanded: &[],
            filter: "nothing-matches-this",
            truncated: false,
        },
        Fixture {
            name: "non-ASCII",
            nodes: vec![file("日本語.txt"), file("café.txt")],
            expanded: &[],
            filter: "é",
            truncated: false,
        },
    ]
}

/// The fixture's row model, built by the uncached pipeline the cache
/// memoizes: flatten the tree, then narrow it.
fn uncached_rows(fixture: &Fixture) -> Vec<TreeRow> {
    let expanded: std::collections::BTreeSet<String> = fixture
        .expanded
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    let mut rows = Vec::new();
    visible_rows(&fixture.nodes, &expanded, &mut rows);
    filter_rows(&rows, fixture.filter)
}

fn state_for(fixture: &Fixture, root: &Path) -> FileTreeState {
    let mut state = FileTreeState::default();
    let (generation, request_id) = state.begin_rebuild(root.to_path_buf());
    let listing = TreeListing {
        prefix: String::new(),
        nodes: fixture.nodes.clone(),
        truncated: fixture.truncated,
        truncated_reason: fixture
            .truncated
            .then(|| "stopped at the maximum of 4096 entries".to_string()),
        examined: fixture.nodes.len(),
    };
    assert!(
        state.apply_listing(generation, request_id, root.to_path_buf(), listing),
        "the fixture must apply"
    );
    for path in fixture.expanded {
        state.expand_and_select(path);
    }
    for character in fixture.filter.chars() {
        state.filter_push(character);
    }
    state
}

/// The cached slice is exactly the rows the uncached pipeline produces, for
/// every fixture — the equivalence the cache's correctness rests on.
#[test]
fn the_cached_rows_are_the_rows_the_uncached_pipeline_produces() {
    let root = tempfile::tempdir().expect("tempdir");
    for fixture in fixtures() {
        let state = state_for(&fixture, root.path());
        assert_eq!(
            state.rows(),
            uncached_rows(&fixture).as_slice(),
            "the cached rows for the {:?} fixture differ from the pipeline's",
            fixture.name
        );
    }
}

/// A warm pane draws the same buffer a cold one does, over the whole fixture
/// set: the cache changes the cost, not the picture.
#[test]
fn the_pane_draws_the_same_buffer_warm_and_cold() {
    let root = tempfile::tempdir().expect("tempdir");
    for fixture in fixtures() {
        let state = state_for(&fixture, root.path());
        // The first draw rebuilds anything the fixture's last mutation left
        // stale; the second draw reads the cache. Same picture, or the
        // milestone's primary acceptance property is broken.
        let cold = pane_text(&state, 48, 12);
        let warm = pane_text(&state, 48, 12);
        assert_eq!(
            cold, warm,
            "the {:?} fixture's warm render differs from its cold one",
            fixture.name
        );
    }
}

fn pane_text(state: &FileTreeState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("a test terminal");
    terminal
        .draw(|frame| {
            FileTreeView::new(state, true, Arc::new(Theme::default())).render(frame, frame.area());
        })
        .expect("the pane draws");
    let buffer = terminal.backend().buffer().clone();
    buffer_text(&buffer)
}

// ── The listing bound is unchanged ──────────────────────────────────────

/// A listing at the bound is drawn from the cache like any other: the bound
/// bounds the rows, not the per-frame cost.
#[test]
fn a_listing_at_the_bound_is_not_rebuilt_per_frame() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    let nodes: Vec<TreeNode> = (0..MAX_LISTING_NODES)
        .map(|index| file(&format!("file{index:05}.txt")))
        .collect();
    deliver(&mut app, root.path(), listing(nodes));
    show_tree_pane(&mut app);
    let _warm = render(&mut app, 120, 40);

    assert_eq!(
        app.file_tree_state.rows().len(),
        MAX_LISTING_NODES,
        "the listing bound is unchanged"
    );
    let cost = frame_cost(&mut app);
    assert_eq!(
        cost.file_tree_row_builds, 0,
        "a listing at the bound flattened once per frame"
    );
    assert_eq!(
        cost.file_tree_filter_passes, 0,
        "a listing at the bound filtered once per frame"
    );
}

// ── The structure a surface adopts is the unfiltered listing ────────────

/// `structure()` is the surface-facing projection and it must stay the
/// **unfiltered** listing: each surface applies its own filter.
#[test]
fn the_structure_carries_the_unfiltered_rows_while_the_filter_is_live() {
    let root = workspace();
    let mut app = app_with_listing(root.path());
    app.file_tree_state.filter_push('t');

    let structure: FileTreeStructure = app.file_tree_state.structure();
    assert_eq!(
        structure.rows.len(),
        app.file_tree_state.all_rows().len(),
        "the structure narrowed the listing the box filters itself"
    );
    assert!(
        structure.rows.iter().any(|row| row.path == "dir"),
        "the box's own filter must be able to reveal a row the pane's hides"
    );
    assert_eq!(app.file_tree_state.status, FileTreeStatus::Ready);
}
