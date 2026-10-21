# The Core Daemon

By default, `codegg` talks to a single long-lived background process called
the daemon. This document explains what it is for, how to control it, and what
changes when you run without it.

## What the daemon is

The daemon is the core runtime: the part of CodeGG that owns session state,
job execution, and the event stream. Frontends — the TUI, the HTTP server —
are clients of it.

Ordinary CodeGG clients share exactly **one default daemon per OS user**,
enforced by an advisory lock (`daemon.lock`) that the daemon holds for its
whole lifetime. This is not an optimization; it is the invariant the rest of
the system is built on. `CODEGG_DAEMON_HOME` deliberately selects a separate
runtime root, with its own lock and state; CodeGG does not discover or attach
to those roots unless that override is supplied. The developer-only Harness
Lab uses private roots this way and does not change the ordinary user daemon.

It exists so that:

- **Runtime state is durable across frontends.** Sessions, jobs, and
  subagents survive a frontend exit and reappear in the next one.
- **Work is coordinated across workspaces and projects.** Several TUI
  instances, or a TUI plus a server, share one scheduler rather than each
  independently consuming the machine.
- **Process-consuming work is admitted machine-wide.** Tests, managed
  processes, subagent dispatch, and tool programs all pass through a central
  queue with permits and concurrency limits instead of each frontend spawning
  whatever it likes.

If you never open a second window, you will mostly notice the daemon as
something that keeps running in the background after you quit CodeGG.

## Where its files live

Inside a single per-user root directory:

| File | Purpose |
|---|---|
| `daemon.lock` | The singleton lock. Authoritative — this is what "is a daemon running" really means. |
| `daemon.json` | Identity record: daemon id, generation, PID, endpoint, protocol version, start time, binary version. Read by `daemon status` and by `daemon stop`, which verifies the live daemon's id against it before signalling. |
| `core.sock` | The local socket endpoint (a named pipe on Windows). |
| `daemon.log` | Startup and runtime diagnostics. |

Defaults per platform:

| Platform | Root |
|---|---|
| macOS | `$HOME/Library/Application Support/codegg` |
| Linux | `$XDG_RUNTIME_DIR/codegg`, else `$XDG_DATA_HOME/codegg`, else `$HOME/.local/share/codegg` |
| Other Unix | `/tmp/codegg` |

Set **`CODEGG_DAEMON_HOME`** to move that root somewhere else. Every daemon
subcommand and every frontend honors it, so pointing the whole system at one
directory keeps them consistent. The override must identify state you intend
to keep separate: lock, metadata, socket, catalog, configuration, and other
user-scoped writers are rooted through their corresponding CodeGG/XDG
overrides. See `dev/harness_lab/README.md` for the isolated test setup.

## You do not normally start the daemon

Plain `codegg` is **connect-or-start**. It tries to connect to a running
daemon; if there is not a reachable one, it starts one for you and waits for
it to become ready.

The daemon it starts is detached from your terminal. Quitting the TUI does
not stop it, and the next `codegg` reconnects to the same instance. This is
the intended behavior, not a leak — use `codegg daemon stop` when you want it
gone.

Readiness is a live identity probe, not just an open socket, so a stale or
incompatible socket is never mistaken for a working daemon.

## `daemon start` blocks — keep this in mind

```bash
codegg daemon start
```

**This command runs the daemon in the foreground and does not return.** It
does not fork, detach, or daemonize itself. Leave it running in its own
terminal, or background it explicitly if that is what you want:

```bash
codegg daemon start &     # your shell, backgrounded
nohup codegg daemon start >/dev/null 2>&1 &   # detached from your shell
```

In practice you rarely need this command at all — connect-or-start covers the
common case — but it is the way to run a daemon deliberately in its own
terminal when debugging.

### Starting a second one

If a healthy daemon already holds the lock, `daemon start` connects to it,
prints

```
Daemon already running (lock held at <path>); not starting a second instance.
```

and exits `0`. It never takes the singleton from a live daemon.

### When the lock is held but the socket is dead

CodeGG refuses to guess. It will not unlink a lock it cannot prove is stale,
and `--force-take-lock` cannot override the authoritative lock either. The
error tells you whether the recorded PID is alive. Confirm with
`codegg daemon status` and, if the daemon is genuinely gone, follow the stale
lock steps in `docs/TROUBLESHOOTING.md`.

## Lifecycle commands

| Command | What it does |
|---|---|
| `codegg daemon start` | Starts the daemon **in the foreground** (see above). `--endpoint` picks the socket; `--force-take-lock` is accepted but cannot override a held lock. |
| `codegg daemon stop` | Graceful shutdown. Verifies the live daemon's identity first, then signals it. Unix only. |
| `codegg daemon status` | Reports whether a daemon is reachable, plus its id, generation, PID, start time, protocol version, binary version, endpoint, uptime, active sessions, connected clients, and a one-line scratch usage summary. |
| `codegg daemon scratch-usage` | Measures the daemon's disposable scratch cache: tree count, total size, age, and the reclaim path's verdict for each tree. A tree the walk did not finish reading carries **no** verdict — it is reported as `unknown`, never as reclaimable. `--json` emits the raw report; `--refresh` runs a fresh measurement instead of the daemon's last snapshot. |
| `codegg daemon scratch-reclaim` | Disposes of one scratch tree, named by its opaque digest. A dry run is the default: the command reports what it would remove and removes nothing. `--apply` actually removes it. |
| `codegg daemon logs` | Shows the tail of `daemon.log`. `--lines` defaults to `50`; `--file` reads a specific file. |
| `codegg daemon attach` | Opens the TUI against a running daemon over the local socket. `--endpoint`, `--session`, `--new`. |

```bash
codegg daemon status
codegg daemon scratch-usage
codegg daemon logs --lines 200
codegg daemon stop
```

`daemon scratch-usage` is a **measurement**, not a lifecycle event. It reads
the daemon's own disposable scratch cache and reports counts, sizes, ages, and
the opaque digest that names each tree — never a path, a file name, file
content, or a session identity. The daemon answers from its last snapshot;
`--refresh` asks for exactly one fresh bounded walk. The command is served on
the local daemon socket only.

Each tree carries the reclaim path's own verdict, and the verdict has three
states: **yes**, **no**, and **unknown**. A tree whose walk hit a budget
before the tree was fully read has no verdict at all — the entries it did not
reach could be ones the reclaim path would refuse, so the report says
`unknown` rather than claiming the tree is reclaimable. A tree that vanished
while it was being read is `unknown` for the same reason.

`daemon scratch-reclaim` is the operator disposition for a tree the automatic
reclaim path can never accept — a tree holding a planted symlink, say. It is
named by the same opaque digest `scratch-usage` reports. The command is a dry
run by default: it reports the tree's entry count, size, and the automatic
path's verdict, and removes nothing until `--apply` is passed. The lease is
honoured exactly as the sweep honours it, so a tree whose session still has an
accepted turn, a non-terminal run, or a non-terminal job is refused. The
ownership, containment, and owner-record checks are unchanged; only the
entry-content refusals are relaxed, and every attempt is audited.

The daemon also runs the retention sweep itself, on its own interval. It
reclaims a session's tree once that session has been idle past
`scratch.retention_days` (14 days by default), a soft-deleted session's tree
once its lease is clear, and a tree that no durable session owns once it has
been untouched past `scratch.orphan_grace_hours` (24 hours by default). The
sweep never runs on the agent hot path and never blocks a tool call. The whole
policy — including `scratch.enabled`, which turns the sweep off entirely — is
in the `scratch` configuration section; see `architecture/config.md` for the
defaults and the floors. The daemon reads that section once, at startup, so a
policy change needs a daemon restart.

`daemon stop` is a bounded, verified operation: it will not signal a PID it
has not confirmed is the live daemon. On timeout it reports the failure
rather than force-killing an unverified process. Windows daemon stop is not
available yet.

## Alternative transports

### `--standalone` (recommended for a quick isolated run)

```bash
codegg --standalone
```

Runs the core **in your process**, with no daemon, no lock, and no
connect-or-start. This is a visible non-production mode, useful for
reproducing a problem without interference from a long-lived daemon, or for
embedding.

What you give up: **machine-wide scheduling.** The standalone core does not
participate in the singleton's admission control, so heavy work from a
standalone run is not coordinated with anything else. Durable state is also
not shared with a running daemon.

### `--stdio` (compatibility)

Spawns a `codegg core-stdio` subprocess and exchanges newline-delimited JSON
over stdio. It is treated as standalone and never touches the singleton lock.
The flag is hidden from normal help because it exists for compatibility and
testing, not as a daily workflow.

### `server`

The HTTP/WebSocket `server` subcommand currently requires
`--standalone-core`; it refuses to start otherwise, because it must not
silently construct a second production core. See `architecture/server.md`.

## `daemon attach` vs remote `attach`

These are different commands and it is easy to conflate them.

```bash
codegg daemon attach --session <session-id>   # local Unix socket, same machine
codegg attach http://localhost:3000 --token … # remote HTTP, server builds only
```

`daemon attach` connects the TUI to a daemon on this machine over the local
socket. The top-level `attach-daemon` command is a hidden compatibility alias
for it; prefer the `daemon` form.

`attach` speaks HTTP to a CodeGG server, possibly on another host. It exists
only in builds compiled with the `server` Cargo feature.

## Configuration

Daemon behavior lives under the `daemon` key:

```jsonc
{
  "daemon": {
    "auto_start": true,
    "mode": "daemon_client",
    "project_scope": "user",
    "event_log_capacity": 4096,
    "startup_timeout_ms": 10000,
    "shutdown_timeout_ms": 10000
  }
}
```

| Key | Effect |
|---|---|
| `auto_start` | Whether a frontend may start the daemon when none is reachable. Defaults to `true`. Set it to `false` to require an explicit `codegg daemon start` in another terminal. |
| `mode` | Transport for the connecting frontend: `daemon_client` (default, connect-or-start), `standalone_inproc`, or `standalone_stdio`. |
| `project_scope` | The directory the daemon resolves agents against. `"user"` uses your home directory; anything else uses the directory you started the daemon from. Set this before starting a daemon if you always want it to start from the same place. |
| `event_log_capacity` | Size of the in-memory event log. Defaults to `4096`. |
| `startup_timeout_ms` | How long a client waits for a freshly started daemon to become ready. Defaults to `10000`. |
| `shutdown_timeout_ms` | How long graceful shutdown may take to drain clients before the daemon exits. |

`daemon.enabled` and `daemon.socket` also exist in the schema but are not read
by any production code path; do not rely on them.

## Troubleshooting

`docs/TROUBLESHOOTING.md` covers the cases you are most likely to hit: the
"already running" message, stale sockets after a crash, an unreachable
daemon, endpoint overrides, and graceful stop behavior.

To confirm the current state directly:

```bash
codegg daemon status
codegg daemon logs --lines 200
```

## See also

- `architecture/core.md` — transport modes, the singleton lifecycle, and the
  connect-or-start contract.
- `architecture/scheduler.md` — the machine-wide admission guarantee and what
  standalone mode steps outside of it.
- `architecture/client.md` — the native client and frontend/daemon boundary.
- `docs/install.md` — installing CodeGG and its managed runfiles.
- `docs/TROUBLESHOOTING.md` — symptom-driven daemon diagnosis.
