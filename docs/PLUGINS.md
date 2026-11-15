# Plugins

Plugins add commands and hooks, or contribute passive content such as skills and MCP servers. Check a plugin's source and requested access before enabling it, especially if it can run local code.

## Install and manage plugins

Use these TUI slash commands:

| Command | What it does |
|---------|--------------|
| `/plugins` | List registered plugins and their activation state. |
| `/plugin-info <id-or-name>` | Review a plugin's runtime, capabilities, permissions, and diagnostics. |
| `/plugin-install <path>` | Install a plugin from a local directory. |
| `/plugin-enable <id-or-name>` | Set a global activation override to enabled. |
| `/plugin-disable <id-or-name>` | Set a global activation override to disabled. |
| `/plugin-doctor [id-or-name]` | Run read-only checks on plugin setup and health. |
| `/plugin-remove <id-or-name>` | Remove an installed plugin. Built-in plugins cannot be removed. |

An install path must name an existing directory containing one of these manifests:

- `manifest.toml` for a CodeGG plugin;
- `plugin.json` for an Agent Plugins package;
- `.claude-plugin/plugin.json` for a Claude-compatible package.

A path containing `..` is refused. Check `/plugins` after installation to see the effective activation state. The enable and disable commands save global overrides.

## What packages can do

A CodeGG `manifest.toml` can declare a runtime, capabilities, permissions, and passive contributions. Runtime types are:

- **WASM**: runs in a Wasmtime sandbox.
- **Process**: starts a local command. Process plugins are not sandboxed, so only use ones you trust.
- **Builtin**: a handler shipped with CodeGG.

Portable `plugin.json` packages are only partially supported. CodeGG can read their skills and an optional `mcp.json` server declaration. Other portable components are not implemented and may be ignored. Reading a package manifest does not run package scripts or connect to an MCP server. Once a plugin is active, its MCP declarations use CodeGG's normal MCP connection and policy handling. Review any declared command or URL before enabling it.

Contribution files are resolved relative to the installed package directory and checked for path escapes. Plugin-provided content and server output should still be treated as untrusted input.

## Examples and further guidance

The repository's `examples/plugins/` directory contains sample packages and SDKs. The detailed runtime and manifest reference is in `architecture/plugin.md`.

## Source verification

Verified 2026-11-15 against `src/plugin/package.rs`, `src/plugin/install.rs`, `src/plugin/manifest.rs`, `src/plugin/contributions.rs`, `src/plugin/management.rs`, `src/plugin/runtime/process.rs`, `src/plugin/runtime/wasm.rs`, `src/tui/command.rs`, and `src/tui/commands/plugin_management.rs`. Confirmed the accepted manifest names, local-path validation, available TUI management commands, passive package behavior, contribution path checks, runtime types, and that process plugins are not sandboxed.
