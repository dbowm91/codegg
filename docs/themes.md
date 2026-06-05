# Themes

codegg's theme system has three layers:

1. **Frontend-neutral schema** — `crate::theme::schema::SemanticTheme`. The
   canonical, in-memory representation. All importers (native codegg TOML,
   Halloy TOML, future Base16) decode into this schema.
2. **Registry** — `crate::theme::registry::ThemeRegistry`. Owns the available
   themes, accepts user-supplied directories, applies fallback, and emits
   diagnostics.
3. **Projection** — `crate::theme::target::ratatui` (and eventually
   `crate::theme::target::iced`) maps the semantic schema to a frontend's
   style catalog. The TUI today uses `crate::tui::theme::Theme`; a future
   iced GUI would use a different projection. Halloy themes are *not*
   parsed into either projection directly.

```
native codegg TOML  ┐
Halloy TOML         ├─►  SemanticTheme  ──►  ratatui::Theme
future Base16       ┘
```

## Built-in themes

codegg ships with **50** Halloy-format built-in themes sourced from the
upstream [Halloy themes gallery](https://themes.halloy.chat) and bundled as
TOML files in `assets/themes/halloy/`. They are loaded with `include_str!`
so they are always available without filesystem access, and the parser
treats them identically to user-supplied Halloy files. **Cyber Red** is
the default theme.

The id is the kebab-case slug (e.g. `cyber-red`); the display name is the
original case-preserved name from the Halloy gallery (e.g. `"Cyber Red"`).
Either can be passed to `/theme use`.

Bundled themes (alphabetical by id):

| id | name |
|----|------|
| `acton` | acton |
| `bam` | bam |
| `base16-atelier-forest-light` | base16-atelier-forest-light |
| `berlin` | berlin |
| `black-but-with-important-highlights` | black but with important highlights |
| `booberry` | Booberry |
| `broc` | broc |
| `catppuccin-latte` | Catppuccin Latte |
| `catppuccin-macchiato` | Catppuccin Macchiato |
| `catppuccin-mocha` | Catppuccin Mocha |
| `cork` | cork |
| **`cyber-red`** | **Cyber Red** (default) |
| `cyberpunk` | Cyberpunk |
| `dark-green` | Dark Green |
| `discord` | Discord |
| `discord-80-saturation` | Discord (80% Saturation) |
| `dracula` | Dracula |
| `ferra` | ferra |
| `ferra-light` | Ferra Light |
| `flexor-dark` | Flexor Dark |
| `forest` | forest |
| `gruvbox` | Gruvbox |
| `halcyon-dark` | Halcyon Dark |
| `intellij-light` | IntelliJ Light |
| `kanagawa` | Kanagawa |
| `lisbon` | lisbon |
| `macaw-dark` | Macaw Dark |
| `macaw-light` | Macaw Light |
| `matrix` | Matrix |
| `midnight` | midnight |
| `noctis-lilac` | Noctis Lilac |
| `nord` | Nord |
| `nostromo-terminal` | Nostromo Terminal |
| `one-dark` | One Dark |
| `oslo` | oslo |
| `oxocarbon` | Oxocarbon |
| `plum` | plum |
| `portland` | portland |
| `rose-pine` | Rose Pine |
| `rose-pine-dawn` | Rose Pine Dawn |
| `rose-pine-moon` | Rose Pine Moon |
| `solarized-dark` | Solarized Dark |
| `sonokai` | Sonokai |
| `sunset` | sunset |
| `tofino` | tofino |
| `tokyo-night-storm` | Tokyo Night Storm |
| `vanimo` | vanimo |
| `vesper` | VESPER |
| `vik` | vik |
| `zenburn` | Zenburn |

## Selecting a built-in theme

The active theme is persisted in two places:

1. **SQLite** (`user_preferences.theme.active`) — the authoritative
   source, written every time you commit a new theme. Survives a
   config-file reset.
2. **Config file** (`[theme].name`) — mirrored for external tooling.

On startup, codegg reads the SQLite value first, then the config file,
then falls back to the default (`cyber-red`).

```toml
[theme]
name = "catppuccin-mocha"
fallback = "cyber-red"
```

If `name` cannot be resolved, codegg falls back to `fallback`, then the
default theme id (`cyber-red`).

You can change the active theme at runtime through:

- The theme picker dialog (`/themes` or `/theme` in the prompt) — see
  the *Live preview* section below.
- The `/theme use <name>` slash command.
- Editing `name` in the config file (re-applied on the next start).

## Adding a native codegg theme

Native themes are TOML files that mirror the semantic schema. The simplest
form looks like this:

```toml
[meta]
id = "everforest-dark"
name = "Everforest Dark"

[base]
background = "#2d3433"
foreground = "#d3c6ab"

[ui]
accent_primary = "#7dbaac"
accent_secondary = "#b5a7a8"
border = "#424b48"
border_focused = "#7dbaac"
selection = "#424b48"
selection_dim = "#4c5552"
panel_background = "#323937"
input_background = "#252b29"
title_background = "#323937"

[text]
muted = "#7e8983"
link = "#7dbaac"

[status]
success = "#8eb777"
warning = "#dbac50"
error = "#e66d5b"
info = "#7dbaac"
debug = "#b5a7a8"
trace = "#7e8983"

[conversation]
user = "#d3c6ab"
assistant = "#b5a7a8"
system = "#7e8983"
tool_call = "#7dbaac"
tool_result = "#8eb777"
timestamp = "#7e8983"

[code]
foreground = "#d3c6ab"
syntect_theme = "everforest-dark"

[diff]
added = "#8eb777"
removed = "#e66d5b"
modified = "#dbac50"

[agents]
planner = "#7dbaac"
coder = "#8eb777"
reviewer = "#dbac50"
tester = "#7dbaac"
security = "#e66d5b"
```

Any color you omit is filled in from the fallback theme. The `id` is also
optional: when absent, codegg derives the id from the file name.

Point codegg at the directory containing the theme:

```toml
[theme]
name = "everforest-dark"
directories = ["~/.config/codegg/themes"]
fallback = "dark"
```

codegg loads every `*.toml` file in `directories`. Subdirectories are
ignored in phase 1. If two files share the same id, the later-loaded entry
overrides the earlier one and a warning diagnostic is recorded.

## Importing Halloy themes

codegg reads Halloy-format theme files directly — both bundled and
user-supplied files go through the same parser. The TUI takes a
`.halloy`-compatible TOML file (the same format Halloy itself uses)
and applies it as-is. This makes it easy to follow the Halloy theme
community: a fresh `.toml` from `themes.halloy.chat` drops in without
any conversion.

Three ways to load a Halloy file:

### 1. Drop into the user themes directory

Put the file in `~/.config/codegg/themes/` (or any directory you list in
`[theme].directories`):

```toml
[theme]
directories = ["~/.config/codegg/themes"]
```

codegg scans each directory for `*.toml` files, detects the format
heuristically (presence of `[general]`/`[buffer]`/`[text]`), and adds the
theme to the registry. The id is derived from the file stem.

### 2. Point at a specific file

```toml
[theme]
name = "tokyo-night-storm"
source = "halloy"
path = "~/.config/halloy/themes/tokyo-night-storm.toml"
fallback = "cyber-red"
```

`source = "auto"` lets codegg detect the format by file content;
`source = "native"` forces the native codegg parser.

### 3. The Halloy gallery

Open [`themes.halloy.chat`](https://themes.halloy.chat), click "Download
TOML file" on a theme, and drop the file into your themes directory. The
file uses Halloy's TOML schema verbatim and is loaded by the same parser
that powers the bundled set.

### Halloy field mapping

The mapping is approximate and lossy. Halloy is an iced IRC client with
GUI-specific fields that codegg has no equivalent for. We map:

| Halloy key | codegg slot |
|------------|-------------|
| `[general].background` | `base.background` |
| `[general].border` | `ui.border` |
| `[general].horizontal_rule` | `ui.border` (only if `border` absent) |
| `[general].highlight_indicator` | `ui.accent_primary` (fallback) |
| `[general].unread_indicator` | `ui.accent_primary` (final fallback) |
| `[text].primary` | `base.foreground` |
| `[text].secondary` | `text.muted` |
| `[text].tertiary` | `conversation.assistant` |
| `[text].success` | `status.success`, `diff.added` |
| `[text].warning` | `status.warning`, `diff.modified` |
| `[text].error` | `status.error`, `diff.removed` |
| `[text].info` | `status.info`, `conversation.tool_call`, `agents.planner` |
| `[text].debug` | `status.debug` |
| `[text].trace` | `status.trace` |
| `[buffer].background` | `ui.panel_background` |
| `[buffer].background_text_input` | `ui.input_background` |
| `[buffer].background_title_bar` | `ui.title_background` |
| `[buffer].border` | (parsed; alpha-stripped if 8-hex) |
| `[buffer].border_selected` | `ui.border_focused` |
| `[buffer].selection` | `ui.selection` |
| `[buffer].code` | `code.foreground` |
| `[buffer].url` | `text.link` |
| `[buffer].timestamp` | `conversation.timestamp` |
| `[buffer].action` | `conversation.tool_call` |
| `[buffer].topic` | `conversation.assistant` |
| `[buffer].nickname` | `conversation.user` |
| `[buffer].highlight` | `agents.coder` |
| `[buffer.server_messages].default` | `status.info` |
| `[buttons.primary].background_selected` | `ui.accent_primary` (preferred) |
| `[buttons.secondary].background_selected` | `ui.accent_secondary` |

`ui.selection_dim` is derived by pulling the selection halfway back toward
the background. Anything we cannot resolve falls back to a sensible
default, never an error.

### 8-digit hex (alpha)

Halloy themes often use 8-digit hex (`#rrggbbaa`) for "transparent"
borders or RGBA selection backgrounds. codegg's parser accepts both 6-
and 8-digit hex; the alpha channel is silently discarded (the theme
layer has no concept of alpha today). `#00000000` → `#000000`,
`#73000054` → `#730000`.

### Halloy text style values

Halloy accepts two shapes for color values:

```toml
# Direct color string
primary = "#ffffff"

# Table with optional font style
primary = { color = "#ffffff", font_style = "bold" }
```

codegg accepts both. The font style is currently ignored when importing.

### Known limitations

- Halloy has fields that codegg has no equivalent for (e.g. server-message
  per-event-type colors, channel-specific highlights). They are
  silently ignored.
- The mapping is approximate: a Halloy theme will not look pixel-identical
  in codegg because the TUI has different visual regions.
- The `code.syntect_theme` value is not derived from Halloy. The ratatui
  projection picks a default based on background luminance, or you can
  provide a native codegg theme file that sets it explicitly.

## Slash commands

| Command | Description |
|---------|-------------|
| `/themes` / `/theme` | Open the theme picker dialog (live preview). |
| `/theme list` | Show the names of all available themes in a toast. |
| `/theme use <name>` | Apply the named theme and persist the choice. |
| `/theme reload` | Rebuild the registry from disk and re-apply the current theme. |
| `/theme diagnostics` | Show all theme validation/import diagnostics. |
| `/theme <name>` | Shorthand: applies the named theme and persists the choice. |

The theme picker is also reachable through the keybinding for the dialog
and through command palette completion.

## Live preview

The theme picker applies the highlighted theme to the whole TUI as you
navigate:

- **Up / Down** — moves the highlight. The TUI immediately recolors.
  Nothing is persisted yet.
- **Enter** — commits the new theme. Writes to `user_preferences` and
  the config file. Closes the dialog.
- **Esc** — reverts the live theme to whatever was active when the
  dialog opened. Closes the dialog without committing.
- **Close** (any other dismissal path) — same as Esc: reverts the live
  theme. The previewed theme is discarded.

This means you can sweep through themes to find one you like, then
commit only when ready. The footer of the picker changes to *"↑/↓
preview  Enter commit  Esc revert"* once a preview is in progress.

## Diagnostics

The registry accumulates two kinds of diagnostics:

- **Warnings** — e.g. low contrast between foreground and background, an
  invalid color string in an imported theme, a duplicate id when a user
  theme overrides a built-in. These never prevent loading.
- **Errors** — e.g. a built-in theme that fails to parse. These are
  recorded but the registry still starts (just without that theme).

Diagnostics are written to the tracing log at startup. They are also
available through `/theme diagnostics`, where each diagnostic produces a
toast with the level, theme id, field, and message.

## Future GUI work

When codegg adds an iced GUI, do not parse Halloy themes into iced widget
styles. Continue to parse Halloy into `SemanticTheme`, then project
`SemanticTheme` into the GUI's style system. This keeps the TUI and GUI
visually aligned and avoids a dependency on Halloy's IRC-specific schema.
