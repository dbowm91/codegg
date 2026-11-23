# Troubleshooting

## Daemon will not connect

Run `codegg daemon status` and `codegg daemon logs`. If no daemon is running, start one with `codegg daemon start` or launch `codegg`, which connects to or starts the local daemon.

Do not remove `daemon.lock` by hand. It is an operating-system lock held by the daemon process. Use `codegg daemon stop` to stop a running daemon.

For a custom endpoint, check the endpoint passed to the command and `CODEGG_CORE_ENDPOINT`. See [docs/daemon.md](daemon.md).

## HTTP server refuses to start

A build with the `server` feature requires `--standalone-core` for `codegg server`. The HTTP server does not proxy to the singleton daemon. The remote `codegg attach <url>` command uses the server URL; `codegg daemon attach` connects to the local daemon.

## Session does not appear

Check that the active project and workspace are the ones where the session was created. The CLI and TUI use different session storage paths; see [docs/cli.md](cli.md) and [docs/tui.md](tui.md) for the distinction. Do not delete session data while troubleshooting.

## Provider or model is unavailable

- Run `codegg providers` to see providers whose credentials resolved.
- Run `codegg models` or `codegg models -p <provider>` to inspect model discovery.
- Run `codegg auth status` to inspect stored account metadata without showing secrets.
- Check the provider's environment variable or config entry. See [docs/providers.md](providers.md).

Model discovery does not always verify a credential. A provider may return a public or local model catalog, so the first request can still report an authentication error.

## Configuration is ignored or rejected

Run `codegg validate --config <path>`. A misspelled key may be ignored, while a value with the wrong shape may prevent a config layer from loading. Check [docs/configuration.md](configuration.md) for file precedence and accepted shapes.

## Approval or sandbox problem

In the TUI, use `/doctor` to check daemon connectivity, `/approval` to inspect the approval mode, and `/sandbox` to inspect the sandbox profile. A configured deny remains final. If required filesystem containment is unavailable, the request is refused before the command or process starts. See [docs/security-semantics.md](security-semantics.md).

## Language server, MCP, or search is unavailable

- Run `codegg doctor lsp` and check the configured language-server command. The model-facing LSP tool is opt-in through `experimental.lsp_tool`. See [docs/LSP.md](LSP.md).
- Run `codegg doctor mcp` and check the server's config entry and command or URL. See [docs/MCP.md](MCP.md).
- Run `codegg doctor search` to check the Eggsearch backend and its sidecar. See [docs/install.md](install.md).

## Plugin does not load

Check the plugin manifest, module path, and required features. See [docs/PLUGINS.md](PLUGINS.md) for the supported package formats and setup.

## Getting help

When asking for help, include your operating system, the relevant `codegg doctor` output, and a redacted config excerpt. Never include credentials, master keys, or other secrets.

## Source verification

Checked 2026-11-23 against `crates/codegg-cli/src/main.rs`, `src/core/instance.rs`, `crates/codegg-core/src/storage/mod.rs`, `crates/codegg-config/src/schema.rs`, and `crates/codegg-tui/src/command.rs`.
