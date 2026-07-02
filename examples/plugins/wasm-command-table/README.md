# wasm-command-table

A WASM plugin that exposes a `/wasm-table` command returning a dialog with a table of loaded plugins.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/wasm_command_table.wasm plugin.wasm
```

## Install

```bash
# Linux
cp -r . ~/.local/share/codegg/plugins/wasm-command-table/
# macOS
cp -r . "$HOME/Library/Application Support/codegg/plugins/wasm-command-table/"
# Windows (PowerShell)
# Copy-Recurse . "$env:LOCALAPPDATA\codegg\plugins\wasm-command-table\"
```

## Run

In codegg, type: `/wasm-table`

## ABI

Uses the modern `codegg_plugin_invoke` entry point. The plugin parses the JSON invocation, builds a table response, and returns it as a `PluginResponse` with an `OpenDialog` effect.
