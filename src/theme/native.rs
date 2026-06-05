//! Native codegg theme file format (TOML).
//!
//! All colors are strings. The file mirrors the [`SemanticTheme`](crate::theme::schema::SemanticTheme)
//! schema but in a flat, import-friendly shape.

use std::path::PathBuf;

use serde::Deserialize;

use crate::theme::color::Rgb;
use crate::theme::error::ThemeError;
use crate::theme::schema::{
    AgentColors, BaseColors, CodeColors, ConversationColors, DiffColors, SemanticTheme,
    StatusColors, TextColors, ThemeSource, UiColors,
};

/// Raw native schema as it appears on disk. Every color is a string.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NativeThemeFile {
    pub meta: NativeMeta,
    pub base: NativeBase,
    pub ui: NativeUi,
    pub text: NativeText,
    pub status: NativeStatus,
    pub conversation: NativeConversation,
    pub code: NativeCode,
    pub diff: NativeDiff,
    pub agents: NativeAgent,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NativeMeta {
    pub id: Option<String>,
    pub name: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NativeBase {
    pub background: Option<String>,
    pub foreground: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NativeUi {
    pub accent_primary: Option<String>,
    pub accent_secondary: Option<String>,
    pub border: Option<String>,
    pub border_focused: Option<String>,
    pub selection: Option<String>,
    pub selection_dim: Option<String>,
    pub panel_background: Option<String>,
    pub input_background: Option<String>,
    pub title_background: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NativeText {
    pub muted: Option<String>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NativeStatus {
    pub success: Option<String>,
    pub warning: Option<String>,
    pub error: Option<String>,
    pub info: Option<String>,
    pub debug: Option<String>,
    pub trace: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NativeConversation {
    pub user: Option<String>,
    pub assistant: Option<String>,
    pub system: Option<String>,
    pub tool_call: Option<String>,
    pub tool_result: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NativeCode {
    pub foreground: Option<String>,
    pub syntect_theme: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NativeDiff {
    pub added: Option<String>,
    pub removed: Option<String>,
    pub modified: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NativeAgent {
    pub planner: Option<String>,
    pub coder: Option<String>,
    pub reviewer: Option<String>,
    pub tester: Option<String>,
    pub security: Option<String>,
}

impl NativeThemeFile {
    /// Parse a native theme TOML string.
    pub fn parse(input: &str) -> Result<Self, ThemeError> {
        toml::from_str(input).map_err(|e| ThemeError::TomlParse(e.to_string()))
    }
}

/// Parse a native theme TOML string into a [`SemanticTheme`]. The fallback
/// theme supplies any colors that are absent from the input.
pub fn parse_native_theme(
    input: &str,
    source: ThemeSource,
    fallback: &SemanticTheme,
) -> Result<SemanticTheme, ThemeError> {
    let file = NativeThemeFile::parse(input)?;
    Ok(resolve(file, source, fallback))
}

fn resolve(
    file: NativeThemeFile,
    source: ThemeSource,
    fallback: &SemanticTheme,
) -> SemanticTheme {
    let id = file
        .meta
        .id
        .as_deref()
        .map(SemanticTheme::normalize_id)
        .or_else(|| {
            if let ThemeSource::NativeFile { path } = &source {
                path.file_stem().map(|s| SemanticTheme::normalize_id(&s.to_string_lossy()))
            } else if let ThemeSource::HalloyFile { path } = &source {
                path.file_stem().map(|s| SemanticTheme::normalize_id(&s.to_string_lossy()))
            } else {
                None
            }
        })
        .unwrap_or_else(|| fallback.id.clone());

    let name = file.meta.name.unwrap_or_else(|| id.clone());

    SemanticTheme {
        id: id.clone(),
        name,
        source,
        base: BaseColors {
            background: pick(file.base.background, fallback.base.background),
            foreground: pick(file.base.foreground, fallback.base.foreground),
        },
        ui: UiColors {
            accent_primary: pick(file.ui.accent_primary, fallback.ui.accent_primary),
            accent_secondary: pick(file.ui.accent_secondary, fallback.ui.accent_secondary),
            border: pick(file.ui.border, fallback.ui.border),
            border_focused: pick(file.ui.border_focused, fallback.ui.border_focused),
            selection: pick(file.ui.selection, fallback.ui.selection),
            selection_dim: pick(file.ui.selection_dim, fallback.ui.selection_dim),
            panel_background: pick(
                file.ui.panel_background,
                fallback.ui.panel_background,
            ),
            input_background: pick(file.ui.input_background, fallback.ui.input_background),
            title_background: pick(file.ui.title_background, fallback.ui.title_background),
        },
        text: TextColors {
            muted: pick(file.text.muted, fallback.text.muted),
            link: pick(file.text.link, fallback.text.link),
        },
        status: StatusColors {
            success: pick(file.status.success, fallback.status.success),
            warning: pick(file.status.warning, fallback.status.warning),
            error: pick(file.status.error, fallback.status.error),
            info: pick(file.status.info, fallback.status.info),
            debug: pick(file.status.debug, fallback.status.debug),
            trace: pick(file.status.trace, fallback.status.trace),
        },
        conversation: ConversationColors {
            user: pick(file.conversation.user, fallback.conversation.user),
            assistant: pick(file.conversation.assistant, fallback.conversation.assistant),
            system: pick(file.conversation.system, fallback.conversation.system),
            tool_call: pick(file.conversation.tool_call, fallback.conversation.tool_call),
            tool_result: pick(
                file.conversation.tool_result,
                fallback.conversation.tool_result,
            ),
            timestamp: pick(file.conversation.timestamp, fallback.conversation.timestamp),
        },
        code: CodeColors {
            foreground: pick(file.code.foreground, fallback.code.foreground),
            syntect_theme: file.code.syntect_theme.or_else(|| fallback.code.syntect_theme.clone()),
        },
        diff: DiffColors {
            added: pick(file.diff.added, fallback.diff.added),
            removed: pick(file.diff.removed, fallback.diff.removed),
            modified: pick(file.diff.modified, fallback.diff.modified),
        },
        agents: AgentColors {
            planner: pick(file.agents.planner, fallback.agents.planner),
            coder: pick(file.agents.coder, fallback.agents.coder),
            reviewer: pick(file.agents.reviewer, fallback.agents.reviewer),
            tester: pick(file.agents.tester, fallback.agents.tester),
            security: pick(file.agents.security, fallback.agents.security),
        },
    }
}

fn pick(value: Option<String>, fallback: Rgb) -> Rgb {
    match value {
        Some(raw) => match Rgb::from_hex(&raw) {
            Ok(rgb) => rgb,
            Err(_) => fallback,
        },
        None => fallback,
    }
}

/// Convenience: build a native theme from a file on disk.
pub fn parse_native_file(
    path: &std::path::Path,
    fallback: &SemanticTheme,
) -> Result<SemanticTheme, ThemeError> {
    let content = std::fs::read_to_string(path)?;
    parse_native_theme(
        &content,
        ThemeSource::NativeFile {
            path: path.to_path_buf(),
        },
        fallback,
    )
}

/// Sanity check path used by bundled defaults.
#[allow(dead_code)]
pub fn bundled_default_path() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::schema::ThemeSource;
    use crate::theme::theme_registry_test_helpers::test_fallback;

    #[test]
    fn parses_minimal_native() {
        let input = r###"
            [meta]
            id = "minimal"
            name = "Minimal"

            [base]
            background = "#101010"
            foreground = "#eeeeee"
        "###;
        let fallback = test_fallback();
        let theme = parse_native_theme(input, ThemeSource::Inline, &fallback).unwrap();
        assert_eq!(theme.id, "minimal");
        assert_eq!(theme.base.background.to_hex(), "#101010");
        assert_eq!(theme.base.foreground.to_hex(), "#eeeeee");
    }

    #[test]
    fn missing_required_color_falls_back() {
        // Only background is given. Foreground should come from the fallback.
        let input = r###"
            [meta]
            id = "no-fg"

            [base]
            background = "#000000"
        "###;
        let fallback = test_fallback();
        let theme = parse_native_theme(input, ThemeSource::Inline, &fallback).unwrap();
        assert_eq!(theme.base.background.to_hex(), "#000000");
        assert_eq!(theme.base.foreground, fallback.base.foreground);
    }

    #[test]
    fn invalid_color_falls_back_to_dark_value() {
        let input = r###"
            [meta]
            id = "bad"

            [base]
            background = "not-a-color"
            foreground = "#000000"
        "###;
        let fallback = test_fallback();
        let theme = parse_native_theme(input, ThemeSource::Inline, &fallback).unwrap();
        // Invalid color resolves to the fallback's background.
        assert_eq!(theme.base.background, fallback.base.background);
    }

    #[test]
    fn id_derived_from_path_when_absent() {
        let input = r###"
            [base]
            background = "#101010"
            foreground = "#eeeeee"
        "###;
        let fallback = test_fallback();
        let theme = parse_native_theme(
            input,
            ThemeSource::NativeFile {
                path: PathBuf::from("/themes/My Custom Theme.toml"),
            },
            &fallback,
        )
        .unwrap();
        assert_eq!(theme.id, "my-custom-theme");
    }

    #[test]
    fn all_builtins_round_trip() {
        for entry in super::super::registry::BUILTIN_THEME_FILES.iter() {
            let fallback = test_fallback();
            // Bundled themes are Halloy-format. The registry dispatches on
            // `looks_like_halloy`; do the same here so this test only asserts
            // the native parser on native files.
            if super::super::halloy::looks_like_halloy(entry.content) {
                let (theme, _diags) = super::super::halloy::parse_halloy_theme(
                    entry.content,
                    Some(std::path::Path::new(&format!("{}.toml", entry.name))),
                    &fallback,
                )
                .unwrap_or_else(|e| {
                    panic!("failed to parse built-in {}: {}", entry.id, e)
                });
                assert_eq!(theme.id, entry.id, "id mismatch for built-in {}", entry.id);
            } else {
                let theme = parse_native_theme(entry.content, ThemeSource::Builtin, &fallback)
                    .unwrap_or_else(|e| {
                        panic!("failed to parse built-in {}: {}", entry.id, e)
                    });
                assert_eq!(&theme.id, entry.id, "id mismatch for built-in {}", entry.id);
            }
        }
    }
}
