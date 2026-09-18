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

- Credentials for at least one configured LLM provider, plus network
  connectivity to that provider, for hosted-provider operation.
- Source installation additionally needs Rust `1.89` or newer and Git to
  fetch the checkout (`cargo install --locked --path .`).

The supported prebuilt installation is self-contained for the normal
terminal workflow: launching `codegg`, connecting a provider, and using
the packaged core requires no separately installed Rust/Cargo,
eggsearch, eggsact, sandbox-helper package, Python runtime, or manual
`CODEGG_MASTER_KEY` / `CODEGG_ENCRYPTION_KEY` / `OPENCODE_ENCRYPTION_KEY`
environment variable. The installer places `codegg`,
`codegg-sandbox-helper`, and `codegg-eggsearch` (pinned 0.3.9) together;
CodeGG resolves the sidecars relative to its own executable.

Git is required only for Git-backed repository operations that need it;
language servers are required only for language-server features. Neither
is a prerequisite for launching CodeGG and connecting to a provider.

Downloading the installer itself needs an ordinary OS downloader/shell
(`curl`/`sh` as shown below); that is an installation-time requirement,
distinct from the runtime dependencies above.

Linux and macOS are the primary Unix runtime targets represented in the current daemon, sandbox, and path handling. Platform-specific behavior is documented in the architecture guides where relevant.

## Install

### Prebuilt installer (supported Linux/macOS hosts)

The installer downloads the release asset for your host from the canonical
CodeGG GitHub repository over HTTPS, verifies its SHA-256 checksum against
the release manifest, and atomically installs the managed runfile bundle
(`codegg`, `codegg-sandbox-helper`, `codegg-eggsearch`, plus
`THIRD-PARTY-NOTICES.txt` when the release carries it) into one
user-writable directory. It never uses privilege escalation, never edits
shell profiles, and never starts background services. You invoke only
`codegg`; the helpers are resolved by CodeGG relative to its own
executable, so only the install directory needs to be on `PATH`.

Supported hosts:

| OS | Architecture | Release target |
|---|---|---|
| Linux | x86_64 / amd64 | `x86_64-unknown-linux-gnu` |
| Linux | aarch64 / arm64 | `aarch64-unknown-linux-gnu` |
| macOS | x86_64 | `x86_64-apple-darwin` |
| macOS | arm64 / aarch64 | `aarch64-apple-darwin` |

Latest release (default installs to `~/.local/bin/codegg`):

> **Release availability:** installer support is implemented, but no GitHub
> releases have been published yet, so there is currently nothing for the
> installer to download. Until the first release is published, install from
> source below. Once releases exist, the installer commands apply as written.

```bash
curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh | sh
```

Pinned version (must match a published release tag):

```bash
curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh | CODEGG_VERSION=0.1.0 sh
```

Custom install directory:

```bash
curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh | CODEGG_INSTALL_DIR="$HOME/bin" sh
```

Pipe-to-shell executes remote code. To inspect before running, download first:

```bash
curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh -o install.sh
sh install.sh
```

After installation, ensure the destination directory is on `PATH` (the
installer prints guidance when it is not). Replacing the executable does not
restart a running daemon; the new binary takes effect on the next launch.
The installer only works for releases that carry the documented prebuilt
asset set (see `RELEASING.md`); older releases without those assets are not
installer-compatible. Other hosts should install from source below.

### From source

> Note: installing from source is NOT the self-contained contract. `cargo
> install --path .` installs only `codegg` (plus `codegg-sandbox-helper`
> via `cargo build`); the pinned upstream eggsearch sidecar
> (`codegg-eggsearch`, currently 0.3.9 from
> `https://github.com/eggstack/eggsearch`) must be provided separately —
> build it from the pinned tag and place it next to `codegg`, or configure
> an explicit `[search.eggsearch].command` / `[mcp.eggsearch]` override.
> The supported self-contained installation is the prebuilt installer
> bundle above. See `RELEASING.md` for the pinning and provenance policy.

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

codegg is not currently published on crates.io, so `cargo install codegg`
does not resolve yet. crates.io publication remains a manual maintainer step
(see `RELEASING.md`); until the first version is published, use the
installer above or install from source.

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

CodeGG uses the external `eggsearch` MCP server for web, repository, security,
research, batch-fetch, and evidence-bundle tools by default. Prebuilt
installer bundles already include the pinned `codegg-eggsearch` sidecar, so
no separate eggsearch installation is needed; run `codegg doctor search` to
verify the sidecar, MCP tool coverage, and provider degradation. Source
installs must provide the pinned sidecar separately (see above). Raw
eggsearch MCP tools remain hidden by default; CodeGG wrappers keep model
output bounded and trust-framed.

## Daemon model

Normal `codegg` startup uses a single daemon for the current OS user. The TUI connects to an existing daemon when one is available and, by default, starts it automatically when it is not. The daemon owns durable runtime state and coordinates process-consuming work across registered workspaces.

You normally do not need to manage it manually. When needed:

```bash
codegg daemon status
codegg daemon logs
codegg daemon stop
codegg daemon start
codegg daemon attach --new
```

`codegg daemon attach` connects the TUI to the running daemon over the
local socket (the old top-level `attach-daemon` spelling still parses as
a hidden compatibility alias). It is distinct from the feature-gated
remote HTTP `codegg attach`, which only exists in `server` builds.

`--standalone` runs an in-process core without the singleton daemon. `--stdio` runs the compatibility stdio core transport. These modes are useful for development, diagnostics, and integrations, but they do not provide the daemon's machine-wide scheduling behavior. The deprecated `--core-transport`, `--stdio`, `--core-endpoint`, and `attach-daemon` spellings still parse for compatibility but are hidden from normal `--help`; see [`architecture/core.md`](architecture/core.md) for the transport model.

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

Semantic model routing is optional. Configure an exact `virtual:<name>` model
under `model_routers` to run a bounded selector and resolve one configured
concrete route; concrete model selections bypass it. Codegg first keeps the
provider connection selected for the turn/session, then applies only a
compatible concrete model through that connection. A direct connection cannot
be migrated to another provider by a route. If the selected connection is
EggPool, EggPool remains responsible for account/provider routing behind its
endpoint. Selector failure or invalid output uses the configured default route
when compatible, and caller cancellation is propagated.

Codegg currently performs semantic selection per turn and does not maintain an
EggPool-style sticky affinity cache. The shared `sticky` and `affinity_ttl_s`
fields are retained for policy/fingerprint compatibility; they do not pin a
Codegg session to a route. Semantic model routing is model selection, not
provider failover.

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
printf '%s' "$OPENAI_API_KEY" | codegg auth set-key openai
codegg auth status
codegg auth logout openai
```

`codegg auth status` does not print stored secrets. On a fresh profile, the first protected credential write automatically creates and persists a CodeGG-managed master key. `CODEGG_MASTER_KEY` and the supported compatibility encryption-key variables remain optional higher-precedence overrides for deployments that manage key material externally. If existing ciphertext was created with an external key and that key is later unavailable, CodeGG fails closed rather than replacing it. Some typed authentication modes exist in the configuration schema but are not yet runtime-complete; API-key and stored-key flows are the documented production paths today.

See [`architecture/provider.md`](architecture/provider.md) and [`architecture/auth.md`](architecture/auth.md).

### First-run provider onboarding (`/connect`)

A clean install needs no preconfigured encryption environment variable.
Launch `codegg`, run `/connect`, choose a provider, and paste the required
credential (plus an endpoint only when that provider needs one). The first
protected write bootstraps the managed credential key automatically.

Eggpool is one optional upstream choice in that list — a local/shared
OpenAI-compatible proxy with its own host, default port `11300`, and TLS
policy — not a requirement for any other provider. `/connect` adds and
configures connections; `/connections` inspects, manages, and selects the
existing durable connections for the session.

## TUI and sessions

Launching `codegg` without a subcommand opens the TUI. Sessions are persistent by default and can be resumed, inspected, exported, and imported from the CLI:

```bash
codegg sessions
codegg session <session-id>
codegg export <session-id> -o session.json
codegg import session.json
```

Inside the TUI, the command system exposes session/context controls, agent selection, planning and goals, tasks, testing, diffs, search, LSP controls, MCP/plugin management, themes, keybindings, and diagnostics. Use the in-app help/command discovery rather than treating README keybindings as a fixed API; the TUI command registry is the authoritative surface.

The TUI supports multiple open project tabs with a project picker; execution is always scoped to the active tab's workspace without changing the process working directory. The keyboard-focusable sidebar includes an agent-run tree inspector for nested run inspection. See [`architecture/tui.md`](architecture/tui.md) for internals.

Human shell commands have an explicit context boundary: `!command` runs locally without promoting its output into model context, while `!!command` deliberately promotes the bounded/redacted result. See [`architecture/human_shell.md`](architecture/human_shell.md).

### Approval modes and sandbox profiles

Two orthogonal dimensions control autonomy and containment, visible in the status bar and via `/policy`:

- **Approval:** `interactive` (escalations ask you), `automatic` (escalations go to a bounded read-only reviewer; defers to you when no reviewer model is configured — never silent Yolo), `yolo` (escalations auto-allow within the authority ceiling; explicit denies still deny).
- **Sandbox:** `read-only`, `workspace-write` (filesystem containment on supported hosts), `full-host` (no CodeGG filesystem containment; the process has your OS user's host authority).

Select them with `/approval [interactive|automatic|yolo]` and `/sandbox [read-only|workspace-write|full-host]`. `Yolo` inside containment needs one confirmation; any `full-host` selection needs an explicit confirmation, and `yolo`+`full-host` needs two. The same contract is available headlessly (`--approval-mode`, `--sandbox`, `--yolo`). The last selection is restored on startup with any ceiling narrowing reported. See [`architecture/permission.md`](architecture/permission.md) and [`architecture/security.md`](architecture/security.md).

## Tools and coding workflow

The native tool registry includes file reads/edits, glob/grep search, patching, shell and Python execution, Git operations, testing, LSP-backed code intelligence, deterministic tools from `eggsact`, and higher-level review/security workflows.

Git support includes typed status/diff/log/show/blame reads and guarded mutation flows for common repository operations. LSP mutation workflows use preview/revalidation semantics rather than blindly applying stale edits. Process-heavy work submitted through the normal daemon path is admitted by the shared scheduler.

See [`architecture/git.md`](architecture/git.md), [`architecture/lsp.md`](architecture/lsp.md), and [`architecture/testing.md`](architecture/testing.md).

Detailed LSP integration notes are in [`docs/LSP.md`](docs/LSP.md).

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

`exec` accepts the same policy contract (`--approval-mode`, `--sandbox`, `--yolo`); without flags it keeps its legacy permissive behavior for existing CI. Output selection is `--format text|json` on both `--run` and `exec`; the older `--output-format` (one-shot) and `--json-output`/`-j` (`exec`) spellings remain as compatibility aliases.

## Safety model

codegg is an execution-capable coding agent: depending on configuration and permissions, it can read and modify files, run processes, operate Git, contact configured services, and invoke external tools. Treat its permission configuration as part of your security boundary.

The implementation includes path validation, permission checks, command preflight, SSRF protections, bounded/redacted tool output, conservative plugin handling, Linux Landlock integration, and preview/revalidation for mutating LSP operations. These controls reduce risk; they are not a reason to grant the agent access to secrets or destructive environments it does not need.

See [`architecture/security.md`](architecture/security.md), [`architecture/permission.md`](architecture/permission.md), and [`architecture/preflight.md`](architecture/preflight.md).

The security signal pipeline (command classification, escalation modes, security tool actions) is documented in [`docs/security-semantics.md`](docs/security-semantics.md).

## Diagnostics and troubleshooting

Validate configuration and inspect runtime integrations with:

```bash
codegg validate
codegg doctor
codegg doctor search
codegg daemon status
```

`doctor` accepts an optional positional subsystem (`all`, `search`,
`mcp`, `lsp`, `deterministic-tools`, `providers`, `credentials`,
`installation`) and defaults to all. The older
`doctor --subsystem <name>` spelling still parses as a deprecated
compatibility alias. `doctor installation` reports the installation-owned
runfiles and helper/sidecar state; only subsystems with real diagnostic
implementations are advertised. Run `codegg doctor --help` for the current
choices. Troubleshooting notes are in [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md).

## Development

The repository intentionally keeps routine verification relatively small:

```bash
scripts/verify.sh quick
scripts/verify.sh full
cargo fmt
```

For focused tests, prefer the crate or subsystem you changed rather than running unrelated verification by default. Contributor architecture, crate boundaries, generated assets, feature gates, and testing conventions are documented in [`AGENTS.md`](AGENTS.md).

Dependency policy, execution ownership inventory, and cross-platform notes are in [`docs/dependency-maintenance.md`](docs/dependency-maintenance.md) and [`docs/execution-ownership.md`](docs/execution-ownership.md).

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
