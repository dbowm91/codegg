//! Projections from [`SemanticTheme`] to frontend-specific theme types.
//!
//! Each target frontend gets its own file (or section) that maps the
//! canonical [`SemanticTheme`](super::schema::SemanticTheme) into the
//! frontend's style system. Currently the only target is `ratatui`.

use ratatui::style::Color;

use super::schema::SemanticTheme;
use crate::tui::theme::Theme;

impl From<&SemanticTheme> for Theme {
    fn from(s: &SemanticTheme) -> Self {
        let rgb = |c: super::color::Rgb| Color::Rgb(c.r, c.g, c.b);
        Theme {
            name: s.name.clone(),
            background: rgb(s.base.background),
            foreground: rgb(s.base.foreground),
            primary: rgb(s.ui.accent_primary),
            secondary: rgb(s.ui.accent_secondary),
            success: rgb(s.status.success),
            warning: rgb(s.status.warning),
            error: rgb(s.status.error),
            muted: rgb(s.text.muted),
            border: rgb(s.ui.border),
            selection: rgb(s.ui.selection),
            selection_dim: rgb(s.ui.selection_dim),
            alternate_bg: rgb(s.ui.panel_background),
            input_bg: rgb(s.ui.input_background),
            code_theme: s
                .code
                .syntect_theme
                .clone()
                .unwrap_or_else(|| "base16-ocean.dark".to_string()),
            link: rgb(s.text.link),
        }
    }
}

impl From<SemanticTheme> for Theme {
    fn from(s: SemanticTheme) -> Self {
        Theme::from(&s)
    }
}
