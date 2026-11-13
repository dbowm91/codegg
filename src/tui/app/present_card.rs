//! The turn-tail deliverable card.
//!
//! It renders the files a turn declared through the `present` tool, and it
//! keeps **no** copy of what was delivered: the card is derived from the
//! session's own message list every frame, so the tool result in the
//! transcript stays the single record and a session that never declared
//! anything renders nothing at all.
//!
//! The parser is the tool's own ([`crate::tool::present::parse_present_display`]),
//! so the card cannot drift from the text it reads, and a refusal — which is a
//! failed tool call at its own position in the transcript — never parses and
//! therefore never renders here.

use crate::session::message::ToolStatus;
use crate::tool::present::{parse_present_display, PresentedFile, MAX_PRESENTED_FILES};
use crate::tui::components::messages::{MessagesWidget, MsgPart};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Most rows the card may occupy, header included. A persistent surface has to
/// be bounded, and a clipped list is honest only because the header states how
/// many files it is showing.
pub const MAX_PRESENT_CARD_ROWS: u16 = 8;

/// The latest declaration in the transcript, if the most recent `present` call
/// succeeded.
///
/// The *latest* call decides: if it was refused, the refusal is the answer and
/// an older declaration must not be shown in its place.
pub fn latest_declaration(messages: &MessagesWidget) -> Option<Vec<PresentedFile>> {
    for index in (0..messages.message_count()).rev() {
        let Some(message) = messages.get_message(index) else {
            continue;
        };
        for part in message.parts.iter().rev() {
            let MsgPart::ToolCall {
                name,
                output,
                status,
                ..
            } = part
            else {
                continue;
            };
            if name != "present" {
                continue;
            }
            if !matches!(status, ToolStatus::Completed) {
                return None;
            }
            return parse_present_display(output);
        }
    }
    None
}

/// Rows the card needs for a declaration, bounded by [`MAX_PRESENT_CARD_ROWS`]
/// and by the files the tool can record.
pub fn card_height(files: &[PresentedFile]) -> u16 {
    let listed = files.len().min(MAX_PRESENTED_FILES);
    (1 + listed as u16).min(MAX_PRESENT_CARD_ROWS)
}

/// Draw the card. Every row is ellipsized rather than wrapped, so the height
/// `card_height` reserved is the height it paints and the transcript below
/// cannot be reflowed by a long path.
pub fn render(frame: &mut Frame, area: Rect, theme: &Theme, files: &[PresentedFile]) {
    if area.width == 0 || area.height == 0 || files.is_empty() {
        return;
    }
    let shown = files.len().min((area.height as usize).saturating_sub(1));
    let mut header = format!("delivered {} file", files.len());
    if files.len() != 1 {
        header.push('s');
    }
    if shown < files.len() {
        header.push_str(&format!(" (showing {shown})"));
    }
    let mut lines = vec![Line::from(Span::styled(
        header,
        Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD),
    ))];
    let budget = (area.width as usize).saturating_sub(2);
    for file in files.iter().take(shown) {
        let mut text = file.path.clone();
        if let Some(description) = &file.description {
            text.push_str("  ");
            text.push_str(description);
        }
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                crate::tui::wrap::ellipsize_to_width(&text, budget),
                Style::default().fg(theme.foreground),
            ),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.background)),
        area,
    );
}
