//! I4, I5 and I6: measured, and recorded in a manifest that may shrink and
//! may not grow.
//!
//! ## Why these three are recorded rather than asserted
//!
//! The roadmap assigns each to a later milestone (I4 → M006, I5 → M007,
//! I6 → M005). The defects are real, so asserting them would fail this
//! milestone; leaving them unmeasured would let them grow unnoticed. The
//! compromise is the one `docs/comment-budget.toml` already established: a
//! checked-in record of the failures that exist now, enforced in both
//! directions — a new failure is an error, and a recorded failure that no
//! longer reproduces is *also* an error, because an unbanked reduction is the
//! same drift with the sign flipped.
//!
//! ## The three measurements
//!
//! * **I4** — a key the surface advertises must change the rendered frame or
//!   close the surface. Read from the surface's own declaration
//!   (`Component::advertised_hints`, the structured `(key, verb)` items) and,
//!   as a completeness cross-check, from the rows it paints; never from a
//!   hand-written list. A key the rows announce and the declaration does not is
//!   reported as `painted_outside`, and a declared spelling the parser cannot
//!   read is reported rather than skipped.
//! * **I5** — every painted cell's foreground must clear
//!   `MIN_DERIVED_TEXT_RATIO` against the fill under it, on every bundled
//!   theme. The bar is the shipped constant; this module defines no threshold.
//! * **I6** — a hint row that shows any part must still carry its exit key.
//!   Measured through `App::composer_hint_text_bounded` at fixed budgets,
//!   because `fit_parts` is private and this milestone may not widen it.
//!
//! Each measurement has a synthetic negative case that proves it bites:
//! [`the_i4_measurement_detects_an_advertised_key_that_does_nothing`],
//! [`the_i5_measurement_detects_an_unreadable_cell`] and
//! [`the_i6_measurement_detects_a_lost_exit_key`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use codegg::theme::color::Rgb;
use codegg::theme::schema::MIN_DERIVED_TEXT_RATIO;
use codegg::theme::ThemeRegistry;
use codegg::tui::app::state::ChatState;
use codegg::tui::app::{App, TuiMsg};
use codegg::tui::components::component::{Component, DialogType, FocusManager};
use codegg::tui::components::dialogs::info::{InfoDialog, InfoType};
use codegg::tui::components::hint_line::{HintItem, HintLine};
use codegg::tui::input::InputMode;
use codegg::tui::panels::{PanelPlacement, SidePanelId};
use codegg::tui::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde::Deserialize;

use super::capture::{self, Frame, Panels};
use super::fixtures;

/// Where the recorded baseline lives, relative to the package root.
pub const MANIFEST_PATH: &str = "docs/tui-render-invariant-baseline.toml";

/// The budgets I6 measures. `0` is unbounded; the rest are the ones §10 names.
pub const HINT_BUDGETS: &[usize] = &[0, 1, 5, 20, 80];

/// The largest fraction of a theme's painted cells the I5 sweep may skip.
///
/// WP-D: "a theme which is mostly unmeasurable is a finding rather than a
/// pass". The measured fraction is 0.0000 for every bundled theme; the bound
/// admits a stray named-colour cell while a frame set that is largely
/// unevaluated fails the sweep, named.
pub const MAX_UNMEASURABLE_FRACTION: f64 = 0.05;

/// The spelling the hint rows use for their exit verb.
pub const EXIT_KEY: &str = "Esc";

/// The producer names `[hint_truncation]` may use.
///
/// `composer_insert` is the Insert-mode composer row: it lists `type` and
/// `Esc normal`, so the exit key is *last* and truncation from the end can
/// drop it. The Normal-mode row carries no exit key at all, so measuring it
/// would report a vocabulary choice as a truncation defect.
pub const HINT_PRODUCERS: &[&str] = &["composer_insert"];

/// The separators a hint row or footer uses between its parts.
const PART_SEPARATORS: [char; 2] = ['|', '·'];

/// The field separator that turns a recorded failure into one comparable key.
/// Not a printable character, so no surface name can contain it by accident.
const SEP: char = '\u{1f}';

/// The reason a re-banked row is emitted with.
///
/// The measurement can report *that* a key is inert; it cannot know why the
/// key cannot become real. The guard requires every row to carry a non-empty
/// reason, so a pasted row carries this placeholder until a reviewer replaces
/// it with the decision the row actually rests on.
const UNWRITTEN_REASON: &str = "<unwritten: the measurement cannot supply the reason>";

// ── The manifest ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub totals: Totals,
    #[serde(default)]
    pub advertised_keys: Vec<AdvertisedKey>,
    #[serde(default)]
    pub contrast: Vec<ContrastEntry>,
    #[serde(default)]
    pub hint_truncation: Vec<HintTruncation>,
}

#[derive(Debug, Deserialize)]
pub struct Totals {
    pub advertised_keys: usize,
    pub contrast: usize,
    pub hint_truncation: usize,
}

#[derive(Debug, Deserialize)]
pub struct AdvertisedKey {
    pub surface: String,
    pub key: String,
    /// Why this key cannot become real without a decision this milestone does
    /// not take.
    ///
    /// Required, and required to be non-empty: the recorded set is the residue
    /// of a decision, so a row without a justification is an unexplained
    /// exemption rather than a measurement. `scripts/check_tui_render_invariant_baseline.py`
    /// enforces it (rule 1).
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct ContrastEntry {
    pub surface: String,
    pub theme: String,
    pub fg: String,
    pub bg: String,
}

#[derive(Debug, Deserialize)]
pub struct HintTruncation {
    pub producer: String,
    pub budget: usize,
    pub exit_key: String,
}

/// Read the recorded baseline.
pub fn recorded() -> Manifest {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(MANIFEST_PATH);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()))
}

// ── The measurement ─────────────────────────────────────────────────

/// How many painted cells one theme contributed, and how many of them the
/// theme does not determine.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CellCounts {
    /// Every painted cell the sweep read for this theme, skipped ones included.
    pub painted: usize,
    /// Painted cells with a named or indexed `Color`, whose ratio is not
    /// computable.
    pub unmeasurable: usize,
}

impl CellCounts {
    /// The skipped share of the theme's painted cells.
    ///
    /// Zero for a theme with no painted cells, which the sweep's assertion
    /// treats as a defect rather than a pass.
    pub fn unmeasurable_fraction(&self) -> f64 {
        if self.painted == 0 {
            0.0
        } else {
            self.unmeasurable as f64 / self.painted as f64
        }
    }
}

/// Everything the harness measures, as comparable keys, plus the text that
/// would bank it.
#[derive(Debug, Default)]
pub struct Measured {
    pub advertised_keys: BTreeSet<String>,
    pub contrast: BTreeSet<String>,
    pub hint_truncation: BTreeSet<String>,
    /// Painted cells per theme, and the skipped share of them.
    ///
    /// WP-D requires the blind spot to be per theme: a global count cannot say
    /// that one theme's frames were mostly unevaluated while the other
    /// forty-nine were measured.
    pub per_theme_cells: BTreeMap<String, CellCounts>,
}

fn key(fields: &[&str]) -> String {
    fields.join(&SEP.to_string())
}

impl Measured {
    /// The recorded entries, as the same comparable keys.
    pub fn from_manifest(manifest: &Manifest) -> Self {
        Self {
            advertised_keys: manifest
                .advertised_keys
                .iter()
                .map(|entry| key(&[&entry.surface, &entry.key]))
                .collect(),
            contrast: manifest
                .contrast
                .iter()
                .map(|entry| key(&[&entry.surface, &entry.theme, &entry.fg, &entry.bg]))
                .collect(),
            hint_truncation: manifest
                .hint_truncation
                .iter()
                .map(|entry| key(&[&entry.producer, &entry.budget.to_string(), &entry.exit_key]))
                .collect(),
            per_theme_cells: BTreeMap::new(),
        }
    }

    /// The per-theme skip report: one line per measured theme.
    ///
    /// Printed by the I5 test and carried in its failure message, so the
    /// fractions are evidence rather than a number a reader has to recompute.
    pub fn skip_report(&self) -> String {
        let painted: usize = self
            .per_theme_cells
            .values()
            .map(|counts| counts.painted)
            .sum();
        let unmeasurable: usize = self
            .per_theme_cells
            .values()
            .map(|counts| counts.unmeasurable)
            .sum();
        let mut out = format!(
            "I5 per-theme skip report — {} themes, {painted} painted cells, \
             {unmeasurable} unmeasurable\n",
            self.per_theme_cells.len()
        );
        for (theme, counts) in &self.per_theme_cells {
            out.push_str(&format!(
                "  {:<34} {:>7} painted {:>6} unmeasurable ({:.4}%)\n",
                theme,
                counts.painted,
                counts.unmeasurable,
                counts.unmeasurable_fraction() * 100.0
            ));
        }
        out
    }

    /// The measured baseline as the TOML that records it.
    ///
    /// The failure messages print this when the recorded file and the
    /// measurement disagree, so re-banking a section is a copy rather than a
    /// rebuild.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        out.push_str("[totals]\n");
        out.push_str(&format!(
            "advertised_keys = {}\ncontrast = {}\nhint_truncation = {}\n",
            self.advertised_keys.len(),
            self.contrast.len(),
            self.hint_truncation.len()
        ));
        if !self.advertised_keys.is_empty() {
            out.push_str(
                "\n# [[advertised_keys]]: a key a surface advertises and does not act on\n",
            );
            for entry in &self.advertised_keys {
                let mut fields = entry.split(SEP);
                out.push_str(&format!(
                    "\n[[advertised_keys]]\nsurface = {:?}\nkey = {:?}\nreason = {:?}\n",
                    fields.next().unwrap_or_default(),
                    fields.next().unwrap_or_default(),
                    UNWRITTEN_REASON
                ));
            }
        }
        if !self.contrast.is_empty() {
            out.push_str("\n# [[contrast]]: a painted cell below the contrast bar\n");
            for entry in &self.contrast {
                let fields: Vec<&str> = entry.split(SEP).collect();
                out.push_str(&format!(
                    "\n[[contrast]]\nsurface = {:?}\ntheme = {:?}\nfg = {:?}\nbg = {:?}\n",
                    fields[0], fields[1], fields[2], fields[3]
                ));
            }
        }
        if !self.hint_truncation.is_empty() {
            out.push_str("\n# [[hint_truncation]]: a budget that drops the exit key\n");
            for entry in &self.hint_truncation {
                let fields: Vec<&str> = entry.split(SEP).collect();
                out.push_str(&format!(
                    "\n[[hint_truncation]]\nproducer = {:?}\nbudget = {}\nexit_key = {:?}\n",
                    fields[0], fields[1], fields[2]
                ));
            }
        }
        out
    }
}

/// Report the two directions separately: a failure that is not recorded, and a
/// record that no longer fails.
pub fn compare(
    section: &str,
    measured: &BTreeSet<String>,
    recorded: &BTreeSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for entry in measured.difference(recorded) {
        out.push(format!(
            "[{section}] the harness measures {entry:?} and the manifest does not record it"
        ));
    }
    for entry in recorded.difference(measured) {
        out.push(format!(
            "[{section}] the manifest records {entry:?} and it no longer reproduces"
        ));
    }
    out
}

// ── I4: an advertised key must do something ─────────────────────────

fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

/// The key spellings this parser cannot read.
///
/// Named rather than hidden: each is advertised by at least one surface, so
/// each is an advertisement I4 cannot press. Widening the parser moves the
/// recorded set — every newly readable key becomes a pressed key, and the
/// fixtures cannot exercise most of them — so it is a measured limit held by
/// `the_parser_reads_named_keys_and_names_the_ones_it_cannot`, not a widening
/// done here.
pub const UNREADABLE_SPELLINGS: &[&str] = &[
    "Home",
    "End",
    "PageUp",
    "PgUp",
    "PageDown",
    "PgDn",
    "Backspace",
    "⌫",
    "[",
    "]",
];

/// Parse one key spelling out of a hint or footer.
///
/// Returns `None` for anything this parser does not understand, so an
/// unreadable spelling is a limit of the harness rather than an invented
/// finding. [`UNREADABLE_SPELLINGS`] is the measured shape of that limit.
fn key_of(spelling: &str) -> Option<KeyEvent> {
    match spelling {
        "Esc" | "esc" => return Some(press(KeyCode::Esc, KeyModifiers::NONE)),
        "Enter" | "enter" => return Some(press(KeyCode::Enter, KeyModifiers::NONE)),
        "Tab" | "tab" => return Some(press(KeyCode::Tab, KeyModifiers::NONE)),
        "space" | "Space" => return Some(press(KeyCode::Char(' '), KeyModifiers::NONE)),
        "→" => return Some(press(KeyCode::Right, KeyModifiers::NONE)),
        "←" => return Some(press(KeyCode::Left, KeyModifiers::NONE)),
        "↑" => return Some(press(KeyCode::Up, KeyModifiers::NONE)),
        "↓" => return Some(press(KeyCode::Down, KeyModifiers::NONE)),
        _ => {}
    }
    if let Some(body) = spelling.strip_prefix("Shift+") {
        let mut chars = body.chars();
        let (ch, rest) = (chars.next()?, chars.next());
        return rest
            .is_none()
            .then(|| press(KeyCode::Char(ch), KeyModifiers::SHIFT));
    }
    if let Some(body) = spelling.strip_prefix('⇧') {
        let mut chars = body.chars();
        let (ch, rest) = (chars.next()?, chars.next());
        return rest
            .is_none()
            .then(|| press(KeyCode::Char(ch), KeyModifiers::SHIFT));
    }
    if let Some(body) = spelling.strip_prefix('^') {
        let mut chars = body.chars();
        let (ch, rest) = (chars.next()?, chars.next());
        return rest
            .is_none()
            .then(|| press(KeyCode::Char(ch), KeyModifiers::CONTROL));
    }
    let mut chars = spelling.chars();
    let (ch, rest) = (chars.next()?, chars.next());
    if rest.is_some() {
        return None;
    }
    ch.is_ascii_alphanumeric()
        .then(|| press(KeyCode::Char(ch), KeyModifiers::NONE))
}

/// The key spellings one advertised part names.
///
/// `j/k scroll` is two keys and one verb; `l/→ peek` is two spellings of one
/// key. Only the part's first token is a key, so a verb containing a letter
/// cannot be read as one — `Esc cancel` truncated at the frame's right edge is
/// read as `Esc`, never as `Esc` and `c`.
fn spellings(part: &str) -> Vec<&str> {
    let token = part.split_whitespace().next().unwrap_or_default();
    token.split('/').filter(|s| key_of(s).is_some()).collect()
}

/// The glyphs a box or a scrollbar paints beside a row's text.
///
/// Box drawing (`U+2500..=U+257F`) and block elements (`U+2580..=U+259F`) cover
/// every edge, rule, caret and scrollbar thumb the render paths use. No key
/// spelling starts or ends with one, so trimming them from a part's ends
/// cannot lose a key — it can only stop an edge from eating one.
fn is_decoration(ch: char) -> bool {
    ch.is_whitespace() || ('\u{2500}'..='\u{259f}').contains(&ch)
}

/// The advertised `(key, verb)` parts of one hint or footer line.
///
/// Each part is trimmed of the decoration a box or a scrollbar paints around
/// it before it is tokenised. A bordered box glues its edge to the first part
/// (`│↑/↓ navigate`), which tokenises as `│↑` — so the up arrow parsed as
/// nothing and `↑` was silently absent from every numbered surface's announced
/// set while `↓` survived. That is the asymmetry the M006 record could not
/// derive from the source.
fn advertised_parts(line: &str) -> Vec<&str> {
    line.split(PART_SEPARATORS)
        .map(|part| part.trim_matches(is_decoration))
        .filter(|part| !part.is_empty() && part.split_whitespace().count() >= 2)
        .collect()
}

/// The advertised parts of one **rendered** row.
///
/// [`advertised_parts`] plus the rule that a rendered row must carry a part
/// separator to count at all: a list row is content, and content must not pose
/// as an advertisement. `keymap_hint()` needs no such rule — it is an
/// advertisement by construction — so the two readers stay separate rather
/// than one reader with a flag. Without this rule a seeded review list row
/// (`M  <path>  +1 -0`) is read as `M` advertising a verb.
fn footer_parts(line: &str) -> Vec<&str> {
    if !line.contains('|') && !line.contains('·') {
        return Vec::new();
    }
    advertised_parts(line)
}

/// What pressing one advertised key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressOutcome {
    /// The surface returned a [`TuiMsg`], so it claimed the key.
    Claimed,
    /// The press repainted the surface.
    Repainted,
    /// The press returned nothing and changed nothing.
    Inert,
}

impl PressOutcome {
    /// Whether the press is a delivered verb rather than an advertisement
    /// with nothing behind it.
    pub fn handled(self) -> bool {
        self != Self::Inert
    }
}

/// Where a pressed spelling came from.
///
/// A declared key is pressed from the surface's own declaration; a painted key
/// the declaration does not name is pressed from the row and reported, because
/// advertising outside one's own declaration is a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// The surface declared this key in [`Component::advertised_hints`].
    Declared,
    /// The surface paints this key and does not declare it.
    Painted,
}

/// One advertised spelling, and the verdict pressing it produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvertisedPress {
    pub surface: &'static str,
    pub spelling: String,
    pub source: KeySource,
    pub outcome: PressOutcome,
}

/// A declared spelling the parser cannot turn into a key.
///
/// Reported rather than skipped: a hole nobody names is the failure mode the
/// structured read exists to remove.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnreadableAdvertisement {
    pub surface: &'static str,
    pub spelling: String,
}

/// Every press the I4 measurement made, in surface then spelling order.
///
/// This is the per-key report: a reviewer reads it with `--nocapture` and can
/// derive the recorded rows without changing code.
#[derive(Debug, Default)]
pub struct AdvertisedReport {
    pub presses: Vec<AdvertisedPress>,
    /// Declared spellings no key spelling can be read from.
    pub unreadable: Vec<UnreadableAdvertisement>,
    /// The cross-check: keys each surface paints and does not declare.
    pub painted_outside: BTreeSet<String>,
}

impl AdvertisedReport {
    /// The presses that delivered nothing, as the manifest keys them.
    pub fn inert_rows(&self) -> BTreeSet<String> {
        self.presses
            .iter()
            .filter(|press| !press.outcome.handled())
            .map(|press| key(&[press.surface, &press.spelling]))
            .collect()
    }

    /// The table a reviewer reads: one line per press, with its source.
    pub fn table(&self) -> String {
        let mut out = String::new();
        for press in &self.presses {
            let verdict = match press.outcome {
                PressOutcome::Claimed => "claimed",
                PressOutcome::Repainted => "repainted",
                PressOutcome::Inert => "INERT",
            };
            let source = match press.source {
                KeySource::Declared => "declared",
                KeySource::Painted => "painted",
            };
            out.push_str(&format!(
                "  {:<20} {:<8} {source:<9} {verdict}\n",
                press.surface, press.spelling
            ));
        }
        for row in &self.unreadable {
            out.push_str(&format!(
                "  {:<20} {:<8} UNREADABLE\n",
                row.surface, row.spelling
            ));
        }
        out
    }
}

/// Whether pressing `key` changes the frame or is claimed by the surface.
///
/// The frame is read through [`FocusManager`], which is the component that
/// receives a modal key in production: it answers `Tab` for its focus ring
/// before the dialog sees it, so probing the bare component would report
/// every advertised `Tab` as unhandled.
///
/// A returned [`TuiMsg`] counts as handled. `Option<TuiMsg>` is ambiguous
/// only in the `None` direction — `None` means both "handled, nothing to
/// report" and "not handled" — so the measurement treats `Some` as a claim
/// the frame cannot contradict (a scroll request the app applies afterwards
/// does not repaint the dialog) and requires a repaint only for `None`.
///
/// A surface that closes is **not** painted again: production unmounts it in
/// the same pass that reads `CloseDialog`, and some components leave state a
/// re-render cannot read. `QuestionDialog` is one — its `Esc` clears the
/// answer list and returns `CloseDialog`, so rendering it afterwards indexes
/// an empty `answers` (a panic no user can reach, because the dialog is gone).
pub fn advertised_key_is_handled(
    focus: &mut FocusManager,
    key: KeyEvent,
    area: Rect,
    theme: &Arc<Theme>,
) -> bool {
    press_outcome(focus, key, area, theme).handled()
}

/// [`advertised_key_is_handled`], with the reason rather than the verdict.
pub fn press_outcome(
    focus: &mut FocusManager,
    key: KeyEvent,
    area: Rect,
    theme: &Arc<Theme>,
) -> PressOutcome {
    let before = capture::managed(focus, area, theme);
    let message = focus.handle_key(key);
    if message.is_some() {
        return PressOutcome::Claimed;
    }
    let after = capture::managed(focus, area, theme);
    if before.differing(&after).is_empty() {
        PressOutcome::Inert
    } else {
        PressOutcome::Repainted
    }
}

/// Whether `key` is an up-direction movement, which cannot act at the first
/// row of anything.
fn is_up_direction(key: KeyEvent) -> bool {
    key.modifiers.is_empty() && matches!(key.code, KeyCode::Up | KeyCode::Char('k'))
}

/// One fresh mount of a surface, in the state the key under measurement is
/// measured on.
///
/// The state has two declared parts, and this is the only place either is
/// applied: the fixture's **content** (from the I4 table) and, for an
/// up-direction on a surface [`fixtures::PARKED_SURFACES`] names, the
/// **position prelude**. The prelude presses `↓` [`fixtures::PARKED_ROW`] time,
/// so an advertisement for `↑` or `k` is not measured against the first row,
/// where it cannot act. It is a function of the key alone, never of the order
/// keys are pressed in, so no verdict depends on another press.
fn mount(fixture: &fixtures::DialogFixture, theme: &Arc<Theme>, park: bool) -> FocusManager {
    let mut focus = FocusManager::new();
    assert!(
        focus.push((fixture.make)(theme)),
        "one fresh fixture per mount cannot collide on a dialog type"
    );
    if park && fixtures::parks_position(fixture.surface) {
        for _ in 0..fixtures::PARKED_ROW {
            let _ = focus.handle_key(press(KeyCode::Down, KeyModifiers::NONE));
        }
    }
    focus
}

/// The keys one **declared** item names.
///
/// A declared key is written as a painted part's key list is — `j/k` is two
/// keys, `[ ]` is two — so the declaration splits as the row does. Unlike
/// [`spellings`] it does not drop an unreadable spelling: it is reported.
fn item_spellings(item: &HintItem) -> Vec<&str> {
    item.key()
        .split_whitespace()
        .flat_map(|token| token.split('/'))
        .filter(|spelling| !spelling.is_empty())
        .collect()
}

/// What one surface announces, split by where the announcement came from.
#[derive(Debug, Default)]
struct Announcements {
    /// Keys from the surface's own declaration, in the order it declares them.
    declared: Vec<String>,
    /// Keys from the rendered rows, which the declaration may or may not name.
    painted: Vec<String>,
    /// Declared spellings the parser cannot read.
    unreadable: Vec<String>,
}

impl Announcements {
    /// The cross-check: the keys this surface paints and does not declare.
    ///
    /// The row is not a second source of truth, it is the completeness check on
    /// the declaration, and a key here is a finding whatever it does.
    fn painted_outside(&self) -> Vec<&String> {
        self.painted
            .iter()
            .filter(|spelling| !self.declared.contains(spelling))
            .collect()
    }
}

/// The announcements of one surface, computed once from one mount.
///
/// Two readers, not interchangeable. The **declaration** is the surface's own
/// [`Component::advertised_hints`]: where I4 takes its subject. The **rows** are
/// the pixels and the completeness cross-check: a surface that paints an
/// advertisement it did not declare is caught here, and a surface that declares
/// and does not paint is still measured.
fn announcements(
    fixture: &fixtures::DialogFixture,
    theme: &Arc<Theme>,
    area: Rect,
) -> Announcements {
    let mut focus = mount(fixture, theme, false);
    let rendered = capture::managed(&mut focus, area, theme);
    let lines = focus
        .top()
        .map(Component::advertised_hints)
        .unwrap_or_default();
    announcements_of(&lines, &rendered)
}

/// [`announcements`], from a declaration and a frame already in hand.
fn announcements_of(lines: &[HintLine], rendered: &Frame) -> Announcements {
    let mut declared: Vec<String> = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();
    for line in lines {
        for item in line.items() {
            for spelling in item_spellings(item) {
                if key_of(spelling).is_some() {
                    declared.push(spelling.to_string());
                } else {
                    unreadable.push(spelling.to_string());
                }
            }
        }
    }
    let mut painted: Vec<String> = Vec::new();
    for row in rendered.rows() {
        for part in footer_parts(&row) {
            painted.extend(spellings(part).into_iter().map(str::to_string));
        }
    }
    for list in [&mut declared, &mut painted, &mut unreadable] {
        list.sort();
        list.dedup();
    }
    Announcements {
        declared,
        painted,
        unreadable,
    }
}

fn measure_advertised_keys(report: &mut AdvertisedReport, out: &mut BTreeSet<String>) {
    let theme = Arc::new(Theme::dark());
    let area = Rect::new(0, 0, 100, 30);
    for fixture in fixtures::dialogs_with_content() {
        let announcements = announcements(&fixture, &theme, area);
        report
            .unreadable
            .extend(
                announcements
                    .unreadable
                    .iter()
                    .map(|spelling| UnreadableAdvertisement {
                        surface: fixture.surface,
                        spelling: spelling.clone(),
                    }),
            );
        report.painted_outside.extend(
            announcements
                .painted_outside()
                .into_iter()
                .map(|spelling| key(&[fixture.surface, spelling])),
        );
        // The declaration is the subject, so a declared key is pressed from the
        // declaration. A painted key the declaration does not name is pressed
        // too — the row is what a user presses — and it is reported, because a
        // surface that advertises outside its own declaration is a finding.
        //
        // The order is the declaration's, then the remaining painted keys. A
        // spelling appears once, and the credited source is the declaration's.
        let mut pressed: BTreeSet<&str> = BTreeSet::new();
        let mut queue: Vec<(&str, KeySource)> = Vec::new();
        for spelling in &announcements.declared {
            if pressed.insert(spelling) {
                queue.push((spelling, KeySource::Declared));
            }
        }
        for spelling in &announcements.painted {
            if pressed.insert(spelling) {
                queue.push((spelling, KeySource::Painted));
            }
        }
        for (spelling, source) in queue {
            let event = key_of(spelling).expect("the press queue holds only readable spellings");
            // One fresh mount per key. Pressing the whole set against one mount
            // lets an earlier press change the state a later key is measured
            // on, which is how a correct `j` came to be recorded as ignored:
            // `i` had already switched the surface into a mode whose body does
            // not paint the cursor.
            let mut focus = mount(&fixture, &theme, is_up_direction(event));
            let outcome = press_outcome(&mut focus, event, area, &theme);
            report.presses.push(AdvertisedPress {
                surface: fixture.surface,
                spelling: spelling.to_string(),
                source,
                outcome,
            });
            if !outcome.handled() {
                out.insert(key(&[fixture.surface, spelling]));
            }
        }
    }
}

// ── I5: every painted foreground clears the bar ─────────────────────

fn as_rgb(color: Color) -> Option<Rgb> {
    match color {
        Color::Rgb(r, g, b) => Some(Rgb::new(r, g, b)),
        _ => None,
    }
}

fn resolve_on(color: Color, fallback: Color) -> Option<Rgb> {
    match color {
        Color::Reset => as_rgb(fallback),
        other => as_rgb(other),
    }
}

/// Every colour pair in `frame` that fails `MIN_DERIVED_TEXT_RATIO` under
/// `theme`, plus the cells the theme does not determine counted into `counts`.
pub fn contrast_failures(
    frame: &Frame,
    theme: &Theme,
    out: &mut BTreeSet<(String, String)>,
    counts: &mut CellCounts,
) {
    for cell in frame.painted() {
        counts.painted += 1;
        // A reversed cell paints its background as foreground, so the pair the
        // reader sees is swapped. Measuring the unswapped pair would report a
        // failure nobody can see, or miss one they can.
        let (fg, bg) = if cell.modifiers.contains(Modifier::REVERSED) {
            (cell.bg, cell.fg)
        } else {
            (cell.fg, cell.bg)
        };
        let (Some(fg), Some(bg)) = (
            resolve_on(fg, theme.foreground),
            resolve_on(bg, theme.background),
        ) else {
            counts.unmeasurable += 1;
            continue;
        };
        if fg.contrast_ratio(bg) < MIN_DERIVED_TEXT_RATIO {
            out.insert((fg.to_hex(), bg.to_hex()));
        }
    }
}

/// The size every dialog is measured at, the column width every panel is
/// measured at, and the whole-frame size.
const MEASURE_DIALOG: (u16, u16) = (80, 24);
const MEASURE_COLUMN: (u16, u16) = (40, 16);
const MEASURE_APP: (u16, u16) = (100, 32);

fn collect_pairs(frame: &Frame, theme: &Theme, surface: &str, measured: &mut Measured) {
    let mut pairs = BTreeSet::new();
    let counts = measured
        .per_theme_cells
        .entry(theme.id.clone())
        .or_default();
    contrast_failures(frame, theme, &mut pairs, counts);
    for (fg, bg) in pairs {
        measured
            .contrast
            .insert(key(&[surface, &theme.id, &fg, &bg]));
    }
}

fn measure_contrast(measured: &mut Measured) {
    let themes = ThemeRegistry::load_builtins().all_tui_themes();
    assert!(
        themes.len() >= 50,
        "the contrast sweep is only meaningful over the bundled set; found {}",
        themes.len()
    );
    let (dialog_width, dialog_height) = MEASURE_DIALOG;
    let (column_width, column_height) = MEASURE_COLUMN;
    let (app_width, app_height) = MEASURE_APP;
    for theme in themes {
        let arc = Arc::new(theme.clone());
        for fixture in fixtures::dialogs() {
            let mut component = (fixture.make)(&arc);
            let frame = capture::modal(
                component.as_mut(),
                Rect::new(0, 0, dialog_width, dialog_height),
                &arc,
            );
            collect_pairs(&frame, &theme, fixture.surface, measured);
        }
        // Every panel, at the shipped column width. Bespoke panels build
        // their own content; a hosted one gets a box so it paints its body
        // rather than the placeholder.
        let mut panels = Panels::new(Arc::clone(&arc));
        for panel in SidePanelId::ALL {
            panels.host.dock(*panel, PanelPlacement::Right);
            panels.host.set_visible_at(PanelPlacement::Right, true);
            if !panel.is_bespoke() {
                let widget = fixtures::hosted_box(&arc, *panel);
                assert!(panels.host.adopt(*panel, widget), "{panel:?} is hosted");
            }
            let frame = panels.capture(
                PanelPlacement::Right,
                Rect::new(0, 0, column_width, column_height),
                false,
            );
            collect_pairs(&frame, &theme, &format!("panel:{panel:?}"), measured);
        }
        // The whole frame, which is the only surface carrying the composer,
        // the transcript and the sidebar chrome at once.
        let mut app = App::new_for_testing("/tmp".to_string());
        // Paint the frame with the theme under measurement, not with the legacy
        // palette `new_for_testing` pins. `collect_pairs` resolves a `Reset`
        // cell against the theme passed to it, so a dark-painted frame read
        // under another theme produced pairs no single palette can paint:
        // measured before this line, **0 of 94** recorded `app:100x32` rows had
        // both colours from the labelled theme. The honest frame is also the
        // one that shows real failures — it paints `theme.border` on the
        // sidebar and status-bar frames, which is why the `border` family is
        // larger here than a pinned-dark frame made it look.
        app.ui_state.theme = Arc::clone(&arc);
        let frame = capture::app_frame(&mut app, app_width, app_height);
        collect_pairs(&frame, &theme, "app:100x32", measured);
    }
}

// ── I6: a hint row never loses its exit key ─────────────────────────

/// The loss I6 forbids: a non-empty row that does not carry the exit key.
/// An empty row is the documented `fit_parts` rule, not a loss.
fn hint_loses_exit_key(row: &str) -> bool {
    !row.is_empty() && !row.contains(EXIT_KEY)
}

fn measure_hint_truncation(app: &mut App, out: &mut BTreeSet<String>) {
    app.ui_state.input_mode = InputMode::Insert;
    for budget in HINT_BUDGETS {
        let row = app.composer_hint_text_bounded(*budget).unwrap_or_default();
        if hint_loses_exit_key(&row) {
            out.insert(key(&["composer_insert", &budget.to_string(), EXIT_KEY]));
        }
    }
}

/// Measure all three recorded invariants.
pub fn measure() -> Measured {
    let mut measured = Measured::default();
    let mut report = AdvertisedReport::default();
    measure_advertised_keys(&mut report, &mut measured.advertised_keys);
    measure_contrast(&mut measured);
    let mut app = App::new_for_testing("/tmp".to_string());
    measure_hint_truncation(&mut app, &mut measured.hint_truncation);
    measured
}

fn assert_section(
    section: &str,
    live: &BTreeSet<String>,
    recorded: &BTreeSet<String>,
    measured: &Measured,
) {
    let mismatches = compare(section, live, recorded);
    assert!(
        mismatches.is_empty(),
        "the recorded {section} baseline and the measurement disagree:\n{}\n\n\
         the measured baseline, ready to bank into {MANIFEST_PATH}:\n{}",
        mismatches.join("\n"),
        measured.to_toml()
    );
}

// ── The three recorded-invariant tests ──────────────────────────────

#[test]
fn the_recorded_i4_baseline_is_exactly_what_the_harness_measures() {
    let manifest = recorded();
    assert_eq!(
        manifest.totals.advertised_keys,
        manifest.advertised_keys.len(),
        "the recorded total does not match the recorded entries"
    );
    let recorded_entries = Measured::from_manifest(&manifest).advertised_keys;
    let mut measured = Measured::default();
    let mut report = AdvertisedReport::default();
    measure_advertised_keys(&mut report, &mut measured.advertised_keys);
    assert_section(
        "advertised_keys",
        &measured.advertised_keys,
        &recorded_entries,
        &measured,
    );
}

#[test]
fn the_recorded_i5_baseline_is_exactly_what_the_harness_measures() {
    let manifest = recorded();
    assert_eq!(
        manifest.totals.contrast,
        manifest.contrast.len(),
        "the recorded total does not match the recorded entries"
    );
    let recorded_entries = Measured::from_manifest(&manifest).contrast;
    let mut measured = Measured::default();
    measure_contrast(&mut measured);
    assert_section("contrast", &measured.contrast, &recorded_entries, &measured);

    // WP-D: the blind spot is per theme, reported, and bounded, so one
    // theme's frames cannot go unmeasured behind a small global number.
    let report = measured.skip_report();
    println!("{report}");
    let themes = ThemeRegistry::load_builtins().all_tui_themes().len();
    assert_eq!(
        measured.per_theme_cells.len(),
        themes,
        "the skip report must cover every measured theme, otherwise a theme can be \
         absent from it and never reported\n{report}"
    );
    let mut offenders = Vec::new();
    for (theme, counts) in &measured.per_theme_cells {
        assert!(
            counts.painted > 0,
            "`{theme}` contributed no painted cells, so its skip fraction is undefined"
        );
        if counts.unmeasurable_fraction() > MAX_UNMEASURABLE_FRACTION {
            offenders.push(format!(
                "  {theme}: {}/{} = {:.2}% unmeasurable",
                counts.unmeasurable,
                counts.painted,
                counts.unmeasurable_fraction() * 100.0
            ));
        }
    }
    assert!(
        offenders.is_empty(),
        "the {:.0}% per-theme skip bound is exceeded, so the sweep says little about \
         these themes rather than passing them:\n{}\n\n{report}",
        MAX_UNMEASURABLE_FRACTION * 100.0,
        offenders.join("\n")
    );
}

#[test]
fn the_recorded_i6_baseline_is_exactly_what_the_harness_measures() {
    let manifest = recorded();
    assert_eq!(
        manifest.totals.hint_truncation,
        manifest.hint_truncation.len(),
        "the recorded total does not match the recorded entries"
    );
    let recorded_entries = Measured::from_manifest(&manifest).hint_truncation;
    let mut app = App::new_for_testing("/tmp".to_string());
    let mut measured = Measured::default();
    measure_hint_truncation(&mut app, &mut measured.hint_truncation);
    assert_section(
        "hint_truncation",
        &measured.hint_truncation,
        &recorded_entries,
        &measured,
    );
}

// ── The per-key report ──────────────────────────────────────────────

/// The measurement as a reading, not a count.
///
/// `cargo test --test tui_render -- the_i4_per_key_report --nocapture` prints
/// one line per advertised key: the surface, the spelling and the verdict the
/// press produced. The recorded rows are the `INERT` lines, so a re-bank is a
/// reading rather than a guess.
///
/// The assertions here are about the report itself rather than about the
/// recorded set — the recorded set is the next test's property. What this test
/// holds is that the report is complete enough to read: it names a real
/// fixture surface for every press, and its inert lines are the measurement's
/// inert set in both directions.
#[test]
fn the_i4_per_key_report_names_every_announced_key_and_its_verdict() {
    let mut report = AdvertisedReport::default();
    let mut measured = BTreeSet::new();
    measure_advertised_keys(&mut report, &mut measured);

    println!("I4 per-key report — {} presses:", report.presses.len());
    print!("{}", report.table());

    assert!(
        !report.presses.is_empty(),
        "a report with no presses cannot describe the measurement"
    );
    let surfaces = fixtures::dialogs_with_content();
    for press in &report.presses {
        assert!(
            surfaces
                .iter()
                .any(|fixture| fixture.surface == press.surface),
            "`{}` is not a surface the I4 fixture table builds",
            press.surface
        );
        assert!(
            !press.spelling.is_empty(),
            "a press on `{}` names no key",
            press.surface
        );
    }
    assert_eq!(
        report.inert_rows(),
        measured,
        "the report's inert lines and the measurement's recorded set disagree"
    );
}

// ── Non-vacuity: each measurement detects its own defect ────────────

/// A component that advertises a key it does not act on, and closes on none.
struct AdvertisesWhatItIgnores;

impl Component for AdvertisesWhatItIgnores {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn keymap_hint_line(&self) -> Option<HintLine> {
        Some(HintLine::from_items(&[("z", "nothing")]))
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, _theme: &Arc<Theme>) {
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .title(" fixture ");
        frame.render_widget(block, area);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Confirm
    }
}

/// A surface that declares one pair and paints a different one.
///
/// The cross-check's fixture: the declaration is what I4 measures, and the
/// painted row is what a user reads. A pair in the second that the first does
/// not name is the drift the two-reader design exists to catch.
struct PaintsOutsideItsDeclaration;

impl Component for PaintsOutsideItsDeclaration {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn keymap_hint_line(&self) -> Option<HintLine> {
        Some(HintLine::from_items(&[("j/k", "move")]))
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, _theme: &Arc<Theme>) {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(Line::from("Esc close | z zap")),
            area,
        );
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Confirm
    }
}

/// A surface that declares nothing and advertises only in the row it paints.
///
/// The ten baseline surfaces whose advertisement lives in a footer are this
/// shape. They must stay measurable: the row is still read, so a surface that
/// never declares is incomplete rather than invisible.
struct AdvertisesOnlyInItsFooter;

impl Component for AdvertisesOnlyInItsFooter {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, _theme: &Arc<Theme>) {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(Line::from("Esc close | q quit")),
            area,
        );
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Confirm
    }
}

/// A surface whose declaration names a key the parser cannot read.
struct DeclaresUnreadableKey;

impl Component for DeclaresUnreadableKey {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn keymap_hint_line(&self) -> Option<HintLine> {
        Some(HintLine::from_items(&[("F5", "away")]))
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, _theme: &Arc<Theme>) {
        frame.render_widget(
            ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::ALL),
            area,
        );
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Confirm
    }
}

/// The announcements of a fixture component with no dialog table entry.
fn component_announcements(
    component: Box<dyn Component>,
    area: Rect,
) -> (Announcements, Vec<HintLine>) {
    let theme = Arc::new(Theme::dark());
    let mut focus = FocusManager::new();
    assert!(focus.push(component), "one fresh fixture per mount");
    let rendered = capture::managed(&mut focus, area, &theme);
    let lines = focus
        .top()
        .map(Component::advertised_hints)
        .unwrap_or_default();
    (announcements_of(&lines, &rendered), lines)
}

/// A component that really does close on `Esc`, so the measurement's positive
/// branch is exercised rather than assumed.
struct ClosesOnEsc;

impl Component for ClosesOnEsc {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        matches!(key.code, KeyCode::Esc).then_some(TuiMsg::CloseDialog)
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, _theme: &Arc<Theme>) {
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .title(" fixture ");
        frame.render_widget(block, area);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Confirm
    }
}

#[test]
fn the_i4_measurement_detects_an_advertised_key_that_does_nothing() {
    let theme = Arc::new(Theme::dark());
    let area = Rect::new(0, 0, 40, 12);
    let mut ignored = FocusManager::new();
    assert!(ignored.push(Box::new(AdvertisesWhatItIgnores)));
    let hint = ignored
        .top()
        .and_then(Component::keymap_hint)
        .expect("the fixture advertises a key");
    let spelling = spellings(advertised_parts(&hint)[0])[0];
    let event = key_of(spelling).expect("the advertised spelling must parse");
    assert!(
        !advertised_key_is_handled(&mut ignored, event, area, &theme),
        "the fixture does nothing for `{spelling}`, so the measurement must report it"
    );

    let mut closing = FocusManager::new();
    assert!(closing.push(Box::new(ClosesOnEsc)));
    assert!(
        advertised_key_is_handled(&mut closing, key_of("Esc").unwrap(), area, &theme),
        "a surface that closes on the advertised key must read as handled"
    );
}

#[test]
fn the_i5_measurement_detects_an_unreadable_cell() {
    // §10's negative case: a theme whose foreground is its own background, and
    // the shipped theme beside it. A measurement that reported nothing for the
    // first would report nothing anywhere.
    let mut unreadable = Theme::dark();
    unreadable.foreground = unreadable.background;
    let area = Rect::new(0, 0, 40, 12);
    let broken = Arc::new(unreadable.clone());
    let mut broken_frame =
        InfoDialog::new(Arc::clone(&broken), InfoType::Logs, vec!["x".to_string()]);
    let broken_frame = capture::modal(&mut broken_frame, area, &broken);
    let mut failing = BTreeSet::new();
    let mut broken_counts = CellCounts::default();
    contrast_failures(&broken_frame, &unreadable, &mut failing, &mut broken_counts);
    assert!(
        !failing.is_empty(),
        "a foreground identical to its fill must fail the bar, and the measurement \
         found nothing"
    );
    assert_eq!(
        broken_counts.unmeasurable, 0,
        "a broken theme's own colours are RGB, so none of its cells may be skipped"
    );

    let shipped = Theme::dark();
    let shipped_arc = Arc::new(shipped.clone());
    let mut shipped_frame = InfoDialog::new(
        Arc::clone(&shipped_arc),
        InfoType::Logs,
        vec!["x".to_string()],
    );
    let shipped_frame = capture::modal(&mut shipped_frame, area, &shipped_arc);
    let mut passing = BTreeSet::new();
    let mut shipped_counts = CellCounts::default();
    contrast_failures(&shipped_frame, &shipped, &mut passing, &mut shipped_counts);
    assert!(
        passing.len() < failing.len(),
        "the shipped dark theme must fail fewer colour pairs than the deliberately \
         unreadable one ({} against {})",
        passing.len(),
        failing.len()
    );
    assert!(
        shipped_counts.painted > 0,
        "the measurement read no painted cell, so its silence means nothing"
    );
}

/// A cell whose foreground the theme does not determine is counted, not read as
/// passing: the skip rule is the one WP-D has to make visible.
struct PaintsColour {
    fg: Color,
}

impl Component for PaintsColour {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<TuiMsg> {
        None
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, _theme: &Arc<Theme>) {
        let line = Line::from(Span::styled(
            "measured",
            Style::default()
                .fg(self.fg)
                .bg(Color::Rgb(0x10, 0x20, 0x30)),
        ));
        frame.render_widget(ratatui::widgets::Paragraph::new(line), area);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Confirm
    }
}

#[test]
fn the_i5_skip_report_counts_a_colour_the_theme_does_not_determine() {
    let theme = Arc::new(Theme::dark());
    let area = Rect::new(0, 0, 40, 4);

    let mut named = PaintsColour { fg: Color::Cyan };
    let frame = capture::modal(&mut named, area, &theme);
    let mut failing = BTreeSet::new();
    let mut counts = CellCounts::default();
    contrast_failures(&frame, &theme, &mut failing, &mut counts);
    assert_eq!(
        counts.unmeasurable, counts.painted,
        "every painted cell of this fixture has a named foreground, so every one must \
         be skipped"
    );
    assert_eq!(counts.painted, "measured".len(), "one cell per glyph");
    assert!(
        failing.is_empty(),
        "a cell whose ratio cannot be computed is skipped, never recorded as failing"
    );
    assert!(
        counts.unmeasurable_fraction() > MAX_UNMEASURABLE_FRACTION,
        "the fixture must read as a theme the sweep cannot evaluate, got {:.2}%",
        counts.unmeasurable_fraction() * 100.0
    );

    // `Reset` is resolved against the theme, so the same fixture with the
    // terminal default is measured rather than skipped.
    let mut reset = PaintsColour { fg: Color::Reset };
    let frame = capture::modal(&mut reset, area, &theme);
    let mut counts = CellCounts::default();
    contrast_failures(&frame, &theme, &mut failing, &mut counts);
    assert_eq!(counts.painted, "measured".len());
    assert_eq!(
        counts.unmeasurable, 0,
        "`Reset` is resolved against the theme's foreground, so it is measurable"
    );
}

#[test]
fn the_i6_measurement_detects_a_lost_exit_key() {
    // The detector I6 relies on. A row that shows a part but not the exit key is
    // the loss the invariant forbids; an empty row is the documented `fit_parts`
    // rule (a budget too small for the exit item alone), not a loss.
    assert!(
        hint_loses_exit_key("type"),
        "a row showing a verb without the exit key must count as a loss"
    );
    assert!(
        !hint_loses_exit_key(""),
        "an empty row is the documented rule, not a loss"
    );
    assert!(
        !hint_loses_exit_key(&format!("{EXIT_KEY} normal · type")),
        "a row carrying the exit key is not a loss"
    );

    // And the real producer keeps its exit key at every measured budget, which
    // is why the manifest banks no entry here.
    let mut app = App::new_for_testing("/tmp".to_string());
    app.ui_state.input_mode = InputMode::Insert;
    let mut lost = BTreeSet::new();
    measure_hint_truncation(&mut app, &mut lost);
    assert!(
        lost.is_empty(),
        "the composer row must keep its exit key at every measured budget, got {lost:?}"
    );

    let unbounded = app.composer_hint_text_bounded(0).unwrap_or_default();
    assert!(
        unbounded.contains(EXIT_KEY),
        "an unbounded row must still carry the exit key, got {unbounded:?}"
    );
}

/// The chat state a panel measurement needs is only here so the module can
/// name it; `ChatState::new()` is the unbound state, which is what a fresh
/// app holds.
#[allow(dead_code)]
fn unbound_chat() -> ChatState {
    ChatState::new()
}

#[test]
fn the_i4_measurement_takes_every_declared_key_from_the_declaration() {
    // The milestone's property: a declared key reaches the press list through
    // `advertised_hints()`, not through a parsed rendering. The floor is the
    // measured 28 declaring surfaces.
    let theme = Arc::new(Theme::dark());
    let area = Rect::new(0, 0, 100, 30);
    let mut report = AdvertisedReport::default();
    let mut measured = BTreeSet::new();
    measure_advertised_keys(&mut report, &mut measured);

    let mut declaring = 0usize;
    for fixture in fixtures::dialogs_with_content() {
        let announcements = announcements(&fixture, &theme, area);
        if announcements.declared.is_empty() {
            continue;
        }
        declaring += 1;
        for spelling in &announcements.declared {
            assert!(
                report
                    .presses
                    .iter()
                    .any(|press| press.surface == fixture.surface
                        && &press.spelling == spelling
                        && press.source == KeySource::Declared),
                "`{}` declares `{spelling}` and the report does not credit it to the declaration",
                fixture.surface
            );
        }
    }
    assert_eq!(
        declaring, 28,
        "the declaration path must cover every surface that has one; the measured \n         count is 28"
    );
}

#[test]
fn the_i4_cross_check_names_a_pair_painted_outside_the_declaration() {
    // The negative case: a surface declares `j/k move` and paints `Esc close |
    // z zap`. The keys it never declared must be named.
    let (announcements, _) = component_announcements(
        Box::new(PaintsOutsideItsDeclaration),
        Rect::new(0, 0, 40, 6),
    );
    assert_eq!(
        announcements.declared,
        vec!["j", "k"],
        "the declaration must name both keys of `j/k`"
    );
    assert_eq!(
        announcements.painted_outside(),
        vec!["Esc", "z"],
        "a pair the surface paints and does not declare must be reported, not absorbed"
    );
}

#[test]
fn a_surface_that_advertises_only_in_its_footer_is_still_measured() {
    // The ten footer-only baseline surfaces are this shape: nothing declared,
    // so the row is the only evidence, and it must still reach the measurement.
    let (announcements, lines) =
        component_announcements(Box::new(AdvertisesOnlyInItsFooter), Rect::new(0, 0, 40, 6));
    assert!(
        lines.is_empty(),
        "the fixture declares no hint line, so the row is its whole advertisement"
    );
    assert_eq!(
        announcements.painted,
        vec!["Esc", "q"],
        "a footer-only advertisement must still be read"
    );
    assert_eq!(
        announcements.painted_outside(),
        vec!["Esc", "q"],
        "with no declaration every painted key is outside the declaration, which is the gap \n         the conversion closes"
    );
}

#[test]
fn a_declared_spelling_the_parser_cannot_read_is_reported() {
    // Skipping an unreadable declaration is the measurement hole in its current
    // form: the surface says it owns `F5` and nothing says it cannot be pressed.
    let (announcements, _) =
        component_announcements(Box::new(DeclaresUnreadableKey), Rect::new(0, 0, 40, 6));
    assert_eq!(
        announcements.declared,
        Vec::<String>::new(),
        "an unreadable spelling is not a pressable one"
    );
    assert_eq!(
        announcements.unreadable,
        vec!["F5"],
        "the declaration must be reported as unreadable rather than dropped"
    );

    // And the real tree's report names only spellings from the documented
    // blind-spot list, so the list and the report cannot drift apart.
    let mut report = AdvertisedReport::default();
    let mut measured = BTreeSet::new();
    measure_advertised_keys(&mut report, &mut measured);
    assert!(
        !report.unreadable.is_empty(),
        "the vocabulary contains spellings the parser cannot read; a silent report is the hole"
    );
    for row in &report.unreadable {
        assert!(
            UNREADABLE_SPELLINGS.contains(&row.spelling.as_str()),
            "`{}` reports `{}`, which is not in UNREADABLE_SPELLINGS",
            row.surface,
            row.spelling
        );
    }
    assert!(
        report
            .unreadable
            .iter()
            .any(|row| row.surface == "Queue" && row.spelling == "["),
        "`Queue` declares `[ ]`, so both halves of the pair must be reported unreadable"
    );
}

#[test]
fn the_parser_reads_named_keys_and_names_the_ones_it_cannot() {
    // The documented limit, held as a property: widening the parser fails the
    // first assertion, which is the moment the fixtures have to be revisited —
    // a newly readable key becomes a pressed key and moves the recorded set.
    for spelling in UNREADABLE_SPELLINGS {
        assert!(
            key_of(spelling).is_none(),
            "`{spelling}` is listed as unreadable but the parser reads it"
        );
    }
    for spelling in [
        "Esc", "Enter", "Tab", "space", "↑", "↓", "←", "→", "^d", "⇧Q", "j", "4",
    ] {
        assert!(
            key_of(spelling).is_some(),
            "`{spelling}` is part of the vocabulary and must be readable"
        );
    }
}
