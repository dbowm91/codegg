# codegg

CodeGG is a Rust-native AI coding agent for terminal workflows: an interactive
TUI, persistent sessions, multiple LLM providers, and code/shell tooling behind
a permission gate. It is at `0.1.0` and under active development.

## Quick start

You need one LLM provider credential and network access to it.

### 1. Install

```bash
git clone https://github.com/dbowm91/codegg.git
cd codegg
cargo install --locked --path .
```

No GitHub release has been published yet, so there is no prebuilt installer.
`cargo install` also installs `codegg-sandbox-helper`. The managed
`codegg-eggsearch` sidecar (web/repository search) is not built from this
repo; `codegg doctor` reports whether it is present and how to point CodeGG
at one. Other paths are in [`docs/install.md`](docs/install.md).

### 2. Add a provider key

```bash
export ANTHROPIC_API_KEY='...'
codegg providers   # what your config can actually reach
```

`codegg providers` and `codegg models -p anthropic` read your live
configuration — entries appear only when their credential resolves. Full
credential options are in [`docs/providers.md`](docs/providers.md).

### 3. Pick a model

With no model configured, CodeGG falls back to `openai/gpt-4o`, so set one:

```jsonc
{
  "model": "anthropic/claude-sonnet-4-20250514"
}
```

or pass `-m provider/model-id` per invocation. Validate with `codegg
validate` (`codegg validate --config codegg.example.jsonc` checks the
annotated example). Config file locations and the full schema are in
[`docs/configuration.md`](docs/configuration.md).

### 4. Check the setup

```bash
codegg doctor   # what is wired up and what is missing (exit 0 even with problems)
```

### 5. Run it

```bash
codegg                                # new session in the current directory
codegg -c                             # resume the last session
codegg -m anthropic/<model-id>        # pick a model
codegg --cwd /path/to/project         # choose the workspace (must exist)
codegg --run "Explain this project"   # one prompt, then exit
```

`/connect` in the TUI walks you through provider onboarding on a clean
install. Every flag is in [`docs/cli.md`](docs/cli.md).

## Example usage

One-shot and CI (`codegg exec` reads `{"prompt","model","agent"}` from
`--json`, `--file`, or stdin; a finished run prints one JSON object and exits
non-zero on failure — startup failures such as an unknown provider report a
plain error on stderr instead):

```bash
printf '%s\n' '{"prompt":"Summarize the changes in this repository","model":"anthropic/claude-sonnet-4-20250514"}' \
  | codegg exec --format json --quiet --approval-mode automatic --sandbox workspace-write
```

Sessions and diagnosis:

```bash
codegg sessions                     # list sessions
codegg export <session-id> -o s.json
codegg doctor search                # one subsystem
codegg auth status                  # stored credentials
codegg mcp list                     # configured MCP servers
```

In the TUI, `space help` lists every command (`/models`, `/agent`,
`/approval`, `/sandbox`, `/plan`, `/skills`, `/doctor`, …). The full command
and keybinding reference is in [`docs/tui.md`](docs/tui.md).

## Configuration

JSON with JSONC comments, discovered from `$CODEGG_TUI_CONFIG`, then system,
global, and project locations (walked upward from the working directory):

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

Several config keys are untagged enums whose wrong shape silently discards
your whole file — see [`docs/configuration.md`](docs/configuration.md).

## Documentation

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
| [docs/security-semantics.md](docs/security-semantics.md) | Command classification and escalation. |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Symptom-driven fixes. |
| [docs/execution-ownership.md](docs/execution-ownership.md) | Which component owns process spawning. |
| [docs/dependency-maintenance.md](docs/dependency-maintenance.md) | Dependency and lint policy. |
| [docs/windows-acl-leases.md](docs/windows-acl-leases.md) | Windows sandbox ACL lease semantics. |
| [architecture/overview.md](architecture/overview.md) | Architecture index and implementation contracts. |
| [CHANGELOG.md](CHANGELOG.md) | Project changes. |

## Safety

CodeGG can read and modify files, run processes, operate Git, and contact
configured services. Treat its permission configuration as part of your
security boundary: `/approval` (`interactive`, `automatic`, `yolo`) and
`/sandbox` (`read-only`, `workspace-write`, `full-host`) are visible in the
status bar. Details in [`docs/security-semantics.md`](docs/security-semantics.md)
and [`docs/tui.md`](docs/tui.md).

## Development

```bash
scripts/verify.sh quick    # fmt, guard scripts, workspace check
scripts/verify.sh full     # the above plus clippy and the test suite
cargo fmt
```

Contributor conventions are in [`AGENTS.md`](AGENTS.md).

## License

MIT
