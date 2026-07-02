# wasm-hook-message-transform

A WASM plugin that demonstrates an observation-only lifecycle hook via event subscription.

## Hook Type

Event subscription (`type = "event_subscription"`). This plugin observes all events (`event_type = "*"`) without modifying state.

## Policy

Observation hooks are allowed by default by the plugin lifecycle policy. Mutating or blocking hooks require explicit enablement. This plugin only observes — it never modifies state.

Process lifecycle hooks remain denied by default.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/wasm_hook_message_transform.wasm plugin.wasm
```

## Install

```bash
# Linux
cp -r . ~/.local/share/codegg/plugins/wasm-hook-message-transform/
# macOS
cp -r . "$HOME/Library/Application Support/codegg/plugins/wasm-hook-message-transform/"
# Windows (PowerShell)
# Copy-Recurse . "$env:LOCALAPPDATA\codegg\plugins\wasm-hook-message-transform\"
```

## What It Does

When invoked, returns a `PluginResponse` with:
- `data.observed_event` = the event type string
- A debug diagnostic: `"observed event: <type>"`
