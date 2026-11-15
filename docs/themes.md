# Themes

CodeGG includes built-in themes and can load user themes from TOML files. The default theme is `cyber-red`.

## Choose a theme

Open the picker with `/themes` or `/theme`. Use `j` and `k` or the arrow keys to preview themes. Press `Enter` to save the selection, or `Esc` to restore the theme that was active before previewing.

You can also use these commands:

- `/theme list` to preview available theme ids (up to 20, with a count of any remaining themes).
- `/theme use <name>` to apply and save a theme.
- `/theme reload` to reload theme files.
- `/theme diagnostics` to inspect import and validation messages.

Theme lookups accept ids and display names. For slash commands, prefer the single-token id shown by `/theme list`. The picker and `/theme list` include built-in and successfully loaded user themes.

## Load a native CodeGG theme

Add a theme directory to your config. CodeGG scans only the listed directories and only their top-level `*.toml` files.

```jsonc
{
  "theme": {
    "name": "everforest-dark",
    "directories": ["~/.config/codegg/themes"],
    "fallback": "cyber-red"
  }
}
```

A native theme is TOML. For example:

```toml
[meta]
id = "everforest-dark"
name = "Everforest Dark"

[base]
background = "#2d3433"
foreground = "#d3c6ab"

[ui]
accent_primary = "#7dbaac"
selection = "#424b48"
```

The native schema has additional sections for interface, text, status, conversation, code, diff, and agent colors. Omitted values come from the fallback theme. The `id` is optional; without it, CodeGG derives the id from the file name. Use an id that is already available for `fallback`; an unknown fallback resolves to `cyber-red`.

## Import a Halloy theme

CodeGG reads Halloy-compatible TOML files directly. Add the file's directory to `theme.directories`, or set `theme.path` to a specific file. CodeGG detects whether each file is a Halloy or native theme from its contents, so no parser-forcing setting is needed.

```jsonc
{
  "theme": {
    "name": "tokyo-night-storm",
    "path": "~/.config/halloy/themes/tokyo-night-storm.toml",
    "fallback": "cyber-red"
  }
}
```

Halloy themes are translated into CodeGG colors, so the result may not look identical to Halloy. Some Halloy fields have no CodeGG equivalent and are ignored. Contrast-sensitive colors may be adjusted for readability; use `/theme diagnostics` to inspect warnings and adjustments.

## Source verification

Verified 2026-11-15 against `crates/codegg-config/src/schema.rs`, `crates/codegg-config/src/paths.rs`, `src/theme/registry.rs`, `src/theme/native.rs`, `src/theme/halloy.rs`, `src/tui/command.rs`, `src/tui/components/dialogs/theme.rs`, `src/tui/app/mod.rs`, and `src/tui/app/input.rs`. Confirmed the default theme, picker behavior, supported config keys and JSONC format, top-level directory scan, native fallback handling, Halloy format detection, diagnostics behavior, and the `/theme list` preview limit.

Corrected 2026-11-15: the previous guide said `source = "native"` or `source = "halloy"` forces a parser. File loading calls `load_file_auto`, which selects the parser from the file contents. The instructions now use `path` or `directories` without a parser-forcing setting. The main config examples are JSONC; only the theme files are TOML.
