# Theme Module

The theme module (`src/theme/`) implements a frontend-neutral theme system with a pipeline architecture: multiple import formats decode into a canonical `SemanticTheme`, which is then projected into frontend-specific types (currently ratatui `Theme`).

## Architecture

```
native codegg TOML  ┐
Halloy TOML         ├─►  SemanticTheme  ──►  ratatui::Theme
future Base16       ┘
```

Importers decode into `SemanticTheme`. Frontend projections consume it. The two never mix — importers must not project into ratatui directly.

## Modules

| Module | File | Purpose |
|--------|------|---------|
| `schema` | `schema.rs` | Canonical `SemanticTheme` struct and color group types |
| `color` | `color.rs` | RGB primitive, hex conversion, contrast math |
| `native` | `native.rs` | Native codegg TOML importer/exporter |
| `halloy` | `halloy.rs` | Halloy TOML compatibility importer |
| `validate` | `validate.rs` | Contrast and structural diagnostics |
| `registry` | `registry.rs` | Collection of available themes + resolution rules |
| `target` | `target/` | Frontend projections (currently ratatui only) |
| `error` | `error.rs` | Theme error type |

## SemanticTheme (`schema.rs`)

The canonical, frontend-neutral theme representation. Every field is a concrete RGB color:

```rust
pub struct SemanticTheme {
    pub id: String,
    pub name: String,
    pub source: ThemeSource,        // Builtin | NativeFile | HalloyFile | Inline
    pub base: BaseColors,           // background, foreground
    pub ui: UiColors,               // accent_primary/secondary, border, selection, panel/input/title backgrounds
    pub text: TextColors,           // muted, link
    pub status: StatusColors,       // success, warning, error, info, debug, trace
    pub conversation: ConversationColors,  // user, assistant, system, tool_call, tool_result, timestamp
    pub code: CodeColors,           // foreground, syntect_theme
    pub diff: DiffColors,           // added, removed, modified
    pub agents: AgentColors,        // planner, coder, reviewer, tester, security
}
```

IDs are normalized to lowercase kebab-case via `SemanticTheme::normalize_id()`.

## ThemeRegistry (`registry.rs`)

Single source of truth for available themes. Owns:

- **Bundled themes**: ~45 Halloy-format themes from `assets/themes/halloy/` via `include_str!`
- **User themes**: Loaded from `~/.config/codegg/themes` or directories in `[theme].directories`
- **Diagnostics**: Accumulated during loading and validation

### Loading

1. `load_builtins()` — Parse all bundled Halloy themes
2. `load_with_config(cfg)` — Built-ins + user directories + explicit path + validation
3. `load_dir(dir)` — Load all `*.toml` files from a directory
4. `load_file_auto(path)` — Auto-detect Halloy vs native format

### Resolution

```
requested name → fallback name → "cyber-red" (default) → any theme → placeholder
```

`ThemeResolutionConfig` drives resolution: `name`, `source`, `path`, `directories`, `fallback`, `validate_contrast`.

### Duplicate Handling

User themes override built-ins with the same ID. A diagnostic warning is emitted.

## Import Formats

### Native codegg TOML (`native.rs`)

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

### Halloy TOML (`halloy.rs`)

Parses Halloy IRC client theme format. The `looks_like_halloy()` heuristic detects this format. Bundled themes use this format.

## Validation (`validate.rs`)

- **Contrast checking**: WCAG contrast ratio validation between background/foreground and other color pairs
- **Structural diagnostics**: Missing fields, invalid hex values
- **ThemeDiagnostic**: `Error` or `Warn` level with theme ID, optional file path, and message

## Frontend Projection (`target/`)

`SemanticTheme` → `ratatui::Theme` via `Theme::from(&SemanticTheme)`. The ratatui `Theme` type is what the TUI actually uses for rendering.

Future frontends (e.g., iced GUI) add new files under `target/` that project `SemanticTheme` into their style systems.

## Configuration

```toml
[theme]
name = "catppuccin-mocha"     # requested theme name
source = "halloy"              # optional: auto | builtin | native | halloy
path = "~/themes/custom.toml" # optional: explicit theme file
directories = ["~/themes"]    # optional: additional theme directories
fallback = "cyber-red"         # fallback when requested theme not found
validate_contrast = true       # enable contrast validation
```

## Default Theme

`cyber-red` is the default when no `[theme].name` is configured. The `builtin_fallback()` function provides a hardcoded dark theme as the last-resort placeholder.

## Integration

- `ThemeRegistry` is constructed during app startup via `ThemeRegistry::load_with_config()`
- The resolved `Arc<Theme>` is stored in `UiState::theme`
- The TUI renders using the ratatui `Theme` projection
- Theme diagnostics are logged at startup
