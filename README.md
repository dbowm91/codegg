# codegg

codegg is a pure-Rust AI coding agent with a terminal UI, persistent
sessions, configurable tools, and optional headless/server frontends.

This README is the short orientation. Detailed behavior and design notes live
in the [architecture docs](architecture/overview.md) and the linked guides.

## What it includes

- Multiple LLM providers, model discovery, model profiles, routing, retries,
  and circuit breaking.
- A Ratatui TUI with sessions, agents, subagents, plans, goals, memory,
  context/usage views, themes, keybindings, dialogs, completions, and
  responsive background work.
- File, search, git, shell, Python, LSP, security, deterministic, and MCP
  tools. The `eggsearch` integration provides search/evidence backends;
  `eggsact` provides in-process deterministic tools; and native `egggit`,
  `eggcontext`, `eggsentry`, `egglsp`, and `codegg-git` crates keep core
  functionality local and testable. The `git` tool supports typed reads
  (status, diff, log, show, blame) plus the full mutation surface
  (stage/commit/branch/stash/merge/rebase/cherry-pick/revert/fetch/pull/push/
  reset/clean) with operation-aware continue/abort/skip recovery for
  in-progress operations. See [`architecture/git.md`](architecture/git.md)
  and the post-closure
  [`architecture/git_polish_verification_handoff.md`](architecture/git_polish_verification_handoff.md)
  for the canonical subprocess policy, the rerun argv secret
  lifecycle, and the execution-origin matrix.
- Human shell commands with explicit promotion: `!command` runs locally and
  stays out of model context; `!!command` promotes the result. Output is
  retained, bounded, redacted, and projected through native or RTK-backed
  projectors when configured.
- Preview-first LSP operations, stale-preview protection, semantic workflows,
  diagnostics, restart supervision, and optional memory-only semantic cache.
- Process, WASM, and builtin plugins, portable plugin UI, plugin management,
  and reference SDKs under [`examples/plugins`](examples/plugins/).
- Persistent SQLite sessions, import/export, background tasks, run history,
  supervised test execution, context compaction, and security review.
- Optional HTTP/WebSocket server and remote TUI transport (`server` feature).

Capabilities are preserved behind explicit feature gates where noted; the
architecture and subsystem guides are the source of truth for details.

## Install and run

Rust 1.81 or newer is required.

### Singleton daemon

Codegg runs a single user-scoped daemon per OS user. The first `codegg`
invocation acquires an advisory lock and starts the daemon; subsequent
invocations connect to the running daemon automatically (connect-or-start).
Use `codegg daemon start|stop|status` to manage the daemon lifecycle.

Lock and metadata paths:

| OS | Default location |
|----|-----------------|
| macOS | `$HOME/Library/Application Support/codegg/daemon.lock` |
| Linux | `${XDG_RUNTIME_DIR:-/tmp}/codegg/daemon.lock` |

Override with `CODEGG_DAEMON_HOME`. Use `--standalone` to run an in-process
core without the daemon (visible non-production mode), or `--stdio` for the
hidden `core-stdio` compatibility path. The legacy `--core-transport inproc|stdio`
flags are deprecated and emit a warning.

### Workspaces and execution context

The daemon tracks every session as bound to exactly one registered
[`Workspace`](architecture/core.md#workspace-registry-and-execution-context-phase-2)
(a canonical project root). Workspace identity flows into every daemon-owned
execution path through an immutable
[`ExecutionContext`](architecture/core.md#workspace-registry-and-execution-context-phase-2)
— `TurnSubmit`, `AgentSelect`, and `ModelSelect` reject sessions that have not
been bound to a workspace. Tools, subagents, Bash, Python, Git, LSP, TestRunner,
and RunStore all anchor at `execution.workspace_root`, eliminating process-cwd
as an execution identity source. See `architecture/core.md` for the full contract.

### Workspace services and storage (Phase 3)

Each registered workspace has a daemon-owned
[`WorkspaceServices`](architecture/workspace_services.md) bundle that owns the
per-workspace `RunStore`, path policy, lock table, and resolved configuration
snapshot. Bundles are activated lazily by `WorkspaceServiceRegistry::acquire`
with single-flight activation, leased to consumers through RAII
`WorkspaceServicesLease` handles, and capped by `WorkspaceServicePolicy`
(`max_active_workspaces`, `idle_evict_after`). Phase F Git mutation flows and
the Bash-translation dispatcher contend on the same per-repository
`WorkspaceLockTable::acquire_repository` lock.

The daemon's authoritative SQLite catalog has moved to a user-scoped
location:

- macOS: `~/Library/Application Support/codegg/codegg.db`
- Linux: `$XDG_DATA_HOME/codegg/codegg.db`

`init_legacy_project_store(root)` retains backward compat for the legacy
`<root>/.codegg/sessions.db` path, and `migrate_legacy_project_database`
imports those legacy databases into the catalog idempotently. Storage
layout marker is now `STORAGE_LAYOUT_VERSION = 32`. See
`architecture/workspace_services.md` for the full contract.

### Scheduler-owned execution

In daemon mode, process-consuming work is submitted as a durable job and is
admitted by the single global scheduler before execution. Tests, structured
Bash test/build/lint/format routes, subagents, and scheduled work use this
path; workspace lanes, resource profiles, exclusivity keys, cancellation,
and daemon-generation recovery are visible through the scheduler snapshot
and job protocol.

The scheduler is enabled and mandatory by default. If it is explicitly
disabled, daemon submission returns `SchedulerDisabled` rather than falling
back to an unscheduled process. `--standalone` and `--stdio` remain explicit
compatibility modes and do not participate in the user-scoped machine-wide
admission guarantee. See
`architecture/scheduler.md`.

```bash
git clone https://github.com/anomalyco/codegg
cd codegg
cargo install --path .

codegg                         # start a new session (connect-or-start)
codegg -c                      # resume the most recent session
codegg -m anthropic/claude-sonnet-4-20250514
codegg --run "Explain this code"
codegg daemon status           # show daemon identity, PID, generation
codegg daemon stop             # stop the running daemon
```

For a source checkout without installing:

```bash
cargo run -- --run "Explain this code"
```

Set a provider credential before starting. For example:

```bash
export ANTHROPIC_API_KEY=...
codegg
```

Use `codegg --help` for the complete CLI surface. The main commands are
`providers`, `models`, `sessions`, `session`, `export`, `import`, `validate`,
`doctor`, `exec`, `research`, `mcp`, `auth`, `daemon`, and `completions`.
`server` and `attach` are available when the `server` feature is enabled.

## Configuration and credentials

Configuration is JSON/JSON5. The canonical example is
[`codegg.example.jsonc`](codegg.example.jsonc). Project configuration is
usually `.codegg/codegg.jsonc`; the global file is usually
`~/.config/codegg/codegg.jsonc`. `codegg.json`, `config.json`, `CODEGG_TUI_CONFIG`,
system configuration, and parent-directory project configuration are also
supported by the resolver.

Minimal provider configuration:

```json
{
  "model": "anthropic/claude-sonnet-4-20250514",
  "provider": {
    "anthropic": {
      "auth": { "type": "api_key", "env": "ANTHROPIC_API_KEY" }
    }
  }
}
```

Providers include Anthropic, OpenAI-compatible endpoints, Google/Vertex,
Azure, Bedrock, OpenRouter, Cloudflare, GitLab, xAI, Mistral, and other
configured integrations. Run `codegg providers` and `codegg models` to inspect
the providers and models available in the current configuration.

The typed auth system supports environment-backed API keys, stored API keys,
encrypted values, explicit no-auth, and recognized-but-not-yet-implemented
OAuth/device and external-command modes. Manage stored API keys without
printing secrets:

```bash
codegg auth status
printf '%s' "$OPENAI_API_KEY" | codegg auth set-key openai
printf '%s' "$OPENAI_API_KEY" | codegg auth set-key openai --account work
codegg auth logout openai
```

Storing credentials requires `CODEGG_MASTER_KEY` (or one of the supported
encryption-key aliases). See [authentication](architecture/auth.md) and
[configuration](architecture/config.md).

## TUI essentials

Press `?` for mode-aware help and `/` for the command palette. Common actions:

| Input | Action |
| --- | --- |
| `Enter` | Send the prompt |
| `Shift+Enter` | Insert a newline |
| `Esc` / `Ctrl+C` | Cancel or close the active surface |
| `Ctrl+L` / `Ctrl+N` | Select a model / start a new session |
| `Ctrl+T` / `Ctrl+W` | Toggle the sidebar / close the session |
| `PgUp` / `PgDown` | Scroll the transcript |
| `@` | Start file or agent completion |
| `!cmd` / `!!cmd` | Run a hidden / promoted human shell command |
| `Ctrl+Shift+F` | Toggle fullscreen |

Useful slash-command families include:

- Session and context: `/sessions`, `/new`, `/compact`, `/context`, `/cost`,
  `/usage`, `/timeline`, `/undo`, `/redo`, `/export`.
- Agents and work: `/agent`, `/agents`, `/plan`, `/goal`, `/memory`, `/tasks`,
  `/worktree`, `/research`.
- Engineering: `/test`, `/tests`, `/diff`, `/revert`, `/search`, `/doctor`.
- LSP: `/lsp-status`, `/lsp-servers`, `/lsp-doctor`, `/lsp-preview`,
  `/lsp-preview-apply`, `/lsp-restart`, `/lsp-stop`, and semantic workflows.
- Integration: `/mcps`, `/plugins`, `/plugin-info`, `/plugin-doctor`,
  `/themes`, `/keybinds`, `/tui`, `/tts`.

`/test custom ...` accepts only validated argv-prefix commands and never runs
through a shell. The complete command registry is discoverable in `/help` and
the [TUI guide](architecture/tui.md).

Vim mode is enabled with `"vim_mode": true`. Its normal-mode navigation uses
`j/k`, `g/G`, `i`, `:`, and `q`.

## Integrations

### MCP

Configure local or remote MCP servers under the `mcp` config key. Use
`/mcps` in the TUI or `codegg mcp --help` on the CLI. See
[MCP](docs/MCP.md).

### Skills and agents

Skills are loaded from `~/.config/codegg/skills/` and `.opencode/skills/` (legacy `.codegg/skills/` remains a compatibility path).
Agents can be defined in `~/.config/codegg/agents/` or `.codegg/agents/` as
TOML or Markdown. Use `/skills`, `/agent`, `/agents`, and `@agent-name`.
Examples are in [`examples/agents`](examples/agents/) and the built-in agent
catalog is documented in [`assets/agents`](assets/agents/).

### Plugins

Plugins may contribute commands, panels, status widgets, hooks, and UI. The
runtime supports process, WASM, and builtin implementations; WASM is enabled
with `--features plugins`. Use `/plugins`, `/plugin-install`,
`/plugin-enable`, `/plugin-disable`, `/plugin-doctor`, and
`/plugin-remove`. See [plugin documentation](docs/PLUGINS.md) and the SDK
examples.

### Server and transports

```bash
cargo build --features server
codegg server --host 127.0.0.1 --port 8080
codegg attach http://127.0.0.1:8080 --token TOKEN
```

The TUI connects to the user-scoped singleton daemon by default. Use
`--standalone` for an in-process core or `--stdio` for the compatibility
stdio path. The HTTP server requires `--standalone-core` to construct its
own core in Phase 1; daemon-proxying server mode is planned for a later
phase. See [server architecture](architecture/server.md) and
[client architecture](architecture/client.md).

## Safety and context

The permission system, path validation, SSRF protection, Landlock sandboxing,
security review workflow, command preflight, and conservative plugin policy
are part of the execution boundary. Mutating LSP operations use preview
artifacts and hash revalidation before apply. Read
[security](architecture/security.md), [permission](architecture/permission.md),
[preflight](architecture/preflight.md), and [LSP](docs/LSP.md) before changing
those boundaries.

Context compaction, model-aware context policy, shell-output projection, and
run artifacts are documented in [compaction](architecture/compaction.md),
[cache-aware context](architecture/cache-aware-context.md),
[human shell](architecture/human_shell.md), and
[run storage](architecture/run_store.md).

## Development

```bash
cargo fmt
cargo check --workspace
cargo test -p codegg-core
cargo test --test tui
cargo test --test tui_render
cargo test --workspace --all-features
```

For a resource-capped full run, use:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14
```

See [AGENTS.md](AGENTS.md) for crate boundaries, feature gates, test
selection, generated assets, and contribution conventions. The repository
layout is summarized in [architecture overview](architecture/overview.md).

## Documentation map

- [Architecture index](architecture/overview.md)
- [Core / daemon / workspaces](architecture/core.md)
- [Workspace services and storage (Phase 3)](architecture/workspace_services.md)
- [TUI](architecture/tui.md)
- [Configuration](architecture/config.md)
- [Providers](architecture/provider.md)
- [Search backends](architecture/search_backend.md)
- [Deterministic tools](architecture/deterministic_tools.md)
- [LSP](architecture/lsp.md)
- [Plugins](architecture/plugin.md)
- [Testing](architecture/testing.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md)
- [Changelog](CHANGELOG.md)

## License

MIT
