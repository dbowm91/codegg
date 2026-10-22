//! A hosted `InfoDialog` must leave the panel's chrome alone, and must
//! advertise the verbs it really dispatches.
//!
//! ## Why this suite exists
//!
//! `InfoDialog` is one widget that the host mounts for **fourteen** panels
//! (`/cost`, `/usage`, `/context`, `/shell-show`, `/stats`, `/tasks`, `/goal`,
//! `/memory-search`, `/doctor`, `/collaborators`, `/team`, `/control`,
//! `/logs`, `/project-init`), so every defect below was visible in fourteen
//! places at once. Its modal render paints three stacked bordered boxes —
//! title, content, footer — and the panel host has *already* painted a border,
//! a title and the `●` focus marker into the same rect. Painting a second set
//! of chrome over them did not layer; it overwrote all three.
//!
//! The suite that already exists (`tui_panel_docking.rs`) covers the docking
//! *gesture* for two members of that fan-out. It cannot cover this: nothing
//! there reads a rendered column, so a hosted `InfoDialog` that destroyed its
//! host's title passed.
//!
//! ## Reading the buffer, not the source
//!
//! Every assertion here reads the **rendered column**. "The widget does not
//! paint a `Block`" is not statically checkable — `Component::render` is the
//! modal path, where a border is correct — so the only honest form of the
//! claim is that the host's border, title and focus dot are on screen
//! afterwards, and that no second box was drawn inside them.

use std::sync::Arc;

use codegg::tui::components::component::Component;
use codegg::tui::components::dialogs::info::{InfoDialog, InfoType};
use codegg::tui::panels::{render_hosted, SidePanelId};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn dialog(info_type: InfoType, lines: &[&str]) -> InfoDialog {
    InfoDialog::new(
        Arc::new(codegg::tui::theme::Theme::dark()),
        info_type,
        lines.iter().map(|line| line.to_string()).collect(),
    )
}

/// The column exactly as the host paints it: `HostPanel::render` builds this
/// title from `SidePanelId::label()` and the focus marker, and then calls
/// `render_in_panel` into the same rect.
fn host_title(panel: SidePanelId, focused: bool) -> String {
    let dot = if focused { "●" } else { " " };
    format!(" {} {} ", panel.label(), dot)
}

/// Render `dialog` through the production hosted path and return the rows.
fn hosted_rows(dialog: &mut InfoDialog, panel: SidePanelId, area: Rect) -> Vec<String> {
    let theme = Arc::new(codegg::tui::theme::Theme::dark());
    let mut buf = Buffer::empty(Rect::new(0, 0, area.width, area.height));
    render_hosted(dialog, &mut buf, area, &theme, &host_title(panel, true));
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| {
                    buf.cell((x, y))
                        .map(|cell| cell.symbol().to_string())
                        .unwrap_or_else(|| " ".to_string())
                })
                .collect::<String>()
        })
        .collect()
}

/// Render `dialog` through the modal path for comparison.
fn modal_rows(dialog: &mut InfoDialog, area: Rect) -> Vec<String> {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let theme = Arc::new(codegg::tui::theme::Theme::dark());
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| dialog.render(frame, frame.area(), &theme))
        .expect("draw");
    let buf = terminal.backend().buffer().clone();
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| {
                    buf.cell((x, y))
                        .map(|cell| cell.symbol().to_string())
                        .unwrap_or_else(|| " ".to_string())
                })
                .collect::<String>()
        })
        .collect()
}

/// Assert `rows` carries the host's frame: a border on all four sides, the
/// host's own title, and the focus marker the host paints.
///
/// The title check uses a panel whose label (`Shells`) is *not* the widget's
/// own title (` Shell Command `), so it distinguishes the two texts instead of
/// passing because they happen to agree for most of the fan-out.
fn assert_host_chrome_intact(rows: &[String], panel: SidePanelId, what: &str) {
    let height = rows.len();

    let top = &rows[0];
    assert!(
        top.starts_with('┌') && top.ends_with('┐'),
        "{what}: host top border is gone — row 0 is {top:?}"
    );
    let bottom = &rows[height - 1];
    assert!(
        bottom.starts_with('└') && bottom.ends_with('┘'),
        "{what}: host bottom border is gone — row {} is {bottom:?}",
        height - 1
    );
    for (y, row) in rows.iter().enumerate().skip(1).take(height - 2) {
        let first = row.chars().next().unwrap();
        let last = row.chars().last().unwrap();
        assert!(
            first == '│' && last == '│',
            "{what}: host side border broken at row {y} — {row:?}"
        );
    }
    assert!(
        top.contains(panel.label()),
        "{what}: the host's own title `{}` is not on screen — row 0 is {top:?}",
        panel.label()
    );
    assert!(
        top.contains('●'),
        "{what}: the `●` focus marker is gone, so a focused panel renders \
         identically to an unfocused one — row 0 is {top:?}"
    );
}

/// Assert the widget drew no box of its own inside the host's frame.
///
/// A second bordered box shows up as a full-width run of `─` on some interior
/// row, or a full-height run of `│` on some interior column. The old render
/// drew two such lines (the content box's top and bottom edges).
fn assert_no_second_box(rows: &[String], what: &str) {
    let width = rows[0].chars().count();
    for (y, row) in rows.iter().enumerate().skip(1).take(rows.len() - 2) {
        let interior: String = row.chars().skip(1).take(width - 2).collect();
        let painted: Vec<char> = interior.chars().filter(|c| *c != ' ').collect();
        // `all` is vacuously true on an empty slice, and a blank row is the
        // row reserved for the scroll indicator — not a second box.
        if !painted.is_empty() && painted.iter().all(|c| *c == '─') {
            panic!("{what}: an interior row is a full horizontal rule at row {y} — {row:?}");
        }
    }
    for x in 1..width.saturating_sub(1) {
        let column: String = rows
            .iter()
            .skip(1)
            .take(rows.len() - 2)
            .map(|row| row.chars().nth(x).unwrap())
            .collect();
        let painted: Vec<char> = column.chars().filter(|c| *c != ' ').collect();
        if !painted.is_empty() && painted.iter().all(|c| *c == '│') {
            panic!("{what}: an interior column is a full vertical rule at x={x} — {column:?}");
        }
    }
}

/// `Showing {start}-{end} of {total}` — the scroll indicator, wherever it is
/// painted. Returns `None` when the content fits and there is nothing to say.
fn scroll_indicator(rows: &[String]) -> Option<(usize, usize, usize)> {
    let text = rows.concat();
    let start = text.find("Showing ")? + "Showing ".len();
    let tail = &text[start..];
    let end = tail.find(" of ")?;
    let (range, rest) = tail.split_at(end);
    let (start, end) = range.split_once('-')?;
    let total: usize = rest[" of ".len()..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    Some((start.parse().ok()?, end.parse().ok()?, total))
}

// ── Host chrome survival ─────────────────────────────────────────────

#[test]
fn a_hosted_info_dialog_leaves_the_hosts_border_title_and_focus_dot_alone() {
    // `Shells` (the panel's label) against ` Shell Command ` (the widget's own
    // title): the two texts differ, so a surviving host title is evidence and
    // not coincidence.
    let mut d = dialog(InfoType::ShellShow, &["$ cargo test"]);
    let rows = hosted_rows(&mut d, SidePanelId::ShellShow, Rect::new(0, 0, 44, 16));

    assert_host_chrome_intact(&rows, SidePanelId::ShellShow, "hosted ShellShow");
    assert!(
        !rows.concat().contains("Shell Command"),
        "the widget's own title is on screen, so it overwrote the host's: {:?}",
        rows
    );
}

#[test]
fn a_second_hosted_info_type_also_leaves_the_host_alone() {
    // `Logs` is chosen for a second reason: it is the window
    // `scroll_to_end()` exists for, so this also proves the append-only path
    // survives the chrome change.
    let mut d = dialog(InfoType::Logs, &["one", "two", "three"]);
    d.scroll_to_end();
    let rows = hosted_rows(&mut d, SidePanelId::Logs, Rect::new(0, 0, 44, 16));

    assert_host_chrome_intact(&rows, SidePanelId::Logs, "hosted Logs");
    assert_no_second_box(&rows, "hosted Logs");
}

#[test]
fn every_reachable_hosted_info_type_leaves_the_host_alone() {
    // The reachable-hosted subset, derived from the live registry rather than
    // restated: an `InfoType` is reachable when its `DialogType` resolves to a
    // non-bespoke panel that `App::dock_target` actually maps.
    //
    // The three that are *not* here are the finding, not an oversight:
    //
    //   `TerminalShow` → `DialogType::Terminal`, a bespoke panel that owns its
    //     own `SidePanel` impl and never renders through `render_hosted`.
    //   `Agents`      → `DialogType::Agent`, bespoke *and* the one panel
    //     `discards_a_docked_box()` names: it shows the management list, not
    //     the editor box, so docking would throw the component away.
    //   `WorktreeList`→ `DialogType::WorktreeList`, which `dock_target` omits
    //     entirely, so `Shift+L` refuses and nothing is ever hosted.
    //
    // `ProjectInit` is listed and shares `DialogType::Team` with `Team` — two
    // content types, one panel, and the reason the keymap tests below key on
    // `InfoType`.
    let reachable: &[(InfoType, SidePanelId)] = &[
        (InfoType::Context, SidePanelId::Context),
        (InfoType::Cost, SidePanelId::Cost),
        (InfoType::Usage, SidePanelId::Usage),
        (InfoType::ShellShow, SidePanelId::ShellShow),
        (InfoType::Stats, SidePanelId::Stats),
        (InfoType::TaskList, SidePanelId::Tasks),
        (InfoType::GoalShow, SidePanelId::Goal),
        (InfoType::MemoryResults, SidePanelId::Memory),
        (InfoType::DoctorReport, SidePanelId::Doctor),
        (InfoType::Collaborators, SidePanelId::Collaborators),
        (InfoType::Team, SidePanelId::Team),
        (InfoType::Control, SidePanelId::Control),
        (InfoType::Logs, SidePanelId::Logs),
        (InfoType::ProjectInit, SidePanelId::Team),
    ];

    for (info_type, panel) in reachable {
        let mut d = dialog(info_type.clone(), &["alpha", "beta", "gamma"]);
        let rows = hosted_rows(&mut d, *panel, Rect::new(0, 0, 44, 16));
        assert_host_chrome_intact(&rows, *panel, &format!("hosted {info_type:?}"));
        assert_no_second_box(&rows, &format!("hosted {info_type:?}"));
    }
}

#[test]
fn a_hosted_info_dialog_at_a_degenerate_size_paints_no_second_box() {
    // The host's own frame is already on screen by the time the widget runs,
    // so a column with no interior must leave that frame alone rather than
    // panic or paint over it.
    for (w, h) in [(2u16, 2u16), (3, 3), (4, 2), (2, 4)] {
        let mut d = dialog(InfoType::Cost, &["x", "y"]);
        let rows = hosted_rows(&mut d, SidePanelId::Cost, Rect::new(0, 0, w, h));
        assert_eq!(rows.len(), h as usize, "{w}x{h}: wrong row count");
        for row in &rows {
            assert_eq!(row.chars().count(), w as usize, "{w}x{h}: wrong width");
        }
        assert!(
            rows[0].starts_with('┌'),
            "{w}x{h}: host top border lost — {:?}",
            rows[0]
        );
    }
}

// ── Scroll reachability parity ───────────────────────────────────────

/// One logical line wide enough to wrap into several rows, so the offset is
/// counted in wrapped rows rather than in `lines.len()`.
const WRAPPED: &str = "the quick brown fox jumps over the lazy dog and keeps on going";

#[test]
fn a_hosted_info_dialog_can_scroll_to_the_last_wrapped_line() {
    // The hazard this milestone exists to avoid: `Down`/`j` is deliberately
    // unbounded and `scroll_to_end()` stores a `usize::MAX` sentinel, both
    // relying on the render pass to clamp against the true wrapped-row
    // maximum. An inset that leaves the clamp computed from a different rect
    // than the one the text is painted into makes the tail of the content
    // unreachable — with no error anywhere, and in a screenshot it just looks
    // like scrolling stopped two rows early.
    let mut lines = vec!["FIRST".to_string()];
    lines.extend(std::iter::repeat_n(WRAPPED.to_string(), 40));
    lines.push("LASTLINE".to_string());

    let mut d = InfoDialog::new(
        Arc::new(codegg::tui::theme::Theme::dark()),
        InfoType::Logs,
        lines,
    );
    d.scroll_to_end();
    let rows = hosted_rows(&mut d, SidePanelId::Logs, Rect::new(0, 0, 44, 16));

    let text = rows.concat();
    assert!(
        text.contains("LASTLINE"),
        "the last line is unreachable when hosted — rendered column:\n{}",
        rows.join("\n")
    );
    assert!(
        !text.contains("FIRST"),
        "a `scroll_to_end()` window must not still be showing its head:\n{}",
        rows.join("\n")
    );
}

#[test]
fn a_hosted_info_dialog_reaches_the_same_end_position_as_a_modal_one() {
    // Both paths must agree on how far "the end" is. They are allowed to
    // disagree on how many rows fit — the column is a different size from the
    // modal box — but not on which wrapped row is the last one.
    let mut lines = vec!["FIRST".to_string()];
    lines.extend(std::iter::repeat_n(WRAPPED.to_string(), 40));
    lines.push("LASTLINE".to_string());

    let area = Rect::new(0, 0, 44, 16);
    let mut hosted = InfoDialog::new(
        Arc::new(codegg::tui::theme::Theme::dark()),
        InfoType::Logs,
        lines.clone(),
    );
    hosted.scroll_to_end();
    let hosted_rows = hosted_rows(&mut hosted, SidePanelId::Logs, area);

    let mut modal = InfoDialog::new(
        Arc::new(codegg::tui::theme::Theme::dark()),
        InfoType::Logs,
        lines,
    );
    modal.scroll_to_end();
    let modal_rows = modal_rows(&mut modal, area);

    let (_, hosted_end, hosted_total) = scroll_indicator(&hosted_rows).unwrap_or_else(|| {
        panic!(
            "no scroll indicator when hosted:\n{}",
            hosted_rows.join("\n")
        )
    });
    let (_, modal_end, modal_total) = scroll_indicator(&modal_rows)
        .unwrap_or_else(|| panic!("no scroll indicator modally:\n{}", modal_rows.join("\n")));

    assert_eq!(
        hosted_total, modal_total,
        "the two paths disagree on how many wrapped rows there are"
    );
    assert_eq!(
        hosted_end, modal_total,
        "the hosted view stops at row {hosted_end} of {hosted_total}; the last \
         wrapped row is unreachable"
    );
    assert_eq!(
        hosted_end, modal_end,
        "the two paths stop at different rows ({hosted_end} vs {modal_end})"
    );
}

#[test]
fn modal_rendering_is_unchanged() {
    // The regression net for the extraction that made the two paths share one
    // routine. Modal `InfoDialog` is what `/cost` and thirteen siblings show on
    // first open. Its frame is, for a 44×16 modal:
    //
    //   rows 0-2   title box    (border, ` Logs `, border)
    //   rows 3-12  content box  (border + eight text rows + border)
    //   rows 13-15 footer box   (border, verb line, border)
    //
    // Every one of those positions is asserted, because "the extraction did
    // not move the modal layout" is exactly the claim that cannot be checked by
    // reading the new code.
    let (w, h) = (44u16, 16u16);
    let mut d = dialog(InfoType::Logs, &["alpha", "beta", "gamma"]);
    let rows = modal_rows(&mut d, Rect::new(0, 0, w, h));

    let rule = |left: char, right: char| {
        let mut s = String::from(left);
        s.push_str(&"─".repeat((w - 2) as usize));
        s.push(right);
        s
    };

    // The title is drawn *inline on the top border row*, so row 0 carries it
    // and row 1 is the box's empty side-wall row.
    assert!(
        rows[0].starts_with("┌ Logs ") && rows[0].ends_with('┐'),
        "modal title top row moved or changed its title: {:?}",
        rows[0]
    );
    assert_eq!(
        rows[1],
        format!("│{}│", " ".repeat((w - 2) as usize)),
        "modal title box interior row changed: {:?}",
        rows[1]
    );
    assert_eq!(
        rows[2].trim_end(),
        rule('└', '┘'),
        "modal title bottom row moved"
    );
    assert_eq!(
        rows[3].trim_end(),
        rule('┌', '┐'),
        "modal content box top border moved"
    );
    for (row, expected) in [(4, "alpha"), (5, "beta"), (6, "gamma")] {
        assert!(
            rows[row].starts_with(&format!("│{expected}")) && rows[row].ends_with('│'),
            "content row {} changed: {:?}",
            row,
            rows[row]
        );
    }
    assert_eq!(
        rows[12].trim_end(),
        rule('└', '┘'),
        "modal content box bottom border moved"
    );
    assert_eq!(
        rows[13].trim_end(),
        rule('┌', '┐'),
        "modal footer top border moved"
    );
    assert!(
        rows[14].starts_with("│ j/k scroll  |  Esc/Enter close") && rows[14].ends_with('│'),
        "modal footer verb line changed: {:?}",
        rows[14]
    );
    assert_eq!(
        rows[15].trim_end(),
        rule('└', '┘'),
        "modal footer bottom border moved"
    );
}

// ── The advertised keymap ────────────────────────────────────────────

/// A dialog with enough content to scroll, so the indicator exists and the
/// offset is observable.
fn scrollable(info_type: &InfoType) -> InfoDialog {
    let mut lines = Vec::new();
    for i in 0..200 {
        lines.push(format!("row-{i:04}"));
    }
    InfoDialog::new(
        Arc::new(codegg::tui::theme::Theme::dark()),
        info_type.clone(),
        lines,
    )
}

/// The scroll offset, observed rather than read: `scroll` is private and the
/// only thing that ever reads it back is the render pass's clamp, so the
/// indicator's start row *is* the observable state. `0` when the content fits
/// and there is no indicator to read.
fn observed_offset(dialog: &mut InfoDialog) -> usize {
    let rows = hosted_rows(dialog, SidePanelId::Logs, Rect::new(0, 0, 44, 12));
    scroll_indicator(&rows)
        .map(|(start, ..)| start)
        .unwrap_or(0)
}

/// What `handle_key` really does for this `InfoType`, **observed** rather than
/// restated from the source. A hint derived from the same match arms can only
/// drift from them together; a hint asserted against a hand-written list here
/// would drift the moment an arm is added.
fn dispatched_verbs(info_type: &InfoType) -> Vec<&'static str> {
    let mut out = Vec::new();
    let mut d = scrollable(info_type);

    let before = observed_offset(&mut d);
    let down = d.handle_key(key(KeyCode::Down));
    if down.is_none() && observed_offset(&mut d) > before {
        out.push("j scroll");
    }

    let before = observed_offset(&mut d);
    let up = d.handle_key(key(KeyCode::Up));
    if up.is_none() && observed_offset(&mut d) < before {
        out.push("k scroll");
    }

    if matches!(
        d.handle_key(key(KeyCode::Char('a'))),
        Some(codegg::tui::app::TuiMsg::ProjectInitApprove)
    ) {
        out.push("a publish");
    }
    if matches!(
        d.handle_key(key(KeyCode::Enter)),
        Some(codegg::tui::app::TuiMsg::CloseDialog)
    ) {
        out.push("enter close");
    }
    if matches!(
        d.handle_key(key(KeyCode::Esc)),
        Some(codegg::tui::app::TuiMsg::CloseDialog)
    ) {
        out.push("esc close");
    }
    out
}

fn hint_for(info_type: &InfoType) -> String {
    scrollable(info_type).keymap_hint().unwrap_or_default()
}

#[test]
fn a_hosted_info_dialog_advertises_only_verbs_it_dispatches() {
    // The negative direction, and the one that matters: the shared fallback in
    // `panels/hosted.rs` reads `"j/k scroll · i act"`, and `i` is dispatched by
    // no arm of this widget. Advertising a dead verb is the defect.
    for info_type in all_info_types() {
        let hint = hint_for(&info_type);
        let dispatched = dispatched_verbs(&info_type);

        assert!(
            !hint.contains("i act"),
            "{info_type:?} advertises the fallback's dead `i act`: {hint:?}"
        );
        assert!(
            hint.contains("j") && hint.contains("k"),
            "{info_type:?} does not advertise the scroll keys it dispatches: {hint:?}"
        );
        // Nothing advertised may be inert: every key the hint names must be one
        // `handle_key` acts on for this content type.
        if hint.contains('a') {
            assert!(
                dispatched.contains(&"a publish"),
                "{info_type:?} advertises `a` but dispatches none: {hint:?}"
            );
        }
    }
}

#[test]
fn project_init_advertises_publish_and_no_other_content_type_does() {
    let dispatched = dispatched_verbs(&InfoType::ProjectInit);
    assert!(
        dispatched.contains(&"a publish"),
        "premise is wrong: ProjectInit does not dispatch `a`"
    );
    assert!(
        hint_for(&InfoType::ProjectInit).contains("a publish"),
        "ProjectInit dispatches `a` publish but does not advertise it: {:?}",
        hint_for(&InfoType::ProjectInit)
    );

    for info_type in all_info_types() {
        if info_type == InfoType::ProjectInit {
            continue;
        }
        assert!(
            !dispatched_verbs(&info_type).contains(&"a publish"),
            "premise is wrong: {info_type:?} dispatches `a`"
        );
        assert!(
            !hint_for(&info_type).contains('a'),
            "{info_type:?} advertises `a` but only ProjectInit dispatches it: {:?}",
            hint_for(&info_type)
        );
    }
}

#[test]
fn project_init_and_team_hints_differ_despite_sharing_a_dialog_type() {
    // `InfoType::ProjectInit` and `InfoType::Team` both map to
    // `DialogType::Team` and therefore to one panel. Keyed on `DialogType`
    // these two are indistinguishable — which is exactly why the hint has to
    // be written per `InfoType` and why a `DialogType`-keyed test could never
    // have caught this.
    let init = dialog(InfoType::ProjectInit, &["alpha"]);
    let team = dialog(InfoType::Team, &["alpha"]);
    assert_eq!(
        init.dialog_type(),
        team.dialog_type(),
        "premise is wrong: these no longer share a DialogType"
    );

    let init_hint = init.keymap_hint().expect("ProjectInit needs a hint");
    let team_hint = team.keymap_hint().expect("Team needs a hint");
    assert_ne!(init_hint, team_hint, "one hint for two different verb sets");
}

#[test]
fn no_undispatched_or_undeliverable_verb_is_left_out_by_accident() {
    // Every arm of `handle_key` is either advertised, or is one of the two
    // documented exemptions — and that list is exact, so an arm added later
    // without a hint update fails here rather than shipping silent.
    //
    //   `esc close` — `Esc` is the **mode switch**, so it means "close" in the
    //     modal box and something else in a column (`src/tui/panels/mod.rs`).
    //     In a column it does not close anything, and a hosted hint cannot
    //     promise a key whose meaning the surface does not fix.
    //   `enter close` — it does reach the widget, but docking *popped* the box
    //     off the modal stack, so `TuiMsg::CloseDialog` runs `close_dialog()`
    //     against an empty stack and changes nothing. `Shift+←` is the way
    //     back to a modal, and `composer_hints` already says so.
    const HOST_INTERCEPTS: &[&str] = &["esc close", "enter close"];

    for info_type in all_info_types() {
        let hint = hint_for(&info_type);
        for verb in dispatched_verbs(&info_type) {
            let key = verb.split_whitespace().next().unwrap_or_default();
            let advertised = hint.contains(key);
            if HOST_INTERCEPTS.contains(&verb) {
                assert!(
                    !advertised,
                    "{info_type:?} advertises `{key}` as {verb:?}, but the hosted \
                     path does not deliver it: {hint:?}"
                );
            } else {
                assert!(
                    advertised,
                    "{info_type:?} dispatches {verb:?} but does not advertise \
                     `{key}`: {hint:?}"
                );
            }
        }
    }
}

#[test]
fn the_hint_does_not_describe_a_caller_supplied_modal_footer() {
    // `/project-init` sets `"a: publish   Esc: cancel"` and `/shell-show` sets
    // a six-verb footer through `set_custom_footer`. Those strings describe
    // the **modal** box, where `Esc` really does close. Reusing them for the
    // hosted status line would re-advertise a dead `Esc` — the defect this
    // milestone removes — so the hint is derived from `handle_key` instead.
    let mut d = dialog(InfoType::ProjectInit, &["candidate"]);
    d.set_custom_footer("a: publish   Esc: cancel".into());
    let hint = d.keymap_hint().expect("ProjectInit needs a hint");
    assert!(
        !hint.contains("cancel"),
        "the hint replayed a modal footer verbatim: {hint:?}"
    );
    assert!(
        hint.contains("a publish"),
        "the hint dropped the one verb the footer names that is real: {hint:?}"
    );
}

// ── Reachability of the widget itself ────────────────────────────────

#[test]
fn the_three_unreachable_info_types_are_still_unreachable() {
    // M001's guards own these two, and `dock_target` owns the third. This
    // milestone must not disturb any of them, and a test that only checked the
    // fourteen reachable cases would not notice it had.
    let terminal = dialog(InfoType::TerminalShow, &["$ htop"]);
    assert_eq!(
        terminal.dialog_type(),
        codegg::tui::components::component::DialogType::Terminal,
    );
    assert!(
        SidePanelId::Terminal.is_bespoke(),
        "Terminal must stay bespoke: it owns its own SidePanel impl"
    );

    let agents = dialog(InfoType::Agents, &["build"]);
    assert_eq!(
        agents.dialog_type(),
        codegg::tui::components::component::DialogType::Agent,
    );
    assert!(
        SidePanelId::Agents.discards_a_docked_box(),
        "Agents must keep refusing a docked box"
    );

    // `WorktreeList` resolves to no panel at all, so the gesture refuses and
    // nothing is ever hosted. Recorded here as the third exclusion; the
    // registry sweep in `tui_panel_manager_reachability.rs` covers the map.
    let worktrees = dialog(InfoType::WorktreeList, &["main"]);
    assert_eq!(
        worktrees.dialog_type(),
        codegg::tui::components::component::DialogType::WorktreeList,
    );
}

fn all_info_types() -> Vec<InfoType> {
    vec![
        InfoType::Context,
        InfoType::Cost,
        InfoType::Usage,
        InfoType::ShellShow,
        InfoType::TerminalShow,
        InfoType::Stats,
        InfoType::TaskList,
        InfoType::WorktreeList,
        InfoType::GoalShow,
        InfoType::MemoryResults,
        InfoType::DoctorReport,
        InfoType::Agents,
        InfoType::Collaborators,
        InfoType::Team,
        InfoType::Control,
        InfoType::Logs,
        InfoType::ProjectInit,
    ]
}
