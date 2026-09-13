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
| Plugin runtimes | `src/plugin/runtime/process.rs`, `runtime/wasm.rs`, `runtime/builtin.rs`, `loader.rs` | Process commands, sandboxed WASM (`plugins` feature; `WasmModuleCache` mtime-keyed), native auth hooks (copilot, gitlab, codex, poe) |
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
   decrypt-only `CODEGG_ENC_v1` reader.
5. **Plugin UI goes through `UiNode`/`UiEffect`.** Management renderers in
   `management_ui.rs` stay within `crates/codegg-protocol` wire limits.

## Testing

```bash
cargo test -p codegg mcp::
cargo test -p codegg plugin::
cargo test --test mcp_no_hanging_promises 2>/dev/null || true
python3 -m unittest discover  # examples/plugins/sdk-python (separate)
cargo test -p codegg --features plugins  # WASM-gated paths (via verify full)
```

Docs: `docs/MCP.md` and `docs/PLUGINS.md` are user integration notes;
`architecture/` is authoritative.

## See Also

- `architecture/mcp.md`, `architecture/plugin.md`, `architecture/search_backend.md`
- `.skills/provider-auth/SKILL.md` — credential store vs MCP TokenSet
- `.skills/agent/SKILL.md` — MCP/tool batch boundary in the loop
