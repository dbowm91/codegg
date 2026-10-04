//! M006-E: agent change-review widget.
//!
//! Renders one pending agent change as a file list plus a diff, with the
//! accept/reject affordance and the daemon's verdict.
//!
//! The diff itself is drawn by the **existing** `DiffViewer` in
//! `components/diff.rs` — this widget constructs one and replaces its public
//! `hunks` with the hunks parsed from the candidate's patch. Nothing here
//! re-implements diff rendering, and the viewer's scrolling and inline /
//! side-by-side toggle come along for free.
//!
//! ## Reads the decision, never makes it
//!
//! The daemon owns whether an apply is legal. This widget shows the daemon's
//! answer verbatim — including the unsaved-changes instruction — because a
//! refusal the user cannot read is a refusal the user cannot act on.

use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::app::state::change_review::{ChangeReviewState, ReviewVerdict};
use crate::tui::components::diff::DiffViewer;
use crate::tui::theme::Theme;

/// Rows reserved above the diff for the header, file list, and verdict.
const HEADER_ROWS: u16 = 3;

pub struct ChangeReviewView<'a> {
    pub state: &'a ChangeReviewState,
    pub theme: Arc<Theme>,
}

impl<'a> ChangeReviewView<'a> {
    pub fn new(state: &'a ChangeReviewState, theme: Arc<Theme>) -> Self {
        Self { state, theme }
    }

    pub fn render(self, frame: &mut Frame, area: Rect) {
        let theme = Arc::clone(&self.theme);
        let state = self.state;
        let title = format!(
            " Review: {} · {} · {} files{}{} ",
            state.title,
            state.verdict_summary(),
            state.review.patches.len(),
            if state.stale { " · STALE" } else { "" },
            if state.focused {
                " [a]pply [Esc]reject"
            } else {
                ""
            },
        );
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if state.focused {
                theme.primary
            } else {
                theme.border
            }))
            .style(Style::default().bg(theme.background));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        // A refusal is the most important thing on screen when it happens, so
        // it gets its own rows above the diff rather than living only in a
        // toast that has already scrolled away.
        let refusal_rows = match &state.verdict {
            ReviewVerdict::Refused { message } => {
                let wrapped = message.lines().count().max(1).min(4) as u16 + 1;
                let notice = Rect {
                    x: inner.x,
                    y: inner.y,
                    width: inner.width,
                    height: wrapped,
                };
                frame.render_widget(
                    Paragraph::new(
                        message
                            .lines()
                            .map(|line| {
                                Line::from(Span::styled(
                                    format!("✗ {line}"),
                                    Style::default().fg(theme.error),
                                ))
                            })
                            .collect::<Vec<_>>(),
                    ),
                    notice,
                );
                wrapped
            }
            _ => 0,
        };

        let header_height = HEADER_ROWS.saturating_add(refusal_rows).min(inner.height);
        let header_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: header_height,
        };
        frame.render_widget(Paragraph::new(self.header_lines()), header_area);

        let body_y = inner.y + header_height;
        if body_y >= inner.y + inner.height {
            return;
        }
        let body = Rect {
            x: inner.x,
            y: body_y,
            width: inner.width,
            height: inner.height - header_height,
        };

        let Some(patch) = state.active_patch() else {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "This change has no diff to show.",
                    Style::default().fg(theme.muted),
                )),
                body,
            );
            return;
        };

        // Reuse the existing viewer: construct it, then replace its public
        // hunks with the parsed patch. The viewer's own rendering, scrolling,
        // and side-by-side toggle are used unchanged.
        let mut viewer = DiffViewer::new(
            String::new().into_boxed_str(),
            String::new().into_boxed_str(),
            patch.path.clone().into_boxed_str(),
        );
        viewer.hunks = patch.hunks.clone();
        viewer.set_theme(&theme);
        frame.render_widget(&viewer, body);
    }

    fn header_lines(&self) -> Vec<Line<'static>> {
        let theme = Arc::clone(&self.theme);
        let state = self.state;
        let mut lines = Vec::new();

        if state.stale {
            lines.push(Line::from(Span::styled(
                "⚠ base content changed since this candidate was created — regenerating it is required",
                Style::default().fg(theme.warning),
            )));
        }

        // One row per file, the active one highlighted, so a multi-file change
        // is reviewable file by file rather than as one opaque blob.
        let file_line: Vec<Span> = state
            .review
            .patches
            .iter()
            .enumerate()
            .flat_map(|(index, patch)| {
                let active = index == state.file_index;
                let style = if active {
                    Style::default()
                        .fg(theme.background)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.muted)
                };
                let marker = if active { "▸ " } else { "  " };
                let suffix = match patch.truncated.as_ref() {
                    Some(_) => " (truncated)",
                    None => "",
                };
                vec![
                    Span::styled(marker, Style::default().fg(theme.primary)),
                    Span::styled(format!("{}{suffix}", patch.path), style),
                    Span::raw("  "),
                ]
            })
            .collect();
        lines.push(Line::from(file_line));

        for truncation in state.review.truncations() {
            lines.push(Line::from(Span::styled(
                format!("… {}", truncation.reason),
                Style::default().fg(theme.warning),
            )));
        }
        for path in &state.review.malformed_patches {
            lines.push(Line::from(Span::styled(
                format!("… {path} could not be read as a diff and was skipped"),
                Style::default().fg(theme.warning),
            )));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::change_review::ChangeReviewState;
    use crate::tui::unified_diff::parse_review;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn theme() -> Arc<Theme> {
        Arc::new(Theme::default())
    }

    fn open_review(patches: &[(&str, &str)]) -> ChangeReviewState {
        let owned: Vec<(String, String)> = patches
            .iter()
            .map(|(path, patch)| ((*path).to_string(), (*patch).to_string()))
            .collect();
        let mut state = ChangeReviewState::default();
        state.open(
            "preview-1".to_string(),
            "edit".to_string(),
            "Rename helper".to_string(),
            "agent".to_string(),
            false,
            parse_review(&owned),
        );
        state
    }

    fn render_to_text(state: &ChangeReviewState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                ChangeReviewView::new(state, theme()).render(frame, frame.area());
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

    #[test]
    fn renders_the_title_files_and_diff() {
        let state = open_review(&[(
            "src/lib.rs",
            "@@ -1,2 +1,2 @@\n fn a() {}\n-old();\n+new();\n",
        )]);
        let text = render_to_text(&state, 90, 24);
        assert!(text.contains("Review"), "the title should render: {text}");
        assert!(text.contains("Rename helper"), "the change title: {text}");
        assert!(text.contains("src/lib.rs"), "the file list: {text}");
        assert!(text.contains("old();"), "the removed line: {text}");
        assert!(text.contains("new();"), "the added line: {text}");
        assert!(text.contains("[a]pply"), "the accept affordance: {text}");
        assert!(
            text.contains("[Esc]reject"),
            "the reject affordance: {text}"
        );
    }

    #[test]
    fn a_refusal_is_rendered_with_the_daemons_own_words() {
        let mut state = open_review(&[("src/lib.rs", "@@ -1,1 +1,1 @@\n-a\n+b\n")]);
        let (generation, request) = (state.generation, state.request.begin());
        state.begin_accept();
        let message =
            "src/lib.rs has unsaved editor changes; save and regenerate the preview".to_string();
        assert!(state.apply_refusal(request, generation, message.clone()));

        let text = render_to_text(&state, 100, 24);
        assert!(
            text.contains("unsaved editor changes"),
            "the actionable detail must be on screen, got:\n{text}"
        );
        assert!(
            text.contains("save and regenerate"),
            "the instruction: {text}"
        );
    }

    #[test]
    fn a_stale_candidate_is_warned_about_before_any_apply() {
        let mut state = open_review(&[("a.rs", "@@ -1,1 +1,1 @@\n-a\n+b\n")]);
        state.stale = true;
        let text = render_to_text(&state, 100, 20);
        assert!(
            text.contains("base content changed"),
            "a stale candidate must be visible, got:\n{text}"
        );
    }

    #[test]
    fn a_truncated_diff_says_it_is_truncated() {
        let mut big = String::from("@@ -1,1 +1,1 @@\n");
        for _ in 0..(crate::tui::unified_diff::MAX_REVIEW_LINES_PER_HUNK * 2) {
            big.push_str("+line\n");
        }
        let state = open_review(&[("big.rs", &big)]);
        let text = render_to_text(&state, 100, 24);
        assert!(
            text.contains("more than") || text.contains("truncated"),
            "a truncated diff must be labelled, got:\n{text}"
        );
    }

    #[test]
    fn a_malformed_patch_is_reported_rather_than_silently_dropped() {
        let state = open_review(&[
            ("good.rs", "@@ -1,1 +1,1 @@\n-a\n+b\n"),
            ("bad.rs", "not a diff"),
        ]);
        let text = render_to_text(&state, 100, 24);
        assert!(
            text.contains("could not be read as a diff"),
            "a skipped file must be surfaced, got:\n{text}"
        );
    }

    #[test]
    fn multiple_files_are_listed_and_the_active_one_is_marked() {
        let mut state = open_review(&[
            ("a.rs", "@@ -1,1 +1,1 @@\n-a\n+b\n"),
            ("b.rs", "@@ -1,1 +1,1 @@\n-c\n+d\n"),
        ]);
        state.move_file(1);
        let text = render_to_text(&state, 90, 24);
        assert!(text.contains("a.rs"), "both files are listed: {text}");
        assert!(text.contains("b.rs"), "both files are listed: {text}");
        assert!(
            text.contains("Diff: b.rs"),
            "the active file is the second one: {text}"
        );
        assert!(
            text.contains("│ d"),
            "the second file's added line should be shown, got:\n{text}"
        );
    }

    #[test]
    fn a_review_with_no_files_renders_a_message_not_a_blank_pane() {
        let state = open_review(&[]);
        let text = render_to_text(&state, 60, 16);
        assert!(text.contains("no diff to show"), "got:\n{text}");
    }

    #[test]
    fn a_zero_area_view_does_not_panic() {
        let state = open_review(&[("a.rs", "@@ -1,1 +1,1 @@\n-a\n+b\n")]);
        let _ = render_to_text(&state, 0, 0);
        let _ = render_to_text(&state, 1, 1);
    }

    #[test]
    fn an_applied_verdict_is_summarised() {
        let mut state = open_review(&[("a.rs", "@@ -1,1 +1,1 @@\n-a\n+b\n")]);
        let generation = state.generation;
        let request = state.request.begin();
        state.begin_accept();
        assert!(state.apply_success(
            request,
            generation,
            vec!["a.rs".to_string()],
            "cp-1".to_string()
        ));
        let text = render_to_text(&state, 90, 20);
        assert!(text.contains("applied to 1 file(s)"), "got:\n{text}");
    }
}
