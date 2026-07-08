//! Projections from [`SemanticTheme`] to frontend-specific theme types.
//!
//! Each target frontend gets its own file (or section) that maps the
//! canonical [`SemanticTheme`](super::schema::SemanticTheme) into the
//! frontend's style system. Currently the only target is `ratatui`.
//!
//! ## Syntect theme selection
//!
//! The semantic theme carries an optional `syntect_theme` name (e.g.
//! `base16-ocean.dark`). When the embedded name matches the luminance
//! of the resolved theme we keep it; otherwise we fall back to a
//! syntect theme whose dark/light suffix agrees with the theme's
//! background. This avoids rendering dark-on-dark syntax highlighting
//! inside light themes (and vice versa).

use ratatui::style::Color;

use super::color::Rgb;
use super::schema::SemanticTheme;
use crate::tui::theme::Theme;

const DEFAULT_DARK_CODE_THEME: &str = "base16-ocean.dark";
const DEFAULT_LIGHT_CODE_THEME: &str = "base16-github.light";

fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

fn is_light_background(theme: &SemanticTheme) -> bool {
    !theme.base.background.is_dark()
}

fn is_light_code_theme(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains(".light") || lower.ends_with("light")
}

fn select_code_theme(theme: &SemanticTheme) -> String {
    let wants_light = is_light_background(theme);
    match theme.code.syntect_theme.as_deref() {
        Some(name) if is_light_code_theme(name) == wants_light => name.to_string(),
        Some(_) | None => {
            if wants_light {
                DEFAULT_LIGHT_CODE_THEME.to_string()
            } else {
                DEFAULT_DARK_CODE_THEME.to_string()
            }
        }
    }
}

impl From<&SemanticTheme> for Theme {
    fn from(s: &SemanticTheme) -> Self {
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
            code_theme: select_code_theme(s),
            link: rgb(s.text.link),
        }
    }
}

impl From<SemanticTheme> for Theme {
    fn from(s: SemanticTheme) -> Self {
        Theme::from(&s)
    }
}
