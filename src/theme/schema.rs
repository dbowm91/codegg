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
    NativeFile {
        path: PathBuf,
    },
    HalloyFile {
        path: PathBuf,
    },
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
    /// Foreground to draw **on** `selection`. Derived, never authored.
    ///
    /// A selected row is a filled surface, so it needs a foreground that reads
    /// against that fill — the same problem `selection_dim` solves for a
    /// background, one level up. Nothing upstream can supply it: Halloy
    /// declares `buttons.*.foreground_selected` but **0 of the 50 bundled
    /// themes define it**, so reading that slot would yield `None` for every
    /// real theme and ship a token that looks correct and is always empty.
    /// Deriving it instead imposes no requirement on theme authors, which is
    /// the property that lets any Halloy theme ingest unchanged.
    pub selection_text: Rgb,
    /// Foreground to draw **on** `accent_primary`. Derived on the same rule.
    pub accent_text: Rgb,
    pub panel_background: Rgb,
    pub input_background: Rgb,
    pub title_background: Rgb,
}

/// Minimum WCAG contrast a derived on-surface foreground must reach before it
/// is accepted. Matches `validate.rs`'s `SELECTION_TEXT_RATIO`, which is the
/// bar these tokens are held to.
pub const MIN_DERIVED_TEXT_RATIO: f64 = 3.0;

/// Pick the more legible of the theme's own two body colours as a foreground
/// for `surface`.
///
/// A theme always carries a foreground that reads on its background and a
/// background that reads on itself; one of the two also reads on any *other*
/// fill the theme paints over. Which one is a property of the theme, not of the
/// widget, so it is derived once here rather than chosen per call site.
///
/// Neither candidate is guaranteed: a mid-grey selection over a mid-grey
/// palette can defeat both. When that happens the answer is whichever of black
/// or white reads better on the surface. Taking the maximum of the two is what
/// makes the fallback a *guarantee* rather than a guess — it always clears
/// 4.5:1, where picking by `is_dark` would leave a surface at luminance 0.3
/// with white and a ratio of 2.1.
pub fn derive_text_on(body_foreground: Rgb, body_background: Rgb, surface: Rgb) -> Rgb {
    let best = if body_foreground.contrast_ratio(surface) >= body_background.contrast_ratio(surface)
    {
        body_foreground
    } else {
        body_background
    };
    if best.contrast_ratio(surface) >= MIN_DERIVED_TEXT_RATIO {
        return best;
    }
    let black = Rgb::new(0, 0, 0);
    if black.contrast_ratio(surface) >= Rgb::new(255, 255, 255).contrast_ratio(surface) {
        black
    } else {
        Rgb::new(255, 255, 255)
    }
}

impl UiColors {
    /// Fill the two derived selection foregrounds from the theme's body colours.
    ///
    /// Called by every importer *after* `base` and `ui` are fully resolved, so a
    /// theme that overrides `selection` or `accent_primary` in its own file
    /// derives against its own value rather than inheriting a token derived
    /// from the fallback's.
    pub fn derive_selection_texts(&mut self, body_foreground: Rgb, body_background: Rgb) {
        self.selection_text = derive_text_on(body_foreground, body_background, self.selection);
        self.accent_text = derive_text_on(body_foreground, body_background, self.accent_primary);
    }
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
