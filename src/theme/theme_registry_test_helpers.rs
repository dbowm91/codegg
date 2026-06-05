//! Test-only helpers for the theme module.

#![allow(dead_code)]

use crate::theme::color::Rgb;
use crate::theme::schema::{
    AgentColors, BaseColors, CodeColors, ConversationColors, DiffColors, SemanticTheme,
    StatusColors, TextColors, ThemeSource, UiColors,
};

/// Build a minimal fallback for tests that just need any valid
/// [`SemanticTheme`].
pub fn test_fallback() -> SemanticTheme {
    SemanticTheme {
        id: "fallback".to_string(),
        name: "fallback".to_string(),
        source: ThemeSource::Inline,
        base: BaseColors {
            background: Rgb::new(0, 0, 0),
            foreground: Rgb::new(255, 255, 255),
        },
        ui: UiColors {
            accent_primary: Rgb::new(120, 180, 255),
            accent_secondary: Rgb::new(180, 140, 255),
            border: Rgb::new(50, 50, 60),
            border_focused: Rgb::new(120, 180, 255),
            selection: Rgb::new(40, 60, 90),
            selection_dim: Rgb::new(50, 70, 100),
            panel_background: Rgb::new(18, 18, 24),
            input_background: Rgb::new(8, 8, 12),
            title_background: Rgb::new(18, 18, 24),
        },
        text: TextColors {
            muted: Rgb::new(140, 140, 150),
            link: Rgb::new(60, 140, 240),
        },
        status: StatusColors {
            success: Rgb::new(80, 200, 120),
            warning: Rgb::new(255, 180, 60),
            error: Rgb::new(255, 80, 80),
            info: Rgb::new(120, 180, 255),
            debug: Rgb::new(180, 140, 255),
            trace: Rgb::new(140, 140, 150),
        },
        conversation: ConversationColors {
            user: Rgb::new(220, 220, 225),
            assistant: Rgb::new(180, 140, 255),
            system: Rgb::new(140, 140, 150),
            tool_call: Rgb::new(120, 180, 255),
            tool_result: Rgb::new(80, 200, 120),
            timestamp: Rgb::new(140, 140, 150),
        },
        code: CodeColors {
            foreground: Rgb::new(220, 220, 225),
            syntect_theme: Some("base16-ocean.dark".to_string()),
        },
        diff: DiffColors {
            added: Rgb::new(80, 200, 120),
            removed: Rgb::new(255, 80, 80),
            modified: Rgb::new(255, 180, 60),
        },
        agents: AgentColors {
            planner: Rgb::new(120, 180, 255),
            coder: Rgb::new(80, 200, 120),
            reviewer: Rgb::new(255, 180, 60),
            tester: Rgb::new(100, 220, 200),
            security: Rgb::new(255, 80, 80),
        },
    }
}
