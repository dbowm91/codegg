//! Theme registry.
//!
//! The registry is the single source of truth for available themes. It owns:
//!
//! - Built-in native codegg themes bundled via `include_str!`
//! - User-provided themes loaded from `~/.config/codegg/themes` or
//!   additional directories configured in `[theme].directories`
//! - Diagnostics accumulated during loading and validation

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::schema::ThemeConfig;
use crate::theme::error::ThemeError;
use crate::theme::halloy::parse_halloy_theme;
use crate::theme::native::parse_native_theme;
use crate::theme::schema::{SemanticTheme, ThemeSource};
use crate::theme::validate::{validate_theme, ThemeDiagnostic};
use crate::tui::theme::Theme;

/// One bundled theme. The id is the slugified stem (kebab-case). The name
/// is the case-preserved display name (matches the original Halloy file
/// name, e.g. "Cyber Red"). The content is the Halloy-format TOML.
pub struct BundledTheme {
    pub id: &'static str,
    pub name: &'static str,
    pub content: &'static str,
}

/// Bundled Halloy-format themes. Sourced from
/// `https://themes.halloy.chat`. The id is the kebab-case slug and is
/// used as the canonical lookup key; the name is the original case-
/// preserved display name. The content uses the Halloy TOML schema so
/// the parser can treat bundled and user-supplied files identically.
pub const BUILTIN_THEME_FILES: &[BundledTheme] = &[
    BundledTheme {
        id: "acton",
        name: "acton",
        content: include_str!("../../assets/themes/halloy/acton.toml"),
    },
    BundledTheme {
        id: "bam",
        name: "bam",
        content: include_str!("../../assets/themes/halloy/bam.toml"),
    },
    BundledTheme {
        id: "base16-atelier-forest-light",
        name: "base16-atelier-forest-light",
        content: include_str!("../../assets/themes/halloy/base16-atelier-forest-light.toml"),
    },
    BundledTheme {
        id: "berlin",
        name: "berlin",
        content: include_str!("../../assets/themes/halloy/berlin.toml"),
    },
    BundledTheme {
        id: "black-but-with-important-highlights",
        name: "black but with important highlights",
        content: include_str!("../../assets/themes/halloy/black but with important highlights.toml"),
    },
    BundledTheme {
        id: "booberry",
        name: "Booberry",
        content: include_str!("../../assets/themes/halloy/Booberry.toml"),
    },
    BundledTheme {
        id: "broc",
        name: "broc",
        content: include_str!("../../assets/themes/halloy/broc.toml"),
    },
    BundledTheme {
        id: "catppuccin-latte",
        name: "Catppuccin Latte",
        content: include_str!("../../assets/themes/halloy/Catppuccin Latte.toml"),
    },
    BundledTheme {
        id: "catppuccin-macchiato",
        name: "Catppuccin Macchiato",
        content: include_str!("../../assets/themes/halloy/Catppuccin Macchiato.toml"),
    },
    BundledTheme {
        id: "catppuccin-mocha",
        name: "Catppuccin Mocha",
        content: include_str!("../../assets/themes/halloy/Catppuccin Mocha.toml"),
    },
    BundledTheme {
        id: "cork",
        name: "cork",
        content: include_str!("../../assets/themes/halloy/cork.toml"),
    },
    BundledTheme {
        id: "cyber-red",
        name: "Cyber Red",
        content: include_str!("../../assets/themes/halloy/Cyber Red.toml"),
    },
    BundledTheme {
        id: "cyberpunk",
        name: "Cyberpunk",
        content: include_str!("../../assets/themes/halloy/Cyberpunk.toml"),
    },
    BundledTheme {
        id: "dark-green",
        name: "Dark Green",
        content: include_str!("../../assets/themes/halloy/Dark Green.toml"),
    },
    BundledTheme {
        id: "discord",
        name: "Discord",
        content: include_str!("../../assets/themes/halloy/Discord.toml"),
    },
    BundledTheme {
        id: "discord-80-saturation",
        name: "Discord (80% Saturation)",
        content: include_str!("../../assets/themes/halloy/Discord (80_ Saturation).toml"),
    },
    BundledTheme {
        id: "dracula",
        name: "Dracula",
        content: include_str!("../../assets/themes/halloy/Dracula.toml"),
    },
    BundledTheme {
        id: "ferra",
        name: "ferra",
        content: include_str!("../../assets/themes/halloy/ferra.toml"),
    },
    BundledTheme {
        id: "ferra-light",
        name: "Ferra Light",
        content: include_str!("../../assets/themes/halloy/Ferra Light.toml"),
    },
    BundledTheme {
        id: "flexor-dark",
        name: "Flexor Dark",
        content: include_str!("../../assets/themes/halloy/Flexor Dark.toml"),
    },
    BundledTheme {
        id: "forest",
        name: "forest",
        content: include_str!("../../assets/themes/halloy/forest.toml"),
    },
    BundledTheme {
        id: "gruvbox",
        name: "Gruvbox",
        content: include_str!("../../assets/themes/halloy/Gruvbox.toml"),
    },
    BundledTheme {
        id: "halcyon-dark",
        name: "Halcyon Dark",
        content: include_str!("../../assets/themes/halloy/Halcyon Dark.toml"),
    },
    BundledTheme {
        id: "intellij-light",
        name: "IntelliJ Light",
        content: include_str!("../../assets/themes/halloy/IntelliJ Light.toml"),
    },
    BundledTheme {
        id: "kanagawa",
        name: "Kanagawa",
        content: include_str!("../../assets/themes/halloy/Kanagawa.toml"),
    },
    BundledTheme {
        id: "lisbon",
        name: "lisbon",
        content: include_str!("../../assets/themes/halloy/lisbon.toml"),
    },
    BundledTheme {
        id: "macaw-dark",
        name: "Macaw Dark",
        content: include_str!("../../assets/themes/halloy/Macaw Dark.toml"),
    },
    BundledTheme {
        id: "macaw-light",
        name: "Macaw Light",
        content: include_str!("../../assets/themes/halloy/Macaw Light.toml"),
    },
    BundledTheme {
        id: "matrix",
        name: "Matrix",
        content: include_str!("../../assets/themes/halloy/Matrix.toml"),
    },
    BundledTheme {
        id: "midnight",
        name: "midnight",
        content: include_str!("../../assets/themes/halloy/midnight.toml"),
    },
    BundledTheme {
        id: "noctis-lilac",
        name: "Noctis Lilac",
        content: include_str!("../../assets/themes/halloy/Noctis Lilac.toml"),
    },
    BundledTheme {
        id: "nord",
        name: "Nord",
        content: include_str!("../../assets/themes/halloy/Nord.toml"),
    },
    BundledTheme {
        id: "nostromo-terminal",
        name: "Nostromo Terminal",
        content: include_str!("../../assets/themes/halloy/Nostromo Terminal.toml"),
    },
    BundledTheme {
        id: "one-dark",
        name: "One Dark",
        content: include_str!("../../assets/themes/halloy/One Dark.toml"),
    },
    BundledTheme {
        id: "oslo",
        name: "oslo",
        content: include_str!("../../assets/themes/halloy/oslo.toml"),
    },
    BundledTheme {
        id: "oxocarbon",
        name: "Oxocarbon",
        content: include_str!("../../assets/themes/halloy/Oxocarbon.toml"),
    },
    BundledTheme {
        id: "plum",
        name: "plum",
        content: include_str!("../../assets/themes/halloy/plum.toml"),
    },
    BundledTheme {
        id: "portland",
        name: "portland",
        content: include_str!("../../assets/themes/halloy/portland.toml"),
    },
    BundledTheme {
        id: "rose-pine",
        name: "Rose Pine",
        content: include_str!("../../assets/themes/halloy/Rose Pine.toml"),
    },
    BundledTheme {
        id: "rose-pine-dawn",
        name: "Rose Pine Dawn",
        content: include_str!("../../assets/themes/halloy/Rose Pine Dawn.toml"),
    },
    BundledTheme {
        id: "rose-pine-moon",
        name: "Rose Pine Moon",
        content: include_str!("../../assets/themes/halloy/Rose Pine Moon.toml"),
    },
    BundledTheme {
        id: "solarized-dark",
        name: "Solarized Dark",
        content: include_str!("../../assets/themes/halloy/Solarized Dark.toml"),
    },
    BundledTheme {
        id: "sonokai",
        name: "Sonokai",
        content: include_str!("../../assets/themes/halloy/Sonokai.toml"),
    },
    BundledTheme {
        id: "sunset",
        name: "sunset",
        content: include_str!("../../assets/themes/halloy/sunset.toml"),
    },
    BundledTheme {
        id: "tofino",
        name: "tofino",
        content: include_str!("../../assets/themes/halloy/tofino.toml"),
    },
    BundledTheme {
        id: "tokyo-night-storm",
        name: "Tokyo Night Storm",
        content: include_str!("../../assets/themes/halloy/Tokyo Night Storm.toml"),
    },
    BundledTheme {
        id: "vanimo",
        name: "vanimo",
        content: include_str!("../../assets/themes/halloy/vanimo.toml"),
    },
    BundledTheme {
        id: "vesper",
        name: "VESPER",
        content: include_str!("../../assets/themes/halloy/VESPER.toml"),
    },
    BundledTheme {
        id: "vik",
        name: "vik",
        content: include_str!("../../assets/themes/halloy/vik.toml"),
    },
    BundledTheme {
        id: "zenburn",
        name: "Zenburn",
        content: include_str!("../../assets/themes/halloy/Zenburn.toml"),
    },
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ThemeSourceConfig {
    #[default]
    Auto,
    Builtin,
    Native,
    Halloy,
}

impl ThemeSourceConfig {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "builtin" => Self::Builtin,
            "native" => Self::Native,
            "halloy" => Self::Halloy,
            _ => Self::Auto,
        }
    }
}

/// User-configurable resolution request.
#[derive(Debug, Clone, Default)]
pub struct ThemeResolutionConfig {
    pub name: Option<String>,
    pub source: Option<ThemeSourceConfig>,
    pub path: Option<String>,
    pub directories: Vec<String>,
    pub fallback: Option<String>,
    pub validate_contrast: bool,
}

impl ThemeResolutionConfig {
    pub fn from_config(cfg: Option<&ThemeConfig>) -> Self {
        match cfg {
            None => Self::default(),
            Some(c) => Self {
                name: c.name.clone(),
                source: c.source.as_ref().map(|s| match s {
                    crate::config::schema::ThemeSourceConfig::Auto => ThemeSourceConfig::Auto,
                    crate::config::schema::ThemeSourceConfig::Builtin => ThemeSourceConfig::Builtin,
                    crate::config::schema::ThemeSourceConfig::Native => ThemeSourceConfig::Native,
                    crate::config::schema::ThemeSourceConfig::Halloy => ThemeSourceConfig::Halloy,
                }),
                path: c.path.clone(),
                directories: c.directories.clone().unwrap_or_default(),
                fallback: c.fallback.clone(),
                validate_contrast: c.validate_contrast.unwrap_or(true),
            },
        }
    }
}

pub struct ThemeRegistry {
    themes: HashMap<String, SemanticTheme>,
    diagnostics: Vec<ThemeDiagnostic>,
}

impl Default for ThemeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeRegistry {
    pub fn new() -> Self {
        Self {
            themes: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Load only the built-in bundled themes.
    pub fn load_builtins() -> Self {
        let mut registry = Self::new();
        let dark_fallback = builtin_fallback();
        for entry in BUILTIN_THEME_FILES {
            let result = if looks_like_halloy_content(entry.content) {
                parse_halloy_theme(
                    entry.content,
                    Some(std::path::Path::new(&format!("{}.toml", entry.name))),
                    &dark_fallback,
                )
                .map(|(mut t, d)| {
                    // Force the case-preserved display name so the picker
                    // shows "Cyber Red" rather than "cyber-red", and mark
                    // every bundled theme as `Builtin` so user themes
                    // overriding it still trigger the duplicate-id warning.
                    t.name = entry.name.to_string();
                    t.id = entry.id.to_string();
                    t.source = ThemeSource::Builtin;
                    (t, d)
                })
            } else {
                parse_native_theme(entry.content, ThemeSource::Builtin, &dark_fallback)
                    .map(|mut t| {
                        t.id = entry.id.to_string();
                        t.name = entry.name.to_string();
                        let diags = validate_theme(&t);
                        (t, diags)
                    })
            };
            match result {
                Ok((theme, diags)) => {
                    registry.themes.insert(theme.id.clone(), theme);
                    registry.diagnostics.extend(diags);
                }
                Err(e) => {
                    registry.diagnostics.push(ThemeDiagnostic::error(
                        entry.id,
                        None,
                        format!("failed to parse built-in theme: {}", e),
                    ));
                }
            }
        }
        registry
    }

    /// Load built-ins and overlay user-configured paths/directories.
    pub fn load_with_config(cfg: Option<&ThemeConfig>) -> Self {
        Self::load_with_resolution(&ThemeResolutionConfig::from_config(cfg))
    }

    pub fn load_with_resolution(cfg: &ThemeResolutionConfig) -> Self {
        let mut registry = Self::load_builtins();

        // Load user theme directories.
        for dir in &cfg.directories {
            let path = expand_home(dir);
            if path.is_dir() {
                if let Err(e) = registry.load_dir(&path) {
                    registry.diagnostics.push(ThemeDiagnostic::warn(
                        "registry",
                        None,
                        format!("failed to load dir {}: {}", path.display(), e),
                    ));
                }
            } else {
                registry.diagnostics.push(ThemeDiagnostic::warn(
                    "registry",
                    None,
                    format!("theme directory not found: {}", path.display()),
                ));
            }
        }

        // Apply an explicit `path` if provided.
        if let Some(ref raw) = cfg.path {
            let path = expand_home(raw);
            let theme_id = path
                .file_stem()
                .map(|s| SemanticTheme::normalize_id(&s.to_string_lossy()))
                .unwrap_or_else(|| "imported".to_string());
            match registry.load_file_auto(&path) {
                Ok(()) => {
                    if let Some(source) = cfg.source.as_ref() {
                        registry.diagnostics.push(ThemeDiagnostic::warn(
                            "registry",
                            None,
                            format!(
                                "theme {} loaded from {}; explicit source={:?} may not match detected format",
                                theme_id,
                                path.display(),
                                source
                            ),
                        ));
                    }
                }
                Err(e) => {
                    registry.diagnostics.push(ThemeDiagnostic::error(
                        "registry",
                        Some(path.to_string_lossy().as_ref()),
                        format!("failed to load: {}", e),
                    ));
                }
            }
        }

        // Validate if requested.
        if cfg.validate_contrast {
            let ids: Vec<String> = registry.themes.keys().cloned().collect();
            for id in ids {
                if let Some(theme) = registry.themes.get(&id) {
                    let diags = validate_theme(theme);
                    registry.diagnostics.extend(diags);
                }
            }
        }

        registry
    }

    /// Load every `*.toml` file in `dir`. Top-level files only. Subdirectories
    /// are ignored. Returns the number of files successfully loaded.
    pub fn load_dir(&mut self, dir: &Path) -> Result<usize, ThemeError> {
        let entries = std::fs::read_dir(dir)?;
        let mut count = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            match self.load_file_auto(&path) {
                Ok(()) => count += 1,
                Err(e) => {
                    self.diagnostics.push(ThemeDiagnostic::warn(
                        "registry",
                        Some(path.to_string_lossy().as_ref()),
                        format!("skipped: {}", e),
                    ));
                }
            }
        }
        Ok(count)
    }

    /// Load a single file, detecting whether it is a Halloy theme or a native
    /// codegg theme.
    pub fn load_file_auto(&mut self, path: &Path) -> Result<(), ThemeError> {
        let content = std::fs::read_to_string(path)?;
        let fallback = self.fallback_theme();
        if crate::theme::halloy::looks_like_halloy(&content) {
            let (theme, diags) = parse_halloy_theme(&content, Some(path), &fallback)?;
            self.diagnostics.extend(diags);
            self.insert_or_warn(theme, Some(path));
            return Ok(());
        }

        let theme = parse_native_theme(
            &content,
            ThemeSource::NativeFile { path: path.to_path_buf() },
            &fallback,
        )?;
        let diags = validate_theme(&theme);
        self.diagnostics.extend(diags);
        self.insert_or_warn(theme, Some(path));
        Ok(())
    }

    fn insert_or_warn(&mut self, theme: SemanticTheme, path: Option<&Path>) {
        if let Some(existing) = self.themes.get(&theme.id) {
            // User themes should override built-ins. We keep the existing
            // entry; the new theme wins by re-inserting.
            if matches!(existing.source, ThemeSource::Builtin)
                || matches!(theme.source, ThemeSource::Builtin)
            {
                let source = match path {
                    Some(p) => p.display().to_string(),
                    None => "<inline>".to_string(),
                };
                self.diagnostics.push(ThemeDiagnostic::warn(
                    &theme.id,
                    None,
                    format!("duplicate id; later entry overrides earlier ({})", source),
                ));
            }
        }
        self.themes.insert(theme.id.clone(), theme);
    }

    pub fn insert(&mut self, theme: SemanticTheme) {
        self.themes.insert(theme.id.clone(), theme);
    }

    pub fn get(&self, name: &str) -> Option<&SemanticTheme> {
        self.themes.get(name)
    }

    /// Convenience: project to the ratatui-facing `Theme` type.
    pub fn get_tui(&self, name: &str) -> Option<Theme> {
        self.themes.get(name).map(Theme::from)
    }

    /// Convenience: project to `Arc<Theme>` for `UiState::theme`.
    pub fn get_tui_arc(&self, name: &str) -> Option<Arc<Theme>> {
        self.get_tui(name).map(Arc::new)
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.themes.keys().cloned().collect();
        names.sort();
        names
    }

    /// Project every theme into a ratatui `Theme`. Sorted by id.
    pub fn all_tui_themes(&self) -> Vec<Theme> {
        self.names()
            .into_iter()
            .filter_map(|n| self.get_tui(&n))
            .collect()
    }

    pub fn diagnostics(&self) -> &[ThemeDiagnostic] {
        &self.diagnostics
    }

    pub fn into_diagnostics(self) -> Vec<ThemeDiagnostic> {
        self.diagnostics
    }

    /// Pick a theme using the same rules as `App::with_config` resolution.
    pub fn resolve(&self, cfg: &ThemeResolutionConfig) -> SemanticTheme {
        let requested = cfg.name.as_deref().unwrap_or(DEFAULT_THEME_ID);
        let fallback = cfg.fallback.as_deref().unwrap_or(DEFAULT_THEME_ID);
        if let Some(theme) = self.themes.get(requested) {
            return theme.clone();
        }
        if let Some(theme) = self.themes.get(fallback) {
            return theme.clone();
        }
        if let Some(theme) = self.themes.get(DEFAULT_THEME_ID) {
            return theme.clone();
        }
        // Last resort: any theme.
        self.themes
            .values()
            .next()
            .cloned()
            .unwrap_or_else(builtin_fallback)
    }

    /// Resolve a name to a ratatui `Theme`, honoring fallback.
    pub fn resolve_tui(&self, cfg: &ThemeResolutionConfig) -> Theme {
        Theme::from(&self.resolve(cfg))
    }

    pub fn resolve_tui_arc(&self, cfg: &ThemeResolutionConfig) -> Arc<Theme> {
        Arc::new(self.resolve_tui(cfg))
    }

    fn fallback_theme(&self) -> SemanticTheme {
        self.themes
            .get(DEFAULT_THEME_ID)
            .cloned()
            .or_else(|| self.themes.get("midnight").cloned())
            .unwrap_or_else(builtin_fallback)
    }
}

/// `~` expansion. Falls back to the input unchanged if home is not available.
pub fn expand_home(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if input == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(input)
}

/// Default theme id used when no `[theme].name` is configured.
pub const DEFAULT_THEME_ID: &str = "cyber-red";

/// Re-export the Halloy `looks_like_halloy` check for callers that need to
/// decide whether a string is Halloy-format before parsing.
pub fn looks_like_halloy_content(content: &str) -> bool {
    crate::theme::halloy::looks_like_halloy(content)
}

/// Return a sensible built-in fallback theme. Used when no built-ins have
/// been loaded yet. We don't bundle a literal "dark" theme anymore (the
/// Halloy gallery's `midnight` is the closest thing) so we return the
/// placeholder for the rare "no themes loaded at all" case.
pub fn builtin_fallback() -> SemanticTheme {
    placeholder_fallback()
}

fn placeholder_fallback() -> SemanticTheme {
    use crate::theme::schema::{
        AgentColors, BaseColors, CodeColors, ConversationColors, DiffColors, StatusColors,
        TextColors, UiColors,
    };
    use crate::theme::color::Rgb;
    SemanticTheme {
        id: "dark".to_string(),
        name: "Dark".to_string(),
        source: ThemeSource::Builtin,
        base: BaseColors {
            background: Rgb::new(15, 15, 20),
            foreground: Rgb::new(220, 220, 225),
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
            muted: Rgb::new(100, 100, 110),
            link: Rgb::new(60, 140, 240),
        },
        status: StatusColors {
            success: Rgb::new(80, 200, 120),
            warning: Rgb::new(255, 180, 60),
            error: Rgb::new(255, 80, 80),
            info: Rgb::new(120, 180, 255),
            debug: Rgb::new(180, 140, 255),
            trace: Rgb::new(100, 100, 110),
        },
        conversation: ConversationColors {
            user: Rgb::new(220, 220, 225),
            assistant: Rgb::new(180, 140, 255),
            system: Rgb::new(100, 100, 110),
            tool_call: Rgb::new(120, 180, 255),
            tool_result: Rgb::new(80, 200, 120),
            timestamp: Rgb::new(100, 100, 110),
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

/// Resolve a theme by name from the registry and return a ratatui projection.
pub fn resolve_theme_for_app(
    registry: &ThemeRegistry,
    name: Option<&str>,
    fallback: Option<&str>,
) -> Theme {
    if let Some(name) = name {
        if let Some(theme) = registry.get_tui(name) {
            return theme;
        }
    }
    if let Some(name) = fallback {
        if let Some(theme) = registry.get_tui(name) {
            return theme;
        }
    }
    registry
        .get_tui("dark")
        .unwrap_or_else(Theme::dark)
}

#[allow(dead_code)]
pub(crate) fn _re_exports_for_out_of_crate() {
    let _: Option<&Path> = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn builtins_load() {
        let registry = ThemeRegistry::load_builtins();
        assert!(registry.themes.contains_key("cyber-red"));
        assert!(registry.themes.contains_key("catppuccin-mocha"));
        assert!(registry.themes.contains_key("midnight"));
        assert!(registry.themes.len() >= 40, "got {}", registry.themes.len());
    }

    #[test]
    fn resolve_with_fallback() {
        let registry = ThemeRegistry::load_builtins();
        let cfg = ThemeResolutionConfig {
            name: Some("catppuccin-mocha".to_string()),
            fallback: Some("cyber-red".to_string()),
            ..Default::default()
        };
        let theme = registry.resolve(&cfg);
        assert_eq!(theme.id, "catppuccin-mocha");
    }

    #[test]
    fn resolve_falls_back_when_unknown() {
        let registry = ThemeRegistry::load_builtins();
        let cfg = ThemeResolutionConfig {
            name: Some("does-not-exist".to_string()),
            fallback: Some("cyber-red".to_string()),
            ..Default::default()
        };
        let theme = registry.resolve(&cfg);
        assert_eq!(theme.id, "cyber-red");
    }

    #[test]
    fn default_theme_is_cyber_red() {
        let registry = ThemeRegistry::load_builtins();
        let cfg = ThemeResolutionConfig::default();
        let theme = registry.resolve(&cfg);
        assert_eq!(theme.id, "cyber-red");
        assert_eq!(theme.name, "Cyber Red");
    }

    #[test]
    fn user_theme_overrides_builtin() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Drop a Halloy-format file that matches an existing bundled id
        // (Cyber Red) to exercise the duplicate-id warning path.
        std::fs::write(
            tmp.path().join("Cyber Red.toml"),
            include_str!("../../assets/themes/halloy/Cyber Red.toml"),
        )
        .unwrap();
        let mut registry = ThemeRegistry::load_builtins();
        registry.load_dir(tmp.path()).unwrap();
        assert!(
            registry
                .diagnostics
                .iter()
                .any(|d| d.theme_id == "cyber-red" && d.message.contains("duplicate id")),
            "expected a duplicate-id warning; got diagnostics: {:?}",
            registry.diagnostics
        );
    }

    #[test]
    fn expand_home_tilde() {
        let path = expand_home("~/themes");
        let home = dirs::home_dir().unwrap();
        assert_eq!(path, home.join("themes"));
    }

    #[test]
    fn get_tui_projects_semantic_theme() {
        let registry = ThemeRegistry::load_builtins();
        let theme = registry.get_tui("cyber-red").unwrap();
        assert_eq!(theme.name, "cyber-red");
        // Project a SemanticTheme manually and compare.
        let semantic = registry.get("cyber-red").unwrap();
        let projected: Theme = semantic.into();
        assert_eq!(projected.name, theme.name);
    }

    #[test]
    fn all_themes_sorted() {
        let registry = ThemeRegistry::load_builtins();
        let names = registry.names();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn user_dir_merges_into_registry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let theme_toml = r###"
            [meta]
            id = "everforest-custom"
            name = "Everforest Custom"

            [base]
            background = "#1f2a25"
            foreground = "#d3c6ab"
        "###;
        std::fs::write(tmp.path().join("everforest-custom.toml"), theme_toml).unwrap();
        let mut registry = ThemeRegistry::load_builtins();
        registry.load_dir(tmp.path()).unwrap();
        let semantic = registry.get("everforest-custom").unwrap();
        assert_eq!(semantic.base.background.to_hex(), "#1f2a25");
        assert!(matches!(semantic.source, ThemeSource::NativeFile { .. }));
    }

    #[test]
    fn config_resolution() {
        let cfg = ThemeConfig {
            name: Some("everforest-dark".to_string()),
            source: Some(crate::config::schema::ThemeSourceConfig::Builtin),
            path: None,
            directories: Some(vec!["~/themes".to_string()]),
            validate_contrast: Some(true),
            fallback: Some("dark".to_string()),
        };
        let resolved = ThemeResolutionConfig::from_config(Some(&cfg));
        assert_eq!(resolved.name.as_deref(), Some("everforest-dark"));
        assert!(matches!(resolved.source, Some(ThemeSourceConfig::Builtin)));
        assert_eq!(resolved.fallback.as_deref(), Some("dark"));
        assert!(resolved.validate_contrast);
        // `directories` is used; keep the map above happy.
        let _ = resolved.directories.len();
    }

    // Make `tempfile` available in the test build.
    #[allow(dead_code)]
    fn _ensure_tempfile_in_scope(_: HashMap<String, ()>) {}
}
