# The Terminal UI

CodeGG's terminal UI is the primary way to drive an agent. It connects to (or
starts) the user-scoped singleton daemon, opens a project tab, and scopes every
action to that tab's workspace.

For the daemon's lifecycle and connection rules see `docs/daemon.md`. For
provider connections see `docs/providers.md`. The authoritative module contract
lives in `architecture/tui.md`.

## Launching

```bash
codegg                       # new session in the current directory
codegg -c                    # resume the last session
codegg -s <SESSION_ID>       # open a specific session
codegg --fork <SESSION_ID>   # fork an existing session
codegg --no-session          # ephemeral session, nothing persisted
codegg -m <MODEL>            # override the model
codegg -a <AGENT>            # override the agent
codegg --standalone          # in-process core, no daemon; global scheduling unavailable
```

`--standalone` runs the core inside the frontend process. It is a visible
non-production mode and the scheduler's global admission control is not
available, so tools that require the daemon scheduler refuse to run.

Launch flags are documented in full in `docs/cli.md`.

## Project tabs and scoping

The TUI can keep several projects open at once as a tab strip. Switching the
active tab re-points the project context; project-scoped actions resolve their
`ProjectExecutionContext` from the active tab before dispatch, so every action
lands in that tab's workspace.

Two consequences worth knowing:

- Execution is always scoped to the active tab's workspace. The process working
  directory is never changed — the launch directory is used exactly once, by
  the bootstrap path that creates the initial compatibility tab.
- The sidebar is keyboard-focusable and includes an Agent Runs tree for nested
  run inspection. The tree renders the active projection turn's bounded run
  nodes parent-first, with status, attention, progress, and joined
  branch/worktree/result-commit hints. It is a projection view, not a second
  run store — pressing Enter on a node delegates to the lazy run-detail surface
  rather than copying transcripts into sidebar state.

Switch tabs with `NextProjectTab` / `PreviousProjectTab` / `SelectProjectTabByIndex`
and close one with `CloseProjectTab`. A local project picker (`WorkspaceRegister`
then `ProjectRegister`) registers a new project into the catalog.

## Sessions from the CLI

Four subcommands read and move session records without opening the UI:

```bash
codegg sessions                 # list active sessions for the current project
codegg sessions --archived      # include archived sessions
codegg session <ID>             # inspect one session
codegg export <ID>              # export a session to JSON
codegg export <ID> -o out.json  # ...to a file
codegg import out.json          # import a session from JSON
```

These commands operate on the **project-local** store at
`<project>/.codegg/sessions.db`, resolved from the current working directory
(`init_migrated_legacy_project_store`). The database is created and migrated
on first use, so they work in a brand-new checkout. The same entry point backs
the CLI session commands, an in-process (`--standalone` / `--stdio`) core, and
`codegg server --standalone-core`.

Note that this project-local store is legacy and distinct from the user-scoped
daemon catalog: the ordinary daemon-backed TUI connects over the socket and
uses the catalog (`<data-root>/codegg.db`), not this project-local file.

## Slash commands

Anything typed as `/name` in the prompt is dispatched through the command
registry in `src/tui/command.rs`. That registry is the source of truth — it is
large, gains entries continuously, and includes project-local and
plugin-contributed commands, so this page deliberately does not enumerate it.

Discover commands in-app instead:

- Type `/` into the prompt to open the command palette, which filters
  live as you type.
- `/help` opens the help dialog for the full, current list.
- Unknown names simply are not dispatched; the palette is the reliable view.

A few commonly used entries:

| Command | Purpose |
|---------|---------|
| `/help` | Open in-app help |
| `/status` | Session and runtime status |
| `/agents` | List available agents |
| `/sessions` | Session browser |
| `/policy` | Current approval and sandbox policy detail |
| `/approval` | Select approval mode (`/approval-mode` is an alias) |
| `/sandbox` | Select sandbox profile (`/sandbox-profile` is an alias) |
| `/themes` | Theme picker (`/theme` is an alias) |
| `/connect`, `/connections` | Provider connection management |
| `/doctor` | In-app diagnostics for the search backend, MCP servers, and providers |
| `/reload` | Refresh project runtime assets — `/reload skills`, `/reload agents` |
| `/init` | Review a bounded root `AGENTS.md` proposal; press `a` in the preview to publish |
| `/exit` | Quit (`quit` and `q` are aliases) |

Registry metadata — domain, scope, source, keywords — exists for palette and
help presentation only. It does not bypass dispatch, observer mode, daemon
authorization, or permission gates.

## Themes

`/theme` opens the picker with no arguments, and accepts `list`, `use <name>`,
`reload`, and `diagnostics`:

```text
/theme list
/theme use cyber-red
/theme reload
/theme diagnostics
```

Themes are looked up **by ID only**. The registry is keyed on each theme's `id`
and lookup is an exact map lookup, so `/theme use "Cyber Red"` — the display
name — fails with `Unknown theme: Cyber Red`. Use the id. `cyber-red` is the
default id, used when no `[theme].name` is configured.

If `/theme reload` drops a theme you were using, CodeGG falls back to the
default theme id. Around 50 Halloy-format themes ship in `assets/themes/halloy/`;
see `docs/themes.md` for the format and authoring details.

## Human shell

Prefix a prompt with `!` to run a command locally:

```text
!cargo test
!!cargo test
```

The single bang runs the command and keeps its command and output out of the
model's context. The double bang
stages one bounded, redacted result in the conversation for the next submitted
turn. It does not start inference on its own. `/shell-include` stages a selected
view the same way; `/shell-ask` submits one question with the selected evidence
to the bound session. Promotion requires a bound, non-observer session; failed
submissions do not mark an output as promoted.
Human-shell output always passes the shared secret redactor and a 32 KiB limit,
even when general shell projection redaction is configured `off`. Staged output
remains in the active TUI until the next turn is accepted.
The WebSocket TUI transport does not support a separate staged-context
request, so approved output is placed in an empty composer for explicit
submission; an existing draft is preserved.

Classification happens before dispatch: `!!` is matched first. The effective
`human_shell.auto_promote_bangbang` setting controls whether its output is
staged; set it to `false` to run privately like `!`.

Output storage is bounded. Each command gets a 1 MB per-command budget out of
an 8 MB total, and at most 100 commands are retained in history. Within one
command, the retained output is 512 KiB total — a 256 KiB head and a 256 KiB
tail — and the byte count of everything in between is recorded as omitted
rather than dropped silently. The default per-command timeout is 300 seconds.

Commands are also screened before they run: a blocked command is refused with
a reason, and one classified as dangerous opens a confirmation dialog first
(`confirm_dangerous`, default true).

`/shell-list`, `/shell-show`, `/shell-include`, `/shell-ask`, `/shell-rerun`,
and `/shell-kill` manage the history and the promote/include actions.
`architecture/human_shell.md` is the authoritative contract.

The `[human_shell]` settings `enabled`, `default_timeout_secs`,
`auto_promote_bangbang`, `confirm_dangerous`, and `ansi` are read by the TUI.
Human output sent to model context is ANSI-stripped, redacted, and capped at
32 KiB even when shell-output redaction is configured off.

## Interactive terminal

`/terminal-create <command> [args...]` opens a daemon-owned Unix PTY. The
terminal starts in viewing mode; press `i` to focus it. While focused, literal
`Esc` is sent to the program (for example, Vim), and `Ctrl-]` returns keyboard
focus to the TUI. Press `Esc` while viewing to close and detach. The screen
interprets VT cursor movement, erase, SGR colors, and alternate-screen control
in bounded local state; child escape sequences are not written to the outer
terminal. Resizing the TUI resizes the PTY to the usable dialog viewport.

## Approval and sandbox

Two independent dimensions. Approval decides *who answers an escalation*;
the sandbox decides *how far the process can reach*. Neither implies the other.

### Approval modes

| Mode | Behavior |
|------|----------|
| `interactive` | Escalations are sent to you. This is the default. |
| `automatic` | Escalations go to a bounded read-only reviewer that can only allow actions inside the current authority and sandbox ceiling. Explicit denies still deny. If no reviewer model is configured, `automatic` defers to you — it never silently behaves like `yolo`. |
| `yolo` | Approval prompts are skipped. Explicit denies and the configured sandbox still apply. |

### Sandbox profiles

| Profile | Behavior |
|---------|----------|
| `read-only` | Filesystem writes are denied by containment. Network has no OS isolation. |
| `workspace-write` | Writes are confined to the workspace roots on supported hosts. Network has no OS isolation. |
| `full-host` | CodeGG filesystem containment is disabled — the agent has your OS user's host authority outside the workspace, and network access is unrestricted. |

### Confirmations

Selecting a policy combination is the only time these dialogs appear; once the
selection lands, frontends must not re-prompt per action.

| Combination | Confirmations required |
|-------------|------------------------|
| `interactive` or `automatic`, with `read-only` or `workspace-write` | 0 |
| `yolo`, with `read-only` or `workspace-write` | 1 |
| `interactive` or `automatic`, with `full-host` | 1 |
| `yolo` with `full-host` | 2 |

Each confirmation is a separate dialog, and the second is only shown after the
first is accepted.

### Persistence and ceilings

Your last selection is restored on startup. The daemon resolves it against any
project or administrator ceiling, and the TUI reports when your stored
preference was narrowed — requested and effective values can differ, and the
restoration toast says so rather than silently applying the weaker policy.

### Headless equivalents

```bash
codegg --approval-mode interactive
codegg --approval-mode automatic
codegg --approval-mode yolo
codegg --yolo                          # exact alias for --approval-mode yolo
codegg --sandbox read-only
codegg --sandbox workspace-write
codegg --sandbox full-host
```

These map to the same daemon policy contract as the TUI selectors. `--yolo`
does not change the sandbox: containment still applies unless you also pass
`--sandbox full-host`. `--sandbox` is likewise orthogonal to approval mode.

The permission rules that gate individual tool calls, and the deterministic
security signal pipeline that can escalate them, are described in
`docs/security-semantics.md`. The contracts are in
`architecture/permission.md` and `architecture/security.md`.

## Where to go next

- `architecture/tui.md` — module map, async command pattern, focus model
- `architecture/core.md` — core facade and transport adapters
- `docs/cli.md` — every CLI subcommand and flag
- `docs/agents-skills.md` — agents and skills from the UI
- `docs/tools.md` — what the agent can actually call
- `docs/themes.md` — theme authoring
- `docs/TROUBLESHOOTING.md` — common failures
