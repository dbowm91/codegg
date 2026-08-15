# codegg

codegg is a Rust-native AI coding agent for terminal workflows. It combines an interactive TUI, persistent sessions, multiple LLM providers, code and shell tooling, custom agents and skills, MCP integration, LSP support, and a user-scoped daemon that can coordinate work across projects.

The project is currently at `0.1.0` and under active development. The README describes the user-facing behavior implemented on `main`; deeper implementation notes live under [`architecture/`](architecture/).

## Highlights

- Terminal-first Ratatui interface with persistent sessions, model and agent selection, context/usage views, planning, goals, memory, task tracking, and background work.
- Native tools for files, search, shell/Python execution, Git, LSP, deterministic text operations, testing, and security-oriented workflows.
- Multiple LLM backends with model discovery, provider-specific request handling, retries, circuit breaking, and model profiles.
- Custom agents and portable skills, including compatible project/global skill locations used by CodeGG, `.agents`, OpenCode, and Claude-style harnesses.
- MCP clients and servers, plus an ACP v1 stdio frontend via `codegg acp`.
- A single user-scoped daemon by default, so sessions and process-consuming work can be coordinated rather than each TUI instance independently owning the machine.
- Optional HTTP/WebSocket server, remote attach support, WASM plugins, and image support behind Cargo feature flags.

## Requirements

- Rust `1.81` or newer.
- Git for source installation and Git-backed agent operations.
- Credentials for at least one configured LLM provider.
- Any external programs required by integrations you enable, such as language servers or local MCP servers.

Linux and macOS are the primary Unix runtime targets represented in the current daemon, sandbox, and path handling. Platform-specific behavior is documented in the architecture guides where relevant.

## Install

codegg is currently installed from source:

```bash
git clone https://github.com/dbowm91/codegg.git
cd codegg
cargo install --locked --path .
```

To run directly from a checkout instead:

```bash
cargo run -- --help
cargo run --
```

The default build includes the TUI and clipboard support. Additional Cargo features include:

- `server` — HTTP/WebSocket server and remote attach client.
- `plugins` — WASM plugin runtime.
- `image` — terminal image support.

For example:

```bash
cargo install --locked --path . --features server,plugins
```

## Quick start

Set a provider credential, inspect the models that provider exposes, and start the TUI:

```bash
export ANTHROPIC_API_KEY='...'

codegg providers
codegg models -p anthropic
codegg -m anthropic/<model-id>
```

Once a default model is configured, ordinary use is simply:

```bash
codegg
```

Useful startup forms include:

```bash
codegg -c                              # resume the most recent session
codegg -s <session-id>                 # open a specific session
codegg -m <provider>/<model-id>        # override the model
codegg -a <agent>                      # override the agent
codegg --cwd /path/to/project          # choose the workspace
codegg --run "Explain this project"   # run one prompt and exit
```

Use `codegg --help` for the complete CLI surface.

## Daemon model

Normal `codegg` startup uses a single daemon for the current OS user. The TUI connects to an existing daemon when one is available and, by default, starts it automatically when it is not. The daemon owns durable runtime state and coordinates process-consuming work across registered workspaces.

You normally do not need to manage it manually. When needed:

```bash
codegg daemon status
codegg daemon logs
codegg daemon stop
codegg daemon start
```

`--standalone` runs an in-process core without the singleton daemon. `--stdio` runs the compatibility stdio core transport. These modes are useful for development, diagnostics, and integrations, but they do not provide the daemon's machine-wide scheduling behavior.

See [`architecture/core.md`](architecture/core.md), [`architecture/scheduler.md`](architecture/scheduler.md), and [`architecture/client.md`](architecture/client.md) for the runtime model.

## Configuration

Configuration is JSON/JSON5; JSONC comments are supported. The full example is [`codegg.example.jsonc`](codegg.example.jsonc).

Project configuration is discovered upward from the working directory at locations such as:

```text
.codegg/codegg.jsonc
.codegg/codegg.json
codegg/codegg.jsonc
codegg/codegg.json
```

Global configuration lives under the platform configuration directory in `codegg/codegg.jsonc` (for example `~/.config/codegg/codegg.jsonc` on a typical Linux system). `CODEGG_TUI_CONFIG` can also point at an explicit configuration file. System configuration is supported as well.

A minimal provider configuration looks like:

```jsonc
{
  "model": "openai/<model-id>",
  "provider": {
    "openai": {
      "auth": {
        "type": "api_key",
        "env": "OPENAI_API_KEY"
      }
    }
  }
}
```

Configuration can additionally control agents, model profiles, permissions, compaction/context policy, tools, formatters, LSP servers, MCP servers, skills, plugins, keybindings, notifications, daemon behavior, and other runtime options. See [`architecture/config.md`](architecture/config.md).

## Providers and credentials

The current built-in registration path supports Anthropic, OpenAI, Google, OpenRouter, OpenCode Zen, Mistral, Groq, DeepInfra, Cerebras, Cohere, Together, Perplexity, xAI, Venice, MiniMax, OpenCode Go, and General Compute when the corresponding credentials/configuration are present.

Run these commands against your configuration rather than relying on a static model list:

```bash
codegg providers
codegg models
codegg models -p openai
```

Environment-backed API keys are the simplest authentication path. Provider configuration can also reference the encrypted user credential store:

```bash
export CODEGG_MASTER_KEY='...'
printf '%s' "$OPENAI_API_KEY" | codegg auth set-key openai
codegg auth status
codegg auth logout openai
```

`codegg auth status` does not print stored secrets. Stored keys require `CODEGG_MASTER_KEY` (or a supported compatibility encryption-key variable). Some typed authentication modes exist in the configuration schema but are not yet runtime-complete; API-key and stored-key flows are the documented production paths today.

See [`architecture/provider.md`](architecture/provider.md) and [`architecture/auth.md`](architecture/auth.md).

## TUI and sessions

Launching `codegg` without a subcommand opens the TUI. Sessions are persistent by default and can be resumed, inspected, exported, and imported from the CLI:

```bash
codegg sessions
codegg session <session-id>
codegg export <session-id> -o session.json
codegg import session.json
```

Inside the TUI, the command system exposes session/context controls, agent selection, planning and goals, tasks, testing, diffs, search, LSP controls, MCP/plugin management, themes, keybindings, and diagnostics. Use the in-app help/command discovery rather than treating README keybindings as a fixed API; the TUI command registry is the authoritative surface.

Human shell commands have an explicit context boundary: `!command` runs locally without promoting its output into model context, while `!!command` deliberately promotes the bounded/redacted result. See [`architecture/human_shell.md`](architecture/human_shell.md).

## Tools and coding workflow

The native tool registry includes file reads/edits, glob/grep search, patching, shell and Python execution, Git operations, testing, LSP-backed code intelligence, deterministic tools from `eggsact`, and higher-level review/security workflows.

Git support includes typed status/diff/log/show/blame reads and guarded mutation flows for common repository operations. LSP mutation workflows use preview/revalidation semantics rather than blindly applying stale edits. Process-heavy work submitted through the normal daemon path is admitted by the shared scheduler.

See [`architecture/git.md`](architecture/git.md), [`architecture/lsp.md`](architecture/lsp.md), and [`architecture/testing.md`](architecture/testing.md).

## Agents and skills

codegg ships built-in agents and supports project/global custom agents. Agent definitions can extend or override built-ins while remaining subject to the runtime permission/safety envelope. Examples live in [`examples/agents/`](examples/agents/).

Portable skills use `SKILL.md` packages. Project skill discovery currently understands:

```text
.codegg/skills/<name>/SKILL.md
.agents/skills/<name>/SKILL.md
.opencode/skills/<name>/SKILL.md
.claude/skills/<name>/SKILL.md
```

Equivalent global locations are also supported. Skill discovery is bounded and containment-checked; metadata such as `allowed-tools` does not itself grant permissions.

See [`architecture/agent.md`](architecture/agent.md) and [`architecture/skills.md`](architecture/skills.md).

## MCP and ACP

MCP servers can be configured under the `mcp` configuration key and managed from the CLI:

```bash
codegg mcp --help
```

The TUI also exposes MCP discovery/management commands. See [`docs/MCP.md`](docs/MCP.md).

For editor or harness integration, codegg exposes an ACP v1 agent over newline-delimited JSON-RPC on stdio:

```bash
codegg acp
```

## Plugins

Plugins can contribute commands, hooks, panels, status widgets, and other UI/runtime behavior. Process and built-in plugin paths are part of the normal runtime; WASM execution requires the `plugins` Cargo feature.

Plugin documentation and examples are in [`docs/PLUGINS.md`](docs/PLUGINS.md) and [`examples/plugins/`](examples/plugins/).

## Optional server/remote frontend

Building with the `server` feature exposes `codegg server` and `codegg attach`:

```bash
cargo build --release --features server
./target/release/codegg server --standalone-core --host 127.0.0.1 --port 3000
```

The server frontend remains an optional path separate from the normal local daemon/TUI workflow. Consult [`architecture/server.md`](architecture/server.md) before exposing it beyond localhost, including its authentication and transport constraints.

## Non-interactive use

For a simple one-shot prompt:

```bash
codegg --run "Summarize the changes in this repository"
```

For structured automation/CI input, use `exec`:

```bash
printf '%s\n' '{"prompt":"Review this repository","model":"openai/<model-id>","agent":"build"}' \
  | codegg exec --json-output --quiet
```

Other useful CLI entry points include `research`, `doctor`, `validate`, `completions`, `upgrade`, and the session import/export commands. Run the relevant `--help` before scripting against a subcommand.

## Safety model

codegg is an execution-capable coding agent: depending on configuration and permissions, it can read and modify files, run processes, operate Git, contact configured services, and invoke external tools. Treat its permission configuration as part of your security boundary.

The implementation includes path validation, permission checks, command preflight, SSRF protections, bounded/redacted tool output, conservative plugin handling, Linux Landlock integration, and preview/revalidation for mutating LSP operations. These controls reduce risk; they are not a reason to grant the agent access to secrets or destructive environments it does not need.

See [`architecture/security.md`](architecture/security.md), [`architecture/permission.md`](architecture/permission.md), and [`architecture/preflight.md`](architecture/preflight.md).

## Diagnostics and troubleshooting

Validate configuration and inspect runtime integrations with:

```bash
codegg validate
codegg doctor
codegg daemon status
```

`doctor` can focus on supported diagnostic subsystems; run `codegg doctor --help` for the current choices. Troubleshooting notes are in [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md).

## Development

The repository intentionally keeps routine verification relatively small:

```bash
scripts/verify.sh quick
scripts/verify.sh full
cargo fmt
```

For focused tests, prefer the crate or subsystem you changed rather than running unrelated verification by default. Contributor architecture, crate boundaries, generated assets, feature gates, and testing conventions are documented in [`AGENTS.md`](AGENTS.md).

Useful starting points:

- [`architecture/overview.md`](architecture/overview.md) — architecture index.
- [`architecture/core.md`](architecture/core.md) — core/daemon/workspace model.
- [`architecture/config.md`](architecture/config.md) — configuration.
- [`architecture/provider.md`](architecture/provider.md) — providers.
- [`architecture/tui.md`](architecture/tui.md) — TUI internals and command surface.
- [`architecture/agent.md`](architecture/agent.md) — agent orchestration.
- [`architecture/skills.md`](architecture/skills.md) — skills and discovery.
- [`CHANGELOG.md`](CHANGELOG.md) — project changes.

## License

MIT
