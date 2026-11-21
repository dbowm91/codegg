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
            id: s.id.clone(),
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
            selection_text: rgb(s.ui.selection_text),
            accent_text: rgb(s.ui.accent_text),
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

#[cfg(test)]
mod tests {
    use crate::theme::registry::{ThemeRegistry, BUILTIN_THEME_FILES};

    fn color_token(color: ratatui::style::Color) -> String {
        match color {
            ratatui::style::Color::Rgb(r, g, b) => format!("rgb({r},{g},{b})"),
            other => format!("debug:{other:?}"),
        }
    }

    fn field(out: &mut String, name: &str, value: &str) {
        out.push_str(name);
        out.push('=');
        out.push_str(value);
        out.push('\n');
    }

    fn project_text() -> String {
        let registry = ThemeRegistry::load_builtins();
        let mut themes = registry.all_tui_themes();
        themes.sort_by(|left, right| left.id.cmp(&right.id));
        let mut out = String::new();
        field(
            &mut out,
            "bundled_files",
            &BUILTIN_THEME_FILES.len().to_string(),
        );
        field(&mut out, "projected", &themes.len().to_string());
        for theme in &themes {
            field(&mut out, "id", &theme.id);
            field(&mut out, "name", &theme.name);
            field(&mut out, "background", &color_token(theme.background));
            field(&mut out, "foreground", &color_token(theme.foreground));
            field(&mut out, "primary", &color_token(theme.primary));
            field(&mut out, "secondary", &color_token(theme.secondary));
            field(&mut out, "success", &color_token(theme.success));
            field(&mut out, "warning", &color_token(theme.warning));
            field(&mut out, "error", &color_token(theme.error));
            field(&mut out, "muted", &color_token(theme.muted));
            field(&mut out, "border", &color_token(theme.border));
            field(&mut out, "selection", &color_token(theme.selection));
            field(&mut out, "selection_dim", &color_token(theme.selection_dim));
            field(
                &mut out,
                "selection_text",
                &color_token(theme.selection_text),
            );
            field(&mut out, "accent_text", &color_token(theme.accent_text));
            field(&mut out, "alternate_bg", &color_token(theme.alternate_bg));
            field(&mut out, "input_bg", &color_token(theme.input_bg));
            field(&mut out, "code_theme", &theme.code_theme);
            field(&mut out, "link", &color_token(theme.link));
        }
        out
    }

    fn fixture_path() -> std::path::PathBuf {
        let relative = std::path::Path::new("tests/fixtures/tui-theme-projection/bundled.txt");
        let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        loop {
            let candidate = dir.join(relative);
            if candidate.is_file() {
                return candidate;
            }
            if !dir.pop() {
                break;
            }
        }
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    #[test]
    fn bundled_theme_ratatui_projection_matches_the_frozen_fixture() {
        let path = fixture_path();
        let projected = project_text();
        let frozen = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "frozen theme projection missing at {}: {error}",
                path.display()
            )
        });
        assert_eq!(
            frozen, projected,
            "bundled theme projection drifted from the pre-move fixture"
        );
    }
}
