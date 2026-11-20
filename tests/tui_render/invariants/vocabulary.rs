//! I6's vocabulary: every surface that advertises keys uses the one dialect.
//!
//! `HintLine` is the only producer, and this module is the check that it stayed
//! the only producer. It reads two things a surface can advertise through:
//!
//! * the **declaration** — [`Component::keymap_hint`], which is what the panel
//!   host paints and what the I4 invariant presses; and
//! * the **painted row** — the cells a surface actually puts on screen.
//!
//! The second half was missing: a footer written as `"↑/↓ navigate  |  Enter
//! select  |  Esc close"` inside a `Paragraph` is invisible to a scan that reads
//! declarations, which is how two dialects survived a milestone named "one hint
//! vocabulary". The painted read comes from the rendered frame, not the source —
//! a regex over Rust strings misreads the language, and a row that is not
//! painted cannot be reported.

use std::sync::Arc;

use codegg::tui::app::App;
use codegg::tui::input::InputMode;
use codegg::tui::panels::{
    agents::AgentsPanel, session::SessionPanel, terminal::TerminalPanel, PanelHints,
    PanelPlacement, SidePanel,
};
use codegg::tui::theme::Theme;
use ratatui::layout::Rect;

use super::capture::{self, Frame};
use super::fixtures;

/// Every way one rendered hint leaves the vocabulary.
fn dialect_findings(hint: &str, bounded: bool) -> Vec<String> {
    let mut findings = Vec::new();
    if hint.contains('|') {
        findings.push(format!("a second separator in {hint:?}"));
    }
    if hint.contains("  ") {
        findings.push(format!("a hand-spaced row in {hint:?}"));
    }
    // The item checks read the row split at **every** separator, not only the
    // one this vocabulary uses. A pipe-separated row split only at ` · ` is one
    // long item whose "verb" is the rest of the row, so the casing check
    // reported the next item's key as a Title Case verb — a true finding for a
    // false reason, which is worse than no finding at all.
    for part in segments(hint) {
        let Some((key, verb)) = part.split_once(' ') else {
            continue;
        };
        // A colon is drift only where it *joins a key to its verb* —
        // `Enter: cancel`. A colon inside a value (`Search: foo`,
        // `Showing 1-22 of 60`) is the value's own punctuation, and flagging it
        // would demand that a status line stop saying what it is a status of.
        if key.ends_with(':') && key_token(key.trim_end_matches(':')) {
            findings.push(format!("a colon-separated verb in {hint:?}"));
        }
        for word in verb.split_whitespace() {
            if word.starts_with(char::is_uppercase) {
                findings.push(format!("a Title Case verb {word:?} in {hint:?}"));
            }
        }
    }
    // The exit-key rule is a property of a row that is TRUNCATED. Its
    // rationale is that a row which drops whole parts from the end can lose
    // the key that leaves; a row rendered unbounded cannot lose anything, so
    // holding it to this rule would demand a reordering with no defect behind
    // it. `bounded` is the difference between the two, and it is passed rather
    // than guessed from the string.
    if bounded {
        // The rule is about the row's **items**, not its first byte. A row may
        // legitimately open with a label (`Confirm  …`) or a value
        // (`Question 1/1  …`), and those are not the hint; the hint starts at
        // the first item that names a key and a verb. From there, the exit item
        // must lead, because the box clips the row from the end.
        let items = segments(hint);
        if let Some(start) = items.iter().position(|item| is_key_verb_item(item)) {
            let hint_items = &items[start..];
            if hint_items.iter().any(|item| item.contains("Esc")) && !hint_items[0].contains("Esc")
            {
                findings.push(format!("an exit key that is not first in {hint:?}"));
            }
        }
    }
    findings
}

/// The glyphs a frame draws its own edges with.
///
/// A painted row is read out of the frame, so it arrives wrapped in the box it
/// lives in. The edges are chrome, not content, and a leading `│` would make
/// the first item's key unreadable to [`key_token`] — the check would then
/// silently stop seeing the row it was added to see.
const FRAME_GLYPHS: &str = "│┃┆┊╎─━┄┅┈┉┌┐└┘├┤┬┴┼╭╮╰╯╔╗╚╝╠╣╦╩╬▏▕ ";

/// The glyphs that separate one painted column from the next.
///
/// A frame row is a *line of the screen*, not of a widget: two panes share a
/// row, and a split-diff body row is two columns joined by a `│`, so
/// `-  1  │ let x = 1;` reads as two one-character hint items unless the rule is
/// a boundary.
const COLUMN_RULES: &[char] = &['│', '┃', '┆', '┊', '╎'];

/// Brackets a hint may be wrapped in.
///
/// `/plan` and `/question` carry a keymap inside a parenthesised border title.
/// The brackets are the wrapper, not part of the first item: a reader that took
/// `Options (` as a key would report neither the pipe nor the casing it wraps.
const ROW_BRACKETS: &[char] = &['(', ')'];

fn strip_frame_glyphs(row: &str) -> &str {
    row.trim_matches(|c: char| FRAME_GLYPHS.contains(c))
}

/// `row` split at every separator a hint row has ever been written with.
///
/// The one vocabulary's ` · `, the legacy `|`, and a bare run of two or more
/// spaces are all separators: the first is the dialect and the other two are
/// what this check exists to find. A single space is not, because it is what
/// joins a key to its verb.
fn segments(row: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = row.char_indices().collect();
    let mut breaks: Vec<(usize, usize)> = Vec::new();
    let mut k = 0;
    while k < chars.len() {
        let (at, c) = chars[k];
        if c == '|' {
            breaks.push((at, at + 1));
            k += 1;
            continue;
        }
        if c == '·' {
            breaks.push((at, at + '·'.len_utf8()));
            k += 1;
            continue;
        }
        if ROW_BRACKETS.contains(&c) {
            breaks.push((at, at + c.len_utf8()));
            k += 1;
            continue;
        }
        if c == ' ' {
            let mut end = k;
            while end < chars.len() && chars[end].1 == ' ' {
                end += 1;
            }
            if end - k >= 2 {
                breaks.push((at, at + (end - k)));
            }
            k = end;
            continue;
        }
        k += 1;
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    for (from, to) in breaks {
        out.push(&row[start..from]);
        start = to;
    }
    out.push(&row[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// The key names a hint may lead an item with. Proper names only.
const KEY_NAMES: &[&str] = &[
    "Esc",
    "Enter",
    "Tab",
    "Backspace",
    "Space",
    "PgUp",
    "PgDn",
    "PageUp",
    "PageDown",
    "Home",
    "End",
    "Ins",
    "Insert",
    "Del",
    "Delete",
    "Bksp",
    "Ctrl",
    "Shift",
    "Alt",
    "↑↓",
    "jk",
    "hl",
    "↵",
    "⌫",
    "↑",
    "↓",
    "←",
    "→",
    "⇧",
];

/// Whether `word` names a key: a proper name, a one-character key, a modifier
/// plus a key, or an alias joined by `/` or a digit range.
fn key_token(word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    if KEY_NAMES.contains(&word) {
        return true;
    }
    for prefix in ["⇧", "^", "Ctrl+", "Shift+", "Alt+"] {
        if let Some(rest) = word.strip_prefix(prefix) {
            if !rest.is_empty() && (KEY_NAMES.contains(&rest) || rest.chars().count() == 1) {
                return true;
            }
        }
    }
    if word.contains('/') {
        return word.split('/').all(|part| {
            !part.is_empty() && (KEY_NAMES.contains(&part) || part.chars().count() == 1)
        });
    }
    if let Some((low, high)) = word.split_once('-') {
        let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        if digits(low) && digits(high) {
            return true;
        }
    }
    let mut chars = word.chars();
    match (chars.next(), chars.next()) {
        (Some(only), None) => only.is_alphanumeric() || "[]↑↓←→⌫↵⇧+-".contains(only),
        _ => false,
    }
}

/// The key an item leads with, if it leads with one.
fn leading_key(segment: &str) -> Option<&str> {
    let word = segment.split_whitespace().next()?;
    let word = word.trim_end_matches(':');
    key_token(word).then_some(word)
}

/// Whether one item is `<key> <verb>` rather than a bare word or a bare key.
fn is_key_verb_item(segment: &str) -> bool {
    leading_key(segment).is_some() && segment.split_whitespace().count() >= 2
}

/// Whether a painted row is an advertisement rather than prose.
///
/// Two **key-led items that each carry a verb** is the shape every hint row has
/// and almost no sentence does. Both halves matter: a help body's
/// `j/k    Move up/down` row has one key-led item, and a split-diff body row's
/// `-  1` has two keys and no verb.
fn is_advertisement_row(row: &str) -> bool {
    let parts = segments(row);
    parts.len() >= 2 && parts.iter().filter(|part| is_key_verb_item(part)).count() >= 2
}

/// Every advertisement row the frame paints, in row order.
fn painted_rows(frame: &Frame) -> Vec<String> {
    (0..frame.height)
        .map(|y| frame.row(y))
        .flat_map(|row| {
            row.split(|c: char| COLUMN_RULES.contains(&c))
                .map(|column| strip_frame_glyphs(column.trim()).to_string())
                .collect::<Vec<_>>()
        })
        .filter(|row| !row.is_empty() && is_advertisement_row(row))
        .collect()
}

/// Every mountable surface that advertises a keymap, with what it renders.
fn rendered_hints() -> Vec<(&'static str, String)> {
    let theme = Arc::new(Theme::dark());
    fixtures::dialogs()
        .into_iter()
        .filter_map(|fixture| {
            (fixture.make)(&theme)
                .keymap_hint()
                .map(|hint| (fixture.surface, hint))
        })
        .collect()
}

/// Every advertisement row every dialog actually paints.
///
/// This is the half the scan was missing: `/tree` declared `j/k move · e expand
/// · f fork · enter select` and painted a pipe-separated row underneath it, and
/// both the milestone and its corrective read green.
fn painted_hints() -> Vec<(&'static str, String)> {
    let theme = Arc::new(Theme::dark());
    // The size the invariant harness measures every other property at, so a
    // row that is clipped here is clipped where the rest of this module looks.
    let area = Rect::new(0, 0, 100, 30);
    fixtures::dialogs_with_content()
        .into_iter()
        .flat_map(|fixture| {
            let mut component = (fixture.make)(&theme);
            let frame = capture::modal(component.as_mut(), area, &theme);
            painted_rows(&frame)
                .into_iter()
                .map(move |row| (fixture.surface, row))
        })
        .collect()
}

/// The side panels' hint rows and the composer's own row, each with whether it
/// is rendered under a budget.
///
/// These are the surfaces a reader spends the most time in, and until now the
/// vocabulary check could not see them: it scanned `fixtures::dialogs()` only,
/// so a panel that hand-formatted `j/k move | enter select` was caught by
/// nothing. The panels render **unbounded** (`HintLine::render(0)`) because
/// their row is given the column's width by the frame, while the composer row
/// takes a real budget — which is why the exit-key rule applies to one and not
/// the other.
fn panel_and_row_hints() -> Vec<(&'static str, String, bool)> {
    let hints = PanelHints {
        focused: true,
        placement: PanelPlacement::Right,
        chat_project: None,
        input_mode: InputMode::Normal,
    };
    let mut rows: Vec<(&'static str, String, bool)> = Vec::new();
    if let Some(hint) = SessionPanel.composer_hints(&hints) {
        rows.push(("PanelSession", hint, false));
    }
    if let Some(hint) = AgentsPanel::default().composer_hints(&hints) {
        rows.push(("PanelAgents", hint, false));
    }
    if let Some(hint) = TerminalPanel::default().composer_hints(&hints) {
        rows.push(("PanelTerminal", hint, false));
    }
    let mut app = App::new_for_testing("/tmp".to_string());
    if let Some(hint) = app.composer_hint_text_bounded(0) {
        rows.push(("ComposerRow", hint, true));
    }
    rows
}

/// The recorded hint of every surface, so "the vocabulary changed" cannot hide
/// "a verb disappeared", and a surface that stops advertising is a failure.
const RECORDED: &[(&str, &str)] = &[
    ("Tree", "j/k move · e expand · f fork · enter select"),
    ("Files", "j/k move · ^d/^u page · l/→ peek · h/← up · enter open · i filter · r refresh · q/⇧Q close"),
    ("Theme", "j/k navigate · enter apply"),
    ("Mcp", "j/k move · enter actions"),
    ("Diff", "j/k scroll · PgUp/PgDn page · s side-by-side · Home/End jump"),
    ("Keybind", "j/k move · enter remap · r reset · e export · i import"),
    ("Plan", "j/k select"),
    ("Review", "j/k move · enter diff"),
    ("ResearchBrowser", "j/k move · enter open · r refresh"),
    ("SecurityReview", "j/k move · PgUp/PgDn page · f filter · n notes · p prompts · enter preview · h jump hunk · ]/[ hunk nav"),
    ("Plugin", "j/k scroll"),
    ("TaskView", "j/k move · ^d/^u page · Home/End end · g/G end · enter open · d detail · r refresh · x cancel · u resume · t trigger · T rotate · X revoke · e trigger refresh"),
    ("Queue", "j/k move · [ ] reorder · a add · e edit · d delete · s send now · r refresh"),
    ("Context", "j/k scroll"),
    ("Cost", "j/k scroll"),
    ("Usage", "j/k scroll"),
    ("Stats", "j/k scroll"),
    ("ShellShow", "j/k scroll"),
    ("Terminal", "j/k scroll"),
    ("TaskList", "j/k scroll"),
    ("WorktreeList", "j/k scroll"),
    ("GoalShow", "j/k scroll"),
    ("MemoryResults", "j/k scroll"),
    ("DoctorReport", "j/k scroll"),
    ("Logs", "j/k scroll"),
    ("Collaborators", "j/k scroll"),
    ("Team", "j/k scroll"),
    ("Control", "j/k scroll"),
];

#[test]
fn every_surface_hint_uses_the_one_vocabulary() {
    let dialogs = rendered_hints();
    let panels = panel_and_row_hints();
    // The floor is the measured count, not a comfortable fraction of it: a
    // floor far below the real surface count lets half the scan disappear and
    // still pass, which is the failure mode this check exists to prevent. Add
    // a surface and this number goes up with it.
    assert!(
        dialogs.len() + panels.len() >= 32,
        "the scan must enumerate the surfaces that advertise keys, found {} \
         ({} dialog(s) + {} panel/row(s))",
        dialogs.len() + panels.len(),
        dialogs.len(),
        panels.len()
    );
    let findings: Vec<String> = dialogs
        .iter()
        .flat_map(|(surface, hint)| {
            let surface: &'static str = surface;
            dialect_findings(hint, true)
                .into_iter()
                .map(move |finding| format!("{surface}: {finding}"))
        })
        .chain(panels.iter().flat_map(|(surface, hint, bounded)| {
            let surface: &'static str = surface;
            dialect_findings(hint, *bounded)
                .into_iter()
                .map(move |finding| format!("{surface}: {finding}"))
        }))
        .collect();
    assert!(
        findings.is_empty(),
        "surfaces outside the hint vocabulary:\n{}",
        findings.join("\n")
    );
}

#[test]
fn every_painted_advertisement_row_uses_the_one_vocabulary() {
    let rows = painted_hints();
    // The floor is the measured number of advertisement rows the fixtures
    // paint. It is a floor rather than an exact count because a surface may
    // legitimately paint more than one row, but it may not stop advertising:
    // a scan that finds nothing is a scan that checks nothing.
    assert!(
        rows.len() >= PAINTED_ROW_FLOOR,
        "the painted scan must find the rows the surfaces advertise, found {} \
         (floor {PAINTED_ROW_FLOOR})",
        rows.len()
    );
    let findings: Vec<String> = rows
        .iter()
        .flat_map(|(surface, row)| {
            let surface: &'static str = surface;
            dialect_findings(row, true)
                .into_iter()
                .map(move |finding| format!("{surface}: {finding}"))
        })
        .collect();
    assert!(
        findings.is_empty(),
        "painted rows outside the hint vocabulary:\n{}",
        findings.join("\n")
    );
}

/// The measured number of advertisement rows the fixture table paints.
///
/// 47 at the content-bearing table — a row count, not a surface count, because
/// `/session` paints five and `/mcp` three. Raise it with a surface that paints a
/// row: a scan that finds fewer rows than the product has stopped looking.
const PAINTED_ROW_FLOOR: usize = 47;

const CONVERTED_FOOTERS: &[(&str, &str)] = &[
    ("Plugin", "Esc close · j/k scroll"),
    ("Keybind", "Esc close · enter remap · r reset"),
];

#[test]
fn every_footer_the_scan_converted_is_actually_painted() {
    let theme = Arc::new(Theme::dark());
    let area = Rect::new(0, 0, 100, 30);
    let fixtures = fixtures::dialogs_with_content();
    let missing: Vec<String> = CONVERTED_FOOTERS
        .iter()
        .filter_map(|(surface, expected)| {
            let fixture = fixtures
                .iter()
                .find(|fixture| fixture.surface == *surface)
                .unwrap_or_else(|| panic!("`{surface}` is not in the fixture table"));
            let mut component = (fixture.make)(&theme);
            let frame = capture::modal(component.as_mut(), area, &theme);
            let rows = painted_rows(&frame);
            (!rows.iter().any(|row| row.contains(expected))).then(|| {
                format!(
                    "{surface}: not painted: {expected:?}; advertisement rows:\n  {}",
                    rows.join("\n  ")
                )
            })
        })
        .collect();
    assert!(
        missing.is_empty(),
        "a converted footer the frame does not contain is a footer that presents \
         nothing:\n{}",
        missing.join("\n")
    );
}

#[test]
fn the_painted_row_extractor_reads_a_footer_and_not_prose() {
    // Non-vacuity: the three fixtures are the shapes the scan must find; the two
    // rows below are a help-table row with one key-led segment and a row of
    // labelled values, and neither is an advertisement.
    for footer in [
        "┌─ Esc close · j/k move ─┐",
        "│↑/↓ navigate  |  Enter select  |  Esc close│",
        "│Enter remap  Esc cancel│",
    ] {
        let row = strip_frame_glyphs(footer.trim_end()).to_string();
        assert!(
            is_advertisement_row(&row),
            "the extractor must read a painted footer: {footer:?} -> {row:?}"
        );
    }
    assert!(
        !is_advertisement_row("j/k          Move up/down"),
        "one key-led segment is a help-table row, not a hint row"
    );
    assert!(
        !is_advertisement_row("Policy: ask · workspace: main"),
        "a row of labelled values advertises no key"
    );
    assert_eq!(
        segments("j/k move · Enter select"),
        vec!["j/k move", "Enter select"],
        "the one separator must split the row it joins"
    );
    assert_eq!(
        segments("a  b | c"),
        vec!["a", "b", "c"],
        "a double space and a pipe must split too, or the drift is invisible"
    );
}

#[test]
fn the_recorded_hint_of_every_surface_is_what_it_renders() {
    let hints = rendered_hints();
    let measured: Vec<(&str, &str)> = hints
        .iter()
        .map(|(surface, hint)| (*surface, hint.as_str()))
        .collect();
    let paste: Vec<String> = measured
        .iter()
        .map(|(surface, hint)| format!("    ({surface:?}, {hint:?}),"))
        .collect();
    assert_eq!(
        measured,
        RECORDED,
        "the recorded hint set and the measurement disagree; the measurement is:\n{}",
        paste.join("\n")
    );
}

#[test]
fn the_vocabulary_check_rejects_both_recorded_dialects() {
    assert!(
        dialect_findings("j/k move · Enter select", true).is_empty(),
        "the target dialect must pass"
    );
    for fixture in [
        "j/k move | enter select",
        "Enter: cancel",
        "j/k Move",
        "Enter close · Esc cancel",
    ] {
        assert!(
            !dialect_findings(fixture, true).is_empty(),
            "{fixture:?} must be reported, and it is not"
        );
    }
}
