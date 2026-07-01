# Phase 13 Plan: Plugin SDKs and Examples

## Objective

Provide practical, tested examples and small SDK helpers so plugin authors can build Codegg plugins without reverse-engineering internal DTOs. Examples should come before elaborate SDK abstractions.

This phase should make the plugin system approachable while preserving the protocol-first model:

- simplest process plugin: stdout only;
- structured process plugin: JSON `PluginResponse`;
- WASM command plugin: JSON ABI;
- WASM lifecycle hook plugin;
- status/panel UI plugin;
- Rust helper crate for WASM plugins;
- Python helper package for process/stdin/stdout plugins.

PyO3 remains out of scope. The Python path should be subprocess JSON helpers first.

## Example Layout

Add an examples tree:

```text
examples/plugins/
  process-quota-text/
    command/quota.md
    scripts/quota.py
    README.md
  process-quota-json/
    command/quota-json.md
    scripts/quota_json.py
    README.md
  wasm-command-table/
    Cargo.toml
    src/lib.rs
    plugin.toml
    README.md
  wasm-hook-message-transform/
    Cargo.toml
    src/lib.rs
    plugin.toml
    README.md
  wasm-status-widget/
    Cargo.toml
    src/lib.rs
    plugin.toml
    README.md
  builtin-reference/
    README.md
```

Keep examples small enough to maintain.

## Example 1: Process Stdout Command

Goal: demonstrate the zero-SDK path.

`command/quota.md`:

```markdown
---
description: Show provider quota
runtime: process
command: python3
args: ["scripts/quota.py"]
stdout: text
timeout_ms: 5000
---
```

`quota.py` prints plain text. Codegg renders output through `EmitChat`/info surface.

Acceptance:

- works without importing a Codegg SDK;
- output is visible;
- does not require JSON stdin.

## Example 2: Process Structured JSON Command

Goal: demonstrate `PluginInvocation` stdin and `PluginResponse` stdout.

`command/quota-json.md`:

```markdown
---
description: Show provider quota as a dialog
runtime: process
command: python3
args: ["scripts/quota_json.py"]
stdin: json
stdout: json
timeout_ms: 5000
output: ["dialog", "chat"]
---
```

The script should:

1. read `PluginInvocation` JSON from stdin;
2. build a `PluginResponse` with `OpenDialog` or `EmitChat`;
3. write JSON to stdout;
4. write warnings to stderr only when appropriate.

Acceptance:

- validates the JSON shape documented in `codegg-protocol`;
- demonstrates diagnostics;
- demonstrates graceful error output.

## Example 3: WASM Command Returning Table

Goal: demonstrate modern WASM ABI with `codegg_plugin_invoke`.

Rust crate compiled to `wasm32-unknown-unknown` should:

- export `allocate`;
- export `deallocate`;
- export `codegg_plugin_invoke(ptr, len) -> i64`;
- parse `PluginInvocation`;
- return `PluginResponse` with a `UiEffect::OpenDialog` containing `UiNode::Table`.

Acceptance:

- build instructions are reproducible;
- ABI code is minimal and reusable;
- response opens a table/dialog in Codegg.

## Example 4: WASM Lifecycle Hook

Goal: demonstrate non-command plugin behavior safely.

Pick a low-risk hook first:

- event observation hook; or
- post-tool hook; or
- message transform hook only if policy gates are documented.

Prefer event observation or post-tool hook because they are safer and less likely to mutate model context.

Acceptance:

- hook manifest declares hook capability;
- hook returns diagnostics/effects;
- docs explain required policy settings;
- process lifecycle hooks remain denied by default.

## Example 5: Status Widget / Panel

Goal: demonstrate durable UI surfaces.

A WASM or process plugin returns:

- `AddStatusItem` with a short text body;
- `OpenPanel` with a key-value/table body.

Acceptance:

- IDs are namespaced by plugin id;
- unsupported clients degrade or omit safely;
- remote snapshot includes durable panel/status metadata.

## Rust WASM SDK

### Location

Prefer:

```text
crates/codegg-plugin-sdk/
```

or, if adding a workspace crate is too heavy initially:

```text
examples/plugins/sdk-rust/
```

A real workspace crate is preferable once examples prove stable.

### Contents

The SDK should provide:

- re-exported DTOs or DTO-compatible structs;
- helpers for allocation/deallocation;
- helper to pack `(ptr, len)` into `i64`;
- helper macro or function for `codegg_plugin_invoke`;
- builders for common `PluginResponse` and `UiNode` values.

Minimal API:

```rust
pub fn response_chat(markdown: impl Into<String>) -> PluginResponse;
pub fn response_dialog(id: impl Into<String>, title: impl Into<String>, body: UiNode) -> PluginResponse;
pub fn table(columns: Vec<String>, rows: Vec<Vec<String>>) -> UiNode;
pub fn key_values(entries: Vec<(String, String)>) -> UiNode;
```

Do not hide the protocol shape too much. Plugin authors should still understand invocation/response.

### Tests

- ABI pack/unpack round trip;
- chat response serialization;
- dialog/table response serialization;
- allocate/invoke helper test if possible under WASM target.

## Python Process SDK

### Location

Prefer:

```text
examples/plugins/sdk-python/codegg_plugin/
```

Do not publish a package in this phase. Keep it vendorable.

### Contents

Simple helpers:

```python
from codegg_plugin import read_invocation, emit_chat, open_dialog, table, diagnostic, write_response

inv = read_invocation()
write_response(open_dialog("quota", "Quota", table([...], [...])) )
```

Features:

- stdin read;
- stdout JSON write;
- stderr diagnostics helper;
- no network/file abstraction;
- no secret helper in this phase.

### Tests

- read sample invocation;
- emit chat response JSON;
- open dialog/table response JSON;
- diagnostics JSON;
- invalid stdin yields structured error.

## Documentation

Update `docs/PLUGINS.md` with:

- quickstart: process stdout plugin;
- quickstart: process JSON plugin;
- quickstart: WASM command plugin;
- manifest examples;
- runtime/trust notes;
- permission/output surface notes;
- validation command examples.

Add `examples/plugins/README.md` with a matrix:

```text
Example | Runtime | Requires SDK | UI Surface | Hook? | Safety Notes
```

## Validation Commands

Add or document commands:

```bash
cargo test --workspace
cargo test --features plugins
cargo check --target wasm32-unknown-unknown -p <example-wasm-plugin>
python3 examples/plugins/process-quota-json/scripts/quota_json.py < sample_invocation.json
```

If WASM target is not installed, docs should explain:

```bash
rustup target add wasm32-unknown-unknown
```

## Acceptance Criteria

- At least two process examples work: stdout-only and JSON response.
- At least one WASM command example builds and documents the ABI.
- At least one lifecycle hook example exists, preferably observation/post-tool.
- Rust SDK/helper exists or is staged under examples with tests.
- Python helper exists under examples and is tested with sample JSON.
- Docs include quickstarts and safety notes.
- Examples do not require secrets or network access.
- Examples are small enough to maintain in CI.
