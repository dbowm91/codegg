# codegg

CodeGG is a Rust-native AI coding agent for terminal workflows: an interactive
TUI, persistent sessions, multiple LLM providers, code and shell tooling,
custom agents and skills, MCP and LSP integration, and a user-scoped daemon
that coordinates work across projects.

It is at `0.1.0` and under active development. This README covers getting
started; [`docs/`](docs/) has the detailed guides, and
[`architecture/`](architecture/) has the implementation contracts.

## Quick start

You need one LLM provider credential and network access to it. That is the
only runtime requirement — Rust, Git, and language servers are needed for
building from source and for Git/LSP features respectively, not for launching
the agent.

### 1. Install

No GitHub release has been published yet, so the prebuilt installer has
nothing to download. Install from source:

```bash
git clone https://github.com/dbowm91/codegg.git
cd codegg
cargo install --locked --path .
```

This installs `codegg` and its sandbox helper. It does **not** install the
pinned `codegg-eggsearch` sidecar, which powers web and repository search —
build it from eggsearch tag `v0.3.9` and place it next to the `codegg` binary,
or override it in config. `codegg doctor` tells you if it is missing.

Optional Cargo features: `server` (HTTP/WebSocket server), `plugins` (WASM
plugin runtime), `image` (terminal images).

```bash
cargo install --locked --path . --features server,plugins
```

Other installation paths, including the prebuilt installer once releases
exist, are in [`docs/install.md`](docs/install.md).

### 2. Configure a provider

The simplest path is an environment variable:

```bash
export ANTHROPIC_API_KEY='...'
```

To confirm what CodeGG can actually reach:

```bash
codegg providers              # providers your config can use
codegg models -p anthropic    # models that provider exposes
```

These read your real configuration, so the output depends on what you have
set up. Full credential options — including the encrypted store — are in
[`docs/providers.md`](docs/providers.md).

A credential on its own is not quite enough. With no model configured, CodeGG
falls back to `openai/gpt-4o`, so a run fails with `Provider not found: openai`
until you either set `"model"` in config (see **Configuration** below) or pass
`-m provider/model-id` on the command line.

### 3. Check the setup

```bash
codegg validate               # is the config valid?
codegg edit                   # open the config in $EDITOR to change it
codegg doctor                 # what is wired up and what is missing?
```

`doctor` reports the search backend, MCP servers, language servers,
deterministic tools, providers, stored credentials, and the installation
itself. It exits 0 even when it reports problems, so read the output.

### 4. Run it

```bash
codegg
```

In the TUI, `/connect` walks you through provider onboarding on a clean
install. Common startup forms:

```bash
codegg -c                             # resume the most recent session
codegg -s <session-id>                # open a specific session
codegg -m anthropic/<model-id>        # pick a model
codegg -a build                       # pick an agent
codegg --cwd /path/to/project         # choose the workspace
codegg -m anthropic/<model-id> --run "Explain this project"   # one prompt, then exit
```

`--cwd` must name an existing directory; it is equivalent to `cd`-ing there
first, so config discovery and project selection both resolve against it.

### 5. Non-interactive

```bash
codegg -m anthropic/claude-sonnet-4-20250514 --run "Summarize the changes in this repository"

printf '%s\n' '{"prompt":"Review this repository","model":"anthropic/claude-sonnet-4-20250514","agent":"build"}' \
  | codegg exec --format json --quiet
```

`codegg exec` is the CI-oriented entry point; set `--approval-mode` and
`--sandbox` explicitly for a bounded run. It prints a single JSON object with
`success`, `result`, `toolsUsed`, `tokensUsed`, `durationMs`, `error`, and
`code`, and exits non-zero on failure. The full CLI surface is in
[`docs/cli.md`](docs/cli.md).

## What you get

- **Terminal-first TUI** with persistent sessions, project tabs, model and
  agent selection, planning, goals, memory, and task tracking.
- **Native tools** for files, search, shell and Python execution, Git, LSP,
  deterministic text operations, testing, and security workflows.
- **Multiple providers** with model discovery, request shaping, retries,
  circuit breaking, and model profiles.
- **Custom agents and portable skills**, sharing skill locations with
  `.agents`, OpenCode, and Claude-style harnesses.
- **MCP clients and servers**, plus an ACP v1 stdio agent via `codegg acp`.
- **A single user-scoped daemon** by default, so process-consuming work is
  coordinated across projects instead of each TUI owning the machine.
- **Optional** HTTP/WebSocket server, remote attach, WASM plugins, and images
  behind Cargo features.

## Configuration

JSON with JSONC comments, discovered from `$CODEGG_TUI_CONFIG`, then system,
global, and project locations (walked upward from the working directory). A
minimal config:

```jsonc
{
  "model": "anthropic/claude-sonnet-4-20250514",
  "provider": {
    "anthropic": {
      "auth": { "type": "api_key", "env": "ANTHROPIC_API_KEY" }
    }
  }
}
```

A fully annotated example is in
[`codegg.example.jsonc`](codegg.example.jsonc) — validate it with
`codegg validate --config codegg.example.jsonc`.

Several config keys are untagged enums whose wrong shape silently discards
your whole file. The gotchas that matter, and the full key reference, are in
[`docs/configuration.md`](docs/configuration.md).

## Documentation

### Getting started

| Guide | Covers |
|---|---|
| [docs/install.md](docs/install.md) | Every installation path, upgrading, requirements. |
| [docs/cli.md](docs/cli.md) | Every command, flag, and hidden compatibility spelling. |
| [docs/configuration.md](docs/configuration.md) | File locations, schema, model routing. |
| [docs/providers.md](docs/providers.md) | Built-in providers, credentials, `/connect`. |
| [docs/daemon.md](docs/daemon.md) | The daemon, transports, scheduling. |
| [docs/tui.md](docs/tui.md) | Sessions, slash commands, approval and sandbox. |

### Features and integration

| Guide | Covers |
|---|---|
| [docs/agents-skills.md](docs/agents-skills.md) | Writing agents and `SKILL.md` skills. |
| [docs/repository-init.md](docs/repository-init.md) | Read-only repository evidence and `AGENTS.md` draft semantics. |
| [docs/tools.md](docs/tools.md) | Tool families, Git, deterministic tools, LSP. |
| [docs/LSP.md](docs/LSP.md) | Language-server configuration. |
| [docs/MCP.md](docs/MCP.md) | MCP servers, clients, transports. |
| [docs/PLUGINS.md](docs/PLUGINS.md) | Plugin manifests, capabilities, SDK examples. |
| [docs/themes.md](docs/themes.md) | Bundled themes and custom theme loading. |
| [docs/playwright.md](docs/playwright.md) | Playwright browser-testing plugins. |

### Reference

| Guide | Covers |
|---|---|
| [docs/security-semantics.md](docs/security-semantics.md) | Command classification and escalation. |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Symptom-driven fixes. |
| [docs/execution-ownership.md](docs/execution-ownership.md) | Which component owns process spawning. |
| [docs/dependency-maintenance.md](docs/dependency-maintenance.md) | Dependency and lint policy. |
| [architecture/overview.md](architecture/overview.md) | Architecture index (85 deep dives). |
| [CHANGELOG.md](CHANGELOG.md) | Project changes. |

## Safety

CodeGG is execution-capable: depending on your configuration and permissions
it can read and modify files, run processes, operate Git, contact configured
services, and invoke external tools. Treat its permission configuration as
part of your security boundary.

Two orthogonal controls are visible in the status bar and set with
`/approval` and `/sandbox`:

- **Approval** — `interactive` (escalations ask you), `automatic`
  (escalations go to a bounded read-only reviewer), or `yolo` (escalations
  auto-allow within the authority ceiling; explicit denies still deny).
- **Sandbox** — `read-only`, `workspace-write` (filesystem containment on
  supported hosts), or `full-host` (no CodeGG filesystem containment).

See [`docs/security-semantics.md`](docs/security-semantics.md) and
[`docs/tui.md`](docs/tui.md).

## Development

```bash
scripts/verify.sh quick    # fmt, guard scripts, workspace check
scripts/verify.sh full     # the above plus clippy and the test suite
cargo fmt
```

Contributor conventions — crate boundaries, feature gates, generated assets,
testing, and the module guides under `.opencode/skills/` — are in
[`AGENTS.md`](AGENTS.md).

## License

MIT
