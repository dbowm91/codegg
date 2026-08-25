# Theme Module

The theme module (`src/theme/`) implements a frontend-neutral theme system
with a pipeline architecture: multiple import formats decode into a
canonical `SemanticTheme`, which is then projected into frontend-specific
types (currently ratatui `Theme`).

## Purpose

Provides a single canonical theme representation decoupled from any
frontend. Importers convert external formats to `SemanticTheme`;
projections convert it to ratatui styles. The two never mix.

## Where It Lives

`src/theme/` — 10 source files, all in one directory.

## How It Works

### Pipeline

```
native codegg TOML  ┐
Halloy TOML         ├─►  SemanticTheme  ──►  ratatui::Theme
future Base16       ┘
```

Importers decode into `SemanticTheme`. Frontend projections consume it.
When a future iced GUI is added, add a new file under `target/` that
projects `SemanticTheme` into the GUI's style system.

### ThemeRegistry (`src/theme/registry.rs`)

Single source of truth for available themes. Owns:

- **Bundled themes**: 50 Halloy-format themes from `assets/themes/halloy/`
  via `include_str!`
- **User themes**: Loaded from `~/.config/codegg/themes` or directories
  in `[theme].directories`
- **Diagnostics**: Accumulated during loading and validation

**Loading**:
1. `load_builtins()` — Parse all bundled Halloy themes
2. `load_with_config(cfg)` — Built-ins + user directories + explicit path
   + validation
3. `load_dir(dir)` — Load all `*.toml` files from a directory
4. `load_file_auto(path)` — Auto-detect Halloy vs native format

**Resolution**:
```
requested name → fallback name → "cyber-red" (default) → any theme → placeholder
```

`ThemeResolutionConfig` drives resolution: `name`, `source`, `path`,
`directories`, `fallback`, `validate_contrast`.

**Duplicate handling**: User themes override built-ins with the same ID.
A diagnostic warning is emitted.

## Key Types & APIs

### SemanticTheme (`src/theme/schema.rs:17`)

```rust
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
```

IDs normalized to lowercase kebab-case via `SemanticTheme::normalize_id()`.

### ThemeSource (`src/theme/schema.rs:32`)

```rust
pub enum ThemeSource {
    Builtin,
    NativeFile { path: PathBuf },
    HalloyFile { path: PathBuf },
    Inline,  // default
}
```

### Color Groups (`src/theme/schema.rs`)

- **BaseColors**: `background`, `foreground`
- **UiColors**: `accent_primary`, `accent_secondary`, `border`,
  `border_focused`, `selection`, `selection_dim`, `panel_background`,
  `input_background`, `title_background`
- **TextColors**: `muted`, `link`
- **StatusColors**: `success`, `warning`, `error`, `info`, `debug`, `trace`
- **ConversationColors**: `user`, `assistant`, `system`, `tool_call`,
  `tool_result`, `timestamp`
- **CodeColors**: `foreground`, `syntect_theme`
- **DiffColors**: `added`, `removed`, `modified`
- **AgentColors**: `planner`, `coder`, `reviewer`, `tester`, `security`

### ThemeRegistry (`src/theme/registry.rs`)

```rust
pub struct ThemeRegistry { ... }
```

Accessed via `App::theme_registry: Arc<ThemeRegistry>`.

Key functions:
- `load_with_config(cfg)` — main constructor
- `resolve_theme_for_app(config, registry)` — resolution entry point
- `builtin_fallback()` — hardcoded dark theme placeholder
- `expand_home(path)` — path expansion helper

### Frontend Projection (`src/theme/target.rs`)

`SemanticTheme` → `ratatui::Theme` via `Theme::from(&SemanticTheme)`.
The ratatui `Theme` type is what the TUI uses for rendering.

Syntect theme selection falls back based on background luminance to
avoid dark-on-dark or light-on-light syntax highlighting.

## Import Formats

### Native codegg TOML (`src/theme/native.rs`)

```toml
[meta]
id = "my-theme"
name = "My Theme"

[base]
background = "#1f2a25"
foreground = "#d3c6ab"

[ui]
accent_primary = "#78b4ff"
# ... all color fields as hex strings
```

### Halloy TOML (`src/theme/halloy.rs`)

Parses Halloy IRC client theme format. The `looks_like_halloy()`
heuristic detects this format. All 50 bundled themes use this format.

### Validation (`src/theme/validate.rs`)

- **Contrast checking**: WCAG contrast ratio validation between
  background/foreground and other color pairs
- **Structural diagnostics**: Missing fields, invalid hex values
- **ThemeDiagnostic**: `Error` or `Warn` level with theme ID, optional
  file path, and message

## Configuration Surface

```toml
[theme]
name = "catppuccin-mocha"     # requested theme name
source = "halloy"              # optional: auto | builtin | native | halloy
path = "~/themes/custom.toml" # optional: explicit theme file
directories = ["~/themes"]    # optional: additional theme directories
fallback = "cyber-red"         # fallback when requested theme not found
validate_contrast = true       # enable contrast validation
```

### Default Theme

`cyber-red` is the default when no `[theme].name` is configured.
The `builtin_fallback()` function provides a hardcoded dark theme as
the last-resort placeholder.

### Bundled Theme Count

50 Halloy-format themes in `assets/themes/halloy/`, bundled via
`include_str!` in `BUILTIN_THEME_FILES` (`src/theme/registry.rs:36`).

## Invariants & Gotchas

- **`target/` is a file, not a directory**: `src/theme/target.rs` is a
  single file containing the ratatui projection, not a subdirectory.
- **Importers must not project**: Importers decode to `SemanticTheme`
  only. Frontend projections consume it. Never mix the two.
- **50 bundled themes**: The `BUILTIN_THEME_FILES` constant embeds all
  50 Halloy themes. Count tested implicitly by registry loading.
- **Syntect theme luminance fallback**: Dark/light code theme is
  selected based on background luminance, not user preference.

## Integration

- `ThemeRegistry` constructed during app startup via
  `ThemeRegistry::load_with_config()`
- Resolved `Arc<Theme>` stored in `UiState::theme`
- TUI renders using the ratatui `Theme` projection
- Theme diagnostics logged at startup
- Theme picker dialog accessible via `/themes` command

## Testing

```bash
cargo test -p codegg -- theme    # Theme module tests
```

## Related Docs

- [tui.md](tui.md) — TUI rendering that consumes the Theme
- [config.md](config.md) — Theme config schema
