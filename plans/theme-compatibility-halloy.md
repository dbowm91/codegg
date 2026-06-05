# Plan: Theme Registry and Halloy Compatibility Layer

## Goal

Replace codegg's hardcoded TUI theme variants with a data-driven theme system that can load bundled native themes, user-provided native themes, and Halloy-compatible TOML themes. The implementation should preserve the current ratatui/TUI behavior while creating a frontend-neutral semantic theme layer that can later be projected into an iced GUI.

This is primarily a refactor of theme loading and theme representation. It should not require a broad rewrite of the existing TUI rendering code.

## Current repository state

The current theme system is centered in `src/tui/theme.rs`. That file defines the runtime `Theme` struct used by TUI widgets and also embeds all built-in variants as a static `THEMES: &[ThemeData]` array. The module exposes `Theme::from_name`, many named constructors such as `Theme::dark()` and `Theme::dracula()`, `all_themes()`, `find_theme()`, and `theme_names()`.

The current `Theme` fields are:

```rust
pub struct Theme {
    pub name: String,
    pub background: Color,
    pub foreground: Color,
    pub primary: Color,
    pub secondary: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub muted: Color,
    pub border: Color,
    pub selection: Color,
    pub selection_dim: Color,
    pub alternate_bg: Color,
    pub input_bg: Color,
    pub code_theme: &'static str,
    pub link: Color,
}
```

`UiState` stores the active theme as `Arc<Theme>`. This is good and should be retained.

`App::with_config` currently initializes the theme with `Arc::new(Theme::dark())`.

`ThemePickerDialog::new` currently calls `crate::tui::theme::all_themes()` and stores a `Vec<Theme>`. Selection currently resolves by theme name with `crate::tui::theme::find_theme(&theme_name)` and then assigns `self.ui_state.theme = Arc::new(theme)`.

The config schema currently does not include theme configuration fields. This means theme choice is not first-class persistent config yet.

## Design principles

The Halloy schema should be treated as an import format, not codegg's native schema.

The native codegg theme model should be semantic and frontend-neutral. The TUI and future iced GUI should both project from the same semantic model rather than sharing ratatui-specific or iced-specific types.

The compatibility target is approximate semantic/palette compatibility, not pixel-perfect parity with Halloy. Halloy is an iced GUI IRC client; codegg's TUI has different visual regions and different terminal rendering constraints.

Avoid large behavioral changes in the first implementation pass. Keep the existing TUI rendering contract stable by continuing to expose a ratatui-facing `Theme` type.

Do not silently mutate imported colors in phase 1. Validate contrast and report diagnostics, but avoid automatic color adjustment unless a later explicit option is added.

## Target architecture

Introduce a frontend-neutral theme module:

```text
src/theme/
  mod.rs
  color.rs
  error.rs
  schema.rs
  registry.rs
  native.rs
  halloy.rs
  validate.rs
  target/
    mod.rs
    ratatui.rs
    iced.rs          # stub or deferred; do not require iced dependency yet
```

Leave `src/tui/theme.rs` in place initially as a ratatui-facing compatibility layer. It may either retain the current `Theme` type or re-export `crate::theme::target::ratatui::RatatuiTheme` as `Theme`.

Preferred transitional strategy:

```text
crate::theme::SemanticTheme       # frontend-neutral resolved theme
crate::theme::ThemeRegistry       # owns available themes and diagnostics
crate::tui::theme::Theme          # current ratatui runtime projection
```

Then:

```text
native codegg TOML -> SemanticTheme -> TUI Theme
Halloy TOML        -> SemanticTheme -> TUI Theme
Base16 TOML/YAML   -> SemanticTheme -> TUI Theme   # optional later
```

Future iced GUI path:

```text
native/Halloy/Base16 -> SemanticTheme -> IcedTheme/projection
```

Do not add `iced` as a dependency for this task. Add only a stub/prose comment for the eventual projection layer.

## Phase 1: Make TUI theme runtime less hardcoded

### 1. Change `code_theme` from `&'static str` to `String` or `Arc<str>`

Imported themes cannot safely provide a `&'static str`. Change the current TUI runtime struct:

```rust
pub code_theme: String,
```

or:

```rust
pub code_theme: Arc<str>,
```

`String` is simpler for a smaller implementation model. Update `ThemeData::to_theme()` and `Theme::code_theme()` accordingly:

```rust
pub fn code_theme(&self) -> &str {
    &self.code_theme
}
```

Search for all usages of `code_theme()` and `.code_theme` and update as needed.

### 2. Preserve current public API during transition

Keep the following functions for now:

```rust
Theme::dark()
Theme::light()
Theme::from_name(name: &str) -> Option<Theme>
all_themes() -> Vec<Theme>
find_theme(name: &str) -> Option<Theme>
theme_names() -> Vec<String>
```

They may delegate to a builtin registry internally. Do not remove named constructors in this pass, even if they are eventually redundant. This avoids incidental breakage.

## Phase 2: Add semantic theme data structures

Create `src/theme/color.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeColor {
    Rgb(Rgb),
    Inherit,
}
```

Implement:

```rust
impl Rgb {
    pub fn from_hex(input: &str) -> Result<Self, ThemeError>;
    pub fn to_hex(self) -> String;
    pub fn relative_luminance(self) -> f64;
    pub fn contrast_ratio(self, other: Self) -> f64;
}
```

Hex parser requirements:

- Accept `#rrggbb`.
- Accept `rrggbb`.
- Optionally accept `#rgb` as a convenience.
- Reject invalid values with a useful error.
- Do not accept alpha in phase 1 unless the implementation is trivial and well-tested.

Implement conversion:

```rust
impl From<Rgb> for ratatui::style::Color {
    fn from(rgb: Rgb) -> Self {
        ratatui::style::Color::Rgb(rgb.r, rgb.g, rgb.b)
    }
}
```

Create `src/theme/schema.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub enum ThemeSource {
    Builtin,
    NativeFile { path: std::path::PathBuf },
    HalloyFile { path: std::path::PathBuf },
}
```

Suggested semantic groups:

```rust
pub struct BaseColors {
    pub background: Rgb,
    pub foreground: Rgb,
}

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

pub struct TextColors {
    pub muted: Rgb,
    pub link: Rgb,
}

pub struct StatusColors {
    pub success: Rgb,
    pub warning: Rgb,
    pub error: Rgb,
    pub info: Rgb,
    pub debug: Rgb,
    pub trace: Rgb,
}

pub struct ConversationColors {
    pub user: Rgb,
    pub assistant: Rgb,
    pub system: Rgb,
    pub tool_call: Rgb,
    pub tool_result: Rgb,
    pub timestamp: Rgb,
}

pub struct CodeColors {
    pub foreground: Rgb,
    pub syntect_theme: Option<String>,
}

pub struct DiffColors {
    pub added: Rgb,
    pub removed: Rgb,
    pub modified: Rgb,
}

pub struct AgentColors {
    pub planner: Rgb,
    pub coder: Rgb,
    pub reviewer: Rgb,
    pub tester: Rgb,
    pub security: Rgb,
}
```

Do not try to make this perfect. The immediate purpose is a stable internal semantic schema that can support both ratatui and iced later.

## Phase 3: Add ratatui projection

Create `src/theme/target/ratatui.rs`.

Implement conversion from `SemanticTheme` into the existing TUI `Theme`:

```rust
impl From<&SemanticTheme> for crate::tui::theme::Theme {
    fn from(theme: &SemanticTheme) -> Self {
        crate::tui::theme::Theme {
            name: theme.id.clone(),
            background: theme.base.background.into(),
            foreground: theme.base.foreground.into(),
            primary: theme.ui.accent_primary.into(),
            secondary: theme.ui.accent_secondary.into(),
            success: theme.status.success.into(),
            warning: theme.status.warning.into(),
            error: theme.status.error.into(),
            muted: theme.text.muted.into(),
            border: theme.ui.border.into(),
            selection: theme.ui.selection.into(),
            selection_dim: theme.ui.selection_dim.into(),
            alternate_bg: theme.ui.panel_background.into(),
            input_bg: theme.ui.input_background.into(),
            code_theme: theme.code.syntect_theme
                .clone()
                .unwrap_or_else(|| default_syntect_for_theme(theme)),
            link: theme.text.link.into(),
        }
    }
}
```

`default_syntect_for_theme` can be simple:

```rust
fn default_syntect_for_theme(theme: &SemanticTheme) -> String {
    if is_dark_rgb(theme.base.background) {
        "base16-ocean.dark".to_string()
    } else {
        "base16-github.light".to_string()
    }
}
```

## Phase 4: Add native codegg theme file format

Create `src/theme/native.rs`.

Native codegg themes should be TOML and close to `SemanticTheme`, but all colors should be strings. Example:

```toml
[meta]
id = "catppuccin-mocha"
name = "Catppuccin Mocha"
source = "builtin"

[base]
background = "#1e1e2e"
foreground = "#cdd6f4"

[ui]
accent_primary = "#89b4fa"
accent_secondary = "#cba6f7"
border = "#45475a"
border_focused = "#89b4fa"
selection = "#313244"
selection_dim = "#3b3c4e"
panel_background = "#232334"
input_background = "#161624"
title_background = "#232334"

[text]
muted = "#7f849c"
link = "#89b4fa"

[status]
success = "#a6e3a1"
warning = "#f9e2af"
error = "#f38ba8"
info = "#89b4fa"
debug = "#cba6f7"
trace = "#7f849c"

[conversation]
user = "#cdd6f4"
assistant = "#cba6f7"
system = "#7f849c"
tool_call = "#89b4fa"
tool_result = "#a6e3a1"
timestamp = "#7f849c"

[code]
foreground = "#cdd6f4"
syntect_theme = "catppuccin-mocha"

[diff]
added = "#a6e3a1"
removed = "#f38ba8"
modified = "#f9e2af"

[agents]
planner = "#89b4fa"
coder = "#a6e3a1"
reviewer = "#f9e2af"
tester = "#94e2d5"
security = "#f38ba8"
```

Implement:

```rust
pub fn parse_native_theme(input: &str, source: ThemeSource) -> Result<SemanticTheme, ThemeError>;
```

Use `serde::Deserialize` structs with string fields, then convert/validate into `Rgb`.

## Phase 5: Move built-ins from Rust constants into bundled TOML

Create:

```text
assets/themes/codegg/dark.toml
assets/themes/codegg/light.toml
assets/themes/codegg/catppuccin-mocha.toml
assets/themes/codegg/catppuccin-latte.toml
assets/themes/codegg/tokyonight.toml
assets/themes/codegg/gruvbox-dark.toml
assets/themes/codegg/nord.toml
assets/themes/codegg/dracula.toml
assets/themes/codegg/rose-pine.toml
assets/themes/codegg/solarized-dark.toml
assets/themes/codegg/solarized-light.toml
assets/themes/codegg/github-dark.toml
assets/themes/codegg/github-light.toml
assets/themes/codegg/high-contrast-dark.toml
```

Curate; do not necessarily carry all 31 current variants forward as first-class built-ins. The existing hardcoded set is broad but expensive to maintain. Keep compatibility aliases where possible.

If a smaller implementation pass wants lower risk, copy all existing hardcoded themes into native TOML first, then prune later. However, the intended long-term direction is fewer bundled native defaults plus user import support.

Loading built-ins can be done with `include_str!` entries initially:

```rust
const BUILTIN_THEME_FILES: &[(&str, &str)] = &[
    ("dark", include_str!("../../assets/themes/codegg/dark.toml")),
    ("light", include_str!("../../assets/themes/codegg/light.toml")),
];
```

Do not introduce a heavy asset dependency unless useful. `include_str!` is simple and predictable.

## Phase 6: Add Halloy compatibility parser

Create `src/theme/halloy.rs`.

Halloy custom themes are TOML. The relevant schema sections are:

```text
[general]
[text]
[buttons.primary]
[buttons.secondary]
[buffer]
[buffer.server_messages]
[formatting]
```

Halloy text style values can be either a direct color string or a table containing `color` and optional `font_style`. Implement untagged serde support:

```rust
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HalloyFontStyle {
    Normal,
    Italic,
    Bold,
    ItalicBold,
}
```

Define permissive section structs with `Option<String>` or `Option<HalloyTextStyle>` for fields. Do not require every Halloy key. Missing keys should be resolved by fallback logic.

Required Halloy-to-codegg mapping:

```text
Halloy [general].background                    -> base.background
Halloy [text].primary                          -> base.foreground
Halloy [text].secondary                        -> text.muted
Halloy [general].border                        -> ui.border
Halloy [buffer].border_selected                -> ui.border_focused
Halloy [buffer].selection                      -> ui.selection
Halloy [buffer].background                     -> ui.panel_background
Halloy [buffer].background_text_input          -> ui.input_background
Halloy [buffer].background_title_bar           -> ui.title_background
Halloy [general].highlight_indicator           -> ui.accent_primary fallback
Halloy [buttons.primary].background_selected   -> ui.accent_primary preferred if present
Halloy [buttons.secondary].background_selected -> ui.accent_secondary preferred if present
Halloy [text].success                          -> status.success and diff.added
Halloy [text].warning                          -> status.warning and diff.modified
Halloy [text].error                            -> status.error and diff.removed
Halloy [text].info                             -> status.info, conversation.tool_call, agents.planner
Halloy [text].debug                            -> status.debug
Halloy [text].trace                            -> status.trace
Halloy [buffer].code                           -> code.foreground
Halloy [buffer].url                            -> text.link
Halloy [buffer].timestamp                      -> conversation.timestamp
```

Fallback policy:

1. Start with codegg `dark` or `light` fallback depending on Halloy background luminance.
2. Overlay parsed Halloy fields.
3. For missing accent colors, derive from `text.info`, `buffer.url`, or fallback primary.
4. For `selection_dim`, derive by slightly lightening/darkening `selection`, or use fallback.
5. For unsupported Halloy fields, ignore them. Do not error.
6. For invalid color strings, record diagnostics and use fallback colors.

Implement:

```rust
pub fn parse_halloy_theme(
    input: &str,
    source_path: Option<&std::path::Path>,
    fallback: &SemanticTheme,
) -> Result<(SemanticTheme, Vec<ThemeDiagnostic>), ThemeError>;
```

If the Halloy file does not include an obvious name/id, derive it from the file stem. Normalize ids to lowercase kebab-case.

## Phase 7: Add validation diagnostics

Create `src/theme/validate.rs`.

Implement:

```rust
#[derive(Debug, Clone)]
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

pub fn validate_theme(theme: &SemanticTheme) -> Vec<ThemeDiagnostic>;
```

Contrast checks:

```text
base.foreground on base.background
text.muted on base.background
ui.accent_primary on base.background
ui.accent_primary on ui.selection
base.foreground on ui.input_background
status.error on base.background
status.warning on base.background
status.success on base.background
text.link on base.background
```

Use WCAG contrast math even though this is terminal UI. Thresholds can be pragmatic:

```text
primary text: warn if < 4.5
muted text: warn if < 3.0
accent/status/link: warn if < 3.0
selection text: warn if < 3.0
```

Do not fail loading on contrast warnings. Fail only if required final fields cannot be resolved.

## Phase 8: Add ThemeRegistry

Create `src/theme/registry.rs`.

Suggested API:

```rust
pub struct ThemeRegistry {
    themes: std::collections::HashMap<String, SemanticTheme>,
    aliases: std::collections::HashMap<String, String>,
    diagnostics: Vec<ThemeDiagnostic>,
}

impl ThemeRegistry {
    pub fn new() -> Self;
    pub fn load_builtins() -> Self;
    pub fn load_with_config(config: Option<&crate::config::schema::ThemeConfig>) -> Self;
    pub fn load_dir(&mut self, dir: &std::path::Path) -> Result<usize, ThemeError>;
    pub fn load_file_auto(&mut self, path: &std::path::Path) -> Result<(), ThemeError>;
    pub fn insert(&mut self, theme: SemanticTheme);
    pub fn get(&self, name: &str) -> Option<&SemanticTheme>;
    pub fn get_tui(&self, name: &str) -> Option<crate::tui::theme::Theme>;
    pub fn names(&self) -> Vec<String>;
    pub fn all_tui_themes(&self) -> Vec<crate::tui::theme::Theme>;
    pub fn diagnostics(&self) -> &[ThemeDiagnostic];
}
```

File format detection:

- If `source = "halloy"` is specified in config, parse as Halloy.
- If native codegg `[meta]` exists with recognizable fields, parse as native.
- If Halloy sections such as `[general]`, `[text]`, and `[buffer]` exist, parse as Halloy.
- Otherwise return an unsupported-format diagnostic.

Directory loading:

- Load only `.toml` files in phase 1.
- Ignore subdirectories initially unless easy.
- If duplicate ids occur, user themes should override built-ins, but record a warning diagnostic.

Default directories:

```text
~/.config/codegg/themes
```

Optional user-configured directory:

```text
~/.config/halloy/themes
```

Do not automatically read Halloy's config. Only read its theme directory if the user adds it or if codegg explicitly supports a documented `include_halloy_dir = true` option later.

## Phase 9: Add config schema fields

Modify `src/config/schema.rs`:

```rust
pub theme: Option<ThemeConfig>,
```

Add:

```rust
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: Option<String>,
    pub source: Option<ThemeSourceConfig>,
    pub path: Option<String>,
    pub directories: Option<Vec<String>>,
    pub validate_contrast: Option<bool>,
    pub fallback: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSourceConfig {
    Auto,
    Builtin,
    Native,
    Halloy,
}
```

Example config:

```toml
[theme]
name = "catppuccin-mocha"
source = "builtin"
fallback = "dark"
validate_contrast = true
```

Example Halloy import:

```toml
[theme]
name = "tokyo-night-storm"
source = "halloy"
path = "~/.config/codegg/themes/tokyo-night-storm.toml"
fallback = "dark"
validate_contrast = true
```

Example directory discovery:

```toml
[theme]
name = "rose-pine"
source = "auto"
directories = [
  "~/.config/codegg/themes",
  "~/.config/halloy/themes",
]
fallback = "dark"
validate_contrast = true
```

Implement path expansion for `~` using `dirs::home_dir()` or an existing utility if the repo has one.

## Phase 10: Wire registry into App and ThemePickerDialog

Modify `App` to own a registry:

```rust
pub theme_registry: Arc<crate::theme::ThemeRegistry>,
```

or, if avoiding `App` field expansion, store in `UiState`. Preferred: `App`, because registry diagnostics and source metadata are not transient UI state.

In `App::with_config`:

1. Load registry with config.
2. Resolve selected theme name.
3. Fall back to configured fallback, then `dark`.
4. Store active `Arc<Theme>`.
5. Store registry on `App`.
6. Optionally emit diagnostics into tracing; do not show toasts at startup unless severe.

Resolution pseudocode:

```rust
let registry = Arc::new(ThemeRegistry::load_with_config(cfg.and_then(|c| c.theme.as_ref())));
let requested = cfg
    .and_then(|c| c.theme.as_ref())
    .and_then(|t| t.name.as_deref())
    .unwrap_or("dark");
let fallback = cfg
    .and_then(|c| c.theme.as_ref())
    .and_then(|t| t.fallback.as_deref())
    .unwrap_or("dark");
let selected = registry
    .get_tui(requested)
    .or_else(|| registry.get_tui(fallback))
    .unwrap_or_else(Theme::dark);
let theme = Arc::new(selected);
```

Modify `ThemePickerDialog::new`:

```rust
pub fn new(theme: Arc<Theme>, themes: Vec<Theme>) -> Self
```

When opening the theme dialog, pass:

```rust
self.theme_registry.all_tui_themes()
```

Replace any direct use of `crate::tui::theme::all_themes()` inside the dialog.

Modify selection handling:

```rust
if let Some(theme) = self.theme_registry.get_tui(&theme_name) {
    self.ui_state.theme = Arc::new(theme);
}
```

Do not call global `find_theme` from app selection once registry is available.

## Phase 11: Persist theme selection

When a user selects a theme in the picker, update config:

```toml
[theme]
name = "selected-theme-id"
```

Preserve existing `[theme]` values like source, path, directories, fallback, and validation settings.

Use the existing config load/save pattern already used elsewhere in `App` for provider/API key updates.

If config save fails, the active theme may still change for the current session, but show an error toast.

Expected user-visible behavior:

- Selecting a theme applies it immediately.
- Successful save shows `Theme: <name>` or `Theme saved: <name>`.
- Failed save shows `Theme applied, but failed to save config: <error>`.

## Phase 12: CLI or slash command support

Add a minimal command surface. Pick whichever is easier in the existing command system.

Suggested commands:

```text
/theme list
/theme use <name>
/theme reload
/theme diagnostics
```

Minimum viable behavior:

- `/theme list`: show names from registry.
- `/theme use <name>`: apply selected theme and persist config.
- `/theme reload`: reload registry from disk and keep current theme if still available.
- `/theme diagnostics`: show validation/import diagnostics.

If slash command implementation is too much for first pass, defer and only support the theme picker plus config loading.

## Phase 13: Tests

Add unit tests for color parsing:

```text
#ffffff -> 255,255,255
ffffff  -> 255,255,255
#fff    -> 255,255,255 if short hex is supported
invalid strings error
contrast ratio black/white is about 21
```

Add native parser tests:

```text
valid native theme parses
missing required color errors or falls back as designed
invalid color produces ThemeError
```

Add Halloy parser tests:

```text
text.primary = "#ffffff" parses
text.primary = { color = "#ffffff", font_style = "bold" } parses
minimal Halloy theme overlays fallback
invalid Halloy color records diagnostic and uses fallback
file-stem-derived id works
```

Add registry tests:

```text
builtins load
user theme overrides builtin id and records warning
get_tui returns ratatui Theme
unknown theme falls back in App resolution helper
```

Add validation tests:

```text
low contrast foreground/background emits warning
high contrast theme emits no primary text warning
```

Do not require snapshot/golden terminal rendering tests in the first pass.

## Phase 14: Documentation

Add or update docs:

```text
docs/themes.md
```

Include:

- How to set a builtin theme.
- How to add native codegg themes.
- How to import/use Halloy `.toml` themes.
- Known limitations of Halloy compatibility.
- Contrast diagnostics.
- Future note that GUI and TUI will share the same semantic theme layer.

Example docs:

```toml
[theme]
name = "everforest-dark"
source = "auto"
directories = ["~/.config/codegg/themes"]
fallback = "dark"
```

Halloy example:

```toml
[theme]
name = "catppuccin-mocha"
source = "halloy"
path = "~/.config/halloy/themes/catppuccin-mocha.toml"
fallback = "dark"
```

Explain that Halloy themes are lossy imports because Halloy has IRC/GUI-specific fields and codegg has coding-agent-specific UI regions.

## Non-goals for this implementation

Do not implement an iced GUI.

Do not add `iced` as a dependency.

Do not fetch themes from `themes.halloy.chat` at runtime.

Do not implement remote theme gallery browsing.

Do not silently auto-adjust imported colors.

Do not remove all existing theme constructor APIs in the same pass.

Do not attempt pixel-perfect Halloy rendering in the TUI.

Do not support every possible Halloy/Base16 variant on day one.

## Suggested implementation order for a smaller model

1. Change `Theme.code_theme` to `String` and update compile errors.
2. Add `src/theme/color.rs`, `error.rs`, `schema.rs`, and `target/ratatui.rs`.
3. Add native parser and one or two bundled native TOML themes.
4. Add `ThemeRegistry::load_builtins()` and bridge old `all_themes/find_theme` to the registry.
5. Add `ThemeConfig` to config schema.
6. Wire registry into `App::with_config` and theme picker.
7. Add Halloy parser and mapping.
8. Add user theme directory loading.
9. Add diagnostics and validation.
10. Add persistence for selected theme.
11. Add docs and tests.
12. Optionally migrate more built-in themes from Rust constants to TOML.

## Acceptance criteria

The project compiles after `Theme.code_theme` changes.

The app still starts with the dark theme by default.

The theme picker still shows bundled themes and applies selection.

A configured builtin theme loads from config.

A native codegg TOML theme in `~/.config/codegg/themes` can be loaded and selected.

A Halloy TOML theme can be loaded through explicit `source = "halloy"` and `path = "..."` config.

Halloy text style values work in both accepted forms:

```toml
primary = "#ffffff"
```

and:

```toml
primary = { color = "#ffffff", font_style = "bold" }
```

Invalid or incomplete imported themes do not panic. They either fail with a useful diagnostic or fall back deterministically.

Contrast diagnostics are available through logs, tests, or a user-facing diagnostics command.

Selecting a theme persists the selected theme name to config.

No iced dependency is introduced.

## Notes for future GUI work

When codegg adds an iced GUI, do not parse Halloy themes directly into iced widget styles. Continue to parse Halloy into `SemanticTheme`, then project `SemanticTheme` into the GUI's style system.

This keeps TUI and GUI visually aligned while avoiding a dependency on Halloy's IRC-specific schema. Halloy remains useful as an inspiration and compatibility source, not as codegg's native theme model.

A future `src/theme/target/iced.rs` can map `SemanticTheme` into iced style catalogs or a custom codegg GUI theme object. The exact iced integration should be deferred until the GUI crate exists.
