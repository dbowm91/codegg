# CLI reference

Every command and flag on this page was verified against the clap
definitions in `src/main.rs` and by running the binary. When the binary and
this page disagree, the binary is authoritative — check `codegg --help` and
`codegg <command> --help`.

Run `codegg --help` for the current top-level surface and
`codegg <command> --help` for any subcommand.

## Contents

- [Running without a subcommand](#running-without-a-subcommand)
- [Global flags](#global-flags)
- [Sessions](#sessions)
- [Providers and credentials](#providers-and-credentials)
- [Configuration and diagnostics](#configuration-and-diagnostics)
- [Daemon](#daemon)
- [Non-interactive use](#non-interactive-use)
- [MCP, ACP and plugins](#mcp-acp-and-plugins)
- [Research](#research)
- [Shell completions](#shell-completions)
- [Upgrade](#upgrade)
- [Hidden compatibility spellings](#hidden-compatibility-spellings)

## Running without a subcommand

With no subcommand, `codegg` opens the TUI. These startup forms are the
common ones:

```bash
codegg                                  # start a new session
codegg -c                               # resume the most recent session
codegg -s <session-id>                  # open a specific session
codegg --fork <session-id>              # fork a session
codegg --no-session                     # ephemeral run, no persistence
codegg -m anthropic/claude-sonnet-4-20250514   # pick a model
codegg -a build                         # pick an agent
codegg --cwd /path/to/project           # choose the workspace
codegg -m anthropic/claude-sonnet-4-20250514 --run "Explain this project"   # one prompt, then exit
```

`--model` accepts either `provider/model-id` or a bare `model-id`. The
`provider/model` form is unambiguous and is what the examples here use.

## Global flags

| Flag | Meaning |
|---|---|
| `-c`, `--continue-session` | Resume the most recent session. Conflicts with `--session`, `--fork`, `--no-session`. |
| `-s`, `--session <SESSION>` | Open a specific session. |
| `--fork <FORK>` | Fork the given session. |
| `--no-session` | Ephemeral run; nothing is persisted. |
| `-m`, `--model <MODEL>` | Override the model. |
| `-a`, `--agent <AGENT>` | Override the agent. |
| `-p`, `--run <PROMPT>` | Run a single prompt and exit. |
| `-f`, `--format <text\|json>` | Output format for one-shot mode. `--output-format` is a compatibility alias. Default `text`. |
| `-q`, `--quiet` | Hide status messages in non-interactive mode. |
| `--cwd <DIRECTORY>` | Set the working directory before startup. Must exist, or the command fails. Equivalent to launching from that directory. |
| `-v` | Verbosity: no flag warn, `-v` info, `-vv` debug, `-vvv`+ trace. |
| `--approval-mode <MODE>` | `interactive`, `automatic`, or `yolo`. |
| `--sandbox <PROFILE>` | `read-only`, `workspace-write`, or `full-host`. |
| `--yolo` | Exact alias for `--approval-mode yolo`. |
| `--standalone` | Run the core in-process, without the singleton daemon. |

Approval mode and sandbox are orthogonal: an automatic approval mode never
implies host-level filesystem access. See `docs/tui.md`.

## Sessions

```bash
codegg sessions                    # list sessions for the current project
codegg sessions --archived         # include archived sessions
codegg session <session-id>        # inspect one session
codegg export <session-id>         # export to stdout
codegg export <session-id> -o session.json
codegg import session.json         # import a session
```

These commands read and write the project-local store at
`<project>/.codegg/sessions.db`. The database is created and migrated on
first use — by the TUI, the daemon, or any of these commands — so they work
in a project that has never been opened.

Sessions created by the TUI are persistent by default and can be resumed
across restarts.

## Providers and credentials

```bash
codegg providers                    # list providers your config can use
codegg models                       # list models across configured providers
codegg models -p anthropic          # list one provider's models
```

These read your actual configuration and credentials rather than a static
list, so the output changes with what you have configured. See
`docs/providers.md`.

The encrypted credential store:

```bash
printf '%s' "$OPENAI_API_KEY" | codegg auth set-key openai
codegg auth status                  # account ids only; never secrets
codegg auth logout openai
```

`auth set-key` reads the key from stdin, so it does not appear in shell
history. On a fresh profile the first protected write creates a CodeGG-managed
master key automatically — no key environment variable is required. See
`docs/providers.md` for the full resolution order.

## Configuration and diagnostics

```bash
codegg validate                     # validate the auto-detected config
codegg validate --config ./my.jsonc # validate a specific file
codegg edit                         # open the config in $EDITOR
codegg edit --config ./my.jsonc     # open a specific file
codegg edit --project               # open the project-local config
codegg edit --editor hx             # override $VISUAL/$EDITOR
codegg doctor                       # run every diagnostic subsystem
codegg doctor providers             # one subsystem
```

### `edit`

`edit` opens a config file in your editor, creating it (and its directory)
first if it does not exist so the editor always gets a real path to write.

The editor is resolved in this order:

1. `--editor <cmd>`
2. `$VISUAL`
3. `$EDITOR`
4. the first of `hx`, `vim`, `vi`, `nano` found on `PATH`

`$VISUAL` outranks `$EDITOR` per the usual Unix convention, because `$VISUAL`
marks the full-screen editor you prefer for interactive editing while
`$EDITOR` is often left as a line-oriented default. Both may be multi-word or
quoted (`EDITOR="code --wait"`, `EDITOR="/opt/My Editor/hx"`). An editor named
by `$VISUAL`/`$EDITOR` is used even if it is missing — CodeGG reports the spawn
failure rather than silently editing the file in a different program.

Which file gets opened, highest precedence first: `--config <path>`, then a
project config discovered from the working directory (`.codegg/codegg.jsonc`),
then the global config. `--project` forces the project-local file, creating it
if no project config exists yet.

After the editor exits, `edit` re-reads the file and reports whether it still
parses. A parse or validation failure is printed to stderr but does not change
the exit status — the edit itself succeeded, and you may still be fixing it.
Run `codegg validate` to confirm.

`doctor` takes an optional positional subsystem:

| Subsystem | What it reports |
|---|---|
| `all` | Everything below (the default). |
| `search` | eggsearch backend, MCP tool coverage, output caps, provider degradation. |
| `mcp` | Configured MCP servers and their connection state. |
| `lsp` | Language-server registry and per-gate exposure state. |
| `deterministic-tools` | `eggsact` backend, profile, and tool visibility counts. |
| `providers` | Configured providers. Read-only; makes no network probes. |
| `credentials` | Stored credential metadata. Never prints secrets. |
| `installation` | Owned runfiles, helper/sidecar state, and the sandbox kernel probe. |

`doctor` exits 0 even when it reports problems — read the output rather than
the exit code.

`--subsystem <name>` still parses as a deprecated alias for the positional
form. Prefer the positional spelling.

## Daemon

```bash
codegg daemon status                # is it running, and where
codegg daemon logs                  # tail recent daemon logs
codegg daemon logs --lines 200
codegg daemon start                 # run the daemon (BLOCKS - see below)
codegg daemon stop                  # stop it
codegg daemon attach                # attach the TUI to it
codegg daemon attach --new          # attach with a new session
```

**`codegg daemon start` runs in the foreground.** It acquires the singleton
lock, binds the socket, and then stays in the foreground for the life of the
daemon — it does not fork or detach. Run it in a separate terminal or
background it. If a healthy daemon already holds the lock, the command prints
a message and exits 0 rather than starting a second instance.

You rarely need to do this by hand: a plain `codegg` connects to an existing
daemon or starts one automatically. See `docs/daemon.md`.

## Non-interactive use

One-shot prompt:

```bash
codegg --run "Summarize the changes in this repository"
codegg --run "Summarize the changes" --format json
```

Structured input for CI, via `exec`:

```bash
printf '%s\n' '{"prompt":"Review this repository","model":"anthropic/claude-sonnet-4-20250514","agent":"build"}' \
  | codegg exec --format json --quiet
```

The same payload can be passed with `--json '<payload>'` or read from a file
with `--file <path>` instead of stdin. The object has three optional string
fields — `prompt` (required), `model`, and `agent`.

```bash
codegg exec --json '{"prompt":"List the public functions in src/lib.rs"}' -q
```

`exec` also takes `-s <session-id>` to resume an existing session, and the
same policy flags as the root command:

```bash
codegg exec --approval-mode interactive --sandbox read-only --quiet \
  --json '{"prompt":"Summarize src/main.rs"}'
```

`--json-output` / `-j` remain as compatibility aliases for `--format json`.
Without explicit policy flags, `exec` keeps its legacy permissive behavior for
existing CI pipelines — set `--approval-mode` and `--sandbox` explicitly if you
want a bounded run.

## MCP, ACP and plugins

```bash
codegg mcp list
codegg mcp add
codegg mcp remove
codegg mcp enable
codegg mcp debug
```

MCP servers are configured under the `mcp` key in the config file. See
`docs/MCP.md`.

For editor and harness integration, CodeGG speaks ACP v1 over
newline-delimited JSON-RPC on stdio:

```bash
codegg acp
```

Plugins are configured through the `plugin` config key and managed from the
TUI; there is no `codegg plugins` CLI subcommand. WASM plugin execution
requires the `plugins` Cargo feature. See `docs/PLUGINS.md`.

## Research

```bash
codegg research "What changed in SQLite's WAL mode since 3.22?"
codegg research "..." --depth deep --audience engineer --allow-network
```

| Flag | Default | Meaning |
|---|---|---|
| `--mode <MODE>` | `narrow-answer` | Research mode. |
| `--audience <AUDIENCE>` | `human` | Target audience for the write-up. |
| `--depth <DEPTH>` | `medium` | Research depth. |
| `--output <OUTPUTS>` | — | Output profile; repeatable. |
| `--source <SOURCES>` | — | `local`, `file:<path>`, or `url:<https-url>`. Repeatable. |
| `--allow-network` | off | Permit network fetching. |

Research depends on the eggsearch backend; check `codegg doctor search` first
if a query returns nothing.

## Shell completions

```bash
codegg completions bash                    # print to stdout
codegg completions zsh -o ~/.zsh/completions   # write _codegg into an existing dir
```

With no `-o`/`--output`, the script is written to stdout, not to a file.
`--output` requires the directory to already exist and names the file per shell
(`codegg.bash`, `_codegg`, `codegg.fish`, `codegg.ps1`, `codegg.elv`).

Supported shells: `bash`, `elvish`, `fish`, `powershell`, `zsh`. The command's
own help prose lists only four — `elvish` appears solely in the `possible values`
line, so trust the value list rather than the sentence.

## Upgrade

```bash
codegg upgrade
```

Updates a managed prebuilt installation as a single verified transaction. It
does not fetch or run `install.sh`. Source installs have no managed bundle to
upgrade; use `cargo install` again instead. See `docs/install.md`.

## Hidden compatibility spellings

These still parse but are hidden from `--help`. They exist so existing scripts
and older configurations keep working; prefer the canonical spelling.

| Hidden | Canonical |
|---|---|
| `--core-transport inproc\|stdio\|socket` | `--standalone`, `--stdio`, `codegg daemon attach` |
| `--stdio` | (compatibility stdio core transport) |
| `--core-endpoint <URI>` | `--endpoint` on `codegg daemon` subcommands |
| `codegg attach-daemon` | `codegg daemon attach` |
| `codegg core-stdio` | (internal transport) |
| `--output-format <FMT>` | `--format` |
| `--json-output` / `-j` | `--format json` |
| `doctor --subsystem <NAME>` | `doctor <NAME>` |

`codegg attach` is a different command entirely — it is the feature-gated
remote HTTP client, takes a server URL, and only exists in `server` builds.
See `docs/daemon.md` and `architecture/server.md`.
