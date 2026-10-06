---
name: mcp-plugin
description: MCP client connections and WASM/process plugin runtime in codegg
version: 1.0.0
tags:
  - mcp
  - plugin
  - integrations
---

# MCP and Plugin Guide

Operational guide for changing `src/mcp/` and `src/plugin/`. The full
contracts live in `architecture/mcp.md` and `architecture/plugin.md`;
this skill covers the lifecycle and security boundaries that are easy
to violate.

## Ownership Map

| Layer | Location | Role |
|-------|----------|------|
| MCP service | `src/mcp/mod.rs`, `local.rs`, `remote.rs` | `McpService`, `McpTool`, `McpClientType`, `McpExposurePolicy`; stdio child vs HTTP+SSE remotes; `McpConnectionManager` |
| MCP auth/CLI | `src/mcp/auth.rs`, `cli.rs`, `ide_server.rs` | `OAuthManager` (PKCE, token encryption, callback server), `add/list/remove/enable/debug`, `openDiff` IDE server |
| Eggsearch wiring | `src/search_backend/`, `src/mcp/` bootstrap | Production `websearch`/`webfetch` wrappers dispatch via `search_backend` → eggsearch MCP; legacy `src/search/` is fallback only — never add new web-search providers there (external `eggsearch` project owns them) |
| Plugin service | `src/plugin/service.rs`, `registry.rs`, `manifest.rs` | `PluginService` hook dispatch + command invocation; `PluginRegistry` capability index; `PluginManifest`/`PluginCapability`/`PluginRuntimeSpec` |
| Plugin runtimes | `src/plugin/runtime/process.rs`, `runtime/wasm.rs`, `runtime/wasm_cache.rs`, `runtime/builtin.rs`, `loader.rs`, `src/plugin/builtin/` | Process commands, sandboxed WASM (`plugins` feature; `WasmModuleCache` mtime-keyed), `BuiltinRuntime`/`BuiltinHandlerRegistry`, native auth hook handlers (copilot, gitlab, codex, poe) |
| Plugin policy | `src/plugin/policy.rs`, `permission.rs`, `lifecycle.rs`, `activation.rs` | Composite `PluginPolicy`, `check_*_allowed`, typed lifecycle I/O, durable global/workspace activation |
| Plugin UX | `src/plugin/management.rs`, `management_ui.rs`, `marketplace.rs`, `install.rs`, `hooks.rs`, `event_bus.rs`, `api.rs` | Manager/doctor views, marketplace, path-validated install/uninstall, hook types, `PluginEventBus`, `API_VERSION` |

## Hard Rules

1. **New web-search providers belong in `eggsearch`, not `src/search/`.**
   `src/search/` is the legacy fallback; production evidence wrappers
   require the eggsearch MCP backend or omit with a diagnostic.
2. **WASM needs the `plugins` feature.** Process and built-in paths are
   always available; never gate them behind the feature flag.
3. **Install paths are validated.** `install_from_path`/`uninstall` enforce
   containment; skill-style `allowed-tools`-equivalent metadata never
   grants permissions.
4. **MCP OAuth is server-scoped lifecycle, not a single secret.** Reuses
   only the canonical master key + crypto from provider-auth; retains a
   decrypt-only `CODEGG_ENC_v1` reader (`src/mcp/auth.rs:22`).
5. **Plugin UI goes through `UiNode`/`UiEffect`.** Management renderers in
   `management_ui.rs` stay within `crates/codegg-protocol` wire limits.
6. **The MCP config key is the server name.** `Config.mcp` is
   `Option<HashMap<String, McpEntry>>`
   (`crates/codegg-config/src/schema.rs:245`), so the TOML is
   `[mcp.<name>]` / JSON `"mcp": { "<name>": {...} }`. There is no
   `mcp.servers` level — nesting one registers a phantom all-`None` entry
   named `servers` instead of the intended servers, and does not fail
   loudly.
7. **The plugin manifest filename is `manifest.toml`.** `loader.rs:61`
   joins `plugin_dir.join("manifest.toml")`; there is no `plugin.toml`
   discovery fallback. Every example under `examples/plugins/` ships
   `manifest.toml`.

## Testing

```bash
cargo test -p codegg mcp::
cargo test -p codegg plugin::
cargo test --test mcp            # MCP client integration tests
cargo test --test mcp_reconnect  # reconnect path
cargo test --test fake_eggsearch_mcp
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests   # separate SDK
cargo test -p codegg --features plugins  # WASM-gated paths (via verify full)
./scripts/validate_plugin_ui.sh
```

Docs: `docs/MCP.md` and `docs/PLUGINS.md` are user integration notes;
`architecture/` is authoritative.

## See Also

- `architecture/mcp.md`, `architecture/plugin.md`, `architecture/search_backend.md`
- `.skills/provider-auth/SKILL.md` — credential store vs MCP TokenSet
- `.skills/agent/SKILL.md` — MCP/tool batch boundary in the loop

## Source verification

Verified 2026-10-06 against `architecture/mcp.md`, `architecture/plugin.md`,
`src/mcp/mod.rs`, `src/mcp/auth.rs`, `src/mcp/cli.rs`, `src/mcp/ide_server.rs`,
`src/mcp/local.rs`, `src/mcp/remote.rs`, `src/plugin/mod.rs`,
`src/plugin/service.rs`, `src/plugin/registry.rs`, `src/plugin/manifest.rs`,
`src/plugin/policy.rs`, `src/plugin/permission.rs`, `src/plugin/install.rs`,
`src/plugin/api.rs`, `src/plugin/event_bus.rs`, `src/plugin/loader.rs`,
`src/plugin/runtime/wasm.rs`, `src/plugin/runtime/wasm_cache.rs`,
`src/plugin/runtime/builtin.rs`, `src/plugin/builtin/mod.rs`,
`crates/codegg-protocol/src/ui.rs`, `examples/plugins/sdk-python`, and
`tests/`. Corrected two module attributions in the Plugin runtimes row
(`WasmModuleCache` lives in `runtime/wasm_cache.rs`, and the native auth
hook handlers for copilot/gitlab/codex/poe live in `src/plugin/builtin/`,
not `runtime/builtin.rs`, which holds `BuiltinRuntime`/
`BuiltinHandlerRegistry`), and removed the `cargo test --test
mcp_no_hanging_promises` command, which no longer exists in `tests/`
(`tests/mcp.rs` and `tests/mcp_reconnect.rs` do). Claims without a
traceable source were removed rather than guessed.
