//! Projections from frontend-neutral [`SemanticTheme`] to frontend-specific
//! types. Currently only the ratatui [`Theme`](crate::tui::theme::Theme) is
//! supported; when a future iced GUI is added, add another file under
//! `target/` that projects into that frontend's style system.

use crate::theme::color::Rgb;
use crate::theme::schema::SemanticTheme;
use crate::tui::theme::Theme;

fn rgb_to_color(rgb: Rgb) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(rgb.r, rgb.g, rgb.b)
}

impl From<&SemanticTheme> for Theme {
    fn from(s: &SemanticTheme) -> Self {
        Theme {
            name: s.name.clone(),
            background: rgb_to_color(s.base.background),
            foreground: rgb_to_color(s.base.foreground),
            primary: rgb_to_color(s.ui.accent_primary),
            secondary: rgb_to_color(s.ui.accent_secondary),
            success: rgb_to_color(s.status.success),
            warning: rgb_to_color(s.status.warning),
            error: rgb_to_color(s.status.error),
            muted: rgb_to_color(s.text.muted),
            border: rgb_to_color(s.ui.border),
            selection: rgb_to_color(s.ui.selection),
            selection_dim: rgb_to_color(s.ui.selection_dim),
            alternate_bg: rgb_to_color(s.ui.panel_background),
            input_bg: rgb_to_color(s.ui.input_background),
            code_theme: s.code.syntect_theme.clone().unwrap_or_default(),
            link: rgb_to_color(s.text.link),
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
    use super::*;
    use crate::theme::schema::{
        BaseColors, CodeColors, ConversationColors, DiffColors, StatusColors, TextColors,
        UiColors,
    };

    fn sample_semantic() -> SemanticTheme {
        SemanticTheme {
            id: "test".into(),
            name: "Test".into(),
            source: crate::theme::schema::ThemeSource::Builtin,
            base: BaseColors {
                background: Rgb::new(10, 20, 30),
                foreground: Rgb::new(200, 210, 220),
            },
            ui: UiColors {
                accent_primary: Rgb::new(80, 140, 200),
                accent_secondary: Rgb::new(160, 120, 220),
                border: Rgb::new(60, 60, 70),
                border_focused: Rgb::new(120, 120, 140),
                selection: Rgb::new(40, 60, 90),
                selection_dim: Rgb::new(50, 70, 100),
                panel_background: Rgb::new(18, 18, 24),
                input_background: Rgb::new(8, 8, 12),
                title_background: Rgb::new(20, 20, 26),
            },
            text: TextColors {
                muted: Rgb::new(100, 100, 110),
                link: Rgb::new(60, 140, 240),
            },
            status: StatusColors {
                success: Rgb::new(80, 200, 120),
                warning: Rgb::new(255, 180, 60),
                error: Rgb::new(255, 80, 80),
                info: Rgb::new(80, 160, 220),
                debug: Rgb::new(140, 140, 150),
                trace: Rgb::new(120, 120, 130),
            },
            conversation: ConversationColors {
                user: Rgb::new(120, 180, 255),
                assistant: Rgb::new(180, 140, 255),
                system: Rgb::new(150, 150, 160),
                tool_call: Rgb::new(220, 180, 80),
                tool_result: Rgb::new(80, 200, 120),
                timestamp: Rgb::new(110, 110, 120),
            },
            code: CodeColors {
                foreground: Rgb::new(220, 220, 220),
                syntect_theme: Some("base16-ocean.dark".into()),
            },
            diff: DiffColors {
                added: Rgb::new(80, 200, 120),
                removed: Rgb::new(255, 80, 80),
                modified: Rgb::new(255, 180, 60),
            },
            agents: crate::theme::schema::AgentColors {
                planner: Rgb::new(120, 180, 255),
                coder: Rgb::new(180, 140, 255),
                reviewer: Rgb::new(80, 200, 120),
                tester: Rgb::new(255, 180, 60),
                security: Rgb::new(255, 80, 80),
            },
        }
    }

    #[test]
    fn projects_base_palette() {
        let t = Theme::from(&sample_semantic());
        assert_eq!(t.name, "Test");
        assert_eq!(t.code_theme, "base16-ocean.dark");
    }
}
