//! Semantic, frontend-neutral theme schema.
//!
//! This schema is the canonical, in-memory representation of a codegg theme.
//! It is the source of truth that gets projected into ratatui [`Theme`](crate::tui::theme::Theme)
//! values (and, eventually, into iced style catalogs). Importer formats
//! (native codegg TOML, Halloy TOML, future Base16) all decode into
//! [`SemanticTheme`]; importers must not project into ratatui directly.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme::color::Rgb;

/// A resolved, frontend-neutral theme. Each field is a concrete RGB color.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticTheme {
    pub id: String,
    pub name: String,
    pub source: ThemeSource,
    pub base: BaseColors,
    pub ui: UiColors,
    pub text: TextColors,
    pub status: StatusColors,
    pub conversation: ConversationColors,
    pub code: CodeColors,
    pub diff: DiffColors,
    pub agents: AgentColors,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeSource {
    Builtin,
    NativeFile { path: PathBuf },
    HalloyFile { path: PathBuf },
    #[default]
    Inline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseColors {
    pub background: Rgb,
    pub foreground: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiColors {
    pub accent_primary: Rgb,
    pub accent_secondary: Rgb,
    pub border: Rgb,
    pub border_focused: Rgb,
    pub selection: Rgb,
    pub selection_dim: Rgb,
    pub panel_background: Rgb,
    pub input_background: Rgb,
    pub title_background: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextColors {
    pub muted: Rgb,
    pub link: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusColors {
    pub success: Rgb,
    pub warning: Rgb,
    pub error: Rgb,
    pub info: Rgb,
    pub debug: Rgb,
    pub trace: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationColors {
    pub user: Rgb,
    pub assistant: Rgb,
    pub system: Rgb,
    pub tool_call: Rgb,
    pub tool_result: Rgb,
    pub timestamp: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeColors {
    pub foreground: Rgb,
    /// Optional name of a syntect theme for inline code highlighting.
    pub syntect_theme: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffColors {
    pub added: Rgb,
    pub removed: Rgb,
    pub modified: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentColors {
    pub planner: Rgb,
    pub coder: Rgb,
    pub reviewer: Rgb,
    pub tester: Rgb,
    pub security: Rgb,
}

impl SemanticTheme {
    /// Normalize an id to lowercase kebab-case.
    pub fn normalize_id(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut prev_dash = true;
        for ch in input.chars() {
            if ch.is_ascii_alphanumeric() {
                for lower in ch.to_lowercase() {
                    out.push(lower);
                }
                prev_dash = false;
            } else if (ch == '-' || ch == '_' || ch.is_whitespace()) && !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
        while out.ends_with('-') {
            out.pop();
        }
        if out.is_empty() {
            "theme".to_string()
        } else {
            out
        }
    }
}
