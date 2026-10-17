//! The twelve remaining dockable boxes must leave the panel's chrome alone, and
//! must advertise the verbs a **docked** box can actually deliver.
//!
//! ## What this suite is for
//!
//! `render_hosted` paints a border, a title and the `●` focus marker into the
//! column, then hands the *same rect* to `Component::render_in_panel`, which
//! defaults to `render`. Every widget here still implemented only `render`, so
//! its own `Block` overwrote all three — focus dot included. A focused panel
//! and an unfocused one rendered identically, and the panel's name was gone.
//!
//! The second half is the keymap. `HostPanel::composer_hints` falls back to a
//! single host-side string, `"j/k scroll · i act"`, for any widget that does
//! not answer [`Component::keymap_hint`]. That string was wrong for every box
//! it described, and each of these widgets carried a *second* advertisement —
//! an in-panel footer or a border title — that was destroyed by exactly the
//! overwrite above, so nobody outside this file could see it either.
//!
//! ## The reason the hints here are shorter than the footers they replace
//!
//! A docked box never sees the keys the host answers first. Reading
//! [`SidePanelHost::handle_key`](codegg::tui::panels) top to bottom, before a
//! widget is consulted the host has claimed:
//!
//! - `Esc` and `q`, as its universal exits — it blurs and returns `Consumed`
//!   without calling the widget at all;
//! - anything resolving to a panel direction. `insert_panel_direction_bindings`
//!   binds **uppercase `H`/`J`/`K`/`L`** and the four arrow keys, unmodified
//!   *and* shifted.
//!
//! So every `Esc close` in the twelve footers below was already dead the moment
//! the box docked, and so was every `↑/↓ navigate` and every `J/K reorder` and
//! `H copy hunk`. A hint that repeats them is the same defect in a new place,
//! which is why `hint_matches` is checked in **both** directions: no advertised
//! verb may be one the host answers, and no verb the widget dispatches may go
//! unadvertised.
//!
//! ## Reading the buffer, not the source
//!
//! "The widget does not paint a `Block`" is not statically checkable —
//! `Component::render` is the modal path, where a border is correct. So every
//! chrome claim here is made against the rendered column: the host's corners,
//! its title and its focus dot are on screen afterwards, and no second box
//! was drawn inside them.
//!
//! ## Which of these are guards rather than signals
//!
//! `no_second_box` passes before this change for any widget whose modal layout
//! is a single full-rect block, because the second border lands exactly on the
//! first one and leaves no interior corner behind. Those tests are a
//! regression net against a *partial* fix — a widget that drops its outer
//! border but leaves an inner one still fails — and are recorded as such rather
//! than claimed as evidence that the work happened.

use std::sync::Arc;

use codegg::tui::components::component::Component;
use codegg::tui::components::dialogs::diff::DiffDialog;
use codegg::tui::components::dialogs::keybind::KeybindDialog;
use codegg::tui::components::dialogs::mcp::McpDialog;
use codegg::tui::components::dialogs::panel_picker::PanelPickerDialog;
use codegg::tui::components::dialogs::plan::PlanDialog;
use codegg::tui::components::dialogs::plugin::PluginDialog;
use codegg::tui::components::dialogs::research::ResearchBrowserDialog;
use codegg::tui::components::dialogs::review::{ReviewDialog, ReviewMode};
use codegg::tui::components::dialogs::security_review::SecurityReviewDialog;
use codegg::tui::components::dialogs::task_view::TaskViewDialog;
use codegg::tui::components::dialogs::theme::ThemePickerDialog;
use codegg::tui::components::dialogs::tree::TreeDialog;
use codegg::tui::panels::{render_hosted, SidePanelId};
use codegg::tui::theme::Theme;

fn theme() -> Arc<Theme> {
    Arc::new(Theme::default())
}

/// A column shaped like a real docked one: tall enough for a body, narrow
/// enough that a footer wraps, which is how the old footers pushed content out.
fn area() -> ratatui::layout::Rect {
    ratatui::layout::Rect::new(0, 0, 40, 16)
}

/// Render `widget` the way the panel host does, and return the column as text.
fn host_rows(widget: &mut dyn Component, id: SidePanelId, focused: bool) -> Vec<String> {
    let area = area();
    let dot = if focused { "●" } else { " " };
    let title = format!(" {} {} ", id.label(), dot);
    let theme = theme();
    let mut buf = ratatui::buffer::Buffer::empty(area);
    render_hosted(widget, &mut buf, area, &theme, &title);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| {
                    buf.cell((x, y))
                        .map(|c| c.symbol().to_string())
                        .unwrap_or_default()
                })
                .collect::<String>()
        })
        .collect()
}

/// The host's box, in full: four corners, a rule down both edges, and the
/// panel's own title — focus dot included, since that dot is the only thing on
/// screen distinguishing a panel that has the keyboard from one that does not.
fn assert_host_chrome(rows: &[String], needle: &str, who: &str) {
    let h = rows.len();
    assert!(h >= 3, "{who}: column too short to be a panel ({h} rows)");
    let w = rows[0].chars().count();

    assert!(
        rows[0].starts_with('┌') && rows[0].ends_with('┐'),
        "{who}: the host's top border was overwritten — row 0 is {:?}",
        rows[0]
    );
    assert!(
        rows[h - 1].starts_with('└') && rows[h - 1].ends_with('┘'),
        "{who}: the host's bottom border was overwritten — last row is {:?}",
        rows[h - 1]
    );
    for (y, row) in rows.iter().enumerate().take(h - 1).skip(1) {
        let chars: Vec<char> = row.chars().collect();
        assert_eq!(
            (chars[0], chars[w - 1]),
            ('│', '│'),
            "{who}: the host's side border was overwritten at row {y} — {row:?}"
        );
    }
    assert!(
        rows[0].contains(needle),
        "{who}: the panel title {needle:?} is the host's, not the widget's — row 0 is {:?}",
        rows[0]
    );
}

/// No box corners inside the host's box.
///
/// Only the four corners are checked, not `│`: content is entitled to draw a
/// vertical rule (`tree` draws `├─`), but nothing in this milestone's content
/// draws a corner.
fn assert_no_second_box(rows: &[String], who: &str) {
    // Rows 0 and `height - 1` are the host's own top and bottom borders, so
    // their corners are expected; only the interior is searched.
    let h = rows.len();
    for (y, row) in rows.iter().enumerate().take(h.saturating_sub(1)).skip(1) {
        let interior: String = row
            .trim_start_matches('│')
            .trim_end_matches('│')
            .to_string();
        for corner in ['┌', '┐', '└', '┘'] {
            assert!(
                !interior.contains(corner),
                "{who}: a second box was drawn inside the panel at row {y} — {row:?}"
            );
        }
    }
}

/// Every key the panel host answers **before** a widget is consulted.
///
/// Spelled out rather than imported because `is_exit_key` and
/// `insert_panel_direction_bindings` are private; the citation is in the module
/// docs above. `Home`/`End`/`PageUp`/`PageDown` are deliberately absent — they
/// are not panel directions, so a widget that dispatches them really does
/// deliver them from a column.
const HOST_ANSWERS_FIRST: &[&str] = &["esc", "Esc", "q", "↑", "↓", "←", "→", "H", "J", "K", "L"];

/// The key tokens a hint spells, one per `·`-separated segment.
fn hint_keys(hint: &str) -> Vec<String> {
    hint.split('·')
        .filter_map(|segment| segment.split_whitespace().next())
        .flat_map(|keys| keys.split('/').map(|k| k.trim().to_string()))
        .collect()
}

/// The two directions of criterion 4, checked as one comparison: the hint names
/// *exactly* the verbs that survive the host.
fn hint_matches(hint: &Option<String>, live: &[&str], who: &str) {
    let Some(hint) = hint else {
        panic!(
            "{who}: answers no keymap_hint, so the panel host advertises \
             \"j/k scroll · i act\" for a box that dispatches none of it"
        );
    };
    assert!(!hint.is_empty(), "{who}: an empty hint teaches nothing");

    let advertised = hint_keys(hint);
    for token in &advertised {
        assert!(
            !HOST_ANSWERS_FIRST.contains(&token.as_str()),
            "{who}: advertises {token:?}, which the panel host answers before \
             the widget ever sees it — the hint teaches a key that cannot fire. Hint: {hint:?}"
        );
        assert!(
            live.contains(&token.as_str()),
            "{who}: advertises {token:?}, which it does not dispatch. Hint: {hint:?}"
        );
    }
    for verb in live {
        assert!(
            advertised.iter().any(|k| k == verb),
            "{who}: dispatches {verb:?} but never advertises it. Hint: {hint:?}"
        );
    }
}

// ---------------------------------------------------------------- tree

/// `/tree` — the session fork tree. Four `Block` refs, and a footer naming
/// `Esc close` and `↑/↓`, neither of which survives docking.
#[test]
fn tree_leaves_the_panel_chrome_alone() {
    let mut dialog = TreeDialog::new(theme());
    let rows = host_rows(&mut dialog, SidePanelId::Tree, true);
    assert_host_chrome(&rows, "Tree ●", "TreeDialog");
    assert_no_second_box(&rows, "TreeDialog");
}

#[test]
fn tree_advertises_only_the_verbs_a_column_can_deliver() {
    let dialog = TreeDialog::new(theme());
    // `j/k` walk, `e` expands, `f` forks, `enter` selects the session. `Esc`
    // and `↑/↓` are the host's.
    hint_matches(
        &dialog.keymap_hint(),
        &["j", "k", "e", "f", "enter"],
        "TreeDialog",
    );
}

// --------------------------------------------------------------- plan

/// `/plan` — a two-option confirm box whose keymap lives in a **border title**
/// (`" Options (↑↓ select | Enter confirm) "`), in four copies. Both verbs are
/// host-answered, so the title advertised a dead pair even modally.
#[test]
fn plan_leaves_the_panel_chrome_alone() {
    let mut dialog = PlanDialog::new("plan-1".into(), "ship the thing".into());
    let rows = host_rows(&mut dialog, SidePanelId::Plan, true);
    assert_host_chrome(&rows, "Plan ●", "PlanDialog");
    assert_no_second_box(&rows, "PlanDialog");
}

#[test]
fn plan_advertises_only_the_verbs_a_column_can_deliver() {
    let dialog = PlanDialog::new("plan-1".into(), "ship the thing".into());
    // `j`/`k` pick yes/no and are the *only* verbs a docked `/plan` has:
    // `enter` returns `CloseDialog`, which cannot close a box the docking
    // gesture already popped off the modal stack.
    hint_matches(&dialog.keymap_hint(), &["j", "k"], "PlanDialog");
}

// --------------------------------------------------------------- diff

/// `/diff` — three render paths, three footers, all naming `Esc close`.
#[test]
fn diff_leaves_the_panel_chrome_alone() {
    let mut dialog = DiffDialog::new(
        "old line\nsecond old\n".into(),
        "new line\nsecond new\n".into(),
        "src/main.rs".into(),
    );
    let rows = host_rows(&mut dialog, SidePanelId::Diff, true);
    assert_host_chrome(&rows, "Diff ●", "DiffDialog");
    assert_no_second_box(&rows, "DiffDialog");
}

#[test]
fn diff_advertises_only_the_verbs_a_column_can_deliver() {
    let dialog = DiffDialog::new("a\n".into(), "b\n".into(), "t".into());
    // `enter` and `Esc` both return `CloseDialog`, so this box has no close
    // verb at all once hosted; everything else it dispatches is paging.
    hint_matches(
        &dialog.keymap_hint(),
        &["j", "k", "s", "^d", "^u", "Home", "End"],
        "DiffDialog",
    );
}

// ------------------------------------------------------------- review

/// `/review` — two modes, two footers.
#[test]
fn review_leaves_the_panel_chrome_alone() {
    let mut dialog = ReviewDialog::new();
    dialog.mode = ReviewMode::FileList;
    let rows = host_rows(&mut dialog, SidePanelId::Review, true);
    assert_host_chrome(&rows, "Review ●", "ReviewDialog");
    assert_no_second_box(&rows, "ReviewDialog");
}

/// A hint that does not vary by mode is the defect this check exists for:
/// `enter` opens a diff in the file list and means nothing in the diff view,
/// and `⌫` is the only way back that survives the host.
#[test]
fn review_advertises_only_the_verbs_its_current_mode_delivers() {
    let mut list = ReviewDialog::new();
    list.mode = ReviewMode::FileList;
    hint_matches(
        &list.keymap_hint(),
        &["j", "k", "enter"],
        "ReviewDialog/FileList",
    );

    let mut detail = ReviewDialog::new();
    detail.mode = ReviewMode::DiffView;
    hint_matches(&detail.keymap_hint(), &["⌫"], "ReviewDialog/DiffView");
}

// ---------------------------------------------------------- task_view

/// `/tasks` view — the longest footer in the codebase, and it names `J/K
/// reorder`, which the host answers as panel directions.
#[test]
fn task_view_leaves_the_panel_chrome_alone() {
    let mut dialog = TaskViewDialog::new("proj".into());
    let rows = host_rows(&mut dialog, SidePanelId::TaskView, true);
    assert_host_chrome(&rows, "Task View ●", "TaskViewDialog");
    assert_no_second_box(&rows, "TaskViewDialog");
}

#[test]
fn task_view_advertises_only_the_verbs_a_column_can_deliver() {
    let dialog = TaskViewDialog::new("proj".into());
    // No `J`/`K`: `insert_panel_direction_bindings` claims bare uppercase
    // `H`/`J`/`K`/`L`, so the reorder modifier cannot fire from a column.
    hint_matches(
        &dialog.keymap_hint(),
        &[
            "j", "k", "^d", "^u", "Home", "End", "g", "G", "enter", "r", "d", "x", "u", "t", "T",
            "X", "e",
        ],
        "TaskViewDialog",
    );
}

// ----------------------------------------------------------- research

/// `/research` — three modes, three footers, and a report view whose scroll
/// offset is never clamped against the rect it is filling.
#[test]
fn research_leaves_the_panel_chrome_alone() {
    let mut dialog = ResearchBrowserDialog::new();
    let rows = host_rows(&mut dialog, SidePanelId::Research, true);
    assert_host_chrome(&rows, "Research ●", "ResearchBrowserDialog");
    assert_no_second_box(&rows, "ResearchBrowserDialog");
}

#[test]
fn research_advertises_only_the_verbs_its_current_mode_delivers() {
    let runs = {
        let mut d = ResearchBrowserDialog::new();
        d.mode = codegg::tui::components::dialogs::research::ResearchBrowserMode::RunsList;
        d.keymap_hint()
    };
    hint_matches(
        &runs,
        &["j", "k", "enter", "r"],
        "ResearchBrowserDialog/RunsList",
    );

    let detail = {
        let mut d = ResearchBrowserDialog::new();
        d.mode = codegg::tui::components::dialogs::research::ResearchBrowserMode::RunDetail;
        d.keymap_hint()
    };
    hint_matches(
        &detail,
        &["j", "k", "enter"],
        "ResearchBrowserDialog/RunDetail",
    );

    let report = {
        let mut d = ResearchBrowserDialog::new();
        d.mode = codegg::tui::components::dialogs::research::ResearchBrowserMode::ReportView;
        d.keymap_hint()
    };
    hint_matches(
        &report,
        &["j", "k", "^d", "^u", "Home", "End", "⌫"],
        "ResearchBrowserDialog/ReportView",
    );
}

/// The §3.3 hazard, caught in the one widget that has the exact shape M002a
/// found in `InfoDialog`: an unbounded scroll whose render pass never clamps.
///
/// `Home` sets `content_scroll` to `usize::MAX`, which reaches ratatui as
/// `u16::MAX` through the field's `as u16`, so the report renders blank — and
/// because the offset only ever grows, `j`/`k` cannot walk back out. A widget
/// whose clamp is owned by its render pass must clamp against **the rect it is
/// actually filling**, which is the whole reason the body is one routine.
#[test]
fn a_docked_report_whose_scroll_is_saturated_ends_at_its_last_line() {
    use codegg::tui::components::dialogs::research::{ReportSection, ResearchBrowserMode};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut dialog = ResearchBrowserDialog::new();
    dialog.mode = ResearchBrowserMode::ReportView;
    let body = (0..40)
        .map(|i| format!("report line {i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    dialog.set_report_content(ReportSection::Report, body);

    let home = KeyEvent::new(KeyCode::Home, KeyModifiers::NONE);
    assert!(
        dialog.handle_key(home).is_none(),
        "Home is not a close verb; it only moves the scroll"
    );

    let rows = host_rows(&mut dialog, SidePanelId::Research, true);
    let text = rows.join("\n");
    assert!(
        text.contains("report line 39"),
        "saturating the report scroll and rendering hosted left the last line \
         unreachable — the tail is scrolled past with nothing to scroll back. \
         Column:\n{text}"
    );
}

// ------------------------------------------------------ security_review

/// `/security-review` — one footer, and it names `H copy hunk`, which is a
/// host direction binding.
#[test]
fn security_review_leaves_the_panel_chrome_alone() {
    let mut dialog = SecurityReviewDialog::new();
    let rows = host_rows(&mut dialog, SidePanelId::Security, true);
    assert_host_chrome(&rows, "Security ●", "SecurityReviewDialog");
    assert_no_second_box(&rows, "SecurityReviewDialog");
}

#[test]
fn security_review_advertises_only_the_verbs_a_column_can_deliver() {
    let dialog = SecurityReviewDialog::new();
    // No `H`: bare uppercase `H` is `InputAction::DirectionLeft`. No `n`/`p`
    // confusion — both remain, they are only ever set here by their own arms.
    hint_matches(
        &dialog.keymap_hint(),
        &["j", "k", "^d", "^u", "f", "n", "p", "enter", "h", "]", "["],
        "SecurityReviewDialog",
    );
}

// -------------------------------------------------------------- plugin

/// `/plugin` detail — three bordered chunks, one of which is a footer.
#[test]
fn plugin_leaves_the_panel_chrome_alone() {
    let mut dialog = PluginDialog::new(
        "id".into(),
        "Widget".into(),
        (0..30).map(|i| format!("line {i}")).collect(),
    );
    let rows = host_rows(&mut dialog, SidePanelId::Plugins, true);
    assert_host_chrome(&rows, "Plugins ●", "PluginDialog");
    assert_no_second_box(&rows, "PluginDialog");
}

#[test]
fn plugin_advertises_only_the_verbs_a_column_can_deliver() {
    let dialog = PluginDialog::new("id".into(), "Widget".into(), vec![]);
    hint_matches(&dialog.keymap_hint(), &["j", "k"], "PluginDialog");
}

/// The plugin body is the one widget whose footer carried a *scroll indicator*
/// (`"12-30/30"`), so it is the §14 case: retiring the footer must not take the
/// only end-of-content signal with it. The indicator is content, not a keymap,
/// and it has to survive into the column.
#[test]
fn a_docked_plugin_still_says_where_the_end_is() {
    let mut dialog = PluginDialog::new(
        "id".into(),
        "Widget".into(),
        (0..30).map(|i| format!("line {i}")).collect(),
    );
    for _ in 0..40 {
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    let rows = host_rows(&mut dialog, SidePanelId::Plugins, true);
    let text = rows.join("\n");
    assert!(
        text.contains("line 29"),
        "scrolled to the end hosted, the last line must be on screen. Column:\n{text}"
    );
}

// ----------------------------------------------------------------- mcp

/// `/mcp` — two bordered panes and two copies of the same footer.
#[test]
fn mcp_leaves_the_panel_chrome_alone() {
    let mut dialog = McpDialog::new();
    let rows = host_rows(&mut dialog, SidePanelId::Mcp, true);
    assert_host_chrome(&rows, "MCP ●", "McpDialog");
    assert_no_second_box(&rows, "McpDialog");
}

/// `Esc` is the only way out of action mode, and the host takes it. The hint
/// therefore differs by mode, and neither version promises an exit.
#[test]
fn mcp_advertises_only_the_verbs_its_current_mode_delivers() {
    let mut browse = McpDialog::new();
    browse.action_mode = false;
    hint_matches(
        &browse.keymap_hint(),
        &["j", "k", "enter"],
        "McpDialog/browse",
    );

    let mut actions = McpDialog::new();
    actions.action_mode = true;
    hint_matches(
        &actions.keymap_hint(),
        &["j", "k", "enter"],
        "McpDialog/action",
    );
}

// ------------------------------------------------------------- keybind

/// `/keybinds` — a footer per mode, and `Esc` is how every mode is left.
#[test]
fn keybind_leaves_the_panel_chrome_alone() {
    let mut dialog = KeybindDialog::new(theme());
    let rows = host_rows(&mut dialog, SidePanelId::Keybinds, true);
    assert_host_chrome(&rows, "Keys ●", "KeybindDialog");
    assert_no_second_box(&rows, "KeybindDialog");
}

#[test]
fn keybind_advertises_only_the_verbs_its_current_mode_delivers() {
    let dialog = KeybindDialog::new(theme());
    hint_matches(
        &dialog.keymap_hint(),
        &["j", "k", "enter", "r", "e", "i"],
        "KeybindDialog",
    );
}

// --------------------------------------------------------------- theme

/// `/theme` — a footer whose wording changes once a preview is live.
#[test]
fn theme_leaves_the_panel_chrome_alone() {
    let mut dialog = ThemePickerDialog::new(theme());
    let rows = host_rows(&mut dialog, SidePanelId::Themes, true);
    assert_host_chrome(&rows, "Themes ●", "ThemePickerDialog");
    assert_no_second_box(&rows, "ThemePickerDialog");
}

#[test]
fn theme_advertises_only_the_verbs_its_current_state_delivers() {
    let mut dialog = ThemePickerDialog::new(theme());
    hint_matches(
        &dialog.keymap_hint(),
        &["j", "k", "enter"],
        "ThemePickerDialog/settled",
    );

    // Navigating arms a live preview, which is a different commitment: `enter`
    // commits rather than applies, and `Esc` would revert — a key the host
    // answers, so the hint must not lean on it.
    dialog.select_down();
    assert!(
        dialog.is_previewing(),
        "navigating should arm a preview; the two hints only differ if it does"
    );
    hint_matches(
        &dialog.keymap_hint(),
        &["j", "k", "enter"],
        "ThemePickerDialog/previewing",
    );
}

// --------------------------------------------------------- panel_picker

/// `PanelPicker` declares `docks_as_viewer() == true` but `App::dock_target`
/// has no `DialogType::PanelPicker` arm, so it falls to `None`. This test
/// exercises the seam it can never reach in production, and exists to pin that
/// down: the picker must still be chrome-free if a mapping is ever added, and
/// adding one is explicitly not this milestone's business.
#[test]
fn panel_picker_leaves_the_panel_chrome_alone_if_it_ever_docks() {
    let mut dialog = PanelPickerDialog::new(theme(), SidePanelId::Files, "widget");
    let rows = host_rows(&mut dialog, SidePanelId::Files, true);
    assert_host_chrome(&rows, "Files ●", "PanelPickerDialog");
    assert_no_second_box(&rows, "PanelPickerDialog");
}

/// §3.5's recorded answer.
///
/// The host resolves the docking gesture and `App::dock_target` returns `None`
/// for this box, so the gesture refuses — the key is swallowed by
/// `PanelPicker::handle_key` never running. For the `/panels` picker itself
/// that is harmless: `Shift+←` has no meaning there. No `dock_target` arm is
/// added, because there is no panel a picker should be adopted into.
#[test]
fn the_panel_picker_has_no_dock_target_and_loses_the_docking_gesture() {
    // A picker for `/agents` has the same property, so the gap is not specific
    // to one caller: the widget opts into docking and nothing consumes it.
    let dialog = PanelPickerDialog::new(theme(), SidePanelId::Files, "widget");
    assert_eq!(
        dialog.dialog_type(),
        codegg::tui::components::component::DialogType::PanelPicker
    );
    assert!(
        dialog.docks_as_viewer(),
        "if this ever becomes false the gesture stops being swallowed and starts \
         being answered, which would make this record stale"
    );
}
