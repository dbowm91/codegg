//! M006-A: document editor widget.
//!
//! Renders one document from a scoped borrow of the controller's replica:
//! a line-number gutter, the current-line highlight, a horizontally
//! scrolled viewport, and a state banner.
//!
//! ## Hard wrap
//!
//! One logical line is exactly one screen row. That one-to-one mapping is
//! what keeps the gutter, the current-line highlight, and cursor arithmetic
//! deterministic; soft wrap is deferred to a later milestone.
//!
//! ## Bounded work (C002)
//!
//! Only the visible slice is materialized, bounded by the viewport height,
//! and each visible line examines at most
//! [`MAX_RENDER_LINE_BYTES`](crate::tui::editor::MAX_RENDER_LINE_BYTES)
//! (32 KiB) with the frame total capped at
//! [`MAX_FRAME_SOURCE_BYTES`](crate::tui::editor::MAX_FRAME_SOURCE_BYTES)
//! (256 KiB). A line past the per-line budget renders from its prefix with
//! a visible `…[too long]` marker and a banner note — never a silent
//! prefix. Cursor and selection byte offsets stay exact; horizontal motion
//! past the prefix clamps to it. See `docs/tui.md` for the exact contract.
//!
//! The widget never reads a file: every byte arrives through
//! `DocumentController::try_snapshot()`, which is daemon-authorized.

use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use codegg_document::DocumentSnapshot;

use crate::tui::editor::{self, EditorMode, EditorPrefix, EditorViewport};
use crate::tui::theme::Theme;

/// Rows of chrome reserved at the **bottom** of the editor block.
///
/// One row, and only one. It carries the **path on the left** and the
/// **status cluster on the right**.
///
/// It used to be two: a banner row and a coordinate readout. The readout's
/// line and column were the cursor restated, and everything else it carried
/// moved down here. The banner row went with them — the disclosures it held
/// (the pending prefix, the find note, the notice) are transient, and a
/// transient signal belongs in the row that is always there rather than in a
/// row that has to be allocated to hold it.
const CHROME_ROWS: u16 = 1;

/// How readily a chrome-cluster element may be dropped when the row is too
/// narrow to hold all of it.
///
/// Higher survives longer, and the order is a reading of *what hurts most to
/// lose*:
///
/// - **busy** is self-evident — the reader can see the work happening;
/// - the **modified** state is a status, not a safety signal;
/// - the **writability** marker and the **scope badge** say what an edit
///   *here* will do, which is why they outrank the status word;
/// - the **disclosure** is what just happened;
/// - a **caveat** ("line too long", "highlighting truncated") outranks even
///   that, because dropping it is not a smaller truth — it is a *misleading*
///   one: a truncated prefix that looks like the whole line is worse than a
///   missing footnote;
/// - the **notice** and the **armed prefix** are the two things that must be
///   on screen at the instant they happen.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum ClusterWeight {
    Busy,
    Modified,
    Writable,
    Scope,
    Disclosure,
    Caveat,
    Notice,
    Prefix,
}

/// One element of the chrome cluster: a weight, and the spans it draws.
struct ClusterGroup<'a> {
    weight: ClusterWeight,
    spans: Vec<Span<'a>>,
}

impl<'a> ClusterGroup<'a> {
    /// A group that draws one string.
    fn one(weight: ClusterWeight, text: String, style: Style) -> Self {
        Self {
            weight,
            spans: vec![Span::styled(text, style)],
        }
    }

    /// The group's display width.
    fn width(&self) -> usize {
        self.spans.iter().map(|span| span.width()).sum()
    }
}

/// Fit the chrome cluster into `budget` columns, dropping what matters least.
///
/// The row has one line and a fixed width, and the cluster is a set of
/// answers that do not all fit in it. Something has to go, and the question
/// is *what*.
///
/// The answer is a **rank**, not a position: the least important group is
/// dropped first, whatever order it was rendered in. A cluster that dropped
/// from one end would take the scope badge before the "line too long"
/// footnote, or the armed prefix before the note that explains it, and both
/// are worse than the other.
///
/// A group that is still too wide on its own is **cut from the right** rather
/// than dropped: "3 matches · partial · n/N next/prev" is worth more as
/// "3 matches · partial…" than as nothing at all, because the head of a
/// sentence is the part that answers the question.
///
/// The armed prefix is the highest rank, so it is the last thing to go and
/// the first thing to survive.
fn fit_cluster(mut groups: Vec<ClusterGroup<'_>>, budget: usize) -> Vec<Span<'_>> {
    let width = |groups: &[ClusterGroup<'_>]| groups.iter().map(ClusterGroup::width).sum::<usize>();
    while groups.len() > 1 && width(&groups) > budget {
        // The lowest rank goes first; among equals the leftmost, so what is
        // left is the right end of the row.
        let victim = groups
            .iter()
            .enumerate()
            .min_by_key(|(_, group)| group.weight)
            .map(|(index, _)| index)
            .expect("the loop keeps at least one group");
        groups.remove(victim);
    }
    if groups.len() == 1 && groups[0].width() > budget {
        let weight = groups[0].weight;
        let cut = cut_group(&groups[0], budget);
        groups.clear();
        groups.push(ClusterGroup { weight, spans: cut });
    }
    groups.into_iter().flat_map(|group| group.spans).collect()
}

/// Cut a group down to `budget` columns, keeping the head of it.
///
/// The head is the part that answers the question: the count of a search, the
/// bound of a find, the wording of an error. The tail is the part that can
/// be guessed.
fn cut_group<'a>(group: &ClusterGroup<'a>, budget: usize) -> Vec<Span<'a>> {
    let mut out: Vec<Span<'_>> = Vec::new();
    let mut used = 0usize;
    for span in &group.spans {
        let width = span.width();
        if used + width <= budget {
            out.push(span.clone());
            used += width;
            continue;
        }
        // This span straddles the budget: cut it, and stop.
        let remaining = budget.saturating_sub(used);
        if remaining > 0 {
            let text = crate::tui::wrap::ellipsize_to_width(&span.content, remaining);
            out.push(Span::styled(text, span.style));
        }
        break;
    }
    out
}

/// Document editor view.
///
/// `snapshot` is borrowed for the duration of one `render` call and is
/// dropped with it; the widget stores no text.
pub struct EditorWidget<'a> {
    /// Workspace-relative path shown in the title.
    pub path: &'a str,
    /// Controller state label.
    pub status: &'a str,
    /// Surfaced error banner, if any.
    pub notice: Option<&'a str>,
    /// Why the last find-char motion did not land, already rendered to text
    /// by [`FindNote::label`], or `""`.
    ///
    /// A pre-rendered `&str` like `status` and `notice`, because the widget
    /// holds no domain state: the disclosure belongs to the motion that
    /// produced it, and the caller already had it in hand.
    pub find_note: &'a str,
    /// Buffer mode.
    pub mode: EditorMode,
    /// Which region owns keyboard input.
    /// Whether the document owns the keyboard.
    ///
    /// One fact, derived from the route's `input_mode` by the caller: INS
    /// puts the cursor in the text, NOR hands typing to the bottom input box
    /// while the Helix verbs stay on the buffer.
    pub buffer_focused: bool,
    /// The armed multi-key prefix, rendered as its pending indicator.
    pub pending_prefix: EditorPrefix,
    /// Whether the `:` command line is open.
    pub command_active: bool,
    /// Authority root the open document is confined to. Drives the global /
    /// project badge in the status row, so a user editing a shared agent
    /// config can see at a glance that a `:wq` will affect every project.
    pub scope: codegg_protocol::document::DocumentScope,
    /// The `:` line without its leading colon.
    pub command_line: &'a str,
    /// Highlighted palette row index.
    pub command_selection: usize,
    /// Whether the `/` search line is open.
    pub search_active: bool,
    /// The `/` line without its leading slash.
    pub search_line: &'a str,
    /// Retained match count for the current search.
    pub search_count: usize,
    /// Whether the current search is known to be partial.
    pub search_partial: bool,
    /// Cursor byte offset into `snapshot`.
    pub cursor_byte: usize,
    /// Whether the attachment holds the writer lease.
    pub writable: bool,
    /// Whether a lifecycle operation is in flight.
    pub busy: bool,
    /// The controller replica. Borrowed, never retained.
    pub snapshot: &'a DocumentSnapshot,
    /// Bounded diagnostics and semantic spans for this document generation.
    pub lsp: &'a crate::tui::editor_lsp::EditorLspState,
    /// Bounded completion popup state for this document generation.
    pub completion: &'a crate::tui::editor_completion::EditorCompletionState,
    /// Server-free syntax highlighting for this document generation.
    ///
    /// Mutated only to resolve spans for the rows this frame paints; the
    /// LSP layer in `lsp.semantic_tokens` stays authoritative wherever the
    /// two overlap.
    pub highlight: &'a mut crate::tui::editor_highlight::EditorHighlight,
    /// Focused editor presentation, mutated only to resolve the viewport.
    pub presentation: &'a mut crate::tui::document_session::TuiDocumentPresentation,
    pub theme: Arc<Theme>,
}

/// The editor's own keys, as the keymap box lists them.
///
/// The editor's **own** verbs, not the whole application's: the box answers
/// "what can I press *in here*", and a list that also carried the composer's
/// and the panels' keys would answer a different question.
///
/// The descriptions are short on purpose. The box is a corner overlay, and a
/// box whose rows are wide enough to need truncating names some of the keys
/// and not others — which is worse than no box at all.
const EDITOR_KEYMAP: &[(&str, &str)] = &[
    ("e / b", "word end / start — selects the word"),
    ("w", "next word start"),
    ("v", "select mode — movement extends"),
    ("x", "select the line"),
    ("d / y", "delete / yank the selection"),
    ("u / Ctrl-r", "undo / redo"),
    (":", "command line — matches as you type"),
    ("/", "search"),
    ("n / N", "next / previous match"),
    ("Ctrl-d / Ctrl-u", "half a page down / up"),
    ("PgDn / PgUp", "a full page down / up"),
    ("z c / t / b", "view: centre / top / bottom"),
    ("g g / g G", "start / end of the buffer"),
    ("g t / c / b", "top / middle / bottom of screen"),
    ("space ?", "this box"),
    ("Esc", "leave select mode / cancel"),
];

impl EditorWidget<'_> {
    /// Paint the editor into `area`.
    pub fn render(mut self, frame: &mut Frame, area: Rect) {
        let theme = Arc::clone(&self.theme);
        let focused = self.buffer_focused;
        // The block's top border carries **no label**. It used to carry
        // `{path} · {mode} · {status}`, and all three moved: the path and the
        // status to the single bottom row, the mode to the badge above the
        // input box. An empty border is the honest result — every signal the
        // title carried is still on screen, and none of it is on screen twice.
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if focused { theme.primary } else { theme.border }))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let text_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(CHROME_ROWS),
        };
        self.render_chrome(frame, inner);
        if text_area.height > 0 {
            self.render_text(frame, text_area);
        }
        // After the text: the match list is a floating overlay, and an overlay
        // painted before the document would be painted over by it.
        self.render_command_matches(frame, text_area);
        self.render_keymap_overlay(frame, text_area);
        self.render_lsp_panel(frame, area);
        self.render_completion_popup(frame, area);
    }

    /// The editor's **keymap box** — its own catalogue of its keys, in the
    /// bottom-right corner.
    ///
    /// A floating overlay: it covers the corner of the text area rather than
    /// taking rows away from the document, so opening it does not reflow the
    /// text under the reader.
    fn render_keymap_overlay(&self, frame: &mut Frame, text_area: Rect) {
        if !self.presentation.keymap_overlay {
            return;
        }
        let theme = Arc::clone(&self.theme);
        let key_width = EDITOR_KEYMAP
            .iter()
            .map(|(keys, _)| keys.chars().count())
            .max()
            .unwrap_or(0);
        let desc_width = EDITOR_KEYMAP
            .iter()
            .map(|(_, desc)| desc.chars().count())
            .max()
            .unwrap_or(0);
        let width = (key_width + desc_width + 4).min(text_area.width as usize) as u16;
        let height = (EDITOR_KEYMAP.len() + 2).min(text_area.height as usize) as u16;
        if width == 0 || height == 0 {
            return;
        }
        let rect = Rect {
            x: text_area.x + text_area.width - width,
            y: text_area.y + text_area.height - height,
            width,
            height,
        };
        let lines: Vec<Line> = EDITOR_KEYMAP
            .iter()
            .map(|(keys, desc)| {
                Line::from(vec![
                    Span::styled(
                        format!(" {keys:>key_width$}  "),
                        Style::default()
                            .fg(theme.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(desc.to_string(), Style::default().fg(theme.foreground)),
                ])
            })
            .collect();
        let block = Block::default()
            .title(" keymap ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.primary))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(rect);
        frame.render_widget(Clear, rect);
        frame.render_widget(block, rect);
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme.background).fg(theme.foreground)),
            inner,
        );
    }

    /// The editor's single bottom row: the path on the left, the status
    /// cluster on the right.
    ///
    /// The left slot is the **input** slot. When the `:` or `/` line is open
    /// it holds that line and the path is erased: the reader is looking at
    /// what they are typing, and a path beside it is a second answer to a
    /// question nobody asked.
    ///
    /// The right slot is the **cluster**: the persistent signals, then the
    /// transient disclosure, then the notice, then the armed prefix. It is
    /// right-aligned rather than wrapped, so the row is always exactly one
    /// row tall and the text area never changes height.
    ///
    /// The two slots **never overlap**. The cluster is fitted to what the
    /// path leaves — the path keeps up to a third of the row, and the cluster
    /// takes the rest — and when even that is not enough, the cluster drops
    /// its least important element rather than running over the path. A
    /// status row painted on top of the path is two half-legible surfaces
    /// where one legible one was asked for.
    fn render_chrome(&self, frame: &mut Frame, inner: Rect) {
        let theme = Arc::clone(&self.theme);
        let row = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(CHROME_ROWS),
            width: inner.width,
            height: CHROME_ROWS.min(inner.height),
        };
        if row.height == 0 || row.width == 0 {
            return;
        }
        let available = row.width as usize;
        // The path keeps what it needs, up to a third of the row. It is the
        // least urgent thing on the row — it does not change — but a path
        // squeezed to one column is a path nobody can read either.
        let (left_text, left_style) = self.chrome_input();
        let path_width = unicode_width::UnicodeWidthStr::width(left_text.as_str());
        let path_share = path_width.min((available / 3).max(1));
        let cluster_budget = available.saturating_sub(path_share).saturating_sub(1);
        let right = fit_cluster(self.chrome_cluster(&theme), cluster_budget);
        let right_width = right.iter().map(|span| span.width()).sum::<usize>();
        let left_budget = available.saturating_sub(right_width + 1).max(1);
        let left_text = crate::tui::wrap::ellipsize_to_width(&left_text, left_budget);
        // Themed like the text area. The block paints its background first,
        // but anything the user has typed into the composer underneath can
        // leave the terminal's own default behind on this row, and a status
        // line in a different colour than the document reads as two
        // different surfaces.
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(left_text, left_style)))
                .style(Style::default().bg(theme.background).fg(theme.foreground)),
            Rect {
                width: left_budget.min(available) as u16,
                ..row
            },
        );
        if right_width == 0 {
            return;
        }
        let right_x = row.x + (available - right_width) as u16;
        frame.render_widget(
            Paragraph::new(Line::from(right))
                .style(Style::default().bg(theme.background).fg(theme.foreground)),
            Rect {
                x: right_x,
                width: right_width.min(available) as u16,
                ..row
            },
        );
    }

    /// The left slot of the bottom row: the modal input line when one is
    /// open, and the file's path when none is.
    fn chrome_input(&self) -> (String, Style) {
        let theme = Arc::clone(&self.theme);
        if self.search_active {
            return (
                format!("/{}_", self.search_line),
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            );
        }
        if self.command_active {
            return (
                format!(":{}_", self.command_line),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            );
        }
        (self.path.to_string(), Style::default().fg(theme.muted))
    }

    /// The right slot of the bottom row, in the order it is read.
    ///
    /// The row is one story, left to right: **where am I** (the persistent
    /// signals), **what just happened** (the transient disclosure), **what is
    /// wrong** (the notice), **what am I about to press** (the armed prefix).
    ///
    /// The persistent signals come first because they never move: the
    /// **modified** state, the scope the document is confined to, writable or
    /// read-only, the LSP semantic coverage, and the "line too long" marker.
    /// The line number and the column are gone — they restated the cursor,
    /// which is already on screen.
    ///
    /// The armed prefix comes **last**, at the far right of the row. It is the
    /// one element that appears and disappears on its own, so it gets a home
    /// nothing else competes for: the reader's eye arrives at the end of the
    /// row after reading the state, and the prefix is what they are waiting
    /// to see. A prefix in the middle of the cluster would have to be looked
    /// for among the words around it, and "did my `g` register?" is a question
    /// that has to be answered at a glance.
    fn chrome_cluster(&self, theme: &Theme) -> Vec<ClusterGroup<'_>> {
        // A modal line **owns** the row. The reader is looking at what they
        // are typing, and a cluster of status beside it is a second answer to
        // a question nobody asked — and the row is only one row tall.
        if self.search_active || self.command_active {
            return Vec::new();
        }
        let mut groups: Vec<ClusterGroup<'_>> = Vec::new();
        // ── The persistent cluster ──────────────────────────────────────
        // A **notice** replaces the persistent signals rather than joining
        // them. A surfaced error ("edit queue is full") is more urgent than
        // the dirty state, and a row that says both is a row that says
        // neither clearly.
        let notice = self.notice;
        if notice.is_none() {
            if self.busy {
                groups.push(ClusterGroup::one(
                    ClusterWeight::Busy,
                    "working… ".to_string(),
                    Style::default()
                        .fg(theme.warning)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            // The **modified** state, moved down from the title rather than
            // dropped: it is the only unsaved-changes signal the editor has.
            groups.push(ClusterGroup::one(
                ClusterWeight::Modified,
                format!("{} ", self.status),
                Style::default().fg(theme.muted),
            ));
            // Always show which root this document is confined to. The difference
            // is invisible in the path itself (`build.toml` reads the same at
            // either scope) but it decides who sees the edit, so it must not be
            // something the user has to remember.
            groups.push(ClusterGroup::one(
                ClusterWeight::Scope,
                format!(
                    "[{}] ",
                    crate::tui::commands::agent_editor::scope_label(self.scope)
                ),
                Style::default().fg(if self.scope.is_global_agent() {
                    theme.warning
                } else {
                    theme.muted
                }),
            ));
            groups.push(ClusterGroup::one(
                ClusterWeight::Writable,
                if !self.writable {
                    "read-only — edits refused ".to_string()
                } else if crate::tui::commands::agent_editor::is_agent_config_path(
                    self.path, self.scope,
                ) {
                    // In an agent file the interesting command is the one that
                    // publishes the agent, so say so here rather than making the
                    // user read `/help` for it.
                    format!("{} ", crate::tui::commands::agent_editor::AGENT_EDITOR_HINT)
                } else {
                    "writable ".to_string()
                },
                Style::default().fg(if self.writable {
                    theme.muted
                } else {
                    theme.warning
                }),
            ));
            // A truncated or partially validated semantic result is never
            // represented as complete coverage.
            if let Some(note) = self.lsp.semantic_coverage_note() {
                groups.push(ClusterGroup::one(
                    ClusterWeight::Caveat,
                    format!("· {note} "),
                    Style::default().fg(theme.warning),
                ));
            }
            // C002: a huge line under the cursor renders from its 32 KiB prefix
            // with a `…[too long]` marker on the row itself. The chrome repeats
            // the signal so the degradation is visible even when the row marker
            // scrolls out of view.
            if editor::line_is_truncated(
                self.snapshot,
                editor::cursor_line(self.snapshot, self.cursor_byte),
            ) || editor::display_column_truncated(self.snapshot, self.cursor_byte)
            {
                groups.push(ClusterGroup::one(
                    ClusterWeight::Caveat,
                    "· line too long — showing prefix ".to_string(),
                    Style::default().fg(theme.warning),
                ));
            }
        }
        // ── The transient disclosure ────────────────────────────────────
        // It answers "what did my last keystroke do?" — the find that landed,
        // the search that matched, the history cursor that moved. It is one
        // group: the count of a search and its "partial" marker are one
        // answer, and dropping half of it would say "3 matches" about a
        // search that only found part of them.
        let mut disclosure: Vec<Span<'_>> = Vec::new();
        if self.search_active || self.search_count > 0 {
            let plural = if self.search_count == 1 { "" } else { "es" };
            if self.search_line.is_empty() {
                // A `/` with nothing typed yet is not a failed search.
                disclosure.push(Span::styled(
                    "type to search ".to_string(),
                    Style::default().fg(theme.muted),
                ));
            } else if self.search_count == 0 {
                disclosure.push(Span::styled(
                    "no matches ".to_string(),
                    Style::default().fg(theme.error),
                ));
            } else {
                disclosure.push(Span::styled(
                    format!("{} match{} ", self.search_count, plural),
                    Style::default().fg(theme.warning),
                ));
            }
            if self.search_partial {
                // Never let a truncated match list read as a complete one.
                disclosure.push(Span::styled(
                    "· partial ".to_string(),
                    Style::default().fg(theme.warning),
                ));
            }
            if !self.search_active && self.search_count > 0 {
                disclosure.push(Span::styled(
                    "· n/N next/prev ".to_string(),
                    Style::default().fg(theme.muted),
                ));
            }
        } else if !self.find_note.is_empty() {
            disclosure.push(Span::styled(
                format!("· {} ", self.find_note),
                Style::default().fg(theme.muted),
            ));
        } else if let Some((position, length)) = self.presentation.history_position() {
            // A time cursor nobody can see is a rename, so the position is
            // disclosed whenever the reader is standing somewhere other than
            // the newest change.
            disclosure.push(Span::styled(
                format!("· history {position}/{length} "),
                Style::default().fg(theme.muted),
            ));
        }
        if !disclosure.is_empty() {
            groups.push(ClusterGroup {
                weight: ClusterWeight::Disclosure,
                spans: disclosure,
            });
        }
        if let Some(notice) = notice {
            groups.push(ClusterGroup::one(
                ClusterWeight::Notice,
                format!("{notice} "),
                Style::default().fg(theme.error),
            ));
        }
        // The armed prefix is **last**: it is the one thing that must be
        // visible at the instant the key is pressed, and the end of the row
        // is where a reader's eye arrives after reading what came before.
        let pending = self.pending_prefix.display();
        if !pending.is_empty() {
            groups.push(ClusterGroup::one(
                ClusterWeight::Prefix,
                format!("{pending} "),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        groups
    }

    /// The `:` command's fuzzy match list, floating on the row **above** the
    /// command line.
    ///
    /// It is an overlay rather than an allocated row on purpose: allocating
    /// one would change the text area's height, which changes the visible
    /// line count, which re-resolves the viewport and can move the cursor's
    /// screen row. The reader's first keystroke on the `:` line would then
    /// reflow the document under them. An overlay covers one row of text
    /// instead, and the text area's height never changes.
    fn render_command_matches(&self, frame: &mut Frame, text_area: Rect) {
        if !self.command_active || text_area.height == 0 {
            return;
        }
        let theme = Arc::clone(&self.theme);
        let matches = editor::complete_editor_command(self.command_line);
        let mut spans: Vec<Span<'_>> = Vec::new();
        if matches.is_empty() {
            spans.push(Span::styled(
                "no matching command",
                Style::default().fg(theme.error),
            ));
        }
        for (index, candidate) in matches.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw("  "));
            }
            let selected = index == self.command_selection;
            let command = candidate.command;
            spans.push(Span::styled(
                format!(":{}", command.spec()),
                if selected {
                    Style::default()
                        .fg(theme.background)
                        .bg(theme.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground)
                },
            ));
            spans.push(Span::styled(
                format!(" {}", command.help()),
                Style::default().fg(theme.muted),
            ));
        }
        let row = Rect {
            x: text_area.x,
            y: text_area.y + text_area.height - 1,
            width: text_area.width,
            height: 1,
        };
        frame.render_widget(Clear, row);
        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .style(Style::default().bg(theme.background).fg(theme.foreground)),
            row,
        );
    }

    /// Gutter plus horizontally scrolled text for the visible slice.
    fn render_text(&mut self, frame: &mut Frame, area: Rect) {
        let theme = Arc::clone(&self.theme);
        let viewport: EditorViewport =
            editor::resolve_viewport(self.presentation, self.snapshot, area.width, area.height);
        let cursor_line = editor::cursor_line(self.snapshot, self.cursor_byte);
        // Resolved once per frame, not per row: `selection_range` clamps and
        // resolves lines, and doing that per row would re-walk the selection
        // for every visible line.
        let selection = editor::selection_ranges(self.presentation, self.snapshot);
        let gutter_style = Style::default().fg(theme.muted);
        let current_gutter = Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD);
        // The current line is a **tint, not a highlight**. The position
        // indicators are the cursor block and the held selection; the line
        // itself only has to say "this one".
        //
        // It used to set `fg(theme.alternate_bg)` — a *background* colour
        // used as a foreground — so on every dark theme the cursor's own
        // line rendered near-black on near-black. Since a file opens in
        // Normal mode, that made the one line a reader is guaranteed to be
        // looking at the one line they could not read. A background keeps
        // the syntax colours, which a foreground overrode.
        let current_style = Style::default().bg(theme.alternate_bg);

        let total_lines = self.snapshot.len_lines();
        let mut lines: Vec<Line> = Vec::with_capacity(viewport.visible_lines as usize);
        // C002 frame budget: content reads across all visible rows cost at
        // most 256 KiB. Rows past the budget render as a marker without
        // reading a single content byte, so a viewport full of huge lines
        // stays bounded. Row byte sizes come from the cheap length check,
        // never from a content read.
        let mut frame_remaining = editor::MAX_FRAME_SOURCE_BYTES;
        for row in 0..viewport.visible_lines as usize {
            let line_index = viewport.first_line + row;
            if line_index >= total_lines {
                break;
            }
            let row_bytes = editor::line_byte_len(self.snapshot, line_index)
                .unwrap_or(0)
                .min(editor::MAX_RENDER_LINE_BYTES);
            let is_current = line_index == cursor_line;
            let number = format!(
                "{:>width$} ",
                line_index + 1,
                width = viewport.gutter as usize - 1
            );
            let marker = self
                .lsp
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.range.start_line as usize == line_index)
                .map(|diagnostic| diagnostic.severity)
                .min();
            let marker = match marker {
                Some(1) => "!",
                Some(2) => "▲",
                Some(_) => "·",
                None => " ",
            };
            let mut spans = vec![Span::styled(
                format!("{number}{marker}"),
                if is_current {
                    current_gutter
                } else {
                    gutter_style
                },
            )];
            let text_style = if is_current {
                current_style
            } else {
                Style::default()
            };
            if row_bytes > frame_remaining {
                // Frame budget exhausted: no content read for this row, just
                // the honest marker. The banner note (cursor line) and the
                // per-row markers on the lines that did render already say
                // the frame is partial.
                spans.push(Span::styled(
                    crate::tui::editor::TRUNCATED_LINE_SUFFIX,
                    Style::default()
                        .fg(theme.warning)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                frame_remaining = frame_remaining.saturating_sub(row_bytes);
                spans.extend(self.line_spans(line_index, viewport, text_style, &selection));
            }
            lines.push(Line::from(spans));
        }
        // An empty document still has one logical line, so `lines` is
        // non-empty. Test the text, not the line count.
        if self.snapshot.is_empty() {
            lines.clear();
            lines.push(Line::from(Span::styled(
                "(empty document)",
                Style::default().fg(theme.muted),
            )));
        }
        frame.render_widget(Clear, area);
        // `Clear` resets every cell to the terminal's own default, so the
        // themed background the block painted a moment ago is gone by the
        // time the text lands. Restating it here is what actually makes the
        // document area theme-colored rather than whatever the user's shell
        // profile happened to leave behind.
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme.background).fg(theme.foreground)),
            area,
        );
        self.render_scrollbar(frame, area, &viewport);
    }

    /// The scrollbar, in the column the viewport already reserves for it.
    ///
    /// The column was always reserved — `text_columns` subtracts one, and
    /// the field doc has always said "after the gutter and scrollbar" — and
    /// nothing ever drew in it. The editor therefore looked like it had no
    /// scrollbar at all, and "how far down am I" had no answer on screen.
    ///
    /// The thumb is sized by how much of the document is visible and placed
    /// by where the view starts, so it is a **map** of the document rather
    /// than an indicator of the cursor: a reader who has scrolled away from
    /// the cursor still sees where they are.
    fn render_scrollbar(&self, frame: &mut Frame, area: Rect, viewport: &EditorViewport) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let theme = Arc::clone(&self.theme);
        let total = self.snapshot.len_lines().max(1);
        let visible = (viewport.visible_lines as usize).max(1);
        let track = area.height as usize;
        // The thumb is never shorter than a row, or a document that fits on
        // screen would have no scrollbar to see at all.
        let thumb = visible.min(total).clamp(1, track);
        let max_first = total.saturating_sub(visible);
        let first = viewport.first_line.min(max_first);
        let offset = if max_first == 0 {
            0
        } else {
            first * (track - thumb) / max_first
        };
        let thumb_style = Style::default().fg(theme.primary);
        let track_style = Style::default().fg(theme.border);
        let lines: Vec<Line> = (0..track)
            .map(|row| {
                let in_thumb = row >= offset && row < offset + thumb;
                Line::from(Span::styled(
                    if in_thumb { "█" } else { "│" },
                    if in_thumb { thumb_style } else { track_style },
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme.background)),
            Rect {
                x: area.x + area.width - 1,
                y: area.y,
                width: 1,
                height: area.height,
            },
        );
    }

    fn render_lsp_panel(&self, frame: &mut Frame, area: Rect) {
        use crate::tui::editor_lsp::{DiagnosticsViewStatus, EditorLspPanel};
        let Some(panel) = self.lsp.panel else { return };
        let height = area.height.min(12);
        let width = area.width.min(100);
        if height < 3 || width < 12 {
            return;
        }
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let theme = Arc::clone(&self.theme);
        // The scroll offset is the payload's own, because one payload is a
        // **picker** over a list longer than the panel is tall: the
        // code-action list scrolls its window to the selected action, so
        // `Enter` can never offer to apply a change the reader cannot see.
        // Every other payload keeps the panel's standing behaviour.
        let (title, mut lines, scroll) = match panel {
            EditorLspPanel::Diagnostics => {
                let status = match self.lsp.diagnostics_status {
                    DiagnosticsViewStatus::Idle => "not requested".to_string(),
                    DiagnosticsViewStatus::Loading => "refreshing".to_string(),
                    DiagnosticsViewStatus::Ready => "current".to_string(),
                    DiagnosticsViewStatus::NotReady => "language server not ready".to_string(),
                    DiagnosticsViewStatus::Unavailable => "unavailable".to_string(),
                };
                let stale = if self.lsp.diagnostics_stale {
                    " · showing stale results"
                } else {
                    ""
                };
                let mut lines = vec![Line::from(Span::styled(
                    format!("{status}{stale}"),
                    Style::default().fg(theme.warning),
                ))];
                lines.extend(self.lsp.diagnostics.iter().take(8).enumerate().map(
                    |(index, item)| {
                        Line::from(format!(
                            "{}{}:{} [{}] {}",
                            if index == self.lsp.selected_index {
                                "> "
                            } else {
                                "  "
                            },
                            item.range.start_line + 1,
                            item.range.start_column + 1,
                            severity_label(item.severity),
                            crate::tui::editor_lsp::truncate_chars(&item.message, 120)
                        ))
                    },
                ));
                if self.lsp.diagnostics.is_empty()
                    && self.lsp.diagnostics_status == DiagnosticsViewStatus::Ready
                {
                    lines.push(Line::from("No diagnostics"));
                }
                (
                    " Diagnostics · Enter jump · r refresh · Esc close ",
                    lines,
                    self.lsp.selected_index,
                )
            }
            EditorLspPanel::ReadResult => {
                let mut lines = Vec::new();
                // The title names what `Enter` does, and the two read-result
                // kinds differ: a location opens a file, a code action opens
                // a change review.
                let mut title = " LSP result · Enter opens safe target · Esc close ";
                // The standing scroll for the read-result panel; the
                // code-action payload overrides it with its own window's
                // offset.
                let mut scroll = self.lsp.selected_index;
                if self.lsp.read_loading {
                    lines.push(Line::from("Request in progress…"));
                }
                if let Some(error) = &self.lsp.read_error {
                    lines.push(Line::from(crate::tui::editor_lsp::truncate_chars(
                        error, 120,
                    )));
                }
                if let Some(result) = &self.lsp.read_result {
                    use codegg_protocol::lsp::{LspReadPayloadDto, LspReadStatus};
                    if matches!(result.payload, Some(LspReadPayloadDto::CodeActions { .. })) {
                        title = " LSP code actions · Enter reviews · Esc close ";
                    }
                    if result.status == LspReadStatus::NotReady {
                        lines.push(Line::from("Language server not ready"));
                    }
                    if result.status == LspReadStatus::Ambiguous {
                        // Not an empty result. Falling through to "No results"
                        // would tell the user the workspace is clean at this
                        // position, when in fact the daemon declined to
                        // choose between two healthy servers.
                        lines.push(Line::from(
                            "Several language servers are running for this workspace",
                        ));
                    }
                    if result.status == LspReadStatus::Unsupported {
                        // A live server that simply does not do this. Reporting
                        // it as "not ready" would send the user looking for a
                        // server that is already running and already answered.
                        let capability = match result.payload.as_ref() {
                            Some(LspReadPayloadDto::UnsupportedCapability { capability }) => {
                                capability.as_str()
                            }
                            _ => "this operation",
                        };
                        lines.push(Line::from(format!(
                            "The language server does not support this ({capability})"
                        )));
                    }
                    match result.payload.as_ref() {
                        Some(LspReadPayloadDto::Hover { text, .. }) => lines.push(Line::from(
                            crate::tui::editor_lsp::truncate_chars(text, 240),
                        )),
                        Some(LspReadPayloadDto::Locations { locations, .. }) => lines.extend(
                            locations.iter().take(8).enumerate().map(|(index, loc)| {
                                Line::from(format!(
                                    "{}{}:{} · {}",
                                    if index == self.lsp.selected_index {
                                        "> "
                                    } else {
                                        "  "
                                    },
                                    loc.path,
                                    loc.start_line + 1,
                                    loc.workspace_relative_path
                                        .as_deref()
                                        .unwrap_or("outside workspace; display only")
                                ))
                            }),
                        ),
                        Some(LspReadPayloadDto::Symbols { symbols, .. }) => lines.extend(
                            symbols.iter().take(8).enumerate().map(|(index, symbol)| {
                                Line::from(format!(
                                    "{}{} · {}:{}",
                                    if index == self.lsp.selected_index {
                                        "> "
                                    } else {
                                        "  "
                                    },
                                    crate::tui::editor_lsp::truncate_chars(&symbol.name, 80),
                                    symbol.range.path,
                                    symbol.range.start_line + 1
                                ))
                            }),
                        ),
                        Some(LspReadPayloadDto::AmbiguousClients { candidates }) => {
                            lines.push(Line::from("Name one to answer:"));
                            lines.extend(candidates.iter().take(8).map(|candidate| {
                                Line::from(format!(
                                    "  · {}",
                                    crate::tui::editor_lsp::truncate_chars(candidate, 80)
                                ))
                            }));
                        }
                        Some(LspReadPayloadDto::Highlights { highlights, .. }) => {
                            lines.extend(highlights.iter().take(8).enumerate().map(
                                |(index, entry)| {
                                    Line::from(format!(
                                        "{}{}:{} · {}",
                                        if index == self.lsp.selected_index {
                                            "> "
                                        } else {
                                            "  "
                                        },
                                        entry.range.start_line + 1,
                                        entry.range.start_column + 1,
                                        // The server's own read/write
                                        // classification, carried rather than
                                        // reinterpreted — the same contract
                                        // `LspSymbolDto::kind` follows.
                                        match entry.kind {
                                            Some(2) => "read",
                                            Some(3) => "write",
                                            Some(_) => "text",
                                            None => "occurrence",
                                        }
                                    ))
                                },
                            ))
                        }
                        Some(LspReadPayloadDto::SignatureHelp {
                            signatures,
                            active_signature,
                            active_parameter,
                        }) => {
                            if signatures.is_empty() {
                                lines.push(Line::from("No signature at this position"));
                            }
                            lines.extend(signatures.iter().take(8).enumerate().map(
                                |(index, signature)| {
                                    Line::from(format!(
                                        "{}{}",
                                        if index == self.lsp.selected_index {
                                            "> "
                                        } else {
                                            "  "
                                        },
                                        crate::tui::editor_lsp::truncate_chars(
                                            &signature.label,
                                            200,
                                        )
                                    ))
                                },
                            ));
                            // Which overload the server considers live, if it
                            // said. Left as a separate line rather than
                            // spliced into the label: the label is the
                            // server's text and rewriting it would make the
                            // panel disagree with what was returned.
                            if let Some(active) = active_signature {
                                let active = *active as usize;
                                let parameter = active_parameter.map(|p| p.to_string());
                                lines.push(Line::from(format!(
                                    "active signature {}{}",
                                    active + 1,
                                    parameter
                                        .map(|p| format!(" · argument {p}"))
                                        .unwrap_or_default()
                                )));
                            }
                        }
                        Some(LspReadPayloadDto::FormatPreview(preview)) => {
                            if preview.diff.is_empty() {
                                lines.push(Line::from("Already formatted — no changes"));
                            } else {
                                if preview.base_stale {
                                    lines.push(Line::from(
                                        "The file changed while this preview was built; it is stale",
                                    ));
                                }
                                lines.push(Line::from(format!(
                                    "{} edit(s) · {}",
                                    preview.edit_count,
                                    preview_disposition(
                                        preview.truncated,
                                        preview.candidate.is_some()
                                    )
                                )));
                                lines.extend(preview.diff.lines().take(16).map(|line| {
                                    Line::from(crate::tui::editor_lsp::truncate_chars(line, 200))
                                }));
                            }
                        }
                        Some(LspReadPayloadDto::RenamePreview(preview)) => {
                            lines.push(Line::from(format!(
                                "{} → {} · {} edit(s) in {} file(s) · {}",
                                preview.old_name.as_deref().unwrap_or("(symbol)"),
                                preview.new_name,
                                preview.edit_count,
                                preview.files.len(),
                                preview_disposition(preview.truncated, preview.candidate.is_some())
                            )));
                            if preview.base_stale {
                                lines.push(Line::from(
                                    "A file changed while this preview was built; it is stale",
                                ));
                            }
                            lines.extend(preview.files.iter().take(8).enumerate().map(
                                |(index, file)| {
                                    Line::from(format!(
                                        "{}{} · {} edit(s)",
                                        if index == self.lsp.selected_index {
                                            "> "
                                        } else {
                                            "  "
                                        },
                                        file.path,
                                        file.edit_count
                                    ))
                                },
                            ));
                            lines.extend(preview.warnings.iter().take(4).map(|warning| {
                                Line::from(format!(
                                    "  ! {}",
                                    crate::tui::editor_lsp::truncate_chars(warning, 120)
                                ))
                            }));
                        }
                        Some(LspReadPayloadDto::CodeActions { actions, truncated }) => {
                            if actions.is_empty() {
                                lines.push(Line::from("No code actions at this position"));
                            } else {
                                // The window **follows the selection**.
                                //
                                // A code action is a choice and the panel is
                                // where the choice is made, so the selected
                                // action has to be *readable*: `Enter` on an
                                // action the reader cannot see would offer to
                                // apply a change they did not read. The
                                // window ends on the selected action, and the
                                // scroll below moves the view to it.
                                const VISIBLE_ACTIONS: usize = 8;
                                let selected = self.lsp.selected_index.min(actions.len() - 1);
                                let start = selected.saturating_sub(VISIBLE_ACTIONS - 1);
                                lines.extend(
                                    actions[start..]
                                        .iter()
                                        .take(VISIBLE_ACTIONS)
                                        .enumerate()
                                        .map(|(offset, action)| {
                                            let index = start + offset;
                                            let kind = action.kind.as_deref().unwrap_or("action");
                                            let preferred = if action.preferred {
                                                " · preferred"
                                            } else {
                                                ""
                                            };
                                            // The disposition is the *same*
                                            // words `Enter` will give if the
                                            // action turns out to be
                                            // unappliable.
                                            let disposition = match crate::tui::editor_lsp::
                                                code_action_unavailable_reason(action)
                                            {
                                                Some(reason) => reason,
                                                None => "ready to apply".to_string(),
                                            };
                                            Line::from(format!(
                                                "{}{} · {kind}{preferred} · {disposition}",
                                                if index == selected { "> " } else { "  " },
                                                crate::tui::editor_lsp::truncate_chars(
                                                    &action.title,
                                                    80
                                                )
                                            ))
                                        }),
                                );
                                scroll = selected - start;
                                if *truncated {
                                    lines.push(Line::from(
                                        "  … the server offered more actions than the result carries",
                                    ));
                                } else if actions.len() > VISIBLE_ACTIONS {
                                    lines.push(Line::from(format!(
                                        "  … {} actions offered; the panel shows {VISIBLE_ACTIONS} at a time",
                                        actions.len()
                                    )));
                                }
                            }
                        }
                        // Two payloads whose explanation is rendered above, next to
                        // the status it belongs to, rather than here: semantic
                        // tokens are a presentation overlay consumed by
                        // `request_editor_semantic_tokens`, and an unsupported
                        // capability already printed its own line. Explicit
                        // arms rather than a wildcard, so a new payload cannot
                        // slip through unrendered and print "No results".
                        Some(LspReadPayloadDto::SemanticTokens { .. }) => {}
                        Some(LspReadPayloadDto::UnsupportedCapability { .. }) => {}
                        None => {}
                    }
                }
                if lines.is_empty() {
                    lines.push(Line::from("No results"));
                }
                (title, lines, scroll)
            }
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.primary))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(rect);
        frame.render_widget(Clear, rect);
        frame.render_widget(block, rect);
        lines.truncate(inner.height as usize);
        frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), inner);
    }

    /// Bounded completion popup for the M003 read.
    ///
    /// Renders typed candidates and explicit non-ready states only; it never
    /// reads document text, so it cannot paint stale content. Untrusted
    /// server strings arrive pre-sanitized by
    /// [`crate::tui::editor_completion::sanitize_completion_text`] and are
    /// rendered as literal spans. On terminals too narrow to fit, the popup
    /// stays hidden while its keys keep working.
    fn render_completion_popup(&self, frame: &mut Frame, area: Rect) {
        use crate::tui::editor_completion::{
            sanitize_completion_text, CompletionViewStatus, MAX_COMPLETION_DETAIL_CHARS,
            MAX_COMPLETION_LABEL_CHARS, MAX_COMPLETION_POPUP_ROWS, MAX_COMPLETION_POPUP_WIDTH,
        };
        let state = self.completion;
        if !state.is_open() {
            return;
        }
        let Some(status) = state.status_line() else {
            return;
        };
        let theme = Arc::clone(&self.theme);
        let mut lines: Vec<Line> = Vec::new();
        match state.status {
            CompletionViewStatus::Loading => {
                lines.push(Line::from("Requesting…"));
            }
            CompletionViewStatus::Ready => {
                if state.candidates.is_empty() {
                    lines.push(Line::from("No suggestions"));
                } else {
                    let rows = MAX_COMPLETION_POPUP_ROWS;
                    let start = state
                        .selected
                        .min(state.candidates.len().saturating_sub(1))
                        .saturating_sub(rows / 2)
                        .min(state.candidates.len().saturating_sub(rows));
                    for (offset, candidate) in
                        state.candidates.iter().skip(start).take(rows).enumerate()
                    {
                        let index = start + offset;
                        let applicable = matches!(
                            candidate.applicability,
                            codegg_protocol::lsp::LspCompletionApplicabilityDto::Applicable { .. }
                        );
                        let mut row = format!(
                            "{} {}",
                            if index == state.selected { ">" } else { " " },
                            sanitize_completion_text(&candidate.label, MAX_COMPLETION_LABEL_CHARS),
                        );
                        if let Some(kind) = &candidate.kind {
                            row.push_str("  (");
                            row.push_str(&sanitize_completion_text(kind, 24));
                            row.push(')');
                        }
                        if let Some(detail) = &candidate.detail {
                            let detail =
                                sanitize_completion_text(detail, MAX_COMPLETION_DETAIL_CHARS);
                            if !detail.is_empty() {
                                row.push_str("  ");
                                row.push_str(&detail);
                            }
                        }
                        if !applicable {
                            row.push_str("  · unavailable");
                        }
                        let style = if index == state.selected {
                            Style::default().bg(theme.selection).fg(theme.foreground)
                        } else if applicable {
                            Style::default().fg(theme.foreground)
                        } else {
                            Style::default().fg(theme.muted)
                        };
                        lines.push(Line::from(Span::styled(row, style)));
                    }
                }
            }
            CompletionViewStatus::NotReady => {
                lines.push(Line::from("Language server warming — retry shortly"));
            }
            CompletionViewStatus::Stale => {
                lines.push(Line::from("Document changed — press F8 to refresh"));
            }
            CompletionViewStatus::Unsynced => {
                lines.push(Line::from("Server hasn't seen this revision yet"));
            }
            CompletionViewStatus::Unsupported => {
                lines.push(Line::from("Server has no completion provider"));
            }
            CompletionViewStatus::Error => {
                lines.push(Line::from(sanitize_completion_text(
                    state.error.as_deref().unwrap_or("completion failed"),
                    120,
                )));
            }
            CompletionViewStatus::Idle => return,
        }
        if state.truncated {
            lines.push(Line::from(Span::styled(
                "…list truncated",
                Style::default().fg(theme.warning),
            )));
        } else if state.incomplete {
            lines.push(Line::from(Span::styled(
                "…more may exist server-side",
                Style::default().fg(theme.warning),
            )));
        }
        let height = ((lines.len() + 2) as u16).min(area.height).min(16);
        let width = area.width.min(MAX_COMPLETION_POPUP_WIDTH);
        if height < 3 || width < 24 {
            return;
        }
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let block = Block::default()
            .title(format!(
                " Completion · {status} · ↑↓ select · Enter apply · Esc "
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.primary))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(rect);
        frame.render_widget(Clear, rect);
        frame.render_widget(block, rect);
        lines.truncate(inner.height as usize);
        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// The visible display-column slice of `line_index`, truncated to the
    /// viewport width.
    ///
    /// C002: examines at most the 32 KiB prefix window
    /// (`bounded_prefix_end`), never the whole logical line, so a
    /// pathologically long line costs a bounded window per frame. The
    /// truncation marker itself is added by [`Self::line_spans`]; this
    /// returns the text window only.
    fn line_slice(&self, line_index: usize, viewport: EditorViewport) -> String {
        let Some(start) = editor::line_start(self.snapshot, line_index) else {
            return String::new();
        };
        let end = editor::line_content_end(self.snapshot, line_index).unwrap_or(start);
        let window_end = editor::bounded_prefix_end(self.snapshot, start, end);
        let text = match self.snapshot.read_bytes(start, window_end) {
            Ok(text) => text,
            Err(_) => return String::new(),
        };
        let width = viewport.text_columns as usize;
        if viewport.column == 0 && display_width_str(&text) <= width {
            return text;
        }
        let mut out = String::new();
        let mut consumed = 0usize;
        for ch in text.chars() {
            let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if consumed + char_width > viewport.column + width {
                break;
            }
            if consumed >= viewport.column {
                out.push(ch);
            }
            consumed += char_width;
        }
        out
    }

    /// Build the styled spans for one row.
    ///
    /// Four layers resolve into one set of spans, in this order, each one
    /// overriding only the fields it owns:
    ///
    /// 1. **Syntax** — an LSP semantic token where a language server
    ///    supplied one, otherwise the built-in [`EditorHighlight`] scan. The
    ///    server wins because it knows the real grammar; the scan is the
    ///    floor that keeps files coloured on a machine with no server.
    /// 2. **Search** — a `/` match, background only.
    /// 3. **Selection** — a held Helix selection, background and foreground.
    /// 4. **Cursor** — the block in Normal, the bar in Insert.
    ///
    /// Every background layer pins the foreground to `theme.foreground` at
    /// the same time. A themed background with a syntax foreground chosen
    /// for a *different* background is exactly the dark-on-dark case that
    /// made selected and cursor-line text unreadable, so a selection that
    /// painted only its background was not a safe compromise — it was the
    /// same bug one layer down.
    fn line_spans(
        &mut self,
        line_index: usize,
        viewport: EditorViewport,
        base: Style,
        selection: &[std::ops::Range<usize>],
    ) -> Vec<Span<'static>> {
        use crate::tui::editor_lsp::utf16_column_to_byte;
        let Some(start) = editor::line_start(self.snapshot, line_index) else {
            return vec![];
        };
        let end = editor::line_content_end(self.snapshot, line_index).unwrap_or(start);
        // C002 bounded fallback: a line past the 32 KiB budget renders its
        // prefix as plain text with a visible marker. Per-token spans are
        // skipped on this path — mapping them would cost the whole line —
        // and ordinary short lines keep their exact highlighting below.
        if editor::bounded_prefix_end(self.snapshot, start, end) < end {
            // A whole-row highlight: the row's bytes are unavailable in full,
            // so a partial range would need per-byte slicing to be exact. The
            // row is a 32 KiB marker either way; highlighting all of it says
            // "selected" honestly and costs nothing.
            let whole_line_selected = selection
                .iter()
                .any(|range| range.start <= start && range.end > start);
            let base = if whole_line_selected {
                base.patch(Style::default().bg(self.theme.selection))
            } else {
                base
            };
            let mut spans = vec![Span::styled(self.line_slice(line_index, viewport), base)];
            spans.push(Span::styled(
                crate::tui::editor::TRUNCATED_LINE_SUFFIX,
                Style::default()
                    .fg(self.theme.warning)
                    .add_modifier(Modifier::BOLD),
            ));
            return spans;
        }
        let Ok(line) = self.snapshot.read_bytes(start, end) else {
            return vec![];
        };
        // Is this **row** selected?
        //
        // A line-mode selection is a row-level fact, and the byte range of an
        // empty line is a single terminator byte the row has no cell for. So
        // the range alone cannot say "this row is selected", and `x` on an
        // empty line highlighted nothing at all.
        let row_selected = self.presentation.selections.iter().any(|selection| {
            editor::line_selection_covers_line(self.snapshot, selection, line_index)
        });
        let mut display = 0usize;
        let mut visible_start = line.len();
        let mut visible_end = line.len();
        let mut started = false;
        let limit = viewport
            .column
            .saturating_add(viewport.text_columns as usize);
        for (byte, ch) in line.char_indices() {
            let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            let next = display.saturating_add(width);
            if !started && next > viewport.column {
                visible_start = byte;
                started = true;
            }
            if started && display < limit {
                visible_end = byte + ch.len_utf8();
            }
            display = next;
        }
        if !started && viewport.column == 0 {
            visible_start = 0;
            visible_end = line.len();
        } else if !started {
            return vec![Span::styled(String::new(), base)];
        }
        let visible_end = visible_end.max(visible_start).min(line.len());
        let mut boundaries = vec![visible_start, visible_end];
        // Foreground sources, in priority order: an LSP semantic token
        // first, the built-in scan second. `find` takes the first covering
        // range, so a server that knows the grammar overrides the scan
        // wherever the two agree, and the scan fills in everything the
        // server said nothing about.
        let mut syntax_ranges: Vec<(std::ops::Range<usize>, Style)> = Vec::new();
        for token in self
            .lsp
            .semantic_tokens
            .iter()
            .filter(|token| token.line as usize == line_index)
        {
            // Well-formed unknown token types render plain (`base`) while
            // valid neighbors keep their theme styles. This is what stops
            // one legitimate `variable`/`parameter`/`property` span from
            // erasing every other highlight on the line.
            let Some(style) = token_style(&self.theme, &token.token_type) else {
                continue;
            };
            let Some(token_end) = token.character.checked_add(token.length) else {
                continue;
            };
            let Some(token_start) = utf16_column_to_byte(&line, token.character) else {
                continue;
            };
            let Some(token_end) = utf16_column_to_byte(&line, token_end) else {
                continue;
            };
            let range = token_start.max(visible_start)..token_end.min(visible_end);
            if range.start < range.end {
                boundaries.push(range.start);
                boundaries.push(range.end);
                syntax_ranges.push((range, style));
            }
        }
        // Server-free layer. A no-op when a server is supplying tokens for
        // this line, and the only colouring there is when one is not.
        for span in self.highlight.spans_for_line(
            self.snapshot,
            self.path,
            &self.theme.code_theme,
            line_index,
        ) {
            let range = span.range.start.max(visible_start)..span.range.end.min(visible_end);
            if range.start < range.end {
                boundaries.push(range.start);
                boundaries.push(range.end);
                syntax_ranges.push((range, Style::default().fg(span.color)));
            }
        }
        // `/` matches, clipped to this row. Match offsets ascend, so a
        // partition point finds the first one inside the line in O(log n)
        // rather than walking every retained match for every visible row.
        let search_ranges = {
            let needle = self.presentation.search_line.len();
            if needle == 0 {
                Vec::new()
            } else {
                let absolute_start = start;
                let absolute_end = start + line.len();
                let first = self
                    .presentation
                    .search_matches
                    .partition_point(|offset| *offset < absolute_start);
                self.presentation
                    .search_matches
                    .iter()
                    .skip(first)
                    .take_while(|offset| **offset < absolute_end)
                    .map(|offset| {
                        let from = offset
                            .saturating_sub(start)
                            .clamp(visible_start, visible_end);
                        let to = (offset + needle)
                            .saturating_sub(start)
                            .clamp(visible_start, visible_end);
                        from..to
                    })
                    .filter(|range| range.start < range.end)
                    .collect::<Vec<_>>()
            }
        };
        for range in &search_ranges {
            boundaries.push(range.start);
            boundaries.push(range.end);
        }
        // Every selection head on this row, primary and extras alike. They
        // have to exist even when a head sits past the last character —
        // otherwise "the cursor is at the end of the line" is the one
        // position a reader cannot see.
        let head_cells = self.head_cells(start, &line, line_index, visible_start, visible_end);
        for (from, to, _) in &head_cells {
            boundaries.push(*from);
            boundaries.push(*to);
        }
        let cursor_style = self.cursor_style();
        let extra_head_style = self.extra_head_style();
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut spans: Vec<Span<'static>> = boundaries
            .windows(2)
            .filter_map(|pair| {
                let range = pair[0]..pair[1];
                if range.start == range.end {
                    return None;
                }
                // The only byte range that legitimately misses the line is a
                // head's virtual cell past its end; it renders as one space
                // rather than disappearing.
                let text = match line.get(range.clone()) {
                    Some(text) => text.to_string(),
                    None if head_cells.iter().any(|(from, _, _)| *from == range.start) => {
                        " ".to_string()
                    }
                    None => return None,
                };
                let style = syntax_ranges
                    .iter()
                    .find(|(token_range, _)| {
                        token_range.start <= range.start && token_range.end >= range.end
                    })
                    .map_or(base, |(_, token_style)| base.patch(*token_style));
                // Absolute offsets back to line-relative, then clipped to the
                // segment so a selection boundary can only colour part of it.
                // Membership is over the whole **set**: a byte inside any
                // member is selected, which is what makes two separate
                // highlighted regions read as two selections.
                let selected = row_selected
                    || selection.iter().any(|member| {
                        let from = member.start.saturating_sub(start);
                        let to = member.end.saturating_sub(start);
                        from < range.end && to > range.start
                    });
                let searched = search_ranges
                    .iter()
                    .any(|found| found.start <= range.start && found.end >= range.end);
                let style = if searched {
                    style.patch(
                        Style::default()
                            .bg(self.theme.selection_dim)
                            .fg(self.theme.foreground),
                    )
                } else {
                    style
                };
                let style = if selected {
                    style.patch(
                        Style::default()
                            .bg(self.theme.selection)
                            .fg(self.theme.foreground),
                    )
                } else {
                    style
                };
                // Heads are painted last so nothing can bury them: a
                // selection or a search hit that happened to cover a
                // cursor's own cell must not make the position unreadable.
                //
                // Each *patches* rather than replaces. Replacing dropped the
                // base outright, and since the Insert caret only carries a
                // foreground and a modifier, that punched an untinted
                // one-cell hole in the current line — the caret cell was the
                // one cell on the row that did not belong to the row.
                //
                // The primary wins over an extra head sharing its cell: it is
                // the one the keyboard is on, so it is the one that must stay
                // readable.
                let at_primary = head_cells
                    .iter()
                    .any(|(from, _, is_primary)| *is_primary && *from == range.start);
                let at_extra = head_cells
                    .iter()
                    .any(|(from, _, is_primary)| !*is_primary && *from == range.start);
                let style = if at_primary {
                    style.patch(cursor_style)
                } else if at_extra {
                    style.patch(extra_head_style)
                } else {
                    style
                };
                Some(Span::styled(text, style))
            })
            .collect();
        if row_selected {
            // The **whole row** is selected, not just the text on it. The
            // selection background runs to the end of the text area, so an
            // empty line — which has no text at all — is still visibly
            // selected rather than looking untouched.
            let painted: usize = spans.iter().map(|span| span.width()).sum();
            let filler = (viewport.text_columns as usize).saturating_sub(painted);
            if filler > 0 {
                spans.push(Span::styled(
                    " ".repeat(filler),
                    base.patch(
                        Style::default()
                            .bg(self.theme.selection)
                            .fg(self.theme.foreground),
                    ),
                ));
            }
        }
        spans
    }

    /// The cell a byte offset occupies on this row, as a line-relative byte
    /// range, or `None` when the offset is scrolled out of the horizontal
    /// window.
    ///
    /// A position past the last character on an otherwise fully visible row
    /// still owns a cell: it is rendered as a single space one column past the
    /// end of the text. That is the position a reader reaches most often — the
    /// end of a line they are about to append to — so it is the one that must
    /// never be invisible.
    fn cell_for_offset(
        &self,
        byte: usize,
        line_start: usize,
        line: &str,
        visible_start: usize,
        visible_end: usize,
    ) -> Option<(usize, usize)> {
        let at = byte.saturating_sub(line_start).min(line.len());
        // Past the last character on an otherwise fully visible row: the
        // virtual cell. Anywhere else it means the position is simply scrolled
        // out of the window, and inventing a cell there would paint a ghost
        // marker somewhere the viewport has already decided not to show.
        if at == visible_end && visible_end < line.len() {
            return None;
        }
        if at < visible_start {
            return None;
        }
        let end = line[at..]
            .chars()
            .next()
            .map_or(at + 1, |ch| at + ch.len_utf8());
        Some((at, end.max(at + 1)))
    }

    /// Every selection head that lands on this row, as line-relative byte
    /// ranges paired with whether that head **is the cursor**.
    ///
    /// The flag means "the keyboard is here", not "this member is primary".
    /// Those coincide on every Helix path — `cursor_byte` *is* the primary's
    /// head — and they come apart only when a motion moved the cursor without
    /// consulting the selection machine, such as a goto. Painting a member
    /// head with the cursor's reverse video in that state would put a second
    /// block exactly where the reader reads "the keyboard is here", which is
    /// the one thing a cursor must never do. So every head that is not the
    /// cursor is drawn as an extra, and the cursor alone carries cursor styling.
    fn head_cells(
        &self,
        line_start: usize,
        line: &str,
        line_index: usize,
        visible_start: usize,
        visible_end: usize,
    ) -> Vec<(usize, usize, bool)> {
        let mut cells: Vec<(usize, usize, bool)> = self
            .presentation
            .selections
            .iter()
            .filter(|selection| {
                editor::cursor_line(self.snapshot, selection.head.byte) == line_index
                    // The cursor's own cell joins this partition below, so a
                    // head that coincides with it must not be drawn twice.
                    && selection.head.byte != self.cursor_byte
            })
            .filter_map(|selection| {
                self.cell_for_offset(
                    selection.head.byte,
                    line_start,
                    line,
                    visible_start,
                    visible_end,
                )
                .map(|(from, to)| (from, to, false))
            })
            .collect();
        // The cursor's own cell joins the same partition as the heads, so it
        // is folded in here rather than handled by a separate code path — two
        // code paths for "which cell is marked" is how they drift apart.
        if let Some((from, to)) =
            self.cursor_cell(line_start, line, line_index, visible_start, visible_end)
        {
            cells.push((from, to, true));
        }
        cells
    }

    /// The cell the cursor occupies on this row, as a line-relative byte
    /// range, or `None` when the cursor is on another line or scrolled out of
    /// the horizontal window.
    fn cursor_cell(
        &self,
        line_start: usize,
        line: &str,
        line_index: usize,
        visible_start: usize,
        visible_end: usize,
    ) -> Option<(usize, usize)> {
        if line_index != editor::cursor_line(self.snapshot, self.cursor_byte) {
            return None;
        }
        self.cell_for_offset(
            self.cursor_byte,
            line_start,
            line,
            visible_start,
            visible_end,
        )
    }

    /// How an **extra** selection head paints itself.
    ///
    /// Distinct from both neighbours by construction. The primary cursor swaps
    /// the theme's own foreground and background; a selected cell wears
    /// `theme.selection` as its background. An extra head therefore takes the
    /// accent colour as its background, which is already the "you are here"
    /// signal the current-line gutter uses — a reader who cannot tell an extra
    /// cursor from a highlight has no way to know how many selections they are
    /// about to destroy with `d`.
    fn extra_head_style(&self) -> Style {
        Style::default()
            .fg(self.theme.background)
            .bg(self.theme.primary)
            .add_modifier(Modifier::BOLD)
    }

    /// How the cursor paints itself.
    ///
    /// **Normal** is reverse video. The cursor must stay legible over
    /// whatever the highlighter painted underneath it, and a fixed accent
    /// background cannot promise that — a dark syntax foreground over a dark
    /// accent is the same unreadable pairing as the selection bug, one layer
    /// down. Swapping the theme's own foreground and background guarantees
    /// contrast for every token color, every theme, without the renderer
    /// having to know which of them is light.
    ///
    /// **Insert** is a thin bar. A block there would cover the character
    /// being typed over — the one thing Insert mode exists to show.
    fn cursor_style(&self) -> Style {
        if self.buffer_focused {
            Style::default()
                .fg(self.theme.primary)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
                .fg(self.theme.background)
                .bg(self.theme.foreground)
        }
    }
}

fn severity_label(severity: u32) -> &'static str {
    match severity {
        1 => "error",
        2 => "warning",
        3 => "info",
        _ => "hint",
    }
}

/// Map one LSP semantic-token class to a theme slot.
///
/// Returns `None` for well-formed but unstyled token types, which the caller
/// renders as plain text (`base`) while keeping valid neighbors. Only the
/// style mapping returns `None`; structural validation in
/// `crate::tui::editor_lsp::validate_semantic_tokens` never rejects a token
/// for its type name.
///
/// All colors come from `Theme` so highlighting stays legible on both dark
/// and light themes. No hardcoded ANSI colors: patching a theme slot over
/// `base` preserves the current-line background and diagnostic gutter while
/// changing only the foreground.
pub(crate) fn token_style(theme: &Theme, token_type: &str) -> Option<Style> {
    Some(match token_type {
        "keyword" | "modifier" => Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD),
        "string" | "regexp" => Style::default().fg(theme.success),
        "number" => Style::default().fg(theme.warning),
        "comment" => Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::ITALIC),
        "function" | "method" | "macro" => Style::default().fg(theme.secondary),
        "type" | "class" | "enum" | "struct" | "interface" | "typeParameter" | "namespace" => {
            Style::default().fg(theme.primary)
        }
        "variable" | "parameter" | "property" | "enumMember" | "event" | "operator"
        | "decorator" | "label" => Style::default().fg(theme.foreground),
        _ => return None,
    })
}

fn display_width_str(text: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    UnicodeWidthStr::width(text)
}

/// What the panel says will happen to the change.
///
/// A preview that carries a candidate is **ready to apply**: the review is
/// open on it, and the review is where the user decides. A preview without
/// one is still a preview — and a *truncated* preview says so, because a
/// partial change is not the change that was previewed, and offering it as
/// applicable would be a lie.
fn preview_disposition(truncated: bool, has_candidate: bool) -> &'static str {
    if has_candidate {
        "ready to apply"
    } else if truncated {
        "not applied — the preview is truncated"
    } else {
        "not applied"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::document_session::TuiDocumentPresentation;
    use crate::tui::editor::FindNote;
    use codegg_document::DocumentBuffer;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Widget chrome that varies per test. The snapshot and presentation
    /// are supplied separately so each test can vary either without
    /// duplicating the terminal plumbing.
    struct Options<'a> {
        status: &'a str,
        notice: Option<&'a str>,
        find_note: &'a str,
        mode: EditorMode,
        buffer_focused: bool,
        pending_prefix: EditorPrefix,
        command_active: bool,
        command_line: &'a str,
        command_selection: usize,
        search_active: bool,
        search_line: &'a str,
        search_count: usize,
        search_partial: bool,
        /// The held selection **set**, for tests that need the renderer's
        /// third background layer — or its extra head markers — painted.
        selections: Vec<crate::tui::helix_selection::Selection<editor::EditorPoint>>,
        /// Retained `/` match offsets, for the search highlight layer.
        search_matches: Vec<usize>,
        /// The history cursor's position and the history's length, as
        /// `(position, length)`.
        ///
        /// The entries themselves are built through the real history API, so
        /// a test that asks for a position gets a history that could have
        /// produced it.
        history: (usize, usize),
        writable: bool,
        busy: bool,
        scope: codegg_protocol::document::DocumentScope,
        /// The LSP state the widget renders.
        ///
        /// Owned rather than borrowed so a test can seed a **panel** — the
        /// read results the widget draws — without reaching into the
        /// widget's own fields.
        lsp: crate::tui::editor_lsp::EditorLspState,
    }

    fn defaults<'a>() -> Options<'a> {
        Options {
            status: "synced",
            notice: None,
            find_note: "",
            mode: EditorMode::Normal,
            buffer_focused: true,
            pending_prefix: EditorPrefix::Idle,
            command_active: false,
            command_line: "",
            command_selection: 0,
            search_active: false,
            search_line: "",
            search_count: 0,
            search_partial: false,
            selections: Vec::new(),
            search_matches: Vec::new(),
            history: (0, 0),
            writable: true,
            busy: false,
            scope: codegg_protocol::document::DocumentScope::Workspace,
            lsp: crate::tui::editor_lsp::EditorLspState::default(),
        }
    }

    /// Give the presentation a history of `length` entries and park the
    /// cursor at `position`.
    ///
    /// The entries are built through the real history API, so a test that
    /// asks for a position gets a history an ordinary editing session could
    /// have produced — not a hand-set field that the widget would read but
    /// nothing could have created.
    fn seed_history(
        presentation: &mut TuiDocumentPresentation,
        (position, length): (usize, usize),
    ) {
        use codegg_document::{TextEdit, TextTransaction};
        for _ in 0..length {
            editor::push_undo(
                presentation,
                TextTransaction::new(vec![TextEdit::new(0..0, "x")]),
                TextTransaction::new(vec![TextEdit::new(0..1, "")]),
                1,
                0,
                Vec::new(),
            );
        }
        presentation.history_cursor = position.min(presentation.history.len());
    }

    /// Paint one editor frame and return the rendered rows as plain text.
    fn render(
        text: &str,
        width: u16,
        height: u16,
        cursor: usize,
        options: Options<'_>,
    ) -> Vec<String> {
        let snapshot = DocumentBuffer::new(text).snapshot();
        let mut presentation = TuiDocumentPresentation {
            cursor_byte: cursor,
            selections: options.selections,
            search_line: options.search_line.to_string(),
            search_matches: options.search_matches.clone(),
            ..TuiDocumentPresentation::default()
        };
        seed_history(&mut presentation, options.history);
        let lsp = &options.lsp;
        let completion = crate::tui::editor_completion::EditorCompletionState::default();
        let mut highlight = crate::tui::editor_highlight::EditorHighlight::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                EditorWidget {
                    path: "src/lib.rs",
                    status: options.status,
                    notice: options.notice,
                    find_note: options.find_note,
                    mode: options.mode,
                    buffer_focused: options.buffer_focused,
                    pending_prefix: options.pending_prefix,
                    command_active: options.command_active,
                    command_line: options.command_line,
                    command_selection: options.command_selection,
                    search_active: options.search_active,
                    search_line: options.search_line,
                    search_count: options.search_count,
                    search_partial: options.search_partial,
                    scope: options.scope,
                    cursor_byte: presentation.cursor_byte,
                    writable: options.writable,
                    busy: options.busy,
                    snapshot: &snapshot,
                    lsp,
                    completion: &completion,
                    highlight: &mut highlight,
                    presentation: &mut presentation,
                    theme: Arc::new(Theme::default()),
                }
                .render(frame, Rect::new(0, 0, width, height));
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    /// The rendered rows of a painted frame, as plain text.
    ///
    /// The same view [`render`] returns, read back from a buffer a test also
    /// wants the **styles** of — so a test can assert on the glyphs and on the
    /// paint at the same time.
    fn lines_of(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    fn paint(text: &str, width: u16, height: u16, cursor: usize) -> String {
        render(text, width, height, cursor, defaults()).join("\n")
    }

    /// Paint one frame and hand back the raw buffer, so a test can assert on
    /// colors rather than on glyphs.
    ///
    /// Every readability bug this file's tests cover was invisible to
    /// `paint`: dark-on-dark text renders exactly like readable text, because
    /// the *glyphs* are right and only the colors are wrong.
    fn render_buffer(
        text: &str,
        width: u16,
        height: u16,
        cursor: usize,
        options: Options<'_>,
    ) -> ratatui::buffer::Buffer {
        let snapshot = DocumentBuffer::new(text).snapshot();
        let mut presentation = TuiDocumentPresentation {
            cursor_byte: cursor,
            selections: options.selections,
            search_line: options.search_line.to_string(),
            search_matches: options.search_matches.clone(),
            ..TuiDocumentPresentation::default()
        };
        seed_history(&mut presentation, options.history);
        let lsp = &options.lsp;
        let completion = crate::tui::editor_completion::EditorCompletionState::default();
        let mut highlight = crate::tui::editor_highlight::EditorHighlight::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                EditorWidget {
                    path: "src/lib.rs",
                    status: options.status,
                    notice: options.notice,
                    find_note: options.find_note,
                    mode: options.mode,
                    buffer_focused: options.buffer_focused,
                    pending_prefix: options.pending_prefix,
                    command_active: options.command_active,
                    command_line: options.command_line,
                    command_selection: options.command_selection,
                    search_active: options.search_active,
                    search_line: options.search_line,
                    search_count: options.search_count,
                    search_partial: options.search_partial,
                    scope: options.scope,
                    cursor_byte: presentation.cursor_byte,
                    writable: options.writable,
                    busy: options.busy,
                    snapshot: &snapshot,
                    lsp,
                    completion: &completion,
                    highlight: &mut highlight,
                    presentation: &mut presentation,
                    theme: Arc::new(Theme::default()),
                }
                .render(frame, Rect::new(0, 0, width, height));
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// Relative luminance of a color, used to assert a pairing is *readable*
    /// rather than asserting which specific color was chosen.
    fn luminance(color: ratatui::style::Color) -> f32 {
        let (r, g, b) = match color {
            ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
            ratatui::style::Color::Reset => return 1.0,
            _ => return 1.0,
        };
        (0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)) / 255.0
    }

    /// The colors and modifiers a body cell was painted with.
    #[derive(Debug, Clone, Copy)]
    struct CellStyle {
        fg: ratatui::style::Color,
        bg: ratatui::style::Color,
        modifier: Modifier,
    }

    /// The painted style of a body cell.
    ///
    /// Row 0 is the block's top border and the two `CHROME_ROWS` sit at the
    /// *bottom*, so body row 0 is buffer row 1 and the banner is
    /// `height - 3`. Text starts at `inner.x + gutter` = 1 + 3 = 4.
    fn body_style(buffer: &ratatui::buffer::Buffer, row: u16, column: u16) -> CellStyle {
        let cell = &buffer[(4 + column, row + 1)];
        CellStyle {
            fg: cell.fg,
            bg: cell.bg,
            modifier: cell.modifier,
        }
    }

    /// Buffer row of the editor's single chrome row: the path and the status
    /// cluster, or the `:` / `/` line while one is open.
    const fn banner_row(height: u16) -> u16 {
        height - 2
    }

    /// Every painted body cell, in reading order.
    ///
    /// Tests that care about *a* cell rather than a specific one use this
    /// instead of hardcoding `(x, y)`: the gutter width, the block border,
    /// and the chrome rows are all things the layout can move, and a test
    /// pinned to a literal coordinate then fails on the layout rather than on
    /// the behavior it was written to pin.
    fn body_cells(buffer: &ratatui::buffer::Buffer, height: u16) -> Vec<CellStyle> {
        let mut cells = Vec::new();
        for y in 1..height.saturating_sub(2) {
            for x in TEXT_X..buffer.area.width.saturating_sub(1) {
                let cell = &buffer[(x, y)];
                cells.push(CellStyle {
                    fg: cell.fg,
                    bg: cell.bg,
                    modifier: cell.modifier,
                });
            }
        }
        cells
    }

    /// Left edge of the text columns: the block's inner edge plus the
    /// 3-column line-number gutter.
    const TEXT_X: u16 = 4;

    /// Buffer row of the editor's chrome row. There is **one**: the line
    /// number and the column are gone, and the search match count rides on
    /// the cluster with everything else.
    const fn readout_row(height: u16) -> u16 {
        height - 2
    }

    /// A held selection covering `byte_range`, as the renderer resolves it.
    fn selection_over(
        snapshot: &codegg_document::DocumentSnapshot,
        byte_range: std::ops::Range<usize>,
    ) -> crate::tui::helix_selection::Selection<editor::EditorPoint> {
        crate::tui::helix_selection::Selection {
            anchor: editor::EditorPoint {
                byte: byte_range.start,
                line: editor::cursor_line(snapshot, byte_range.start),
            },
            head: editor::EditorPoint {
                byte: byte_range.end,
                line: editor::cursor_line(snapshot, byte_range.end),
            },
            mode: crate::tui::helix_selection::SelectionMode::Char,
            growing: false,
        }
    }

    /// A held **line** selection over `line_index`, as the renderer resolves
    /// it.
    ///
    /// The line selection is a row-level fact: an empty line's byte range is
    /// a single terminator byte the row has no cell for, so the renderer
    /// cannot learn "this row is selected" from the range alone.
    fn line_selection_over(
        snapshot: &codegg_document::DocumentSnapshot,
        line_index: usize,
    ) -> crate::tui::helix_selection::Selection<editor::EditorPoint> {
        let byte = editor::line_start(snapshot, line_index).unwrap_or(0);
        crate::tui::helix_selection::Selection {
            anchor: editor::EditorPoint {
                byte,
                line: line_index,
            },
            head: editor::EditorPoint {
                byte,
                line: line_index,
            },
            mode: crate::tui::helix_selection::SelectionMode::Line,
            growing: false,
        }
    }

    /// `x` on an **empty** line has to look selected.
    ///
    /// An empty line has no bytes to tint, so a renderer that resolves
    /// "selected" from the byte range paints nothing at all: the reader
    /// presses `x` on a blank line and the line does not change. The row
    /// question — "is this **row** selected?" — is a different question from
    /// the byte question, and it is the one that decides what the reader sees.
    #[test]
    fn a_line_selection_paints_an_empty_row_across_its_whole_width() {
        let theme = Theme::default();
        // Line 1 is empty: "alpha", "", "beta".
        let snapshot = DocumentBuffer::new("alpha\n\nbeta").snapshot();
        let buffer = render_buffer(
            "alpha\n\nbeta",
            30,
            8,
            0,
            Options {
                buffer_focused: false,
                selections: vec![line_selection_over(&snapshot, 1)],
                ..defaults()
            },
        );
        // The selection background runs to the end of the text area, so the
        // blank row reads as selected instead of untouched.
        //
        // `body_style` counts rows **within the text area**, so the editor's
        // second line is row 1 and its first is row 0.
        let row = 1;
        let painted = (0..20)
            .map(|column| body_style(&buffer, row, column))
            .filter(|style| style.bg == theme.selection)
            .count();
        assert!(
            painted >= 10,
            "the empty row must carry the selection background across its \
             width, got {painted} tinted cells: {buffer:?}"
        );
        // And the line above it — not selected — must not be tinted.
        let untouched = (0..5)
            .map(|column| body_style(&buffer, 0, column))
            .filter(|style| style.bg == theme.selection)
            .count();
        assert_eq!(
            untouched, 0,
            "a line that is not selected must not be tinted: {buffer:?}"
        );
    }

    /// The row-level question must not leak into the byte-level one: a
    /// character selection still highlights only the bytes it covers.
    #[test]
    fn a_character_selection_still_highlights_only_its_bytes() {
        let theme = Theme::default();
        let snapshot = DocumentBuffer::new("alpha beta").snapshot();
        let buffer = render_buffer(
            "alpha beta",
            30,
            8,
            5,
            Options {
                buffer_focused: false,
                selections: vec![selection_over(&snapshot, 0..5)],
                ..defaults()
            },
        );
        let selected = (0..10)
            .map(|column| body_style(&buffer, 0, column))
            .filter(|style| style.bg == theme.selection)
            .count();
        assert_eq!(
            selected, 5,
            "a character selection tints its own bytes and nothing else: \
             {buffer:?}"
        );
    }

    #[test]
    fn every_members_highlight_is_painted_not_just_the_primarys() {
        let theme = Theme::default();
        let snapshot = DocumentBuffer::new("alpha beta gamma").snapshot();
        // Two disjoint members, with the cursor parked on the *primary*'s
        // head — the case where a renderer that still resolved one range would
        // tint exactly one of the two.
        let buffer = render_buffer(
            "alpha beta gamma",
            30,
            8,
            // Parked on the space at byte 5: a cursor is painted last, so
            // parking it on byte 0 would have asserted the cursor's own style
            // and passed for the wrong reason.
            5,
            Options {
                buffer_focused: false,
                selections: vec![
                    selection_over(&snapshot, 0..1),
                    selection_over(&snapshot, 6..10),
                ],
                ..defaults()
            },
        );
        // Text byte N is body column N; the gutter is not counted.
        assert_eq!(
            body_style(&buffer, 0, 0).bg,
            theme.selection,
            "the primary's member paints"
        );
        assert_eq!(
            body_style(&buffer, 0, 6).bg,
            theme.selection,
            "an extra member paints too, or the reader cannot see how many \
             selections `d` is about to destroy"
        );
        assert_ne!(
            body_style(&buffer, 0, 3).bg,
            theme.selection,
            "a byte outside every member stays untinted"
        );
    }

    #[test]
    fn an_extra_head_is_marked_differently_from_the_cursor() {
        let theme = Theme::default();
        let snapshot = DocumentBuffer::new("alpha beta gamma").snapshot();
        let mut extra = selection_over(&snapshot, 6..10);
        extra.head = editor::EditorPoint { byte: 10, line: 0 };
        let buffer = render_buffer(
            "alpha beta gamma",
            30,
            8,
            0,
            Options {
                buffer_focused: false,
                selections: vec![selection_over(&snapshot, 0..1), extra],
                ..defaults()
            },
        );
        let cursor = body_style(&buffer, 0, 0);
        // Byte 10 is the space after "beta" — the extra member's head.
        let head = body_style(&buffer, 0, 10);
        assert_eq!(cursor.bg, theme.foreground, "the cursor is reverse video");
        assert_eq!(
            head.bg, theme.primary,
            "an extra head carries the accent background"
        );
        assert_ne!(
            cursor.bg, head.bg,
            "a reader who cannot tell a cursor from an extra has no way to \
             know where the keyboard is"
        );
    }

    #[test]
    fn renders_line_numbers_and_the_line_content() {
        let joined = paint("alpha\nbeta\ngamma", 30, 8, 0);
        assert!(joined.contains("alpha"), "{joined}");
        assert!(joined.contains("beta"), "{joined}");
        assert!(joined.contains("gamma"), "{joined}");
        assert!(
            !joined.contains("ln 1"),
            "the line number is gone from the chrome: {joined}"
        );
    }

    #[test]
    fn the_viewport_shows_only_the_visible_window() {
        let text = (0..50).map(|i| format!("line{i}\n")).collect::<String>();
        let joined = paint(&text, 30, 8, 0);
        assert!(joined.contains("line0"), "{joined}");
        assert!(
            !joined.contains("line20"),
            "must not render off-screen lines: {joined}"
        );
    }

    #[test]
    fn moving_the_cursor_scrolls_the_viewport() {
        let text = (0..50).map(|i| format!("line{i}\n")).collect::<String>();
        let offset = text.find("line40").unwrap();
        let joined = paint(&text, 30, 8, offset);
        assert!(joined.contains("line40"), "{joined}");
    }

    #[test]
    fn a_read_only_attachment_says_so() {
        // Wide enough for the whole readout row. The scope badge sits before
        // the advisory on purpose: on a narrow box the badge and the cursor
        // position survive truncation while the trailing hint is what gets
        // cut, because the badge is the safety-relevant half.
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                writable: false,
                status: "read-only",
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("read-only"), "{joined}");
        assert!(joined.contains("edits refused"), "{joined}");
    }

    /// The scope badge is the only thing on screen that tells a user whether
    /// `:wq` reaches this project or every project on the machine, and the
    /// path reads identically at either scope. It must therefore always be
    /// rendered, and it must change with the scope.
    #[test]
    fn the_status_row_always_names_the_authority_root() {
        for (scope, expected) in [
            (
                codegg_protocol::document::DocumentScope::Workspace,
                "[project]",
            ),
            (
                codegg_protocol::document::DocumentScope::GlobalAgent,
                "[global]",
            ),
        ] {
            let joined = render(
                "content",
                60,
                8,
                0,
                Options {
                    scope,
                    ..defaults()
                },
            )
            .join("\n");
            assert!(
                joined.contains(expected),
                "{scope:?} must show {expected}:\n{joined}"
            );
            // And never the other one — a badge that always rendered both
            // would carry no information.
            let other = if expected == "[project]" {
                "[global]"
            } else {
                "[project]"
            };
            assert!(
                !joined.contains(other),
                "{scope:?} must not show {other}:\n{joined}"
            );
        }
    }

    /// The editor's single chrome row: the path on the left, the status
    /// cluster on the right.
    ///
    /// It is the **last** row of the block, so the assertion does not depend
    /// on the chrome height or on where the block's borders are.
    fn banner(lines: &[String]) -> String {
        // The last rendered line is the block's **bottom border**; the
        // chrome row is the one inside it.
        lines.iter().rev().nth(1).cloned().unwrap_or_default()
    }

    #[test]
    fn a_surfaced_notice_replaces_the_status_line_and_appends_the_pending_prefix() {
        let lines = render(
            "content",
            60,
            8,
            0,
            Options {
                notice: Some("edit queue is full"),
                pending_prefix: EditorPrefix::Goto,
                ..defaults()
            },
        );
        let banner = banner(&lines);
        assert!(banner.contains("edit queue is full"), "{banner}");
        assert!(
            !banner.contains("synced"),
            "banner status is replaced: {banner}"
        );
        assert!(
            banner.contains("edit queue is full g"),
            "banner and pending prefix share the row: {banner}"
        );
    }

    #[test]
    fn a_clean_status_shows_in_the_banner() {
        let lines = render("content", 60, 8, 0, defaults());
        let banner = banner(&lines);
        assert!(banner.contains("synced"), "{banner}");
    }

    /// The two "the cursor did not move" reasons must be distinguishable on
    /// screen, and neither may appear when the find landed.
    ///
    /// They are one cursor position and two completely different facts, so a
    /// user told nothing has to guess between "that character is not ahead of
    /// you" and "I stopped looking". The common case — a find that worked —
    /// has to stay silent, or every ordinary keystroke would leave debris on
    /// the row.
    #[test]
    fn the_find_disclosure_distinguishes_absent_from_bounded_and_is_silent_on_success() {
        let cases = [
            (FindNote::None, false, false),
            (FindNote::Absent('z'), true, false),
            (FindNote::Bounded('z'), true, true),
        ];
        for (note, expect_visible, expect_bound) in cases {
            let lines = render(
                "content",
                60,
                8,
                0,
                Options {
                    find_note: &note.label(),
                    ..defaults()
                },
            );
            let banner = banner(&lines);
            if expect_visible {
                assert!(
                    banner.contains('z'),
                    "the note must name the target: {banner}"
                );
            } else {
                assert!(
                    !banner.contains('z'),
                    "a landed find must leave the row clean: {banner}"
                );
            }
            assert_eq!(
                banner.contains("MiB"),
                expect_bound,
                "only the bounded note states the bound: {banner}"
            );
        }

        // The two never render identically — that is the whole point.
        assert_ne!(
            FindNote::Absent('z').label(),
            FindNote::Bounded('z').label()
        );
    }

    /// An armed `f` and the disclosure share one row, in that order: the
    /// indicator answers "what is waiting", the note answers "what happened
    /// last", and conflating them would leave the user unable to tell which
    /// is which.
    #[test]
    fn the_find_disclosure_and_the_pending_prefix_share_the_banner_row() {
        let lines = render(
            "content",
            60,
            8,
            0,
            Options {
                find_note: &FindNote::Absent('z').label(),
                pending_prefix: EditorPrefix::FindNext,
                ..defaults()
            },
        );
        let row = banner(&lines);
        // The bordered row is padded out to the pane width, so the banner
        // text ends *before* a run of spaces and the right-hand border.
        let text = row.trim_end().trim_end_matches('│').trim_end();
        assert!(
            text.ends_with(" f"),
            "the pending indicator is last on the row: {text:?}"
        );
        let note_at = text
            .find("no 'z'")
            .expect("the disclosure must render on the same row");
        assert!(
            note_at < text.len() - 1,
            "the note precedes the pending indicator: {text:?}"
        );
    }

    /// The modal lines own the banner while they are open, and suppress the
    /// note — a disclosure about a find that happened before them would read
    /// as an answer to the question the line is asking.
    #[test]
    fn an_open_modal_line_suppresses_the_find_disclosure() {
        let note = FindNote::Absent('z').label();

        let lines = render(
            "content",
            60,
            8,
            0,
            Options {
                command_active: true,
                command_line: "wq",
                find_note: &note,
                ..defaults()
            },
        );
        let row = banner(&lines);
        assert!(row.contains(":wq"), "the `:` line owns the banner: {row}");
        assert!(
            !row.contains("no 'z'"),
            "a stale find note must not sit on the `:` line: {row}"
        );

        let lines = render(
            "content",
            60,
            8,
            0,
            Options {
                search_active: true,
                search_line: "target",
                find_note: &note,
                ..defaults()
            },
        );
        let row = banner(&lines);
        assert!(
            row.contains("/target"),
            "the `/` line owns the banner: {row}"
        );
        assert!(
            !row.contains("no 'z'"),
            "a stale find note must not sit on the `/` line: {row}"
        );
    }

    #[test]
    fn a_conflict_draft_is_visible_in_the_banner() {
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                status: "disk conflict \u{2014} draft kept",
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("disk conflict"), "{joined}");
        assert!(joined.contains("draft kept"), "{joined}");
    }

    #[test]
    fn a_gone_document_says_the_draft_is_kept() {
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                status: "document gone \u{2014} draft kept",
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("document gone"), "{joined}");
        assert!(joined.contains("draft kept"), "{joined}");
    }

    #[test]
    fn a_disconnected_transport_says_the_draft_is_kept() {
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                status: "disconnected \u{2014} draft kept",
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("disconnected"), "{joined}");
    }

    #[test]
    fn an_empty_document_renders_a_placeholder() {
        let joined = paint("", 30, 8, 0);
        assert!(joined.contains("empty document"), "{joined}");
    }

    #[test]
    fn a_narrow_area_does_not_panic() {
        for (width, height) in [(1u16, 1u16), (1, 10), (10, 1), (2, 2), (3, 3)] {
            let lines = render("abc\ndef", width, height, 1, defaults());
            assert_eq!(lines.len(), height as usize, "{width}x{height}");
        }
    }

    #[test]
    fn a_very_long_line_is_truncated_to_the_viewport() {
        let text = "x".repeat(5000);
        let lines = render(&text, 30, 8, 0, defaults());
        let widest = lines
            .iter()
            .map(|line| line.trim_end().chars().count())
            .max()
            .unwrap_or(0);
        assert!(
            widest <= 30,
            "per-frame render must stay viewport-bounded: {widest}"
        );
    }

    #[test]
    fn per_frame_work_is_bounded_by_the_viewport_not_the_document() {
        let text = (0..20_000)
            .map(|i| format!("line{i}\n"))
            .collect::<String>();
        let lines = render(&text, 40, 10, 0, defaults());
        let visible = lines.iter().filter(|line| line.contains("line")).count();
        assert!(
            visible <= 10,
            "at most one row per viewport line: {visible}"
        );
        assert!(lines.iter().any(|line| line.contains("line0")));
    }

    // ── C002 huge-line degradation ────────────────────────────────────

    fn mib_of_x(megabytes: usize) -> String {
        "x".repeat(megabytes * 1024 * 1024)
    }

    /// Frame input cost for `text` the way the renderer accounts it: the
    /// deterministic budget from `measure_frame_cost`, not wall-clock time.
    fn frame_cost(
        text: &str,
        first_line: usize,
        visible_lines: usize,
    ) -> crate::tui::editor::FrameCost {
        let snapshot = DocumentBuffer::new(text).snapshot();
        crate::tui::editor::measure_frame_cost(&snapshot, first_line, visible_lines)
    }

    #[test]
    fn a_1mib_single_line_renders_a_marked_prefix_within_budget() {
        let text = mib_of_x(1);
        let cost = frame_cost(&text, 0, 24);
        assert!(
            cost.total_bytes <= crate::tui::editor::MAX_FRAME_SOURCE_BYTES,
            "frame cost {} exceeds 256 KiB",
            cost.total_bytes
        );
        assert_eq!(cost.truncated_lines, 1);

        // Cursor at the start: the prefix is visible and honestly marked.
        let joined = paint(&text, 80, 24, 0);
        assert!(
            joined.contains("too long"),
            "a truncated line must be visibly marked, got:\n{joined}"
        );
        for line in joined.lines() {
            assert!(
                line.chars().count() <= 80,
                "every row stays viewport-bounded: {line:?}"
            );
        }
    }

    #[test]
    fn cursor_at_the_far_right_of_a_huge_line_stays_bounded_and_marked() {
        let text = mib_of_x(1);
        let cursor = text.len();
        let cost = frame_cost(&text, 0, 24);
        assert!(cost.total_bytes <= crate::tui::editor::MAX_FRAME_SOURCE_BYTES);

        // The cursor byte is past the render window; the frame must still
        // render within budget and say the line is truncated rather than
        // silently showing a prefix as the whole line.
        let joined = paint(&text, 80, 24, cursor);
        assert!(
            joined.contains("too long"),
            "far-right cursor on a huge line must stay marked, got:\n{joined}"
        );
    }

    #[test]
    fn cursor_in_the_middle_of_a_huge_line_stays_bounded_and_marked() {
        let text = mib_of_x(2);
        let joined = paint(&text, 80, 24, text.len() / 2);
        assert!(
            joined.contains("too long"),
            "mid-line cursor on a huge line must stay marked, got:\n{joined}"
        );
        let cost = frame_cost(&text, 0, 24);
        assert!(cost.total_bytes <= crate::tui::editor::MAX_FRAME_SOURCE_BYTES);
    }

    #[test]
    fn a_viewport_of_huge_lines_stays_within_the_frame_budget() {
        let text = (0..30)
            .map(|_| "y".repeat(1024 * 1024))
            .collect::<Vec<_>>()
            .join("\n");
        let cost = frame_cost(&text, 0, 30);
        assert!(
            cost.total_bytes <= crate::tui::editor::MAX_FRAME_SOURCE_BYTES,
            "30 huge lines must cost at most 256 KiB, got {}",
            cost.total_bytes
        );
        // Rendering must not panic and must mark the frame partial.
        let joined = paint(&text, 80, 24, 0);
        assert!(
            joined.contains("too long"),
            "a frame of huge lines must be marked, got:\n{joined}"
        );
    }

    #[test]
    fn a_1x1_terminal_with_a_huge_line_does_not_panic_and_marks() {
        let text = mib_of_x(1);
        let lines = render(&text, 1, 1, 0, defaults());
        assert_eq!(lines.len(), 1);
        let cost = frame_cost(&text, 0, 1);
        assert!(cost.total_bytes <= crate::tui::editor::MAX_FRAME_SOURCE_BYTES);
    }

    #[test]
    fn tabs_and_wide_unicode_inside_the_budget_render_exactly() {
        // Inside the budget nothing changes: tabs, wide CJK, and combining
        // marks render with the same widths as before C002.
        let text = "a\tb汉e\u{301}f";
        let joined = paint(text, 40, 8, 0);
        assert!(joined.contains('a'), "{joined}");
        assert!(joined.contains('b'), "{joined}");
        assert!(joined.contains('汉'), "{joined}");
        assert!(!joined.contains("too long"), "{joined}");
        let cost = frame_cost(text, 0, 8);
        assert_eq!(cost.truncated_lines, 0);
    }

    #[test]
    fn the_gutter_width_does_not_change_as_the_cursor_moves() {
        let text = (0..120).map(|i| format!("line{i}\n")).collect::<String>();
        let content_column = |rendered: &str| {
            rendered
                .lines()
                .filter(|line| line.trim_start().starts_with("line"))
                .map(|line| line.len() - line.trim_start().len())
                .min()
                .unwrap_or(usize::MAX)
        };
        let first = paint(&text, 40, 10, 0);
        let middle = text.find("line60").unwrap();
        let later = paint(&text, 40, 10, middle);
        assert_eq!(
            content_column(&first),
            content_column(&later),
            "gutter must not jitter as the cursor moves"
        );
    }

    #[test]
    fn the_title_carries_nothing() {
        // The title used to spell out NORMAL/INSERT *and* add
        // `[composer]`/`[buffer]`, so the editor carried a second way of
        // naming the same fact — and `[composer]` named an axis that no
        // longer exists.
        //
        // It carries **nothing** now. The path and the status moved to the
        // single bottom row, and the mode is the badge above the input box's
        // job: one vocabulary, one surface, one copy.
        let joined = render("content", 60, 8, 0, defaults()).join("\n");
        let title = joined.lines().next().unwrap_or_default();
        assert!(!title.contains("NOR"), "{title}");
        assert!(!title.contains("INS"), "{title}");
        assert!(!title.contains("NORMAL"), "{title}");
        assert!(!title.contains("INSERT"), "{title}");
        assert!(!title.contains("composer"), "{title}");
        assert!(!title.contains("buffer"), "{title}");
    }

    #[test]
    fn a_busy_operation_is_marked_in_the_status_row() {
        let joined = render(
            "content",
            60,
            8,
            0,
            Options {
                busy: true,
                ..defaults()
            },
        )
        .join("\n");
        assert!(joined.contains("working"), "{joined}");
    }

    /// The chrome row's **armed prefix indicator**, read back from the frame.
    ///
    /// The armed prefix is the only thing the chrome row paints in the
    /// primary + bold style, so this is the indicator itself rather than a
    /// guess at it from the row's text.
    ///
    /// A text-level guess cannot work here. The row carries the scope badge
    /// (`[project]`) and the writable marker, and a substring search for the
    /// indicator's glyph finds *those* instead of the indicator: `[` is in the
    /// badge, `t` is in "project" and in "writable", `m` is in "read-only —
    /// edits refused". The check that matters is "is a prefix armed?", and only
    /// the style can answer it.
    fn armed_indicator(buffer: &ratatui::buffer::Buffer, height: u16) -> String {
        let theme = Theme::default();
        let row = banner_row(height);
        let mut text = String::new();
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, row)];
            if cell.fg == theme.primary && cell.modifier.contains(Modifier::BOLD) {
                text.push_str(cell.symbol());
            }
        }
        text.trim().to_string()
    }

    /// Every armed prefix puts something different on screen, and an idle editor
    /// puts nothing there.
    ///
    /// The distinctness is the point, not the presence: an indicator that rendered
    /// every prefix identically would let a user believe they had armed a grammar
    /// they had not. `space` is here because it was the one pending state with no
    /// indicator at all before M002 — the baseline's `bool` had nowhere to render
    /// to — and `space y` is the gesture that made that visible.
    #[test]
    fn every_pending_prefix_renders_distinctly_and_idle_renders_nothing() {
        let mut seen: Vec<(EditorPrefix, String)> = Vec::new();
        for prefix in [
            EditorPrefix::Idle,
            EditorPrefix::Goto,
            EditorPrefix::Space,
            EditorPrefix::OpenBracket,
            EditorPrefix::CloseBracket,
            EditorPrefix::FindNext,
            EditorPrefix::FindPrevious,
            EditorPrefix::FindTillNext,
            EditorPrefix::FindTillPrevious,
            // The `m` grammar's two levels. Both are here because the `m`
            // level is the one pending state whose *second* level is a
            // different state again: a reader who armed `m` and then `m r`
            // has to be able to see which half they are in, and the two
            // halves must not render alike.
            EditorPrefix::Match,
            EditorPrefix::MatchArg(editor::MatchArg::SurroundAdd),
            EditorPrefix::MatchArg(editor::MatchArg::SurroundDelete),
            EditorPrefix::MatchArg(editor::MatchArg::SurroundReplaceFrom),
            EditorPrefix::MatchArg(editor::MatchArg::SurroundReplaceTo('(')),
            EditorPrefix::MatchArg(editor::MatchArg::AroundObject),
            EditorPrefix::MatchArg(editor::MatchArg::InnerObject),
        ] {
            let buffer = render_buffer(
                "content",
                60,
                8,
                0,
                Options {
                    pending_prefix: prefix,
                    ..defaults()
                },
            );
            let banner = banner(&lines_of(&buffer));
            let indicator = armed_indicator(&buffer, 8);
            if prefix == EditorPrefix::Idle {
                assert!(
                    indicator.is_empty(),
                    "an idle editor must not show an indicator: {banner:?}"
                );
            } else {
                assert_eq!(
                    indicator,
                    prefix.display().as_ref(),
                    "{prefix:?} must render its own indicator, not another one: {banner:?}"
                );
            }
            for (earlier, earlier_banner) in &seen {
                assert_ne!(
                    earlier_banner, &banner,
                    "{earlier:?} and {prefix:?} render identically"
                );
            }
            seen.push((prefix, banner));
        }
    }

    fn render_with_lsp(
        text: &str,
        width: u16,
        height: u16,
        cursor: usize,
        options: Options<'_>,
        lsp: crate::tui::editor_lsp::EditorLspState,
        theme: Theme,
    ) -> Vec<String> {
        let snapshot = DocumentBuffer::new(text).snapshot();
        let mut presentation = TuiDocumentPresentation {
            cursor_byte: cursor,
            selections: options.selections,
            search_line: options.search_line.to_string(),
            search_matches: options.search_matches.clone(),
            ..TuiDocumentPresentation::default()
        };
        seed_history(&mut presentation, options.history);
        let completion = crate::tui::editor_completion::EditorCompletionState::default();
        let mut highlight = crate::tui::editor_highlight::EditorHighlight::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                EditorWidget {
                    path: "src/lib.rs",
                    status: options.status,
                    notice: options.notice,
                    find_note: options.find_note,
                    mode: options.mode,
                    buffer_focused: options.buffer_focused,
                    pending_prefix: options.pending_prefix,
                    command_active: options.command_active,
                    command_line: options.command_line,
                    command_selection: options.command_selection,
                    search_active: options.search_active,
                    search_line: options.search_line,
                    search_count: options.search_count,
                    search_partial: options.search_partial,
                    scope: options.scope,
                    cursor_byte: presentation.cursor_byte,
                    writable: options.writable,
                    busy: options.busy,
                    snapshot: &snapshot,
                    lsp: &lsp,
                    completion: &completion,
                    highlight: &mut highlight,
                    presentation: &mut presentation,
                    theme: Arc::new(theme.clone()),
                }
                .render(frame, Rect::new(0, 0, width, height));
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    // ── Completion popup rendering ────────────────────────────────────

    use codegg_protocol::lsp::{
        LspCompletionApplicabilityDto, LspCompletionCandidateDto, LspCompletionEditDto,
        LspCompletionResultDto, LspCompletionStatus,
    };

    fn completion_candidate(label: &str, applicable: bool) -> LspCompletionCandidateDto {
        LspCompletionCandidateDto {
            label: label.to_string(),
            detail: Some(format!("detail {label}")),
            kind: Some("function".to_string()),
            filter_text: None,
            sort_text: None,
            applicability: if applicable {
                LspCompletionApplicabilityDto::Applicable {
                    edit: LspCompletionEditDto {
                        start_line: 1,
                        start_character: 4,
                        end_line: 1,
                        end_character: 8,
                        new_text: label.to_string(),
                    },
                }
            } else {
                LspCompletionApplicabilityDto::NonApplicable {
                    reason: codegg_protocol::lsp::LspCompletionUnsupportedReason::Snippet,
                }
            },
        }
    }

    fn completion_state(
        candidates: Vec<LspCompletionCandidateDto>,
        status: LspCompletionStatus,
        truncated: bool,
        incomplete: bool,
    ) -> crate::tui::editor_completion::EditorCompletionState {
        use crate::tui::editor_completion::EditorCompletionScope;
        use crate::tui::editor_lsp::EditorLspScope;
        let mut state = crate::tui::editor_completion::EditorCompletionState::default();
        let scope = EditorCompletionScope {
            read_scope: EditorLspScope {
                generation: 0,
                session_id: "session".into(),
                project_id: "project".into(),
                workspace_id: "workspace".into(),
                path: "src/lib.rs".into(),
            },
            document_id: "doc".into(),
            revision: 0,
            line: 1,
            character: 8,
        };
        let request = state.begin(&scope);
        let mut result = LspCompletionResultDto::ready(
            "doc".into(),
            0,
            Some(0),
            candidates,
            truncated,
            incomplete,
        );
        result.status = status;
        assert!(state.apply(request, state.epoch(), &scope, Some(result), None));
        state
    }

    fn render_with_completion(
        text: &str,
        width: u16,
        height: u16,
        completion: &crate::tui::editor_completion::EditorCompletionState,
    ) -> Vec<String> {
        let snapshot = DocumentBuffer::new(text).snapshot();
        let mut presentation = TuiDocumentPresentation {
            cursor_byte: 0,
            ..TuiDocumentPresentation::default()
        };
        let lsp = crate::tui::editor_lsp::EditorLspState::default();
        let options = Options {
            status: "synced",
            notice: None,
            find_note: "",
            mode: EditorMode::Normal,
            buffer_focused: true,
            pending_prefix: EditorPrefix::Idle,
            command_active: false,
            command_line: "",
            command_selection: 0,
            search_active: false,
            search_line: "",
            search_count: 0,
            search_partial: false,
            selections: Vec::new(),
            search_matches: Vec::new(),
            history: (0, 0),
            scope: codegg_protocol::document::DocumentScope::Workspace,
            writable: true,
            busy: false,
            lsp: crate::tui::editor_lsp::EditorLspState::default(),
        };
        let mut highlight = crate::tui::editor_highlight::EditorHighlight::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                EditorWidget {
                    path: "src/lib.rs",
                    status: options.status,
                    notice: options.notice,
                    find_note: options.find_note,
                    mode: options.mode,
                    buffer_focused: options.buffer_focused,
                    pending_prefix: options.pending_prefix,
                    command_active: options.command_active,
                    command_line: options.command_line,
                    command_selection: options.command_selection,
                    search_active: options.search_active,
                    search_line: options.search_line,
                    search_count: options.search_count,
                    search_partial: options.search_partial,
                    scope: options.scope,
                    cursor_byte: presentation.cursor_byte,
                    writable: options.writable,
                    busy: options.busy,
                    snapshot: &snapshot,
                    lsp: &lsp,
                    completion,
                    highlight: &mut highlight,
                    presentation: &mut presentation,
                    theme: Arc::new(Theme::default()),
                }
                .render(frame, Rect::new(0, 0, width, height));
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn completion_popup_lists_candidates_with_selection() {
        let state = completion_state(
            vec![
                completion_candidate("println!", true),
                completion_candidate("print", true),
                completion_candidate("snip", false),
            ],
            LspCompletionStatus::Ready,
            false,
            false,
        );
        let rows = render_with_completion("fn main() {}\n", 80, 24, &state).join("\n");
        assert!(rows.contains("println!"), "labels render:\n{rows}");
        assert!(rows.contains("> println!"), "first row selected:\n{rows}");
        assert!(
            rows.contains("unavailable"),
            "non-applicable is marked:\n{rows}"
        );
        assert!(
            rows.contains("suggestions"),
            "header names the state:\n{rows}"
        );
    }

    #[test]
    fn completion_popup_neutralizes_malicious_labels() {
        let mut evil = completion_candidate("a\x1b[31mb\nc\td", true);
        evil.detail = Some("x".repeat(500));
        let state = completion_state(vec![evil], LspCompletionStatus::Ready, false, false);
        let rows = render_with_completion("fn main() {}\n", 80, 24, &state).join("\n");
        assert!(!rows.contains('\x1b'), "no escape survives:\n{rows}");
        assert!(
            rows.contains("a [31mb c d"),
            "controls become spaces:\n{rows}"
        );
    }

    #[test]
    fn completion_popup_states_render_distinctly() {
        for (status, word) in [
            (LspCompletionStatus::NotReady, "warming"),
            (LspCompletionStatus::Stale, "re-trigger"),
            (LspCompletionStatus::Unsynced, "revision"),
            (LspCompletionStatus::Unsupported, "provider"),
        ] {
            let state = completion_state(vec![], status, false, false);
            let rows = render_with_completion("fn main() {}\n", 80, 24, &state).join("\n");
            assert!(rows.contains(word), "{status:?} must say {word}:\n{rows}");
        }
        let state = completion_state(vec![], LspCompletionStatus::Ready, false, false);
        let rows = render_with_completion("fn main() {}\n", 80, 24, &state).join("\n");
        assert!(
            rows.contains("No suggestions"),
            "empty Ready differs:\n{rows}"
        );
        let mut errored = crate::tui::editor_completion::EditorCompletionState::default();
        let scope = {
            use crate::tui::editor_completion::EditorCompletionScope;
            use crate::tui::editor_lsp::EditorLspScope;
            EditorCompletionScope {
                read_scope: EditorLspScope {
                    generation: 0,
                    session_id: "s".into(),
                    project_id: "p".into(),
                    workspace_id: "w".into(),
                    path: "src/lib.rs".into(),
                },
                document_id: "doc".into(),
                revision: 0,
                line: 0,
                character: 0,
            }
        };
        let request = errored.begin(&scope);
        assert!(errored.apply(
            request,
            errored.epoch(),
            &scope,
            None,
            Some("boom\n\x1b[0m".into())
        ));
        let rows = render_with_completion("fn main() {}\n", 80, 24, &errored).join("\n");
        assert!(rows.contains("boom"), "error text shows:\n{rows}");
        assert!(!rows.contains('\x1b'), "error text is sanitized:\n{rows}");
    }

    #[test]
    fn completion_popup_hides_on_terminals_too_narrow_to_fit() {
        let state = completion_state(
            vec![completion_candidate("println!", true)],
            LspCompletionStatus::Ready,
            false,
            false,
        );
        // 1x1 and narrow frames must not panic; the keys keep working
        // because state and rendering are independent.
        let tiny = render_with_completion("fn main() {}\n", 1, 1, &state).join("\n");
        assert!(!tiny.contains("println!"), "no room means no popup");
        let narrow = render_with_completion("fn main() {}\n", 20, 24, &state).join("\n");
        assert!(
            !narrow.contains("println!"),
            "under 24 columns means no popup"
        );
        let wide = render_with_completion("fn main() {}\n", 80, 24, &state).join("\n");
        assert!(wide.contains("println!"), "room means popup");
    }

    #[test]
    fn completion_popup_scrolls_long_lists_and_marks_budgets() {
        let candidates: Vec<_> = (0..15)
            .map(|index| completion_candidate(&format!("item-{index:02}"), true))
            .collect();
        let mut state = completion_state(candidates, LspCompletionStatus::Ready, true, true);
        for _ in 0..14 {
            state.select_next();
        }
        let rows = render_with_completion("fn main() {}\n", 80, 24, &state).join("\n");
        assert!(
            rows.contains("item-14"),
            "selected tail is visible:\n{rows}"
        );
        assert!(!rows.contains("item-00"), "the head scrolled away:\n{rows}");
        assert!(
            rows.contains("truncated"),
            "the count cap is disclosed:\n{rows}"
        );
    }

    #[test]
    fn completion_popup_on_a_huge_line_stays_bounded() {
        // The popup reads DTO strings only, never the snapshot, so a
        // megabyte line cannot blow the frame budget through this path.
        let text = "x".repeat(1024 * 1024);
        let state = completion_state(
            vec![completion_candidate("println!", true)],
            LspCompletionStatus::Ready,
            false,
            false,
        );
        let rows = render_with_completion(&text, 80, 24, &state).join("\n");
        assert!(rows.contains("println!"), "popup renders over huge lines");
    }

    fn styled_token(
        line: u32,
        character: u32,
        length: u32,
        kind: &str,
    ) -> codegg_protocol::lsp::LspSemanticTokenDto {
        codegg_protocol::lsp::LspSemanticTokenDto {
            line,
            character,
            length,
            token_type: kind.to_string(),
            modifiers: vec![],
        }
    }

    #[test]
    fn token_styles_come_from_theme_slots_and_stay_legible() {
        for theme in [Theme::dark(), Theme::light()] {
            // Every supported class maps to a theme slot, never a hardcoded
            // ANSI color. Unknown vendor types stay plain (None).
            let supported = [
                "keyword",
                "string",
                "number",
                "comment",
                "function",
                "method",
                "type",
                "class",
                "enum",
                "struct",
                "variable",
                "parameter",
                "property",
                "namespace",
            ];
            for kind in supported {
                let style =
                    token_style(&theme, kind).unwrap_or_else(|| panic!("{kind} must be styled"));
                let fg = style.fg.expect("styled token must set a foreground");
                assert_ne!(
                    fg, theme.background,
                    "{kind} foreground must differ from the background"
                );
            }
            assert_eq!(token_style(&theme, "vendor_custom"), None);
            assert_eq!(token_style(&theme, "totally_unknown"), None);
        }
        // Spot-check slots: keyword uses primary bold, comment uses muted
        // italic, string uses success. Hardcoded Yellow/Green/DarkGray would
        // fail this on at least one theme.
        let dark = Theme::dark();
        assert_eq!(
            token_style(&dark, "keyword").unwrap().fg,
            Some(dark.primary)
        );
        assert_eq!(token_style(&dark, "comment").unwrap().fg, Some(dark.muted));
        assert_eq!(token_style(&dark, "string").unwrap().fg, Some(dark.success));
    }

    #[test]
    fn unknown_tokens_render_plain_while_neighbors_keep_styles() {
        use crate::tui::editor::EditorViewport;
        let text = "let value = 42;";
        let snapshot = DocumentBuffer::new(text).snapshot();
        let theme = Arc::new(Theme::default());
        let mut presentation = TuiDocumentPresentation::default();
        // `let` keyword 0..3, `value` variable 4..9 (plain), `42` number
        // 12..14. The middle span is well-formed but unstyled in the old
        // whitelist; it must not erase its neighbors.
        let validated = crate::tui::editor_lsp::validate_semantic_tokens(
            &snapshot,
            vec![
                styled_token(0, 0, 3, "keyword"),
                styled_token(0, 4, 5, "variable"),
                styled_token(0, 12, 2, "number"),
                styled_token(0, 4, 5, "vendor_custom_plain"),
            ],
        );
        // `vendor_custom_plain` overlaps `variable` and is dropped; the
        // three distinct spans survive validation.
        assert_eq!(validated.accepted.len(), 3);
        let lsp = crate::tui::editor_lsp::EditorLspState {
            semantic_tokens: validated.accepted,
            semantic_revision: Some(1),
            ..crate::tui::editor_lsp::EditorLspState::default()
        };
        let completion = crate::tui::editor_completion::EditorCompletionState::default();
        let mut highlight = crate::tui::editor_highlight::EditorHighlight::default();
        let viewport = EditorViewport {
            first_line: 0,
            column: 0,
            gutter: 3,
            text_columns: 40,
            visible_lines: 5,
        };
        let mut widget = EditorWidget {
            path: "src/lib.rs",
            status: "synced",
            notice: None,
            find_note: "",
            mode: EditorMode::Normal,
            buffer_focused: true,
            pending_prefix: EditorPrefix::Idle,
            command_active: false,
            command_line: "",
            command_selection: 0,
            search_active: false,
            search_line: "",
            search_count: 0,
            search_partial: false,
            scope: codegg_protocol::document::DocumentScope::Workspace,
            cursor_byte: 0,
            writable: true,
            busy: false,
            snapshot: &snapshot,
            lsp: &lsp,
            completion: &completion,
            highlight: &mut highlight,
            presentation: &mut presentation,
            theme,
        };
        let spans = widget.line_spans(0, viewport, Style::default(), &[]);
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(joined.contains("let"), "{joined}");
        assert!(joined.contains("value"), "{joined}");
        assert!(joined.contains("42"), "{joined}");
        // At least the keyword and number spans carry a themed foreground;
        // the plain middle does not erase them.
        let themed = spans.iter().filter(|s| s.style.fg.is_some()).count();
        assert!(
            themed >= 2,
            "keyword and number must keep themed styles: {spans:?}"
        );
    }

    #[test]
    fn truncated_and_partial_highlights_are_disclosed_in_the_banner() {
        let truncated = crate::tui::editor_lsp::EditorLspState {
            semantic_revision: Some(3),
            semantic_truncated: true,
            semantic_incomplete: true,
            ..crate::tui::editor_lsp::EditorLspState::default()
        };
        let joined = render_with_lsp("content", 60, 8, 0, defaults(), truncated, Theme::default())
            .join("\n");
        assert!(
            joined.contains("truncated"),
            "truncation must be explicit: {joined}"
        );
        let partial = crate::tui::editor_lsp::EditorLspState {
            semantic_revision: Some(3),
            semantic_incomplete: true,
            ..crate::tui::editor_lsp::EditorLspState::default()
        };
        let joined =
            render_with_lsp("content", 60, 8, 0, defaults(), partial, Theme::default()).join("\n");
        assert!(
            joined.contains("partial"),
            "dropped spans must be explicit: {joined}"
        );
        let clean = crate::tui::editor_lsp::EditorLspState::default();
        let joined =
            render_with_lsp("content", 60, 8, 0, defaults(), clean, Theme::default()).join("\n");
        assert!(
            !joined.contains("truncated") && !joined.contains("partial"),
            "no signal without semantic data: {joined}"
        );
    }

    #[test]
    fn semantic_highlight_preserves_current_line_and_diagnostic_markers() {
        use codegg_protocol::lsp::{LspDiagnosticDto, LspRangeDto};
        let text = "alpha\nbeta\n";
        let lsp = crate::tui::editor_lsp::EditorLspState {
            semantic_tokens: vec![styled_token(0, 0, 5, "keyword")],
            semantic_revision: Some(1),
            diagnostics: vec![LspDiagnosticDto {
                range: LspRangeDto {
                    path: "src/lib.rs".into(),
                    workspace_relative_path: None,
                    start_line: 0,
                    start_column: 0,
                    end_line: 0,
                    end_column: 1,
                },
                severity: 1,
                tag: 0,
                code: None,
                message: "broken".into(),
                source: None,
            }],
            ..crate::tui::editor_lsp::EditorLspState::default()
        };
        // Cursor on line 0, viewport shows both lines.
        let joined = render_with_lsp(text, 40, 8, 0, defaults(), lsp, Theme::default()).join("\n");
        assert!(joined.contains("alpha"), "{joined}");
        assert!(
            joined.contains("!"),
            "{joined} diagnostic marker must survive"
        );
    }

    #[test]
    fn semantic_tokens_do_not_break_horizontal_clipping() {
        let text = format!("{} // {}", "x".repeat(5000), "tail");
        let snapshot = DocumentBuffer::new(&text).snapshot();
        let validated = crate::tui::editor_lsp::validate_semantic_tokens(
            &snapshot,
            vec![styled_token(0, 0, 3, "keyword")],
        );
        let lsp = crate::tui::editor_lsp::EditorLspState {
            semantic_tokens: validated.accepted,
            semantic_revision: Some(1),
            ..crate::tui::editor_lsp::EditorLspState::default()
        };
        let lines = render_with_lsp(&text, 30, 8, 0, defaults(), lsp, Theme::default());
        let widest = lines
            .iter()
            .map(|line| line.trim_end().chars().count())
            .max()
            .unwrap_or(0);
        assert!(
            widest <= 30,
            "semantic spans must stay viewport-bounded: {widest}"
        );
    }

    // ── Cursor, readability, background ──────────────────────────────────

    #[test]
    fn normal_mode_draws_a_reverse_video_cursor() {
        let theme = Theme::default();
        // The cursor sits on `b` of `beta`.
        let height = 8;
        let buffer = render_buffer(
            "alpha\nbeta\ngamma",
            30,
            height,
            6,
            Options {
                mode: EditorMode::Normal,
                buffer_focused: false,
                ..defaults()
            },
        );
        // Found by the reverse-video pairing itself, so the test states the
        // rule rather than a coordinate.
        let reversed = body_cells(&buffer, height)
            .into_iter()
            .filter(|cell| cell.fg == theme.background && cell.bg == theme.foreground)
            .collect::<Vec<_>>();
        assert_eq!(
            reversed.len(),
            1,
            "exactly one cell may be reverse-video, and it is the cursor"
        );
        // And the character under it is still legible glyph-first: reverse
        // video is the one choice that guarantees contrast for every token
        // color without the renderer knowing which of them is light.
        let cursor_symbol = &buffer[(TEXT_X, 1 + 1)];
        assert_eq!(cursor_symbol.symbol(), "b");
    }

    #[test]
    fn insert_mode_draws_a_bar_and_never_a_block() {
        let theme = Theme::default();
        let height = 8;
        let buffer = render_buffer(
            "alpha\nbeta",
            30,
            height,
            6,
            Options {
                mode: EditorMode::Insert,
                buffer_focused: true,
                ..defaults()
            },
        );
        // Self-locating: find the bar by the marker that *defines* it rather
        // than by a hardcoded cell. The point is that exactly one such cell
        // exists, and that its background is the current-line tint — a block
        // would be the theme foreground.
        let bars = body_cells(&buffer, height)
            .into_iter()
            .filter(|cell| cell.modifier.contains(Modifier::UNDERLINED))
            .collect::<Vec<_>>();
        assert_eq!(bars.len(), 1, "exactly one Insert caret must be drawn");
        assert_eq!(
            bars[0].bg, theme.alternate_bg,
            "a block in Insert would hide the character being typed over"
        );
        assert_ne!(
            bars[0].bg, theme.foreground,
            "the Insert caret must not reverse the cell it sits on"
        );
    }

    #[test]
    fn a_cursor_at_the_end_of_a_line_is_still_visible() {
        let theme = Theme::default();
        // Offset 5 is one past `alpha` — the position a reader reaches most
        // often and the one that must never be invisible.
        let buffer = render_buffer(
            "alpha\nbeta",
            30,
            8,
            5,
            Options {
                buffer_focused: false,
                ..defaults()
            },
        );
        let cell = &buffer[(4 + 5, 1)];
        assert_eq!(cell.symbol(), " ");
        assert_eq!(cell.fg, theme.background);
        assert_eq!(cell.bg, theme.foreground);
    }

    #[test]
    fn the_cursor_line_is_readable_on_a_dark_theme() {
        let theme = Theme::default();
        let buffer = render_buffer("alpha\nbeta", 30, 8, 0, defaults());
        // Body row 0 is the cursor's own line; column 1 is not the cursor.
        let style = body_style(&buffer, 0, 1);
        let fg = style.fg;
        let bg = if style.bg == ratatui::style::Color::Reset {
            theme.background
        } else {
            style.bg
        };
        // The defect this pins: the current line used to render as
        // `fg(alternate_bg)`, a background color as a foreground, which on a
        // dark theme is near-black on near-black.
        assert_ne!(
            fg, theme.alternate_bg,
            "the cursor line must not paint a background color as its foreground"
        );
        assert!(
            (luminance(fg) - luminance(bg)).abs() > 0.5,
            "cursor-line text {fg:?} on {bg:?} has too little contrast to read"
        );
    }

    #[test]
    fn the_document_area_carries_the_theme_background() {
        let theme = Theme::default();
        let buffer = render_buffer("alpha", 30, 8, 0, defaults());
        // A blank cell past the end of the text still has to be themed: this
        // is the `Clear`-then-bare-`Paragraph` defect, and it only shows on a
        // cell with no glyph on it.
        let trailing = &buffer[(14, 1)];
        assert_eq!(
            trailing.bg, theme.background,
            "the document area must not fall back to the terminal default"
        );
    }

    #[test]
    fn selected_text_stays_readable_against_the_selection_background() {
        let theme = Theme::default();
        let snapshot = DocumentBuffer::new("alpha\nbeta").snapshot();
        // Select bytes 1..4 (`pha`) on the cursor's own line, with the cursor
        // parked at 4 — the head, not the cells under assertion.
        let buffer = render_buffer(
            "alpha\nbeta",
            30,
            8,
            4,
            Options {
                buffer_focused: true,
                selections: vec![selection_over(&snapshot, 1..4)],
                ..defaults()
            },
        );
        let cell = body_style(&buffer, 0, 1);
        assert_eq!(cell.bg, theme.selection, "selection must paint bg");
        let fg = cell.fg;
        // Pinning the foreground is the contract: "patch only the background"
        // carried a syntax foreground chosen for a different background,
        // which is the same dark-on-dark defect one layer down.
        assert_eq!(fg, theme.foreground);
        assert!(
            (luminance(fg) - luminance(theme.selection)).abs() > 0.3,
            "selected text {fg:?} on {:?} is not readable",
            theme.selection
        );
        // Line 1 carries no selection at all and must not be tinted.
        assert_ne!(body_style(&buffer, 1, 1).bg, theme.selection);
    }

    #[test]
    fn a_search_match_is_highlighted_and_still_readable() {
        let theme = Theme::default();
        // `aaa` sits at 0..3 on the cursor's own line; the cursor block owns
        // column 0, so column 1 is a match cell that is not the cursor.
        let buffer = render_buffer(
            "aaa bbb aaa",
            30,
            8,
            0,
            Options {
                buffer_focused: false,
                search_line: "aaa",
                search_count: 2,
                search_matches: vec![0, 8],
                ..defaults()
            },
        );
        let cell = body_style(&buffer, 0, 1);
        assert_eq!(
            cell.bg, theme.selection_dim,
            "a `/` match must be highlighted in the text"
        );
        let fg = cell.fg;
        assert_eq!(fg, theme.foreground);
        assert!((luminance(fg) - luminance(theme.selection_dim)).abs() > 0.3);
        // Column 4 (`b`) is not part of any match and must not be tinted.
        assert_ne!(body_style(&buffer, 0, 4).bg, theme.selection_dim);
    }

    #[test]
    fn the_cursor_is_painted_over_a_search_match_and_a_selection() {
        let theme = Theme::default();
        let snapshot = DocumentBuffer::new("aaa bbb").snapshot();
        // Every layer stacked on the same cell: the cursor is on the match
        // start, which is also inside the selection.
        let buffer = render_buffer(
            "aaa bbb",
            30,
            8,
            0,
            Options {
                buffer_focused: false,
                selections: vec![selection_over(&snapshot, 0..3)],
                search_line: "aaa",
                search_count: 1,
                search_matches: vec![0],
                ..defaults()
            },
        );
        let cursor_cell = body_style(&buffer, 0, 0);
        assert_eq!(cursor_cell.bg, theme.foreground);
        assert_eq!(cursor_cell.fg, theme.background);
    }

    // ── `/` search rendering ────────────────────────────────────────────

    #[test]
    fn the_search_line_appears_in_the_banner_while_it_is_open() {
        let height = 8;
        let lines = render(
            "alpha",
            40,
            height,
            0,
            Options {
                search_active: true,
                search_line: "host_status",
                ..defaults()
            },
        );
        let banner = &lines[banner_row(height) as usize];
        assert!(
            banner.contains("/host_status_"),
            "the open `/` line belongs on the banner row: {banner:?}"
        );
    }

    #[test]
    fn a_partial_search_says_so_rather_than_reading_as_complete() {
        let height = 8;
        let lines = render(
            "alpha",
            40,
            height,
            0,
            Options {
                search_line: "alph",
                search_count: 3,
                search_partial: true,
                ..defaults()
            },
        );
        // The count shares the `ln/col` readout row, which is where the
        // reader is already looking when a search resolves.
        let readout = &lines[readout_row(height) as usize];
        assert!(readout.contains("3 matches"), "{readout:?}");
        assert!(readout.contains("partial"), "{readout:?}");
        let plain = render(
            "alpha",
            40,
            height,
            0,
            Options {
                search_line: "alph",
                search_count: 3,
                search_partial: false,
                ..defaults()
            },
        );
        let plain_readout = &plain[readout_row(height) as usize];
        assert!(plain_readout.contains("3 matches"), "{plain_readout:?}");
        assert!(!plain_readout.contains("partial"), "{plain_readout:?}");
    }

    // ── The history cursor's position ───────────────────────────────────

    /// A time cursor nobody can see is a rename.
    ///
    /// The position is disclosed when the reader is standing somewhere other
    /// than the newest change, and says nothing at the tip — so the ordinary
    /// case draws exactly what it drew before the cursor existed.
    #[test]
    fn the_history_position_is_disclosed_only_when_the_cursor_is_not_at_the_tip() {
        let height = 8;
        let at_tip = render(
            "alpha",
            40,
            height,
            0,
            Options {
                history: (3, 3),
                ..defaults()
            },
        );
        let banner = &at_tip[banner_row(height) as usize];
        assert!(
            !banner.contains("history"),
            "at the tip there is nothing to disclose: {banner:?}"
        );

        let behind = render(
            "alpha",
            40,
            height,
            0,
            Options {
                history: (1, 3),
                ..defaults()
            },
        );
        let banner = &behind[banner_row(height) as usize];
        assert!(
            banner.contains("history 1/3"),
            "the position belongs on the banner row: {banner:?}"
        );
    }

    /// The rename preview names the files that would change, and says that
    /// nothing is applied.
    ///
    /// The rendering is M013's, but a rendering nobody exercises is a
    /// rendering nobody knows works, and "the preview shows the files" is
    /// the whole point of the chord.
    #[test]
    fn the_rename_preview_names_the_files_it_would_change() {
        use codegg_protocol::lsp::{
            LspReadOperation, LspReadPayloadDto, LspReadResultDto, LspReadStatus, LspRenameFileDto,
            LspRenamePreviewDto,
        };
        let mut options = defaults();
        options.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
        options.lsp.read_result = Some(LspReadResultDto {
            operation: LspReadOperation::RenamePreview,
            status: LspReadStatus::Ready,
            payload: Some(LspReadPayloadDto::RenamePreview(LspRenamePreviewDto {
                old_name: Some("old".to_string()),
                new_name: "renamed".to_string(),
                edit_count: 2,
                files: vec![LspRenameFileDto {
                    path: "src/lib.rs".to_string(),
                    edit_count: 2,
                }],
                warnings: Vec::new(),
                truncated: false,
                base_stale: false,
                candidate: None,
            })),
        });

        let painted = render("hello", 72, 20, 0, options).join("\n");
        assert!(
            painted.contains("renamed"),
            "the preview names the new name, got:\n{painted}"
        );
        assert!(
            painted.contains("src/lib.rs"),
            "and the file it would change, got:\n{painted}"
        );
        assert!(
            painted.contains("not applied"),
            "and says that nothing is applied, got:\n{painted}"
        );
    }

    /// The format preview shows the diff, and says that nothing is applied.
    #[test]
    fn the_format_preview_shows_the_diff_and_says_it_is_not_applied() {
        use codegg_protocol::lsp::{
            LspFormatPreviewDto, LspReadOperation, LspReadPayloadDto, LspReadResultDto,
            LspReadStatus,
        };
        let mut options = defaults();
        options.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
        options.lsp.read_result = Some(LspReadResultDto {
            operation: LspReadOperation::FormatPreview,
            status: LspReadStatus::Ready,
            payload: Some(LspReadPayloadDto::FormatPreview(LspFormatPreviewDto {
                path: "src/lib.rs".to_string(),
                edit_count: 1,
                before_hash: "a".to_string(),
                after_hash: "b".to_string(),
                diff: "- let x = 1;\n+ let x =  1;".to_string(),
                truncated: false,
                base_stale: false,
                candidate: None,
            })),
        });

        let painted = render("hello", 72, 20, 0, options).join("\n");
        assert!(
            painted.contains("let x =  1;"),
            "the preview shows the diff, got:\n{painted}"
        );
        assert!(
            painted.contains("not applied"),
            "and says that nothing is applied, got:\n{painted}"
        );
    }

    /// The code-action panel lists what the server offered, and says which
    /// of the offered actions can be applied.
    ///
    /// The reader has to be able to see *all* of the offer — including the
    /// parts that cannot be applied — because the offer is the server's
    /// answer, and hiding an action would hide the answer. Each line says
    /// what would happen, so a command-only action is visibly not an
    /// application and not a mystery.
    #[test]
    fn the_code_action_panel_lists_the_offer_and_says_which_can_be_applied() {
        use codegg_protocol::lsp::{
            LspCodeActionDto, LspCodeActionUnavailable, LspPreviewCandidateDto, LspPreviewPatchDto,
            LspReadOperation, LspReadPayloadDto, LspReadResultDto, LspReadStatus,
        };
        let mut options = defaults();
        options.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
        options.lsp.read_result = Some(LspReadResultDto {
            operation: LspReadOperation::CodeActionPreview,
            status: LspReadStatus::Ready,
            payload: Some(LspReadPayloadDto::CodeActions {
                actions: vec![
                    LspCodeActionDto {
                        title: "Add the missing import".to_string(),
                        kind: Some("quickfix".to_string()),
                        preferred: true,
                        disabled_reason: None,
                        has_edit: true,
                        has_command: false,
                        candidate: Some(LspPreviewCandidateDto {
                            preview_id: "editor-preview-1".to_string(),
                            preview_revision: 1,
                            preview_digest: "a".repeat(64),
                            kind: "code_action".to_string(),
                            title: "Add the missing import".to_string(),
                            provenance: "editor:code_action:Add the missing import".to_string(),
                            patches: vec![LspPreviewPatchDto {
                                path: "src/lib.rs".to_string(),
                                patch: "@@ -1 +1 @@\n-a\n+b\n".to_string(),
                                original_hash: "b".repeat(64),
                            }],
                        }),
                        unavailable: None,
                    },
                    LspCodeActionDto {
                        title: "Run the test suite".to_string(),
                        kind: None,
                        preferred: false,
                        disabled_reason: None,
                        has_edit: false,
                        has_command: true,
                        candidate: None,
                        unavailable: Some(LspCodeActionUnavailable::Command),
                    },
                ],
                truncated: false,
            }),
        });

        let painted = render("hello", 96, 24, 0, options).join("\n");
        assert!(
            painted.contains("Add the missing import"),
            "the panel names the action, got:\n{painted}"
        );
        assert!(
            painted.contains("ready to apply"),
            "and says the applicable one can be applied, got:\n{painted}"
        );
        assert!(
            painted.contains("Run the test suite"),
            "the unapplicable action is listed too, got:\n{painted}"
        );
        assert!(
            painted.contains("command"),
            "and says why it cannot be applied, got:\n{painted}"
        );
    }

    /// The code-action panel shows the **selected** action, even when the
    /// server offered more actions than the panel is tall.
    ///
    /// A code action is a choice, and the panel is where the choice is made.
    /// If the selection could move off the visible rows, `Enter` would offer
    /// to apply a change the reader could not see — so the window follows
    /// the selection.
    #[test]
    fn the_code_action_panel_keeps_the_selected_action_visible() {
        use codegg_protocol::lsp::{
            LspCodeActionDto, LspPreviewCandidateDto, LspPreviewPatchDto, LspReadOperation,
            LspReadPayloadDto, LspReadResultDto, LspReadStatus,
        };
        let action = |index: usize| LspCodeActionDto {
            title: format!("Action {index}"),
            kind: Some("quickfix".to_string()),
            preferred: false,
            disabled_reason: None,
            has_edit: true,
            has_command: false,
            candidate: Some(LspPreviewCandidateDto {
                preview_id: format!("editor-preview-{index}"),
                preview_revision: 1,
                preview_digest: "a".repeat(64),
                kind: "code_action".to_string(),
                title: format!("Action {index}"),
                provenance: format!("editor:code_action:Action {index}"),
                patches: vec![LspPreviewPatchDto {
                    path: "src/lib.rs".to_string(),
                    patch: "@@ -1 +1 @@\n-a\n+b\n".to_string(),
                    original_hash: "b".repeat(64),
                }],
            }),
            unavailable: None,
        };

        let mut options = defaults();
        options.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
        options.lsp.selected_index = 10;
        options.lsp.read_result = Some(LspReadResultDto {
            operation: LspReadOperation::CodeActionPreview,
            status: LspReadStatus::Ready,
            payload: Some(LspReadPayloadDto::CodeActions {
                actions: (0..12).map(action).collect(),
                truncated: false,
            }),
        });

        let painted = render("hello", 96, 24, 0, options).join("\n");
        assert!(
            painted.contains("> Action 10"),
            "the selected action must be visible and marked, got:\n{painted}"
        );
        assert!(
            !painted.contains("Action 0\n"),
            "the window must have moved past the earlier actions, got:\n{painted}"
        );
    }

    /// A preview that became a candidate is not "not applied" any more.
    ///
    /// The chords stopped showing what *would* change and started doing it,
    /// so the panel's disposition line has to say so. A panel still claiming
    /// "not applied" for a candidate would be a status row lying about the
    /// last keystroke.
    #[test]
    fn a_preview_with_a_candidate_says_it_is_ready_to_apply() {
        use codegg_protocol::lsp::{
            LspFormatPreviewDto, LspPreviewCandidateDto, LspPreviewPatchDto, LspReadOperation,
            LspReadPayloadDto, LspReadResultDto, LspReadStatus,
        };
        let mut options = defaults();
        options.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
        options.lsp.read_result = Some(LspReadResultDto {
            operation: LspReadOperation::FormatPreview,
            status: LspReadStatus::Ready,
            payload: Some(LspReadPayloadDto::FormatPreview(LspFormatPreviewDto {
                path: "src/lib.rs".to_string(),
                edit_count: 1,
                before_hash: "a".to_string(),
                after_hash: "b".to_string(),
                diff: "- let x = 1;\n+ let x =  1;".to_string(),
                truncated: false,
                base_stale: false,
                candidate: Some(LspPreviewCandidateDto {
                    preview_id: "editor-preview-1".to_string(),
                    preview_revision: 1,
                    preview_digest: "a".repeat(64),
                    kind: "formatting".to_string(),
                    title: "Format src/lib.rs".to_string(),
                    provenance: "editor:format:src/lib.rs".to_string(),
                    patches: vec![LspPreviewPatchDto {
                        path: "src/lib.rs".to_string(),
                        patch: "@@ -1 +1 @@\n- let x = 1;\n+ let x =  1;".to_string(),
                        original_hash: "b".repeat(64),
                    }],
                }),
            })),
        });

        let painted = render("hello", 72, 20, 0, options).join("\n");
        assert!(
            painted.contains("ready to apply"),
            "a candidate is ready to apply, got:\n{painted}"
        );
        assert!(
            !painted.contains("not applied"),
            "and must not still claim that nothing is applied, got:\n{painted}"
        );
    }

    /// A truncated preview says why it cannot be applied.
    #[test]
    fn a_truncated_preview_says_it_cannot_be_applied() {
        use codegg_protocol::lsp::{
            LspFormatPreviewDto, LspReadOperation, LspReadPayloadDto, LspReadResultDto,
            LspReadStatus,
        };
        let mut options = defaults();
        options.lsp.panel = Some(crate::tui::editor_lsp::EditorLspPanel::ReadResult);
        options.lsp.read_result = Some(LspReadResultDto {
            operation: LspReadOperation::FormatPreview,
            status: LspReadStatus::Ready,
            payload: Some(LspReadPayloadDto::FormatPreview(LspFormatPreviewDto {
                path: "src/lib.rs".to_string(),
                edit_count: 1,
                before_hash: "a".to_string(),
                after_hash: "b".to_string(),
                diff: "- let x = 1;\n+ let x =  1;".to_string(),
                truncated: true,
                base_stale: false,
                candidate: None,
            })),
        });

        let painted = render("hello", 72, 20, 0, options).join("\n");
        assert!(
            painted.contains("truncated"),
            "a truncated preview must say it is truncated, got:\n{painted}"
        );
        assert!(
            !painted.contains("ready to apply"),
            "and must not offer itself as applicable, got:\n{painted}"
        );
    }
}
