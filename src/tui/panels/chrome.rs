//! The one producer of a panel column's chrome.
//!
//! Four decisions live here and nowhere else: whether a panel draws chrome at
//! all ([`has_chrome`]), the border style, the title string and the title
//! style. Every panel — the five bespoke ones and the hosted host — paints its
//! edge through [`PanelChrome::draw`], so "this panel is focused" means the
//! same thing on every edge.
//!
//! ## The focus policy
//!
//! | | focused | unfocused |
//! |---|---|---|
//! | border | `theme.primary` | `theme.border` |
//! | title | `theme.primary` | `theme.muted` |
//! | marker | trailing `●` | absent |
//!
//! `Modifier::BOLD` is deliberately not part of the signal. The Agents panel
//! applied it unconditionally, and a style that is always on cannot mark a
//! difference.
//!
//! [`ChromeStyles::resolve`] is public because the dialog frame (`Section`,
//! M003) reuses this resolution rather than restating it. A second copy is how
//! the panel edge and the dialog edge drift apart.
//!
//! ## The chrome predicate
//!
//! Chrome is drawn when the area is at least [`MIN_CHROME_WIDTH`] by
//! [`MIN_CHROME_HEIGHT`]. Below that a panel paints borderless into the whole
//! area: a border plus a title in a three-row column costs the content two
//! rows and wraps the title off the edge. Chat and Terminal already declined
//! below that size, and this is that guard in one place rather than six.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Widget};

use crate::tui::theme::Theme;

/// The smallest area that can carry a border and a title.
pub const MIN_CHROME_WIDTH: u16 = 12;
/// See [`MIN_CHROME_WIDTH`].
pub const MIN_CHROME_HEIGHT: u16 = 4;

/// The glyph that marks the panel holding the keyboard.
pub const FOCUS_MARKER: &str = "●";

/// Whether `area` is large enough for a border and a title.
pub fn has_chrome(area: Rect) -> bool {
    area.width >= MIN_CHROME_WIDTH && area.height >= MIN_CHROME_HEIGHT
}

/// The border and title styles one focus state resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeStyles {
    pub border: Style,
    pub title: Style,
}

impl ChromeStyles {
    /// Resolve the focus policy against `theme`.
    pub fn resolve(theme: &Theme, focused: bool) -> Self {
        Self {
            border: Style::default().fg(if focused { theme.primary } else { theme.border }),
            title: Style::default().fg(if focused { theme.primary } else { theme.muted }),
        }
    }
}

/// The title a panel edge carries: padded, and marked when focused.
///
/// The marker is inside the title rather than a separate widget so that it
/// cannot be painted anywhere but on the border it names.
pub fn chrome_title(label: &str, focused: bool) -> String {
    if focused {
        format!(" {label} {FOCUS_MARKER} ")
    } else {
        format!(" {label} ")
    }
}

/// A panel edge: the border, its title, and the focus marker.
#[derive(Debug, Clone, Copy)]
pub struct PanelChrome<'a> {
    label: &'a str,
    focused: bool,
    theme: &'a Theme,
}

impl<'a> PanelChrome<'a> {
    /// Chrome for a panel named `label`, which must not carry padding or a
    /// marker of its own — [`chrome_title`] adds both.
    pub fn new(label: &'a str, focused: bool, theme: &'a Theme) -> Self {
        Self {
            label,
            focused,
            theme,
        }
    }

    /// The styles this chrome paints with.
    pub fn styles(&self) -> ChromeStyles {
        ChromeStyles::resolve(self.theme, self.focused)
    }

    /// The rendered title.
    pub fn title(&self) -> String {
        chrome_title(self.label, self.focused)
    }

    /// Paint the chrome and answer the area left for content.
    ///
    /// Below the threshold no chrome is drawn: the column's backdrop is filled
    /// and the whole area is returned, so a caller never has to restate the
    /// predicate to find its content rect.
    pub fn draw(&self, area: Rect, buf: &mut Buffer) -> Rect {
        if !has_chrome(area) {
            // No edge, but the column still owns its backdrop: a borderless
            // panel must not fall through to the terminal's default fill.
            buf.set_style(area, Style::default().bg(self.theme.background));
            return area;
        }
        let styles = self.styles();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(styles.border)
            .title(self.title())
            .title_style(styles.title)
            .style(Style::default().bg(self.theme.background));
        let inner = block.inner(area);
        block.render(area, buf);
        inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn theme() -> Theme {
        Theme::dark()
    }

    #[test]
    fn the_focus_policy_resolves_every_style_from_the_theme() {
        let theme = theme();
        let focused = ChromeStyles::resolve(&theme, true);
        let unfocused = ChromeStyles::resolve(&theme, false);
        assert_eq!(focused.border.fg, Some(theme.primary));
        assert_eq!(focused.title.fg, Some(theme.primary));
        assert_eq!(unfocused.border.fg, Some(theme.border));
        assert_eq!(unfocused.title.fg, Some(theme.muted));
        assert_ne!(
            focused.border.fg, unfocused.border.fg,
            "the border must change on focus, or the policy marks nothing"
        );
    }

    #[test]
    fn bold_is_not_part_of_the_focus_signal() {
        let styles = ChromeStyles::resolve(&theme(), true);
        assert!(styles.border.add_modifier.is_empty());
        assert!(styles.title.add_modifier.is_empty());
    }

    #[test]
    fn the_focus_marker_is_a_trailing_glyph_inside_the_title() {
        assert_eq!(chrome_title("Agents", true), " Agents ● ");
        assert_eq!(chrome_title("Agents", false), " Agents ");
    }

    #[test]
    fn the_chrome_predicate_matches_the_guard_it_replaces() {
        assert!(!has_chrome(Rect::new(0, 0, 11, 4)));
        assert!(!has_chrome(Rect::new(0, 0, 12, 3)));
        assert!(has_chrome(Rect::new(0, 0, 12, 4)));
        assert!(has_chrome(Rect::new(0, 0, 80, 24)));
    }

    #[test]
    fn draw_returns_an_inner_rect_inset_by_one_on_every_drawn_edge() {
        let theme = theme();
        let chrome = PanelChrome::new("Chat", true, &theme);
        let area = Rect::new(3, 2, 40, 16);
        let mut buf = Buffer::empty(Rect::new(0, 0, 50, 24));
        let inner = chrome.draw(area, &mut buf);
        assert_eq!(inner, Rect::new(4, 3, 38, 14));
        assert_eq!(buf.cell((3, 2)).map(|cell| cell.symbol()), Some("┌"));
        assert_eq!(buf.cell((42, 17)).map(|cell| cell.symbol()), Some("┘"));
        assert_eq!(
            buf.cell((44, 3)).map(|cell| cell.symbol()),
            Some(" "),
            "a cell outside the area must stay untouched"
        );
    }

    #[test]
    fn draw_below_the_threshold_paints_no_border_and_returns_the_whole_area() {
        let theme = theme();
        let chrome = PanelChrome::new("Chat", true, &theme);
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        let inner = chrome.draw(area, &mut buf);
        assert_eq!(inner, area);
        assert!(
            buf.content
                .iter()
                .all(|cell| !matches!(cell.symbol(), "┌" | "┐" | "└" | "┘")),
            "a borderless column must not carry a corner glyph"
        );
        assert_eq!(
            buf.cell((5, 1)).map(|cell| cell.bg),
            Some(theme.background),
            "the borderless column still owns its theme backdrop"
        );
    }

    #[test]
    fn draw_paints_the_theme_backdrop_behind_the_content() {
        let theme = theme();
        let chrome = PanelChrome::new("Chat", false, &theme);
        let area = Rect::new(0, 0, 40, 12);
        let mut buf = Buffer::empty(area);
        chrome.draw(area, &mut buf);
        assert_eq!(
            buf.cell((20, 6)).map(|cell| cell.bg),
            Some(theme.background)
        );
        assert_eq!(
            buf.cell((0, 0)).map(|cell| cell.fg),
            Some(theme.border),
            "the unfocused edge is `theme.border`"
        );
        assert_ne!(theme.border, Color::Reset);
    }
}
