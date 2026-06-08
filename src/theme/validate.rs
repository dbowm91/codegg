//! Validation diagnostics for [`SemanticTheme`](crate::theme::schema::SemanticTheme).
//!
//! Diagnostics are non-fatal. They are reported by the registry and may be
//! surfaced through logs or a `/theme diagnostics` command.

use crate::theme::color::Rgb;
use crate::theme::schema::SemanticTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeDiagnosticLevel {
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct ThemeDiagnostic {
    pub level: ThemeDiagnosticLevel,
    pub theme_id: String,
    pub field: Option<String>,
    pub message: String,
}

impl ThemeDiagnostic {
    pub fn warn(
        theme_id: impl Into<String>,
        field: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            level: ThemeDiagnosticLevel::Warning,
            theme_id: theme_id.into(),
            field: field.map(|s| s.to_string()),
            message: message.into(),
        }
    }

    pub fn error(
        theme_id: impl Into<String>,
        field: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            level: ThemeDiagnosticLevel::Error,
            theme_id: theme_id.into(),
            field: field.map(|s| s.to_string()),
            message: message.into(),
        }
    }
}

/// Pragmatic contrast thresholds. Terminal UIs do not need full WCAG AAA, but
/// the existing 31 themes were hand-curated, so we lean on the conservative
/// side.
const PRIMARY_TEXT_RATIO: f64 = 4.5;
const MUTED_TEXT_RATIO: f64 = 3.0;
const ACCENT_RATIO: f64 = 3.0;
const SELECTION_TEXT_RATIO: f64 = 3.0;

pub fn validate_theme(theme: &SemanticTheme) -> Vec<ThemeDiagnostic> {
    let mut out = Vec::new();
    let bg = theme.base.background;
    let fg = theme.base.foreground;

    check(
        &mut out,
        theme,
        "base.foreground",
        fg,
        bg,
        PRIMARY_TEXT_RATIO,
    );
    check(
        &mut out,
        theme,
        "text.muted",
        theme.text.muted,
        bg,
        MUTED_TEXT_RATIO,
    );
    check(
        &mut out,
        theme,
        "ui.accent_primary",
        theme.ui.accent_primary,
        bg,
        ACCENT_RATIO,
    );
    check(
        &mut out,
        theme,
        "ui.accent_primary (on selection)",
        theme.ui.accent_primary,
        theme.ui.selection,
        SELECTION_TEXT_RATIO,
    );
    check(
        &mut out,
        theme,
        "base.foreground (on input background)",
        fg,
        theme.ui.input_background,
        PRIMARY_TEXT_RATIO,
    );
    check(
        &mut out,
        theme,
        "status.error",
        theme.status.error,
        bg,
        ACCENT_RATIO,
    );
    check(
        &mut out,
        theme,
        "status.warning",
        theme.status.warning,
        bg,
        ACCENT_RATIO,
    );
    check(
        &mut out,
        theme,
        "status.success",
        theme.status.success,
        bg,
        ACCENT_RATIO,
    );
    check(
        &mut out,
        theme,
        "text.link",
        theme.text.link,
        bg,
        ACCENT_RATIO,
    );

    out
}

fn check(
    out: &mut Vec<ThemeDiagnostic>,
    theme: &SemanticTheme,
    field: &str,
    fg: Rgb,
    bg: Rgb,
    threshold: f64,
) {
    let ratio = fg.contrast_ratio(bg);
    if ratio < threshold {
        out.push(ThemeDiagnostic::warn(
            &theme.id,
            Some(field),
            format!("contrast {:.2} < {:.2}", ratio, threshold),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::color::Rgb;
    use crate::theme::schema::{
        AgentColors, BaseColors, CodeColors, ConversationColors, DiffColors, SemanticTheme,
        StatusColors, TextColors, ThemeSource, UiColors,
    };

    fn build_theme(fg: Rgb, bg: Rgb) -> SemanticTheme {
        SemanticTheme {
            id: "test".to_string(),
            name: "Test".to_string(),
            source: ThemeSource::Inline,
            base: BaseColors {
                background: bg,
                foreground: fg,
            },
            ui: UiColors {
                accent_primary: fg,
                accent_secondary: fg,
                border: fg,
                border_focused: fg,
                selection: bg,
                selection_dim: bg,
                panel_background: bg,
                input_background: bg,
                title_background: bg,
            },
            text: TextColors {
                muted: fg,
                link: fg,
            },
            status: StatusColors {
                success: fg,
                warning: fg,
                error: fg,
                info: fg,
                debug: fg,
                trace: fg,
            },
            conversation: ConversationColors {
                user: fg,
                assistant: fg,
                system: fg,
                tool_call: fg,
                tool_result: fg,
                timestamp: fg,
            },
            code: CodeColors {
                foreground: fg,
                syntect_theme: None,
            },
            diff: DiffColors {
                added: fg,
                removed: fg,
                modified: fg,
            },
            agents: AgentColors {
                planner: fg,
                coder: fg,
                reviewer: fg,
                tester: fg,
                security: fg,
            },
        }
    }

    #[test]
    fn low_contrast_emits_warning() {
        // Foreground almost equal to background: extremely low contrast.
        let theme = build_theme(Rgb::new(20, 20, 20), Rgb::new(25, 25, 25));
        let diags = validate_theme(&theme);
        assert!(!diags.is_empty(), "expected warnings for low contrast");
        assert!(diags
            .iter()
            .any(|d| d.field.as_deref() == Some("base.foreground")));
    }

    #[test]
    fn high_contrast_emits_no_primary_warning() {
        let theme = build_theme(Rgb::new(255, 255, 255), Rgb::new(0, 0, 0));
        let diags = validate_theme(&theme);
        assert!(!diags
            .iter()
            .any(|d| d.field.as_deref() == Some("base.foreground")));
    }
}
