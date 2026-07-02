# wasm-status-widget

A WASM plugin that registers a left-side panel and a right-side status widget.

## Surfaces

- **Panel**: `system-info` (left placement) — shows project directory and model as a key-value list.
- **Status Widget**: `project-name` (right placement) — shows the project directory name in the status bar, refreshed every 5 seconds.

## Panel IDs

Panel IDs are auto-namespaced by plugin ID. The `id` in `plugin.toml` is the contribution ID within this plugin's namespace.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/wasm_status_widget.wasm plugin.wasm
```

## Install

```bash
# Linux
cp -r . ~/.local/share/codegg/plugins/wasm-status-widget/
# macOS
cp -r . "$HOME/Library/Application Support/codegg/plugins/wasm-status-widget/"
# Windows (PowerShell)
# Copy-Recurse . "$env:LOCALAPPDATA\codegg\plugins\wasm-status-widget\"
```

## Disabling in Unsupported Clients

Clients that don't support panels or status items will ignore the corresponding UI effects. The plugin gracefully handles this — if the client doesn't advertise `panel` or `status_item` in `PluginUiCapabilities`, the effects are silently not rendered.
