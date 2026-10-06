# The Core Daemon

By default, `codegg` talks to a single long-lived background process called
the daemon. This document explains what it is for, how to control it, and what
changes when you run without it.

## What the daemon is

The daemon is the core runtime: the part of CodeGG that owns session state,
job execution, and the event stream. Frontends — the TUI, the HTTP server —
are clients of it.

There is exactly **one daemon per OS user**, enforced by an advisory lock
(`daemon.lock`) that the daemon holds for its whole lifetime. This is not an
optimization; it is the invariant the rest of the system is built on.

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
| `daemon.json` | Identity metadata: daemon id, generation, PID, endpoint, protocol version, start time, binary version. Diagnostic only. |
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
directory keeps them consistent.

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
| `codegg daemon status` | Reports whether a daemon is reachable, plus its id, generation, PID, start time, protocol version, binary version, endpoint, uptime, active sessions, and connected clients. |
| `codegg daemon logs` | Shows the tail of `daemon.log`. `--lines` defaults to `50`; `--file` reads a specific file. |
| `codegg daemon attach` | Opens the TUI against a running daemon over the local socket. `--endpoint`, `--session`, `--new`. |

```bash
codegg daemon status
codegg daemon logs --lines 200
codegg daemon stop
```

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