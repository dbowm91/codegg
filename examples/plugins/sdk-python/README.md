# sdk-python — Vendorable Python Helper for codegg Plugins

A small, stdlib-only Python package for building codegg process plugins.
This is **not** published to PyPI — copy it into your plugin project or
install from a local path.

## What it provides

- `read_invocation()` / `write_response()` — stdin/stdout protocol I/O
- `ok_response()` / `error_response()` — `PluginResponse` builders
- `emit_chat()`, `show_toast()`, `open_dialog()`, `open_panel()`,
  `add_status_item()` — effect builders
- `text_node()`, `markdown_node()`, `key_value_node()`, `table_node()` —
  UI node builders
- `diagnostic()`, `write_diagnostic()` — diagnostic helpers
- `is_valid_invocation()` — protocol version check
- `PLUGIN_PROTOCOL_VERSION` — the current protocol version constant

## Usage

### Minimal plugin (zero SDK)

```python
import sys
print("hello from my plugin")
```

### With the SDK

```python
import json
from codegg_plugin import (
    read_invocation,
    ok_response,
    write_response,
    emit_chat,
    open_dialog,
    table_node,
)

inv = read_invocation()
provider = "anthropic"
for i, arg in enumerate(inv["args"]):
    if arg == "--provider" and i + 1 < len(inv["args"]):
        provider = inv["args"][i + 1]

resp = ok_response(
    effects=[
        emit_chat(f"Quota for {provider}"),
        open_dialog(
            id="quota",
            title="Quota",
            body=table_node(
                columns=["Provider", "Status"],
                rows=[[provider, "healthy"]],
            ),
        ),
    ],
    data={"provider": provider},
)
write_response(resp)
```

## Installation in a plugin

### Option A: copy into your project

```bash
cp -r examples/plugins/sdk-python/codegg_plugin/ <your-plugin>/codegg_plugin/
```

### Option B: pip install from path

```bash
pip install -e ./examples/plugins/sdk-python
```

## Corresponding Rust types

The Rust SDK builders (see `examples/plugins/sdk-rust/`) mirror these
Python helpers. The wire format is identical — snake_case JSON with `kind`/`type`
tags as defined in `crates/codegg-protocol/src/plugin.rs` and
`crates/codegg-protocol/src/ui.rs`.

| Python builder | Rust type |
|----------------|-----------|
| `ok_response()` | `PluginResponse { ok: true, .. }` |
| `error_response()` | `PluginResponse { ok: false, .. }` |
| `emit_chat()` | `UiEffect::EmitChat { block: ChatBlock }` |
| `open_dialog()` | `UiEffect::OpenDialog { dialog: DialogSpec }` |
| `table_node()` | `UiNode::Table(TableNode { .. })` |
| `text_node()` | `UiNode::Text(TextNode { .. })` |

## Safety

- Stdlib only — no third-party dependencies
- Only reads stdin, writes stdout
- No network, no secrets, no file I/O
