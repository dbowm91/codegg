//! Halloy compatibility parser.
//!
//! Halloy (an iced IRC client) stores themes as TOML with sections
//! `[general]`, `[text]`, `[buttons.primary]`, `[buttons.secondary]`,
//! `[buffer]`, `[buffer.server_messages]`, and `[formatting]`. We do not
//! import all of those; only the fields that map cleanly into the
//! [`SemanticTheme`](crate::theme::schema::SemanticTheme) schema.
//!
//! The mapping is approximate and lossy on purpose. Halloy's
//! `text.success/error/info/...` are not always present in every Halloy
//! theme; missing fields fall back to the codegg fallback theme.

use std::path::Path;

use serde::Deserialize;

use crate::theme::color::Rgb;
use crate::theme::error::ThemeError;
use crate::theme::native::parse_native_theme;
use crate::theme::schema::{SemanticTheme, ThemeSource};
use crate::theme::validate::{validate_theme, ThemeDiagnostic};

/// Halloy text style values can be either a plain color string or a table
/// containing a `color` plus optional `font_style`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum HalloyTextStyle {
    Color(String),
    Style {
        color: String,
        #[serde(default)]
        font_style: Option<HalloyFontStyle>,
    },
}

impl HalloyTextStyle {
    pub fn color(&self) -> &str {
        match self {
            HalloyTextStyle::Color(c) => c.as_str(),
            HalloyTextStyle::Style { color, .. } => color.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HalloyFontStyle {
    Normal,
    Italic,
    Bold,
    ItalicBold,
}

/// Permissive section struct for Halloy. Only the keys we know how to map
/// are actually consulted. Extra keys are ignored.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HalloyFile {
    pub general: HalloyGeneral,
    pub text: HalloyText,
    pub buttons: HalloyButtons,
    pub buffer: HalloyBuffer,
    pub formatting: HalloyFormatting,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HalloyGeneral {
    pub background: Option<String>,
    pub border: Option<String>,
    pub highlight_indicator: Option<String>,
    /// Divider line color.
    pub horizontal_rule: Option<String>,
    /// Color for unread-message / notification indicators.
    pub unread_indicator: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HalloyText {
    pub primary: Option<HalloyTextStyle>,
    pub secondary: Option<HalloyTextStyle>,
    pub tertiary: Option<HalloyTextStyle>,
    pub success: Option<HalloyTextStyle>,
    pub warning: Option<HalloyTextStyle>,
    pub error: Option<HalloyTextStyle>,
    pub info: Option<HalloyTextStyle>,
    pub debug: Option<HalloyTextStyle>,
    pub trace: Option<HalloyTextStyle>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HalloyButtons {
    pub primary: Option<HalloyButtonSection>,
    pub secondary: Option<HalloyButtonSection>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HalloyButtonSection {
    pub background: Option<String>,
    pub background_selected: Option<String>,
    pub foreground: Option<String>,
    pub foreground_selected: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HalloyBuffer {
    pub background: Option<String>,
    pub background_text_input: Option<String>,
    pub background_title_bar: Option<String>,
    pub border: Option<String>,
    pub border_selected: Option<String>,
    pub selection: Option<HalloyTextStyle>,
    pub code: Option<HalloyTextStyle>,
    pub url: Option<HalloyTextStyle>,
    pub timestamp: Option<HalloyTextStyle>,
    /// Color for action messages (e.g. `* alice waves`).
    pub action: Option<HalloyTextStyle>,
    /// Color for channel topic text.
    pub topic: Option<HalloyTextStyle>,
    /// Color for nicknames in chat messages.
    pub nickname: Option<HalloyTextStyle>,
    /// Color used to highlight a line of text (mentions, etc).
    pub highlight: Option<HalloyTextStyle>,
    pub server_messages: Option<HalloyServerMessages>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HalloyServerMessages {
    pub join: Option<HalloyTextStyle>,
    pub part: Option<HalloyTextStyle>,
    pub nick: Option<HalloyTextStyle>,
    /// Default color used when no join/part/nick-specific color is set.
    pub default: Option<HalloyTextStyle>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HalloyFormatting {
    pub highlight: Option<String>,
}

/// Parse a Halloy theme TOML string and overlay it on the fallback.
///
/// Returns the resolved [`SemanticTheme`] along with any
/// [`ThemeDiagnostic`]s accumulated during parsing.
pub fn parse_halloy_theme(
    input: &str,
    source_path: Option<&Path>,
    fallback: &SemanticTheme,
) -> Result<(SemanticTheme, Vec<ThemeDiagnostic>), ThemeError> {
    let file: HalloyFile =
        toml::from_str(input).map_err(|e| ThemeError::TomlParse(e.to_string()))?;

    let source = match source_path {
        Some(p) => ThemeSource::HalloyFile { path: p.to_path_buf() },
        None => ThemeSource::Inline,
    };

    let id = source_path
        .and_then(|p| p.file_stem())
        .map(|s| SemanticTheme::normalize_id(&s.to_string_lossy()))
        .unwrap_or_else(|| fallback.id.clone());

    let mut diagnostics = Vec::new();

    let mut theme = fallback.clone();
    theme.id = id.clone();
    theme.name = id.clone();
    theme.source = source;

    if let Some(bg) = parse_color(&file.general.background, &mut diagnostics, &id, "general.background") {
        theme.base.background = bg;
    }
    if let Some(fg) = parse_text_color(
        &file.text.primary,
        &mut diagnostics,
        &id,
        "text.primary",
    ) {
        theme.base.foreground = fg;
        // `text.primary` is the canonical "default text" color. Use it for
        // user / assistant / system message text so chat reads consistently
        // across all Halloy themes regardless of how the theme author
        // named their nickname / topic slots.
        theme.conversation.user = fg;
        theme.conversation.assistant = fg;
        theme.conversation.system = fg;
    }
    // `text.tertiary` is a softer muted tier and is the canonical Halloy
    // slot for "muted / secondary text". `text.secondary` would be
    // redundant — its value is usually a darker shade of the theme's
    // accent and clashing when used as a muted foreground.
    if let Some(muted) = parse_text_color(
        &file.text.tertiary,
        &mut diagnostics,
        &id,
        "text.tertiary",
    ) {
        theme.text.muted = muted;
    }
    // Uniform border: pick one source and use it for both `ui.border` and
    // `ui.border_focused`. The TUI's border usage is dominated by neutral
    // chrome (panels, dialogs, list tracks) which shouldn't shift color
    // when an element gains focus. The fallback chain prefers the most
    // "prominent" border the theme author provided.
    let border_source = file
        .buffer
        .border_selected
        .clone()
        .or_else(|| file.buffer.border.clone())
        .or_else(|| file.general.border.clone())
        .or_else(|| file.general.horizontal_rule.clone());
    if let Some(c) = parse_color(&border_source, &mut diagnostics, &id, "border") {
        theme.ui.border = c;
        theme.ui.border_focused = c;
    }
    if let Some(c) = parse_text_color(
        &file.buffer.selection,
        &mut diagnostics,
        &id,
        "buffer.selection",
    ) {
        theme.ui.selection = c;
    }
    if let Some(c) = parse_color(
        &file.buffer.background,
        &mut diagnostics,
        &id,
        "buffer.background",
    ) {
        theme.ui.panel_background = c;
    }
    if let Some(c) = parse_color(
        &file.buffer.background_text_input,
        &mut diagnostics,
        &id,
        "buffer.background_text_input",
    ) {
        theme.ui.input_background = c;
    }
    if let Some(c) = parse_color(
        &file.buffer.background_title_bar,
        &mut diagnostics,
        &id,
        "buffer.background_title_bar",
    ) {
        theme.ui.title_background = c;
    }

    // Accent primary: prefer buttons.primary.background_selected, fall back to
    // general.highlight_indicator, then unread_indicator, then fallback.
    let accent_primary = file
        .buttons
        .primary
        .as_ref()
        .and_then(|b| b.background_selected.clone())
        .or_else(|| file.general.highlight_indicator.clone())
        .or_else(|| file.general.unread_indicator.clone());
    if let Some(c) = parse_color(&accent_primary, &mut diagnostics, &id, "accent_primary") {
        theme.ui.accent_primary = c;
    } else {
        diagnostics.push(ThemeDiagnostic::warn(
            &id,
            Some("ui.accent_primary"),
            "no Halloy source mapped; using fallback",
        ));
    }

    if let Some(b) = file.buttons.secondary.as_ref() {
        if let Some(c) = parse_color(
            &b.background_selected,
            &mut diagnostics,
            &id,
            "buttons.secondary.background_selected",
        ) {
            theme.ui.accent_secondary = c;
        }
    }

    if let Some(c) = parse_text_color(&file.text.success, &mut diagnostics, &id, "text.success") {
        theme.status.success = c;
        theme.diff.added = c;
    }
    if let Some(c) = parse_text_color(
        &file.text.warning,
        &mut diagnostics,
        &id,
        "text.warning",
    ) {
        theme.status.warning = c;
        theme.diff.modified = c;
    }
    if let Some(c) = parse_text_color(&file.text.error, &mut diagnostics, &id, "text.error") {
        theme.status.error = c;
        theme.diff.removed = c;
    }
    if let Some(c) = parse_text_color(&file.text.info, &mut diagnostics, &id, "text.info") {
        theme.status.info = c;
        theme.conversation.tool_call = c;
        theme.agents.planner = c;
    }

    if let Some(sm) = &file.buffer.server_messages {
        if let Some(c) = parse_text_color(
            &sm.default,
            &mut diagnostics,
            &id,
            "buffer.server_messages.default",
        ) {
            // Default server-message color → status.info (covers join/part/nick
            // variants when those are absent).
            theme.status.info = c;
        }
    }
    if let Some(c) = parse_text_color(&file.text.debug, &mut diagnostics, &id, "text.debug") {
        theme.status.debug = c;
    }
    // `text.tertiary` is mapped to `text.muted` earlier in this function,
    // near the other `text.*` block, so it stays adjacent to the text
    // palette it belongs to.

    if let Some(c) = parse_text_color(&file.buffer.code, &mut diagnostics, &id, "buffer.code") {
        theme.code.foreground = c;
    }
    if let Some(c) = parse_text_color(&file.buffer.url, &mut diagnostics, &id, "buffer.url") {
        theme.text.link = c;
    }
    if let Some(c) = parse_text_color(
        &file.buffer.timestamp,
        &mut diagnostics,
        &id,
        "buffer.timestamp",
    ) {
        theme.conversation.timestamp = c;
    }
    if let Some(c) = parse_text_color(&file.buffer.action, &mut diagnostics, &id, "buffer.action") {
        theme.conversation.tool_call = c;
    }
    // `buffer.nickname` and `buffer.topic` are intentionally NOT mapped
    // onto `conversation.user` / `conversation.assistant`. Both are
    // already driven by `text.primary` (set near the top of this
    // function) so user and agent messages read consistently across
    // Halloy themes. The Halloy nickname/topic slots are scoped to
    // highlights within an IRC client that we don't reproduce here.
    if let Some(c) = parse_text_color(
        &file.buffer.highlight,
        &mut diagnostics,
        &id,
        "buffer.highlight",
    ) {
        theme.agents.coder = c;
    }

    if let Some(c) = parse_color(&file.formatting.highlight, &mut diagnostics, &id, "formatting.highlight") {
        theme.agents.coder = c;
    }

    // selection_dim: lighten/darken selection slightly, or use border.
    theme.ui.selection_dim = derive_selection_dim(theme.ui.selection, theme.base.background);

    diagnostics.extend(validate_theme(&theme));
    Ok((theme, diagnostics))
}

/// Parse a Halloy theme from a file path. Falls back to [`parse_native_theme`]
/// if the file does not look like a Halloy theme.
pub fn parse_halloy_file(
    path: &Path,
    fallback: &SemanticTheme,
) -> Result<(SemanticTheme, Vec<ThemeDiagnostic>), ThemeError> {
    let content = std::fs::read_to_string(path)?;
    if looks_like_halloy(&content) {
        parse_halloy_theme(&content, Some(path), fallback)
    } else {
        let theme = parse_native_theme(
            &content,
            ThemeSource::NativeFile {
                path: path.to_path_buf(),
            },
            fallback,
        )?;
        let diagnostics = validate_theme(&theme);
        Ok((theme, diagnostics))
    }
}

/// Heuristic: a file looks like a Halloy theme if it contains a `[general]`,
/// `[text]`, and `[buffer]` section, OR has `buttons.primary` and
/// `buffer.background`.
pub fn looks_like_halloy(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let has_general = lower.contains("[general]");
    let has_buffer = lower.contains("[buffer]");
    let has_text = lower.contains("[text]");
    let has_buttons_primary = lower.contains("buttons.primary");
    (has_general && has_buffer) || (has_text && has_buttons_primary) || (has_buffer && has_text)
}

fn parse_color(
    value: &Option<String>,
    diagnostics: &mut Vec<ThemeDiagnostic>,
    theme_id: &str,
    field: &str,
) -> Option<Rgb> {
    let raw = value.as_deref()?;
    match Rgb::from_hex(raw) {
        Ok(rgb) => Some(rgb),
        Err(_) => {
            diagnostics.push(ThemeDiagnostic::warn(
                theme_id,
                Some(field),
                format!("invalid color '{}', using fallback", raw),
            ));
            None
        }
    }
}

fn parse_text_color(
    value: &Option<HalloyTextStyle>,
    diagnostics: &mut Vec<ThemeDiagnostic>,
    theme_id: &str,
    field: &str,
) -> Option<Rgb> {
    let style = value.as_ref()?;
    match Rgb::from_hex(style.color()) {
        Ok(rgb) => Some(rgb),
        Err(_) => {
            diagnostics.push(ThemeDiagnostic::warn(
                theme_id,
                Some(field),
                format!("invalid color '{}', using fallback", style.color()),
            ));
            None
        }
    }
}

fn derive_selection_dim(selection: Rgb, background: Rgb) -> Rgb {
    if selection == background {
        return selection;
    }
    let dr = selection.r as i16 - background.r as i16;
    let dg = selection.g as i16 - background.g as i16;
    let db = selection.b as i16 - background.b as i16;
    // Pull the selection back halfway toward the background to make a
    // gentler "selection_dim" tone. This is purely heuristic.
    let r = (selection.r as i16 - dr / 4).clamp(0, 255) as u8;
    let g = (selection.g as i16 - dg / 4).clamp(0, 255) as u8;
    let b = (selection.b as i16 - db / 4).clamp(0, 255) as u8;
    Rgb::new(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::schema::{
        AgentColors, BaseColors, CodeColors, ConversationColors, DiffColors, StatusColors,
        TextColors, UiColors,
    };

    fn fallback() -> SemanticTheme {
        // Minimal fallback that any caller can construct; we just need *some*
        // valid SemanticTheme to satisfy the parser API.
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

    #[test]
    fn parse_text_style_color_string() {
        let input = r###"
            [text]
            primary = "#abcdef"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.base.foreground, Rgb::new(0xab, 0xcd, 0xef));
        assert!(diags.iter().all(|d| d.field.as_deref() != Some("text.primary")));
    }

    #[test]
    fn parse_text_style_table() {
        let input = r###"
            [text]
            primary = { color = "#ff0000", font_style = "bold" }
        "###;
        let (theme, _diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.base.foreground, Rgb::new(255, 0, 0));
    }

    #[test]
    fn minimal_halloy_overlays_fallback() {
        let input = r###"
            [general]
            background = "#101010"
        "###;
        let (theme, _diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.base.background, Rgb::new(0x10, 0x10, 0x10));
        // Foreground is fallback.
        assert_eq!(theme.base.foreground, Rgb::new(255, 255, 255));
    }

    #[test]
    fn invalid_color_records_diagnostic() {
        let input = r###"
            [text]
            primary = "not-a-color"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert!(diags
            .iter()
            .any(|d| d.field.as_deref() == Some("text.primary")));
        // Foreground remains the fallback value.
        assert_eq!(theme.base.foreground, Rgb::new(255, 255, 255));
    }

    #[test]
    fn file_stem_derives_id() {
        let input = r###"
            [general]
            background = "#202020"
        "###;
        let (theme, _diags) = parse_halloy_theme(
            input,
            Some(Path::new("/themes/Tokyo_Night_Storm.toml")),
            &fallback(),
        )
        .unwrap();
        assert_eq!(theme.id, "tokyo-night-storm");
    }

    #[test]
    fn parses_eight_digit_hex_in_general_border() {
        // The Halloy community uses 8-digit hex (`#00000000`) for borders
        // they want to render as transparent. Make sure the parser accepts it.
        let input = r###"
            [general]
            background = "#101010"
            border = "#00000000"

            [text]
            primary = "#ffffff"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.ui.border, Rgb::new(0, 0, 0));
        // No diagnostics for the 8-digit border.
        assert!(diags
            .iter()
            .all(|d| d.field.as_deref() != Some("general.border")));
    }

    #[test]
    fn parses_full_halloy_gallery_file() {
        // A representative sample of a real Halloy gallery file. Exercises
        // every section: general, text, buffer, buffer.server_messages,
        // buttons.primary, buttons.secondary. Uses 8-digit hex.
        let input = r###"
            [general]
            background = "#0A0000"
            border = "#E41951"
            horizontal_rule = "#3E0101"
            unread_indicator = "#C1DEFF"

            [text]
            primary = "#E41951"
            secondary = "#8F1134"
            tertiary = "#7D3F50"
            success = "#D7053F"
            error = "#C1DEFF"

            [buffer]
            action = "#5BA4DB"
            background = "#090808"
            background_text_input = "#160E0E"
            background_title_bar = "#1F040B"
            border = "#00000000"
            border_selected = "#E41951"
            code = "#EA9995"
            highlight = "#122C38"
            nickname = "#76ABEC"
            selection = "#73000054"
            timestamp = "#750D2A"
            topic = "#E41951"
            url = "#FFADAD"

              [buffer.server_messages]
              default = "#5BA4DB"

            [buttons.primary]
            background = "#000000"
            background_hover = "#4B0A1C"
            background_selected = "#230202"
            background_selected_hover = "#1C0606"

            [buttons.secondary]
            background = "#310000"
            background_hover = "#610B0B"
            background_selected = "#701414"
            background_selected_hover = "#882828"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        // Background.
        assert_eq!(theme.base.background, Rgb::new(0x0A, 0x00, 0x00));
        // 8-digit alpha-stripped borders: selection reads from the 73000054 hex.
        assert_eq!(theme.ui.selection, Rgb::new(0x73, 0x00, 0x00));
        // Server-messages.default → status.info.
        assert_eq!(theme.status.info, Rgb::new(0x5B, 0xA4, 0xDB));
        // text.primary drives user / assistant / system so the chat surface
        // reads with a single text color. (buffer.nickname is now ignored
        // for conversation.user.)
        assert_eq!(theme.conversation.user, Rgb::new(0xE4, 0x19, 0x51));
        assert_eq!(theme.conversation.assistant, Rgb::new(0xE4, 0x19, 0x51));
        assert_eq!(theme.conversation.system, Rgb::new(0xE4, 0x19, 0x51));
        // text.tertiary drives text.muted (replaces the old text.secondary
        // mapping so we don't end up with a near-clone of the accent hue).
        assert_eq!(theme.text.muted, Rgb::new(0x7D, 0x3F, 0x50));
        // Uniform border: buffer.border_selected is the primary source and
        // feeds both ui.border and ui.border_focused. The transparent
        // buffer.border is ignored because buffer.border_selected wins.
        assert_eq!(theme.ui.border, Rgb::new(0xE4, 0x19, 0x51));
        assert_eq!(theme.ui.border_focused, Rgb::new(0xE4, 0x19, 0x51));
        // buffer.action → conversation.tool_call.
        assert_eq!(theme.conversation.tool_call, Rgb::new(0x5B, 0xA4, 0xDB));
        // text.error → status.error.
        assert_eq!(theme.status.error, Rgb::new(0xC1, 0xDE, 0xFF));
        // buffer.code → code.foreground.
        assert_eq!(theme.code.foreground, Rgb::new(0xEA, 0x99, 0x95));
        // No diagnostic errors (warnings about accent_primary fallback are OK
        // but the 8-digit selection field must not warn).
        for d in &diags {
            assert_ne!(
                d.field.as_deref(),
                Some("buffer.selection"),
                "8-digit hex should not warn: {:?}",
                d
            );
        }
    }

    #[test]
    fn border_uniform_when_only_general_border_present() {
        // If only `general.border` is set, both `ui.border` and
        // `ui.border_focused` should resolve to it.
        let input = r###"
            [general]
            background = "#101010"
            border = "#445566"

            [text]
            primary = "#ffeedd"
        "###;
        let (theme, _diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.ui.border, Rgb::new(0x44, 0x55, 0x66));
        assert_eq!(theme.ui.border_focused, Rgb::new(0x44, 0x55, 0x66));
    }

    #[test]
    fn border_falls_back_through_chain() {
        // The fallback chain is buffer.border_selected → buffer.border →
        // general.border → horizontal_rule. Themes missing the prominent
        // slots should still get a border color.
        let input = r###"
            [general]
            background = "#101010"
            horizontal_rule = "#abcdef"

            [text]
            primary = "#ffeedd"
        "###;
        let (theme, _diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.ui.border, Rgb::new(0xab, 0xcd, 0xef));
        assert_eq!(theme.ui.border_focused, Rgb::new(0xab, 0xcd, 0xef));
    }

    #[test]
    fn text_primary_drives_user_assistant_system() {
        // text.primary is the canonical default for user / agent text.
        // Halloy-specific slots like buffer.nickname and buffer.topic
        // must not override it.
        let input = r###"
            [general]
            background = "#101010"

            [text]
            primary = "#abcdef"

            [buffer]
            nickname = "#ff00ff"
            topic = "#00ff00"
        "###;
        let (theme, _diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.conversation.user, Rgb::new(0xab, 0xcd, 0xef));
        assert_eq!(theme.conversation.assistant, Rgb::new(0xab, 0xcd, 0xef));
        assert_eq!(theme.conversation.system, Rgb::new(0xab, 0xcd, 0xef));
    }
}
