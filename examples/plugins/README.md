# Plugin Examples and SDKs

This directory contains example plugins and helper SDKs for the codegg
plugin system. Each example is small, self-contained, and exercises a
specific plugin pattern. Use them as templates when building your own
plugins.

## Layout

```text
examples/plugins/
  process-quota-text/           # zero-SDK process plugin (plain stdout)
  process-quota-json/           # process plugin reading/writing JSON
  wasm-command-table/           # WASM plugin using the modern ABI
  wasm-hook-message-transform/  # WASM observation-hook plugin
  wasm-status-widget/           # WASM plugin with panel + status widget
  builtin-reference/            # reference walkthrough of a builtin plugin
  sdk-python/                   # vendorable Python helper package
  sdk-rust/                     # Rust helper crate for WASM plugins
```

## Example matrix

| Example | Runtime | Requires SDK | UI surface | Hook? | Safety notes |
|---------|---------|--------------|------------|-------|--------------|
| `process-quota-text` | process (stdout) | no | EmitChat (auto) | no | Local executable. No stdin. Plain text only. |
| `process-quota-json` | process (JSON) | optional (`sdk-python`) | EmitChat + OpenDialog | no | Local executable. Reads JSON stdin. Validates protocol version. |
| `wasm-command-table` | wasm | yes (`sdk-rust`) | OpenDialog (table) | no | Sandboxed. Fuel/memory bounded. |
| `wasm-hook-message-transform` | wasm | yes (`sdk-rust`) | none | yes — event subscription (observation only) | Sandboxed. Default policy permits observation hooks. |
| `wasm-status-widget` | wasm | yes (`sdk-rust`) | OpenPanel + AddStatusItem | no | Sandboxed. Panel IDs auto-namespaced by plugin id. |
| `builtin-reference` | builtin (Rust, in-tree) | n/a | n/a | reference only | For codegg contributors; external authors should use process or WASM runtimes. |
| `sdk-python` | n/a (helper) | n/a | builders for all UiEffect / UiNode variants | n/a | Stdlib only. Vendorable. |
| `sdk-rust` | n/a (helper, compiles to wasm) | n/a | builders for all UiEffect / UiNode variants | n/a | Bump allocator. Drops unused WASM heap (no per-pointer free). |

## Quickstart by example

### A) Simplest possible plugin: zero SDK, plain stdout

Copy `process-quota-text/` into your project, then run `/quota`. The
script just prints text — codegg auto-detects that stdout is not JSON
and surfaces it as an EmitChat effect.

See `process-quota-text/README.md`.

### B) Structured response with the Python helper

Copy `process-quota-json/scripts/quota_json.py` and the
`sdk-python/codegg_plugin/` package. Register the frontmatter in
`command/quota-json.md`. The script reads `PluginInvocation` JSON from
stdin and writes a `PluginResponse` with `OpenDialog` + `EmitChat`
effects to stdout.

See `process-quota-json/README.md` and `sdk-python/README.md`.

### C) WASM plugin using the Rust SDK

Build the SDK and an example plugin:

```bash
cd examples/plugins/sdk-rust
cargo build

cd ../wasm-command-table
cargo build --target wasm32-unknown-unknown --release
```

Install:

```bash
cp -r . ~/.local/share/codegg/plugins/wasm-command-table/
# On macOS:
cp -r . "$HOME/Library/Application Support/codegg/plugins/wasm-command-table/"
```

Restart codegg and run `/wasm-table`.

See `wasm-command-table/README.md`.

### D) WASM observation hook

```bash
cd examples/plugins/wasm-hook-message-transform
cargo build --target wasm32-unknown-unknown --release
cp -r . ~/.local/share/codegg/plugins/wasm-hook-message-transform/
# macOS:
# cp -r . "$HOME/Library/Application Support/codegg/plugins/wasm-hook-message-transform/"
```

The plugin subscribes to all `event_subscription` events and emits a
debug diagnostic. Default policy permits observation hooks without
changes.

See `wasm-hook-message-transform/README.md`.

## Validation

### Python SDK

```bash
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v
```

24 unit tests cover protocol parsing, response builders, and effect/node
shapes.

### Rust SDK

```bash
cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml
```

11 tests cover builders, response round-trips, and wire-format
serialization. One wasm-only test is `#[ignore]` because the bump
allocator depends on `wasm32-unknown-unknown` linear memory.

### Process example end-to-end

```bash
cat examples/plugins/process-quota-json/sample_invocation.json | \
  python3 examples/plugins/process-quota-json/scripts/quota_json.py | \
  python3 -m json.tool
```

### WASM build

```bash
cargo build --target wasm32-unknown-unknown \
  --manifest-path examples/plugins/wasm-command-table/Cargo.toml --release
```

If `wasm32-unknown-unknown` is not installed:

```bash
rustup target add wasm32-unknown-unknown
```

## Safety

- All examples are local. None make network calls or read secrets.
- Process plugins are local executables — they are not sandboxed. Treat
  them like any other locally runnable command.
- WASM plugins run inside Wasmtime with fuel + memory limits. See
  `architecture/plugin.md` for the runtime limits and
  `docs/PLUGINS.md` for the security policy defaults.
- Lifecycle hooks (mutating / blocking) are denied by default in
  `PluginPolicy`. The observation-hook example works without policy
  changes; the others would require explicit enablement.

## Manifest reference

Both `command/*.md` (project-local commands) and `plugin.toml` (installed
plugins) are supported. The project-local command frontmatter uses YAML
fields:

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `description` | string | none | Help text |
| `runtime` | `template` \| `process` | `template` | Execution mode |
| `command` | string | required if `runtime: process` | Executable |
| `args` | string[] | `[]` | Args passed before user args |
| `stdin` | `none` \| `json` | `none` | `json` pipes `PluginInvocation` JSON |
| `stdout` | `text` \| `json` \| `auto` | `auto` | `auto` tries JSON, falls back to text |
| `timeout_ms` | u64 | `5000` | Per-invocation timeout |
| `cwd` | string | none | Working directory override |
| `env` | string[] | `[]` | `KEY=VALUE` environment variables |
| `output` | string[] | `[]` | Output surfaces (`chat`, `toast`, `dialog`, `panel`, `status`) |

The installed-plugin `plugin.toml` uses the canonical `PluginManifest`
schema documented in `architecture/plugin.md`.