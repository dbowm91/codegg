# codegg-plugin-sdk

Rust SDK for building WASM plugins for codegg.

## Quick Start

```rust
use codegg_plugin_sdk::builders::*;
use codegg_plugin_sdk::codegg_plugin;
use codegg_protocol::plugin::{PluginInvocation, PluginResponse};

fn handle(inv: PluginInvocation) -> PluginResponse {
    response_chat_markdown("Hello from WASM!")
}

codegg_plugin!(handle);
```

## How It Works

The `codegg_plugin!` macro exports three functions required by the codegg WASM ABI:

| Export | Signature | Purpose |
|--------|-----------|---------|
| `allocate` | `(i32) -> i32` | Allocate memory in the plugin's linear heap |
| `deallocate` | `(i32, i32)` | Free memory (no-op with bump allocator) |
| `codegg_plugin_invoke` | `(i32, i32) -> i64` | Main entry point: receives JSON invocation, returns packed response |

## Memory Model

The SDK uses a simple bump allocator (1 MiB heap). Memory is never freed during a single invocation — this is fine because each invocation is short-lived. The `deallocate` export is a no-op but must exist for ABI compatibility.

## Building for WASM

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown --release
```

The output `.wasm` file goes in `target/wasm32-unknown-unknown/release/`.

## Builder Reference

| Function | Returns | Description |
|----------|---------|-------------|
| `text_node(text)` | `UiNode::Text` | Plain text node |
| `markdown_node(md)` | `UiNode::Markdown` | Markdown text node |
| `code_node(lang, code)` | `UiNode::Code` | Code block with optional language |
| `table_node(cols, rows)` | `UiNode::Table` | Table with headers and rows |
| `key_value_node(entries)` | `UiNode::KeyValue` | Key-value list |
| `progress_node(label, cur, total)` | `UiNode::Progress` | Progress indicator |
| `container_node(title, children)` | `UiNode::Container` | Nested container |
| `response_chat(text, format)` | `PluginResponse` | Emit a chat message |
| `response_chat_markdown(md)` | `PluginResponse` | Emit a markdown chat message |
| `response_dialog(id, title, body, modal)` | `PluginResponse` | Open a dialog |
| `response_panel(id, title, placement, body)` | `PluginResponse` | Open a panel |
| `response_status(id, placement, body)` | `PluginResponse` | Add a status item |
| `ok_response(effects, data)` | `PluginResponse` | Successful response |
| `error_response(message)` | `PluginResponse` | Error response |
| `diagnostic(level, msg)` | `PluginDiagnostic` | Diagnostic message |

## Examples

See the companion example plugins:
- `wasm-command-table` — command that returns a table dialog
- `wasm-hook-message-transform` — event subscription observation hook
- `wasm-status-widget` — panel and status widget registration
