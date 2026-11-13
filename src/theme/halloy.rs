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
use crate::theme::schema::{SemanticTheme, ThemeSource, MIN_DERIVED_TEXT_RATIO};
use crate::theme::validate::{
    validate_theme, ThemeDiagnostic, ACCENT_RATIO, MUTED_TEXT_RATIO, PRIMARY_TEXT_RATIO,
};

/// Minimum WCAG contrast an imported accent must reach against the theme
/// background before it is used as `ui.accent_primary`.
///
/// The TUI draws the accent as foreground text on top of surfaces, so a value
/// below this threshold is not a stylistic choice — it is unreadable. 3.0 is
/// the WCAG large-text threshold, which is the right bar for accents that
/// appear on selection highlights and borders rather than body copy.
const MIN_ACCENT_CONTRAST: f64 = 3.0;

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
        Some(p) => ThemeSource::HalloyFile {
            path: p.to_path_buf(),
        },
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

    if let Some(bg) = parse_color(
        &file.general.background,
        &mut diagnostics,
        &id,
        "general.background",
    ) {
        theme.base.background = bg;
    }
    let background = theme.base.background;
    if let Some(fg) = parse_text_color(&file.text.primary, &mut diagnostics, &id, "text.primary") {
        // `text.primary` is the canonical "default text" color. Use it for
        // user / assistant / system message text so chat reads consistently
        // across all Halloy themes regardless of how the theme author
        // named their nickname / topic slots. It is also body copy, so it is
        // held to the body bar rather than the status bar.
        let fg = legible_on(
            fg,
            background,
            PRIMARY_TEXT_RATIO,
            &id,
            "base.foreground",
            file.text.primary.as_ref().map(HalloyTextStyle::color),
            &mut diagnostics,
        );
        theme.base.foreground = fg;
        theme.conversation.user = fg;
        theme.conversation.assistant = fg;
        theme.conversation.system = fg;
    }
    // `text.tertiary` is the muted tier; `text.secondary` is usually a darker
    // shade of the accent and clashes as a de-emphasis foreground.
    if let Some(muted) =
        parse_text_color(&file.text.tertiary, &mut diagnostics, &id, "text.tertiary")
    {
        theme.text.muted = legible_on(
            muted,
            background,
            MUTED_TEXT_RATIO,
            &id,
            "text.muted",
            file.text.tertiary.as_ref().map(HalloyTextStyle::color),
            &mut diagnostics,
        );
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

    // Accent primary. The TUI paints `accent_primary` as *foreground* text —
    // selection labels, focused borders, active tabs — so it must contrast
    // against the surface it sits on. Halloy's `buttons.primary.background_selected`
    // is a widget *fill* and the gallery sets it at or below its own background,
    // so it is tried only after `unread_indicator`, the author's actual accent.
    // A candidate too close to the background to read is rejected, not shipped.
    let accent_candidates = [
        (
            "general.unread_indicator",
            file.general.unread_indicator.clone(),
        ),
        (
            "general.highlight_indicator",
            file.general.highlight_indicator.clone(),
        ),
        (
            "buttons.primary.background_selected",
            file.buttons
                .primary
                .as_ref()
                .and_then(|b| b.background_selected.clone()),
        ),
        // Secondary text is the readable accent on light themes.
        (
            "text.secondary",
            file.text.secondary.as_ref().map(|s| s.color().to_owned()),
        ),
        (
            "buffer.url",
            file.buffer.url.as_ref().map(|s| s.color().to_owned()),
        ),
        // Last theme-faithful resort: the theme's own body-text color is
        // guaranteed to read against its background, so a muted palette still
        // looks like itself rather than falling through to a hardcoded blue.
        (
            "text.primary",
            file.text
                .primary
                .as_ref()
                .map(|style| style.color().to_owned()),
        ),
    ];
    let mut accent_primary = None;
    for (field, candidate) in accent_candidates {
        let Some(raw) = candidate else { continue };
        let Some(color) = parse_color(&Some(raw.clone()), &mut diagnostics, &id, field) else {
            continue;
        };
        // Only promote a candidate that is actually distinguishable from the
        // background it will be drawn over. A dim accent is worse than the
        // neutral fallback: it reads as a rendering bug.
        if color.contrast_ratio(background) < MIN_ACCENT_CONTRAST {
            diagnostics.push(ThemeDiagnostic::warn(
                &id,
                Some("ui.accent_primary"),
                format!(
                    "{field} ({}) is indistinguishable from the background; \
                     skipping it as an accent",
                    raw
                ),
            ));
            continue;
        }
        accent_primary = Some(color);
        break;
    }
    if let Some(color) = accent_primary {
        theme.ui.accent_primary = color;
    } else {
        diagnostics.push(ThemeDiagnostic::warn(
            &id,
            Some("ui.accent_primary"),
            "no Halloy source mapped to a readable accent; using fallback",
        ));
    }

    // Secondary accent: same foreground-legibility rule as the primary. The
    // Halloy source is a widget *fill*, and 41 of the 50 bundled themes author
    // one that does not read as a foreground; that used to fall through to
    // `placeholder_fallback`'s `#B48CFF`, a colour no theme file mentions.
    // ADR-0062 §2: a rejected candidate is derived from this theme's own text
    // colours instead, gated by the same constant as `accent_primary`.
    let authored_secondary = file.buttons.secondary.as_ref().and_then(|b| {
        parse_color(
            &b.background_selected,
            &mut diagnostics,
            &id,
            "buttons.secondary.background_selected",
        )
    });
    // `ui.accent_secondary` is painted on **two** fills — the window background
    // and the composer's input fill, which is lighter in most themes — so it has
    // to read against both. Gating only against `background` is what let
    // `kanagawa`'s authored value pass there at 3.5:1 while sitting at 2.42:1 on
    // the input fill, which the I5 measurement then recorded as the last row in
    // the whole contrast baseline. `worst_fill` names the fill a colour reads
    // least well on, which is the one a derivation must clear.
    //
    // The input fill joins the gate only when the theme **declares** one. An
    // unauthored `buffer.background_text_input` leaves the fallback's dark
    // placeholder in the field, and gating a light theme against a dark
    // placeholder rejects values that are correct on every fill the theme
    // actually paints.
    let input_fill = if file.buffer.background_text_input.is_some() {
        theme.ui.input_background
    } else {
        background
    };
    let secondary_fills = [background, input_fill];
    let reads_on_every_fill = |c: Rgb| {
        secondary_fills
            .iter()
            .all(|fill| c.contrast_ratio(*fill) >= MIN_ACCENT_CONTRAST)
    };
    let worst_fill = |c: Rgb| -> Rgb {
        secondary_fills
            .into_iter()
            .min_by(|a, b| c.contrast_ratio(*a).total_cmp(&c.contrast_ratio(*b)))
            .unwrap_or(background)
    };
    let mut accent_secondary = authored_secondary.filter(|c| reads_on_every_fill(*c));
    if accent_secondary.is_none() {
        // These three slots are already parsed (and their malformed values
        // already reported) by the `text.primary` / `buffer.url` / accent reads
        // elsewhere in this function.
        accent_secondary = [
            file.text.secondary.as_ref().map(HalloyTextStyle::color),
            file.buffer.url.as_ref().map(HalloyTextStyle::color),
            file.text.primary.as_ref().map(HalloyTextStyle::color),
        ]
        .into_iter()
        .flatten()
        .filter_map(|raw| Rgb::from_hex(raw).ok())
        .find(|c| *c != theme.ui.accent_primary && reads_on_every_fill(*c));
    }
    match accent_secondary {
        Some(c) => {
            if Some(c) != authored_secondary {
                diagnostics.push(ThemeDiagnostic::warn(
                    &id,
                    Some("ui.accent_secondary"),
                    format!(
                        "no readable buttons.secondary.background_selected; \
                         ui.accent_secondary derived as {} from this theme's own text colours",
                        c.to_hex()
                    ),
                ));
            }
            theme.ui.accent_secondary = c;
        }
        None => {
            // No legible text colour at all: answer with a computed
            // legibility value rather than ship the inherited hex unchanged.
            let derived = derive_readable_variant(
                theme.ui.accent_secondary,
                worst_fill(theme.ui.accent_secondary),
                MIN_ACCENT_CONTRAST,
            );
            diagnostics.push(ThemeDiagnostic::warn(
                &id,
                Some("ui.accent_secondary"),
                format!(
                    "no Halloy source mapped to a readable secondary accent; derived {} \
                     against the fill it reads least well on",
                    derived.to_hex()
                ),
            ));
            theme.ui.accent_secondary = derived;
        }
    }

    // `ui.border` is chrome: the edge of every unfocused panel and dialog.
    // Halloy's four border keys mean four different things — the selected
    // buffer's focus indicator, an unselected buffer's edge, the window frame,
    // and a divider — and none is a chrome colour. The chain is kept, but the
    // projection owns the result's legibility (architecture/theme.md,
    // "The projection owns legibility").
    let border_chain = [
        file.buffer.border_selected.as_deref(),
        file.buffer.border.as_deref(),
        file.general.border.as_deref(),
        file.general.horizontal_rule.as_deref(),
    ];
    // An alpha byte of `00` is Halloy's "no paint"; reading it would paint
    // `#000000`, so the next key in the chain is asked instead.
    if let Some(raw) = border_chain
        .into_iter()
        .flatten()
        .find(|raw| !is_transparent(raw))
    {
        if let Some(candidate) =
            parse_color(&Some(raw.to_owned()), &mut diagnostics, &id, "ui.border")
        {
            // The neutral frame key wins when the prominent one does not read;
            // a value that fails both is derived from its own hue.
            let general = file
                .general
                .border
                .as_deref()
                .filter(|raw| !is_transparent(raw))
                .and_then(|raw| Rgb::from_hex(raw).ok())
                .filter(|c| c.contrast_ratio(background) >= MIN_DERIVED_TEXT_RATIO);
            theme.ui.border = match general {
                Some(frame) if candidate.contrast_ratio(background) < MIN_DERIVED_TEXT_RATIO => {
                    diagnostics.push(ThemeDiagnostic::warn(
                        &id,
                        Some("ui.border"),
                        format!(
                            "ui.border: {raw} reads {:.3} < {:.1} on {}; using the file's \
                             general.border {} ({:.3})",
                            candidate.contrast_ratio(background),
                            MIN_DERIVED_TEXT_RATIO,
                            background.to_hex(),
                            frame.to_hex(),
                            frame.contrast_ratio(background)
                        ),
                    ));
                    frame
                }
                _ => legible_on(
                    candidate,
                    background,
                    MIN_DERIVED_TEXT_RATIO,
                    &id,
                    "ui.border",
                    Some(raw),
                    &mut diagnostics,
                ),
            };
        }
    }
    // Focus is painted with `theme.primary`, so the token names that colour.
    theme.ui.border_focused = theme.ui.accent_primary;

    if let Some(c) = parse_text_color(&file.text.success, &mut diagnostics, &id, "text.success") {
        let c = legible_on(
            c,
            background,
            ACCENT_RATIO,
            &id,
            "status.success",
            file.text.success.as_ref().map(HalloyTextStyle::color),
            &mut diagnostics,
        );
        theme.status.success = c;
        theme.diff.added = c;
    }
    // One rule for both arms: `[text].warning` is authored by 0 of the 50
    // bundled themes, so the token inherits `placeholder_fallback`'s
    // `#FFB43C`; an authored value is held to the same bar instead of
    // bypassing it.
    let (warning, authored_warning) = match file.text.warning.as_ref() {
        Some(style) => (
            parse_text_color(&file.text.warning, &mut diagnostics, &id, "text.warning"),
            Some(style.color()),
        ),
        None => (Some(theme.status.warning), None),
    };
    if let Some(warning) = warning {
        let warning = legible_on(
            warning,
            background,
            ACCENT_RATIO,
            &id,
            "status.warning",
            authored_warning,
            &mut diagnostics,
        );
        theme.status.warning = warning;
        theme.diff.modified = warning;
    }
    if let Some(c) = parse_text_color(&file.text.error, &mut diagnostics, &id, "text.error") {
        let c = legible_on(
            c,
            background,
            ACCENT_RATIO,
            &id,
            "status.error",
            file.text.error.as_ref().map(HalloyTextStyle::color),
            &mut diagnostics,
        );
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
            // Default server-message color → status.info.
            theme.status.info = c;
        }
    }
    if let Some(c) = parse_text_color(&file.text.debug, &mut diagnostics, &id, "text.debug") {
        theme.status.debug = c;
    }
    // `text.tertiary` → `text.muted` is resolved with the other `text.*` keys.

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

    if let Some(c) = parse_color(
        &file.formatting.highlight,
        &mut diagnostics,
        &id,
        "formatting.highlight",
    ) {
        theme.agents.coder = c;
    }

    // selection_dim: lighten/darken selection slightly, or use border.
    theme.ui.selection_dim = derive_selection_dim(theme.ui.selection, theme.base.background);
    // The two selection *foregrounds* are derived, not read: Halloy declares
    // `buttons.primary.foreground_selected` and 0 of the 50 bundled themes set
    // it, while all 50 set `background_selected` and `buffer.selection`.
    // Deriving from colours the format does guarantee asks no author for
    // anything, which is what keeps "any Halloy theme ingests unchanged" true;
    // see architecture/theme.md, "Derived tokens must stay derived".
    theme
        .ui
        .derive_selection_texts(theme.base.foreground, theme.base.background);

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

/// A translated palette value that must read on `background`.
///
/// The file owns the hue and the projection owns the legibility (ADR-0062's
/// 2026-11-13 amendment): a value that already clears `min_ratio` is returned
/// untouched, and a failing one is raised toward the more legible pole until it
/// clears, with a `warn` naming `field` and both ratios. `authored_as` is the
/// literal the file wrote, or `None` when the value was inherited.
fn legible_on(
    value: Rgb,
    background: Rgb,
    min_ratio: f64,
    theme_id: &str,
    field: &str,
    authored_as: Option<&str>,
    diagnostics: &mut Vec<ThemeDiagnostic>,
) -> Rgb {
    let ratio = value.contrast_ratio(background);
    if ratio >= min_ratio {
        return value;
    }
    let derived = derive_readable_variant(value, background, min_ratio);
    let provenance = match authored_as {
        Some(raw) => format!("authored {raw}"),
        None => format!("inherited {}", value.to_hex()),
    };
    diagnostics.push(ThemeDiagnostic::warn(
        theme_id,
        Some(field),
        format!(
            "{field}: {provenance} reads {ratio:.3} < {min_ratio:.1} on {}; derived {}",
            background.to_hex(),
            derived.to_hex()
        ),
    ));
    derived
}

/// `true` for a Halloy literal that means "no paint": an 8- or 4-digit hex
/// whose alpha byte is `00`. `Rgb::from_hex` discards alpha, so reading one
/// would paint `#000000` where upstream paints nothing.
fn is_transparent(raw: &str) -> bool {
    let bytes = raw.trim().trim_start_matches('#').as_bytes();
    match bytes.len() {
        8 => bytes[6] == b'0' && bytes[7] == b'0',
        4 => bytes[3] == b'0',
        _ => false,
    }
}

/// `color` moved just far enough along the lightness axis to reach `min_ratio`
/// against `background`, keeping its hue.
///
/// The search returns the first mixture that clears the bar, so a colour that
/// already reads is untouched, and the target always clears it: where black and
/// white read equally (luminance 0.179) both are at 4.58:1.
fn derive_readable_variant(color: Rgb, background: Rgb, min_ratio: f64) -> Rgb {
    if color.contrast_ratio(background) >= min_ratio {
        return color;
    }
    let black = Rgb::new(0, 0, 0);
    let white = Rgb::new(255, 255, 255);
    let target = if black.contrast_ratio(background) >= white.contrast_ratio(background) {
        black
    } else {
        white
    };
    let mix = |t: f64| {
        let lerp = |from: u8, to: u8| {
            (f64::from(from) + (f64::from(to) - f64::from(from)) * t).round() as u8
        };
        Rgb::new(
            lerp(color.r, target.r),
            lerp(color.g, target.g),
            lerp(color.b, target.b),
        )
    };
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if mix(mid).contrast_ratio(background) >= min_ratio {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    mix(hi)
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
    use crate::theme::registry::{builtin_fallback, BundledTheme, BUILTIN_THEME_FILES};
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
                selection_text: Rgb::new(255, 255, 255),
                accent_text: Rgb::new(0, 0, 0),
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
        assert!(diags
            .iter()
            .all(|d| d.field.as_deref() != Some("text.primary")));
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
    fn an_eight_digit_border_with_zero_alpha_is_skipped_not_painted_black() {
        let input = r###"
            [general]
            background = "#101010"
            border = "#00000000"

            [text]
            primary = "#ffffff"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_ne!(
            theme.ui.border,
            Rgb::new(0, 0, 0),
            "an alpha of `00` means no paint; it must not become a black frame"
        );
        assert_eq!(theme.ui.border, fallback().ui.border);
        assert!(!warns_about(&diags, "general.border"));
    }

    #[test]
    fn parses_full_halloy_gallery_file() {
        // A representative gallery file, with every section and 8-digit hex.
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
        // 8-digit alpha-stripped backgrounds: selection reads from 73000054.
        assert_eq!(theme.ui.selection, Rgb::new(0x73, 0x00, 0x00));
        // Server-messages.default → status.info.
        assert_eq!(theme.status.info, Rgb::new(0x5B, 0xA4, 0xDB));
        // text.primary drives user / assistant / system so the chat surface
        // reads with a single text color. It is lifted by one unit: 4.499:1 is
        // below the body bar. (`buffer.nickname` cannot override it.)
        assert_eq!(theme.base.foreground, Rgb::new(0xE4, 0x1A, 0x51));
        assert_eq!(theme.conversation.user, theme.base.foreground);
        assert_eq!(theme.conversation.assistant, theme.base.foreground);
        assert_eq!(theme.conversation.system, theme.base.foreground);
        // text.tertiary drives text.muted, lifted to the muted bar.
        assert_eq!(theme.text.muted, Rgb::new(0x83, 0x49, 0x59));
        // buffer.border_selected is the first border source and clears the bar,
        // so it ships unchanged; focus is the accent the renderer paints.
        assert_eq!(theme.ui.border, Rgb::new(0xE4, 0x19, 0x51));
        assert_eq!(theme.ui.border_focused, theme.ui.accent_primary);
        assert_eq!(theme.ui.border_focused, Rgb::new(0xC1, 0xDE, 0xFF));
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
    fn border_focused_is_the_accent_the_renderer_paints() {
        // The renderer paints focus with `theme.primary` and never reads
        // `border_focused` through the TUI projection; this token must name
        // that colour rather than claim the unfocused stroke.
        let input = r###"
            [general]
            background = "#101010"
            border = "#8899aa"

            [text]
            primary = "#ffeedd"
        "###;
        let (theme, _diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.ui.border, Rgb::new(0x88, 0x99, 0xaa));
        assert_eq!(theme.ui.border_focused, theme.ui.accent_primary);
        assert_ne!(theme.ui.border_focused, theme.ui.border);
    }

    #[test]
    fn border_falls_back_through_chain() {
        // The chain is buffer.border_selected → buffer.border →
        // general.border → horizontal_rule; a theme missing the prominent
        // slots still gets a border color, and the bar gate leaves a value
        // that clears untouched.
        let input = r###"
            [general]
            background = "#101010"
            horizontal_rule = "#abcdef"

            [text]
            primary = "#ffeedd"
        "###;
        let (theme, _diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.ui.border, Rgb::new(0xab, 0xcd, 0xef));
        assert_eq!(theme.ui.border_focused, theme.ui.accent_primary);
    }

    #[test]
    fn text_primary_drives_user_assistant_system() {
        // Halloy-specific slots like buffer.nickname must not override it.
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

    // ── ADR-0062 §2 / the 2026-11-13 amendment: a translated token's
    // legibility belongs to the projection ─────────────────────────────

    /// The bar `halloy.rs` applies to an accent, restated so a change to the
    /// constant alone fails a test rather than silently widening the sweep.
    const REQUIRED_ACCENT_RATIO: f64 = 3.0;

    /// `validate.rs`'s `MUTED_TEXT_RATIO`, restated for the same reason.
    const MUTED_TIER_RATIO: f64 = 3.0;

    /// The 50 bundled themes, parsed the way `ThemeRegistry::load_builtins` does.
    fn bundled_theme(entry: &BundledTheme) -> SemanticTheme {
        let file = format!("{}.toml", entry.id);
        parse_halloy_theme(entry.content, Some(Path::new(&file)), &builtin_fallback())
            .unwrap_or_else(|e| panic!("bundled theme {} failed to parse: {e}", entry.id))
            .0
    }

    fn bundled_themes() -> Vec<SemanticTheme> {
        BUILTIN_THEME_FILES.iter().map(bundled_theme).collect()
    }

    fn warns_about(diags: &[ThemeDiagnostic], field: &str) -> bool {
        diags.iter().any(|d| d.field.as_deref() == Some(field))
    }

    // ── The five gates ──────────────────────────────────────────────

    #[test]
    fn every_bundled_border_clears_the_bar_on_its_own_background() {
        for theme in bundled_themes() {
            assert!(
                theme.ui.border.contrast_ratio(theme.base.background) >= MIN_DERIVED_TEXT_RATIO,
                "{}: ui.border {} reads {:.2}:1 on {}",
                theme.id,
                theme.ui.border,
                theme.ui.border.contrast_ratio(theme.base.background),
                theme.base.background
            );
        }
    }

    #[test]
    fn the_borders_that_already_read_are_byte_identical() {
        // The 28 themes that cleared the bar before the gate. A change to the
        // chain that restyles one of them is scope creep, not a repair.
        const UNCHANGED: [(&str, Rgb); 28] = [
            ("base16-atelier-forest-light", Rgb::new(0x1B, 0x19, 0x18)),
            (
                "black-but-with-important-highlights",
                Rgb::new(0x7F, 0x7F, 0x7F),
            ),
            ("booberry", Rgb::new(0xA4, 0xA0, 0xE8)),
            ("cyber-red", Rgb::new(0xE4, 0x19, 0x51)),
            ("cyberpunk", Rgb::new(0x00, 0xFF, 0xD9)),
            ("dracula", Rgb::new(0x50, 0xFA, 0x7B)),
            ("forest", Rgb::new(0x8F, 0x97, 0x79)),
            ("gruvbox", Rgb::new(0x92, 0x83, 0x74)),
            ("halcyon-dark", Rgb::new(0xC3, 0xA6, 0xFF)),
            ("kanagawa", Rgb::new(0x8B, 0xA4, 0xB0)),
            ("lisbon", Rgb::new(0x8B, 0x83, 0x55)),
            ("matrix", Rgb::new(0x66, 0xFF, 0x66)),
            ("midnight", Rgb::new(0x61, 0xA5, 0xC2)),
            ("noctis-lilac", Rgb::new(0x70, 0x60, 0xEB)),
            ("nord", Rgb::new(0x81, 0xA1, 0xC1)),
            ("oslo", Rgb::new(0x60, 0x86, 0xC5)),
            ("plum", Rgb::new(0x50, 0xFA, 0x7B)),
            ("portland", Rgb::new(0x7F, 0x7F, 0x7F)),
            ("rose-pine", Rgb::new(0x6E, 0x6A, 0x86)),
            ("solarized-dark", Rgb::new(0x93, 0xA1, 0xA1)),
            ("sonokai", Rgb::new(0x76, 0xCC, 0xE0)),
            ("sunset", Rgb::new(0xF7, 0x25, 0x85)),
            ("tofino", Rgb::new(0x3E, 0x7F, 0x42)),
            ("tokyo-night-storm", Rgb::new(0x9E, 0xCE, 0x6A)),
            ("vanimo", Rgb::new(0x59, 0x7B, 0x2A)),
            ("vesper", Rgb::new(0xFF, 0xC7, 0x99)),
            ("vik", Rgb::new(0x2C, 0x73, 0x94)),
            ("zenburn", Rgb::new(0xAE, 0xD6, 0x00)),
        ];
        let themes = bundled_themes();
        assert_eq!(UNCHANGED.len(), 28);
        for (id, expected) in UNCHANGED {
            let theme = themes
                .iter()
                .find(|theme| theme.id == id)
                .unwrap_or_else(|| panic!("{id} is a bundled theme"));
            assert_eq!(
                theme.ui.border, expected,
                "{id}: the border gate restyled a theme that already read"
            );
        }
    }

    #[test]
    fn a_failing_border_is_reported_not_silently_derived() {
        let input = r###"
            [general]
            background = "#333333"

            [buffer]
            border_selected = "#101010"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert!(
            theme.ui.border.contrast_ratio(theme.base.background) >= MIN_DERIVED_TEXT_RATIO,
            "derived border {} reads {:.2}:1",
            theme.ui.border,
            theme.ui.border.contrast_ratio(theme.base.background)
        );
        assert_ne!(theme.ui.border, Rgb::new(0x10, 0x10, 0x10));
        assert!(
            warns_about(&diags, "ui.border"),
            "a derivation must name the key: {diags:?}"
        );
    }

    #[test]
    fn a_border_that_already_reads_is_untouched() {
        let input = r###"
            [general]
            background = "#101010"

            [buffer]
            border_selected = "#c0c0c0"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.ui.border, Rgb::new(0xC0, 0xC0, 0xC0));
        assert!(
            !warns_about(&diags, "ui.border"),
            "a value that already reads is not a finding: {diags:?}"
        );
    }

    #[test]
    fn a_transparent_buffer_border_never_becomes_a_black_frame() {
        let input = r###"
            [general]
            background = "#101010"
            border = "#9A9A9A"

            [buffer]
            border = "#00000000"
        "###;
        let (theme, _diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.ui.border, Rgb::new(0x9A, 0x9A, 0x9A));
        assert_ne!(theme.ui.border, Rgb::new(0, 0, 0));
    }

    #[test]
    fn the_preferred_source_is_general_border_when_only_it_clears() {
        // The `catppuccin-mocha` shape: the selection edge is 2.00:1 and the
        // window frame 3.84:1. The frame is the semantically neutral key, so it
        // is preferred over deriving.
        let input = r###"
            [general]
            background = "#1E1E2E"
            border = "#6C7086"

            [buffer]
            border_selected = "#4C4163"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.ui.border, Rgb::new(0x6C, 0x70, 0x86));
        assert!(warns_about(&diags, "ui.border"), "{diags:?}");
    }

    #[test]
    fn a_black_border_on_a_black_background_is_lifted_not_returned() {
        let input = r###"
            [general]
            background = "#000000"

            [buffer]
            border_selected = "#000000"
        "###;
        let (theme, _diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_ne!(theme.ui.border, theme.base.background);
        assert!(theme.ui.border.contrast_ratio(theme.base.background) >= MIN_DERIVED_TEXT_RATIO);
    }

    #[test]
    fn every_bundled_status_token_clears_the_accent_bar() {
        for theme in bundled_themes() {
            let background = theme.base.background;
            for (token, value) in [
                ("status.success", theme.status.success),
                ("status.warning", theme.status.warning),
                ("status.error", theme.status.error),
            ] {
                assert!(
                    value.contrast_ratio(background) >= ACCENT_RATIO,
                    "{}: {token} {value} reads {:.2}:1 on {background}",
                    theme.id,
                    value.contrast_ratio(background)
                );
            }
        }
    }

    #[test]
    fn every_bundled_body_text_clears_the_primary_bar() {
        // `validate_theme` also holds `base.foreground` to 4.5 on
        // `input_background`. The gate is against the theme background, so two
        // themes keep that pre-existing warning: `cyber-red` (4.13) and
        // `solarized-dark` (4.11). Both fail it at HEAD too; naming them keeps
        // the residue visible instead of implying the gate covered it.
        const INPUT_FILL_RESIDUE: [&str; 2] = ["cyber-red", "solarized-dark"];
        let mut residual = Vec::new();
        for theme in bundled_themes() {
            let background = theme.base.background;
            assert!(
                theme.base.foreground.contrast_ratio(background) >= PRIMARY_TEXT_RATIO,
                "{}: base.foreground {} reads {:.2}:1 on {background}",
                theme.id,
                theme.base.foreground,
                theme.base.foreground.contrast_ratio(background)
            );
            if theme
                .base
                .foreground
                .contrast_ratio(theme.ui.input_background)
                < PRIMARY_TEXT_RATIO
            {
                residual.push(theme.id.clone());
            }
        }
        assert_eq!(residual, INPUT_FILL_RESIDUE);
    }

    #[test]
    fn the_derivation_preserves_hue() {
        let dominant = |c: Rgb| -> u8 {
            if c.r >= c.g && c.r >= c.b {
                0
            } else if c.g >= c.b {
                1
            } else {
                2
            }
        };
        for entry in BUILTIN_THEME_FILES {
            let file: HalloyFile = toml::from_str(entry.content).unwrap();
            let theme = bundled_theme(entry);
            let pairs = [
                (
                    "text.muted",
                    file.text.tertiary.as_ref().map(HalloyTextStyle::color),
                    theme.text.muted,
                ),
                (
                    "status.success",
                    file.text.success.as_ref().map(HalloyTextStyle::color),
                    theme.status.success,
                ),
                (
                    "status.error",
                    file.text.error.as_ref().map(HalloyTextStyle::color),
                    theme.status.error,
                ),
                (
                    "base.foreground",
                    file.text.primary.as_ref().map(HalloyTextStyle::color),
                    theme.base.foreground,
                ),
            ];
            for (token, authored, resolved) in pairs {
                let Some(authored) = authored.and_then(|raw| Rgb::from_hex(raw).ok()) else {
                    continue;
                };
                assert_eq!(
                    dominant(authored),
                    dominant(resolved),
                    "{}: {token} {authored} lost its dominant channel in {resolved}",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn the_four_reverted_values_derive_in_their_authors_hue() {
        // Four of the reverted values had replaced the author's hue; the
        // derivation keeps it — `#8839EF` violet, `#C38418` orange,
        // `#B0D1AB` green, `#D7827E` rose — so these pin the hue-preserving
        // answer rather than the grey or the substituted palette entry.
        let themes = bundled_themes();
        let theme = |id: &str| {
            themes
                .iter()
                .find(|theme| theme.id == id)
                .unwrap_or_else(|| panic!("{id} is a bundled theme"))
        };
        assert_eq!(theme("macaw-dark").text.muted, Rgb::new(0x9A, 0x57, 0xF1));
        assert!(theme("macaw-dark").text.muted.b > theme("macaw-dark").text.muted.r);
        assert_eq!(
            theme("base16-atelier-forest-light").text.muted,
            Rgb::new(0xBA, 0x7E, 0x17)
        );
        assert_eq!(
            theme("intellij-light").status.success,
            Rgb::new(0x83, 0x9B, 0x7F)
        );
        assert_eq!(
            theme("rose-pine-dawn").base.foreground,
            Rgb::new(0x98, 0x5C, 0x59)
        );
    }

    #[test]
    fn the_derivation_is_idempotent() {
        for (color, background) in [
            (Rgb::new(0x88, 0x39, 0xEF), Rgb::new(0x1E, 0x1E, 0x1E)),
            (Rgb::new(0xD7, 0x82, 0x7E), Rgb::new(0xF4, 0xED, 0xE8)),
            (Rgb::new(0x1A, 0x1A, 0x1A), Rgb::new(0x33, 0x33, 0x33)),
        ] {
            let once = derive_readable_variant(color, background, MIN_DERIVED_TEXT_RATIO);
            assert_eq!(
                derive_readable_variant(once, background, MIN_DERIVED_TEXT_RATIO),
                once,
                "deriving {color} twice moved it"
            );
        }
    }

    #[test]
    fn an_omitted_warning_is_derived_to_read_on_the_themes_own_background() {
        let input = r###"
            [general]
            background = "#F1EFEE"

            [text]
            primary = "#202020"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_ne!(
            theme.status.warning,
            Rgb::new(255, 180, 60),
            "a theme that authors no `[text].warning` still ships the placeholder's #FFB43C"
        );
        assert!(
            theme.status.warning.contrast_ratio(theme.base.background) >= REQUIRED_ACCENT_RATIO,
            "derived warning {} reads {:.2}:1 on the theme's background",
            theme.status.warning,
            theme.status.warning.contrast_ratio(theme.base.background)
        );
        // Still a warning colour: the derivation moves lightness, not hue, so
        // the dominant channel of the placeholder's amber is preserved.
        let warning = theme.status.warning;
        assert!(
            warning.r > warning.g && warning.g > warning.b,
            "derived warning {warning} is not amber"
        );
        assert_eq!(
            theme.diff.modified, theme.status.warning,
            "diff.modified mirrors status.warning and must follow the derivation"
        );
        assert!(
            warns_about(&diags, "status.warning"),
            "the inheritance repair must be reported, not applied silently: {diags:?}"
        );
    }

    #[test]
    fn an_omitted_warning_is_left_alone_when_the_canonical_value_already_reads() {
        let input = r###"
            [general]
            background = "#0F0F14"

            [text]
            primary = "#DCDCE1"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(
            theme.status.warning,
            Rgb::new(255, 180, 60),
            "a background the canonical warning already reads on must not be restyled"
        );
        assert!(
            !warns_about(&diags, "status.warning"),
            "a derivation that changed nothing is not a finding: {diags:?}"
        );
    }

    #[test]
    fn a_rejected_secondary_accent_derives_from_the_themes_own_text_colours() {
        // A widget fill at 1.1:1 is rejected; the repair is `text.secondary`.
        let input = r###"
            [general]
            background = "#F1EFEE"
            unread_indicator = "#4A5A6A"

            [text]
            primary = "#202020"
            secondary = "#3A4A5A"

            [buttons.secondary]
            background_selected = "#E8E4E0"
        "###;
        let (theme, diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.ui.accent_secondary, Rgb::new(0x3A, 0x4A, 0x5A));
        assert_ne!(
            theme.ui.accent_secondary,
            fallback().ui.accent_secondary,
            "a rejected candidate must not fall back to the placeholder's #B48CFF"
        );
        assert!(warns_about(&diags, "ui.accent_secondary"));
    }

    #[test]
    fn a_derived_secondary_accent_does_not_collapse_into_the_primary() {
        // A theme can resolve both accents to one colour; the second must not.
        let input = r###"
            [general]
            background = "#F1EFEE"
            unread_indicator = "#4A5A6A"

            [text]
            primary = "#202020"
            secondary = "#4A5A6A"

            [buffer]
            url = "#4A5A6A"

            [buttons.secondary]
            background_selected = "#E8E4E0"
        "###;
        let (theme, _diags) = parse_halloy_theme(input, None, &fallback()).unwrap();
        assert_eq!(theme.ui.accent_primary, Rgb::new(0x4A, 0x5A, 0x6A));
        assert_ne!(theme.ui.accent_secondary, theme.ui.accent_primary);
        assert_eq!(theme.ui.accent_secondary, Rgb::new(0x20, 0x20, 0x20));
    }

    #[test]
    fn every_bundled_theme_resolves_readable_status_and_secondary_accents() {
        let themes = bundled_themes();
        assert_eq!(themes.len(), 50, "the sweep is over the bundled gallery");
        for theme in &themes {
            let background = theme.base.background;
            for (token, value) in [
                ("status.warning", theme.status.warning),
                ("ui.accent_secondary", theme.ui.accent_secondary),
            ] {
                assert!(
                    value.contrast_ratio(background) >= REQUIRED_ACCENT_RATIO,
                    "{}: {token} {value} reads {:.2}:1 on {background}",
                    theme.id,
                    value.contrast_ratio(background)
                );
            }
        }
    }

    #[test]
    fn every_bundled_theme_keeps_a_readable_muted_tier() {
        for theme in bundled_themes() {
            assert!(
                theme.text.muted.contrast_ratio(theme.base.background) >= MUTED_TIER_RATIO,
                "{}: text.muted {} reads {:.2}:1 on {}",
                theme.id,
                theme.text.muted,
                theme.text.muted.contrast_ratio(theme.base.background),
                theme.base.background
            );
        }
    }

    #[test]
    fn the_derived_muted_tiers_stay_below_the_body_tier() {
        // The de-emphasis tier survives the gate: a derived muted value stays
        // distinct from, and dimmer than, the body colour. Gallery-wide
        // collisions of the same shape are 007-status §10 finding 5 and are not
        // part of this repair, so this asserts over the six the gate moved.
        const DERIVED: [&str; 6] = [
            "base16-atelier-forest-light",
            "cyber-red",
            "discord",
            "discord-80-saturation",
            "macaw-dark",
            "vik",
        ];
        let themes = bundled_themes();
        for id in DERIVED {
            let theme = themes
                .iter()
                .find(|theme| theme.id == id)
                .unwrap_or_else(|| panic!("{id} is a bundled theme"));
            let background = theme.base.background;
            assert_ne!(
                theme.text.muted, theme.base.foreground,
                "{id}: muted collapsed into the body colour"
            );
            assert!(
                theme.text.muted.contrast_ratio(background)
                    < theme.base.foreground.contrast_ratio(background),
                "{id}: muted {} is not dimmer than the body colour {}",
                theme.text.muted,
                theme.base.foreground
            );
        }
    }

    #[test]
    fn no_bundled_theme_authors_text_warning() {
        let authored: Vec<&str> = BUILTIN_THEME_FILES
            .iter()
            .filter(|entry| {
                entry.content.lines().any(|line| {
                    let line = line.trim_start();
                    line.starts_with("warning =") || line.starts_with("warning=")
                })
            })
            .map(|entry| entry.id)
            .collect();
        assert!(
            authored.is_empty(),
            "{} theme(s) now author `[text].warning`: {authored:?}. The derivation \
             corrects a value nobody authored; if the gallery starts authoring it, \
             re-measure whether the derivation is still the right repair.",
            authored.len()
        );
    }

    #[test]
    fn derive_readable_variant_only_moves_lightness() {
        let background = Rgb::new(0xF1, 0xEF, 0xEE);
        let readable = Rgb::new(0x20, 0x20, 0x20);
        assert_eq!(
            derive_readable_variant(readable, background, MIN_ACCENT_CONTRAST),
            readable,
            "a colour that already reads must be returned untouched"
        );
        let dominant = |c: Rgb| {
            if c.r >= c.g && c.r >= c.b {
                0
            } else if c.g >= c.b {
                1
            } else {
                2
            }
        };
        for source in [
            Rgb::new(255, 180, 60),
            Rgb::new(180, 140, 255),
            Rgb::new(0, 179, 104),
        ] {
            let out = derive_readable_variant(source, background, MIN_ACCENT_CONTRAST);
            assert!(
                out.contrast_ratio(background) >= MIN_ACCENT_CONTRAST,
                "{source} produced {out} at {:.2}:1",
                out.contrast_ratio(background)
            );
            assert_eq!(
                dominant(source),
                dominant(out),
                "{source} lost its dominant channel in {out}"
            );
            assert_ne!(out, Rgb::new(0, 0, 0), "{source} was flattened to black");
        }
    }
}
