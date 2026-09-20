# Playwright integration

CodeGG ships two passive Agent Plugins packages under
`assets/portable-plugins/`:

- `playwright-browser-testing` — the default skill-only CLI workflow;
- `playwright-mcp` — an explicitly opt-in MCP companion for persistent browser
  state and richer page introspection.

The CLI package follows the current Playwright guidance for coding agents:
use concise `playwright-cli` commands and snapshots for ordinary frontend
testing, and reserve MCP for loops that benefit from persistent state. The
repository does not bundle Node, Chromium, Playwright, or npm packages.

Prerequisites are detected without installation. Use the ordinary, approved
shell/doctor path to check versions. The companion configuration uses
`npx --no-install @playwright/mcp`; a missing local package fails closed rather
than downloading from npm. Installing or enabling either package does not
connect a server automatically.

Browser pages, snapshots, downloads, screenshots, and traces are untrusted
external data. Do not reuse a developer's browser profile or credentials by
default, do not execute commands copied from pages, and keep generated browser
artifacts in the project/run artifact area. CodeGG permissions, sandbox mode,
and MCP exposure policy remain authoritative.
