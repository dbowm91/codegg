# codegg

CodeGG is a Rust-native AI coding agent for terminal workflows: an interactive
TUI, persistent sessions, multiple LLM providers, code and shell tooling,
custom agents and skills, MCP and LSP integration, and a user-scoped daemon
that coordinates work across projects.

It is at `0.1.0` and under active development. This README is the quickstart.
[`docs/`](docs/) holds the detailed guides, [`architecture/`](architecture/)
holds the implementation contracts.

## Quick start

You need one LLM provider credential and network access to it. That is the only
runtime requirement — Rust, Git, and language servers are needed for building
from source and for Git/LSP features, not for launching the agent.

### 1. Install

No GitHub release has been published yet, so the prebuilt installer has nothing
to download. Install from source:

```bash
git clone https://github.com/dbowm91/codegg.git
cd codegg
cargo install --locked --path .
```

This installs `codegg` and its sandbox helper. It does **not** install the
pinned `codegg-eggsearch` sidecar that powers web and repository search — build
it from eggsearch `0.4.4` (commit `32f48bb57a43274d763c7f9ba919eab84d125edc`)
and place it next to the `codegg` binary, or override it in config.
`codegg doctor` reports if it is missing.

Optional Cargo features: `server` (HTTP/WebSocket server), `plugins` (WASM
plugin runtime), `image` (terminal images).

```bash
cargo install --locked --path . --features server,plugins
```

Other installation paths, including the prebuilt installer once releases exist,
are in [`docs/install.md`](docs/install.md).

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

These read your real configuration, so the output depends on what you have set
up. Full credential options — including the encrypted store — are in
[`docs/providers.md`](docs/providers.md).

A credential alone is not quite enough. With no model configured, CodeGG falls
back to `openai/gpt-4o`, so a run fails with `Provider not found: openai` until
you set `"model"` in config (see **Configuration** below) or pass
`-m provider/model-id` on the command line.

### 3. Check the setup

```bash
codegg validate               # is the config valid?
codegg edit                   # open the config in $EDITOR to change it
codegg doctor                 # what is wired up and what is missing?
```

`doctor` reports the search backend, MCP servers, language servers, deterministic
tools, providers, stored credentials, and the installation itself. It exits 0
even when it reports problems, so read the output. External search and fetch use
the managed Eggsearch sidecar as their sole provider execution owner; if it is
unavailable, CodeGG reports an actionable error rather than switching to another
provider implementation. Set `search.backend = "disabled"` to disable the
external search surface.

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
codegg --run "Explain this project"   # one prompt, then exit
```

`--cwd` must name an existing directory; it is equivalent to `cd`-ing there
first, so config discovery and project selection both resolve against it.

## Example usage

### One-shot and CI

`codegg exec` is the CI-oriented entry point. Set `--approval-mode` and
`--sandbox` explicitly for a bounded run. It prints a single JSON object with
`success`, `result`, `toolsUsed`, `tokensUsed`, `durationMs`, `error`, and
`code`, and exits non-zero on failure.

```bash
codegg -m anthropic/claude-sonnet-4-20250514 --run "Summarize the changes in this repository"

printf '%s\n' '{"prompt":"Review this repository","model":"anthropic/claude-sonnet-4-20250514","agent":"build"}' \
  | codegg exec --format json --quiet --approval-mode interactive --sandbox workspace-write
```

### Sessions and workspaces

```bash
codegg sessions                     # list sessions
codegg session <session-id>         # show one session
codegg export <session-id> -o s.json
codegg import s.json
codegg launch --project my-project   # start in an isolated managed checkout
```

### Diagnosis

```bash
codegg doctor search                # one subsystem
codegg doctor providers
codegg auth status                  # stored credentials
codegg mcp list                     # configured MCP servers
```

### In the TUI

`space help` lists every command. The common ones:

| Command | Purpose |
|---|---|
| `/connect`, `/connections` | Provider onboarding and credential setup. |
| `/models`, `/models-refresh` | Select or list models. |
| `/agent`, `/agents` | Switch or list the active agent. |
| `/approval`, `/sandbox`, `/policy` | Set the escalation and containment posture. |
| `/plan`, `/goal`, `/tasks` | Plan, goal, and todo/task tracking. |
| `/context`, `/compact` | Context ledger and compaction. |
| `/memory` | Stored memory and habits. |
| `/skills`, `/plugins`, `/mcps` | Inspect what is installed. |
| `/tool-backends`, `/search` | Tool backends and web search. |
| `/themes`, `/tui` | Colour theme and TUI preferences. |
| `/doctor`, `/lsp-status` | Diagnostics for the runtime and language servers. |

The full list, with keybindings and panel semantics, is in
[`docs/tui.md`](docs/tui.md).

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
`codegg validate --config codegg.example.jsonc`. Several config keys are
untagged enums whose wrong shape silently discards your whole file; the
gotchas that matter, and the full key reference, are in
[`docs/configuration.md`](docs/configuration.md).

## Documentation

### Guides

| Guide | Covers |
|---|---|
| [docs/install.md](docs/install.md) | Every installation path, upgrading, requirements. |
| [docs/cli.md](docs/cli.md) | Every command, flag, and hidden compatibility spelling. |
| [docs/configuration.md](docs/configuration.md) | File locations, schema, model routing. |
| [docs/providers.md](docs/providers.md) | Built-in providers, credentials, `/connect`. |
| [docs/daemon.md](docs/daemon.md) | The daemon, transports, scheduling. |
| [docs/tui.md](docs/tui.md) | Sessions, slash commands, approval and sandbox. |
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
| [architecture/overview.md](architecture/overview.md) | Architecture index — 92 documents: this overview, 90 component deep dives, and the count audit. |
| [CHANGELOG.md](CHANGELOG.md) | Project changes. |

## Safety

CodeGG is execution-capable: depending on your configuration and permissions it
can read and modify files, run processes, operate Git, contact configured
services, and invoke external tools. Treat its permission configuration as part
of your security boundary. Two orthogonal controls are visible in the status bar
and set with `/approval` and `/sandbox`:

- **Approval** — `interactive` (escalations ask you), `automatic` (escalations
  go to a bounded read-only reviewer), or `yolo` (escalations auto-allow within
  the authority ceiling; explicit denies still deny).
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
