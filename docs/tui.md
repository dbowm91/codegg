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
  directory is never changed; the frontend stores the selected workspace root
  on that tab and passes it as the execution context.
- The sidebar is keyboard-focusable and includes an Agent Runs tree for nested
  run inspection. The tree renders the active projection turn's bounded run
  nodes parent-first, with status, attention, progress, and joined
  branch/worktree/result-commit hints. It is a projection view, not a second
  run store — pressing Enter on a node delegates to the lazy run-detail surface
  rather than copying transcripts into sidebar state.

Switch tabs with `NextProjectTab` / `PreviousProjectTab` / `SelectProjectTabByIndex`
and close one with `CloseProjectTab`. A local project picker opens a directory:
an unseen directory creates a project named after that directory, while a
reopened directory loads its existing project. Open the picker with
`Shift+F` in `NOR` mode or the `/projects` command
(which also works mid-prompt).

Each new directory is bound to its own project, even when another directory
is a clone or worktree with the same Git remote. Existing projects that were
previously bound to multiple directories remain readable; use the picker edit
action to rebind one directory before adding another.

The picker shows the active project and its workspace path on the status
line; the sessions dialog repeats that context in its header. In the picker,
`j`/`k` move through results and printable keys filter while in `NOR`.
Press `i` to type a filter including `j` or `k`; `Esc` returns to
`NOR`, clears a filter, then closes on successive presses. `Tab` includes
archived projects, which appear after active projects. In `NOR`, `d`
deactivates an active project and `a` reactivates an archived one. Press
`d` on an archived project to review its current session count and confirm
permanent deletion; `Esc` or `n` dismisses without changing anything.
Destroy permanently deletes the project and its sessions, so it cannot be
undone. Open tabs for the destroyed project close automatically.

Press `e` on a project row to edit its display name and bound directory.
Use `Tab` to switch fields, `Enter` to commit, and `Esc` to cancel. Repath
rebinds the project's sessions to the new existing directory; it never
moves files. Roots must be absolute directories without a symlink alias.
A root already bound to another project is refused with that project's
name. A successful rebind updates open tabs and their displayed paths.

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
- `/help` opens the help dialog — the full, current list of keybindings. **Type
  to filter it**: terms are matched against the key, action, section, and
  condition, so `ctrl alt` or `sidebar` narrows to what you want. Scroll with
  the arrow keys, PageUp/PageDown, Home, and End. `Esc` clears the filter, and a
  second `Esc` closes the dialog.
- Unknown names simply are not dispatched; the palette is the reliable view.

Keybindings with two modifiers use `Ctrl+Alt`, never `Ctrl+Shift`. CodeGG does
not ask the terminal for enhanced key reporting, so a `Ctrl+Shift+<letter>`
keystroke arrives indistinguishable from `Ctrl+<letter>` and fires that key's
plain-`Ctrl` action instead. `Shift+S` opens the panel
manager, `Ctrl+Alt+S` focuses the sidebar, and `Ctrl+Alt+N` / `Ctrl+Alt+B` move
between project tabs.

### Moving between the box and the panel

The directionals are one gesture family, and **which key does what depends on
where your panel is docked**:

| Panel is | Enter the panel | Back to the buffer | Cycle loaded panel |
|---|---|---|---|
| right column *(default)* | `Shift+→` / `Shift+L` | `Shift+←` / `Shift+H` | `Shift+↓`/`↑` / `Shift+J`/`K` |
| left column | `Shift+←` / `Shift+H` | `Shift+→` / `Shift+L` | `Shift+↓`/`↑` / `Shift+J`/`K` |
| bottom row | `Shift+↓` / `Shift+J` | `Shift+↑` / `Shift+K` | `Shift+→`/`←` / `Shift+L`/`H` |

Read it as "the gesture pointing at a panel enters it, the opposite gesture
comes back, and the other two walk the panel list". With a **box** open, the
same gesture instead means "move this box into the panel on that side".

**The main buffer is a stop of its own, not something you step over.** With two
columns up — Terminal left, Chat right — one `Shift+H` from Chat lands in the
buffer and the second reaches Terminal. The buffer sits *between* the columns, so
the gestures walk the whole frame rather than jumping from one end to the other.

So on the default right-column layout: `/chat` then `Shift+L` closes the box
and leaves the conversation running in the panel, `/help` then `Shift+L` does
the same for the keybinding reference, and `/terminal` then `Shift+L` moves
the terminal across. With no box open, the same keys move the keyboard
instead.

Whichever panel is on your selected edge has its keys on the line just above
the input, docked or not — see [Browsing files](#browsing-files) for what that
looks like.

### Moving a panel to the other side

**Pushing a panel toward an empty side moves it there.** There is no "wrong
side" to land on: `/chat` then `Shift+H` walks the chat panel to the left
column, and the right column goes back to **whatever it was showing before** —
the session panel if that was up, collapsed if you had put it away with
`Ctrl+B`. So you never lose the panel you displaced, and you never get an
empty column where something used to be.

Concretely:

1. `Ctrl+B` collapses everything.
2. `/chat` then `Shift+L` puts chat on the right.
3. `/chat` then `Shift+H` puts chat on the left, and the right goes back to
   collapsed — because that is how you left it.

CodeGG says what the vacated edge became ("Chat moved to the Left; the Right is
now Session"), since that edge changes without you pressing anything there.

Each side remembers a short history (the last few panels it was showing), so a
panel you displace by picking another one comes back the same way.

The docked **Help** panel opens in `NOR` rather than `INS`, because a panel is
read and scrolled before it is filtered: `j`/`k` and the arrow keys scroll
immediately, and `i` takes you to the filter. The `/help` *box* opens in `INS`,
because you opened it to type a query.

The **panel manager** lists every panel with a one-line
description; a digit (`1`-`9`, `0`) binds that panel to `Ctrl+<digit>` so you can swap
to it in one keystroke. Bindings are saved to `codegg.json` and survive a restart.
`Ctrl` rather than `Cmd` because a terminal does not deliver a Cmd modifier.

It opens in `NOR`, because it is reached with a Normal-mode chord and is read
before it is typed into:

- **`NOR`** — `j`/`k` and the arrow keys move the highlight; `Shift+H/J/K/L`
  (or `Shift+←/→/↑/↓`) **put the highlighted panel on that edge**; `Enter`
  loads it onto the edge the strip marks; `p` moves that edge; a digit binds;
  `i` takes you to the filter.
- **`INS`** — type to filter, `Backspace` edits, `Esc` goes back to `NOR` with
  your query intact. `Esc` in `NOR` clears the filter first and only closes
  once there is nothing to clear.

The directional is the reason the manager needs its own mode: in `NOR` it is
free, so it means *put this panel over there* — the same gesture the main
buffer uses. In `INS` it is just the letter, so it types into the filter like
any other character.

Opening the manager never changes your frame. If an edge is collapsed it stays
collapsed, and the strip says so (`[Left collapsed]`) rather than popping the
column open behind you.

It has two ways in: `Shift+S` in `NOR` mode, or the `/panels` command from
anywhere. Use `/panels` mid-prompt — in `INS` mode `Shift+S` is just the
letter `S`, so it has to stay typeable.

#### What `Enter` does

`Enter` on a row is **one act**, and the panel then tells you which of three
things it was:

- the panel **fills** — it shows its content, and takes your keyboard;
- a **load started** — the panel says it is loading, and the box arrives on its
  own (that is where `Shift+L` docks it);
- the panel **cannot act** — it says why, in the panel, e.g. *memory search
  needs a query* or *no pending plan confirmation*.

Every panel in the list does one of those three. A row that could only ever sit
empty is not in the list: the Workspace dashboard is a full view of its own,
opened with `Ctrl+O` or `/workspace`, so it is not a panel. A panel with
nothing loaded in it yet says so plainly and points back at `/panels` — it will
never tell you to go and run a command elsewhere, or to press `Shift+←`, since
the panel is already where you asked for it.

`p` inside the manager moves the panel to the next edge — right → left →
bottom — carrying whatever panel is loaded with it, and the choice is saved too
(`panel_placement` in `codegg.json`). The strip above the list always shows all
three with the live one marked, so you can see there are three places to be
before you press it again.

`Ctrl+B` hides and shows the panel you are looking at. It closes whichever pane
has your keyboard, whatever side that pane is on, and leaves the other side
alone.

### Where your input goes

The composer at the bottom is the **only** place you type, and it always shows
what Enter will do:

```
[NOR] [build] [BUILD] [CHAT]
```

`[NOR]`/`[INS]` is your typing mode. `[BUILD]` and `[CHAT]` are the two
destinations — the bright one is armed. `[CHAT]` lights up only while the chat
side panel has the keyboard, so it cannot be armed by accident.

To reach the side panel, press `Esc` (to get to `[NOR]`) and then the
directional that points at it — `Shift+→` or `Shift+L` on the default layout.
The opposite direction returns you. These work in `[NOR]` mode only — while you
are in `[INS]`, `Shift+→` moves the caret as usual. The panel you are
controlling shows a `▸` marker and says where Enter will go.

There are deliberately two indicators because they answer different
questions: the composer badge says *where the text goes*, and the `▸` marker
says *which panel has the keyboard*.

A few commonly used entries:

| Command | Purpose |
|---------|---------|
| `/help` | Open in-app help |
| `/status` | Session and runtime status |
| `/agents` | List available agents |
| `/sessions` | Session browser |
| `/policy` | Current approval and sandbox policy detail |
| `/approval` | Approval-mode fast-select (`/approval-mode` is an alias) |
| `/sandbox` | Sandbox-profile fast-select (`/sandbox-profile` is an alias) |
| `/themes` | Theme picker (`/theme` is an alias) |
| `/connect`, `/connections` | Provider connection management |
| `/doctor` | In-app diagnostics for the search backend, MCP servers, and providers |
| `/reload` | Refresh project runtime assets — `/reload skills`, `/reload agents` |
| `/init` | Review a bounded root `AGENTS.md` proposal; press `a` in the preview to publish |
| `/exit` | Quit (`quit` and `q` are aliases) |

To leave CodeGG: `q` in Normal mode, `/exit`, or `Ctrl+C`. **`Esc` never
quits.** It backs out one layer at a time — close a dialog, dismiss the
completion popup, return from Insert to Normal mode, or discard a half-written
prompt — and then stops. `Ctrl+C` unwinds the same layers before it exits, so a
dialog needs a second press.

Registry metadata — domain, scope, and source exist for palette and help
presentation only. It does not bypass dispatch, observer mode, daemon
authorization, or permission gates.

## Subagents, jobs, and the tasks overlay

When a turn fans out — several subagents, background jobs — CodeGG gives each
subagent a short friendly name so you can refer to it, and shows **the same
name** in every place it appears.

Names are temporary. One is assigned while the subagent is running and released
when it finishes, so two concurrent runs never share a name, and a name is not
a durable handle for that run. Read it as "the subagent I was watching a minute
ago", not as an identifier you can rely on later.

Three surfaces show this, and all three read one projection:

| Surface | Open it with | What it shows |
|---|---|---|
| Tasks overlay | `Shift+T` in `NOR` | Todos, subagents, and jobs together |
| Session panel | the sidebar — `Ctrl+B` toggles it, `Shift+L` / `Shift+→` in `NOR` gives it the keyboard on the default layout | `Subagents` and `Jobs` sections under Todos |
| Buffer tabs | `Shift+N` / `Shift+P` / `Shift+X` | One subagent's transcript per tab, in the main buffer |

All four keys are `[NOR]`-only. In `[INS]` they are ordinary capital letters,
so composing a word containing `T`, `N`, `P`, or `X` never triggers them — and
neither does any other capital letter. `Shift+F` (project picker) and `Ctrl+O`
(Workspace view) work the same way round: the shifted letter is `[NOR]`-only,
the chord is live in both modes.

### The tasks overlay

`Shift+T` opens a single overlay listing **todos, then subagents, then jobs**,
each under its own heading. A group with nothing in it is left out entirely —
there are no empty headings. `j`/`k` move, `Enter` opens a subagent as a tab,
and `Esc` closes the overlay. With nothing in any group the overlay says
`Nothing pending` rather than opening to a blank strip, so a quiet session still
confirms the key did something.

It shows what the turn is *waiting on*, not what it has done: finished
subagents and completed jobs are filtered out. When a job reports a state this
build does not recognise it is shown rather than hidden, because showing
finished work costs a glance and hiding live work costs you the thing you
asked for.

The overlay is driven by `[NOR]` keys only, so you can press `i` and keep
typing with it open.

`Ctrl+T` is unaffected and still owns the compact todo strip above the input.
The two surfaces overlap by design — todos appear in both — because the strip is
a progress line that stays on screen at all times, and the overlay is a
fuller list you open when you want it.

### When the agent is waiting on other work

The status row above the input reports `idle` only when nothing at all is
running. If the main agent has handed work to subagents or background jobs and
is now blocked on the results, it reads `awaiting N tasks` and names the
breakdown — so a long fan-out looks like a wait rather than a hang. Live jobs
count here too, which is why a job running under a finished turn is still
visible there.

### Subagent tabs

`Shift+N` and `Shift+P` move between open subagent tabs and wrap around;
`Shift+X` closes the active one. `Esc` also returns to the conversation. The
cycle runs `Chat` first, then the subagent tabs in the order you opened them.

With no subagent tabs open the keys do nothing rather than appearing to break —
you stay on the conversation. Up to 16 subagent tabs can be open at once; at
that point opening another is **refused** and says so, rather than closing a tab
you were reading.

A subagent tab shows that subagent's transcript. Closing it cancels nothing —
the run keeps going whether or not you are watching it. When a run drops out of
the projection the tab stays open and stops claiming to be live, so the buffer
cannot show "running" for something the daemon has stopped vouching for.

### Session panel sections

The Session panel lists `Todos`, then `Subagents`, then `Jobs`, so the three
"what is this waiting on" surfaces sit together. `j`/`k` and the arrow keys
move through them, `Space` collapses a section, and `Enter` (or a click) acts on
the selected row:

- a **subagent** row opens that subagent's transcript as a tab in the main
  buffer, the same tab `Shift+T` would have opened;
- a **job** row does nothing. Jobs have no transcript to show yet, so the key
  is consumed rather than sending you somewhere arbitrary;
- the **Agent Runs tree** further down the panel keeps its own behaviour:
  `Enter` there opens the run-detail dialog.

Unlike the overlay, the panel keeps rows for finished subagents and marks their
status. It is a persistent index, not a status light: a subagent that finishes
should not vanish from the list you are reading. Sections are bounded, and a
section with more rows than fit ends in a `… N more` line.

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

Your selection is written to `[theme].name` in your **global** config (the
per-user file, not the project's `.codegg/` config), so it applies to every
project and comes back on the next launch. Setting `theme.name` by hand has the
same effect.

## Human shell

Prefix a prompt with `!` to run a command on the selected workspace's daemon:

```text
!cargo test
!!cargo test
```

The daemon resolves the registered workspace and project binding, uses the
daemon host's shell and filtered environment, and requires the project's
`command.execute` capability. The TUI's current directory, shell, and
environment are never sent as execution authority. Workspaces assigned to a
remote execution node are refused until node execution is supported. The
single bang keeps the command output out of the model's context. The double bang
stages one bounded, redacted result in the conversation for the next submitted
turn. It does not start inference on its own. `/shell-include` stages a selected
view the same way; `/shell-ask` stages one question with the selected evidence
before submitting it to the bound session. Promotion requires a bound,
non-observer session; failed submissions do not mark an output as promoted.
Human-shell output always passes the shared secret redactor and a 32 KiB limit,
even when general shell projection redaction is configured `off`. The daemon
acknowledges durable staging for the exact session before the TUI reports it as
staged. Pending projections expire after 24 hours and return after reconnect;
turn admission consumes them once with the controller lease and durable
submission receipt. Reusing the same request ID returns that receipt instead
of starting a duplicate turn. If the daemon restarts before the provider
outcome is known, it marks the accepted turn interrupted and does not replay it.
The WebSocket
TUI transport does not support durable staging, so approved output is clearly
identified as an unsubmitted composer draft; an existing draft is preserved.

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

The daemon applies the caller's persisted sandbox profile and reserves a
shared scheduler process slot before acceptance. Runs continue after a client
disconnect until the configured timeout (300 seconds by default, capped at
one hour); status and bounded output remain available for ten minutes to the
same principal, project, workspace, and session. A daemon restart drops the
ephemeral run and does not replay it. The typed `human_shell.v1` protocol
returns an explicit unsupported
response when an older daemon lacks this feature; the TUI never falls back to
spawning on its own host.

`/shell-list`, `/shell-show`, `/shell-include`, `/shell-ask`, `/shell-rerun`,
and `/shell-kill` manage the history and the promote/include actions.
`architecture/human_shell.md` is the authoritative contract.

The `[human_shell]` settings `enabled`, `default_timeout_secs`,
`auto_promote_bangbang`, `confirm_dangerous`, and `ansi` are read by the TUI.
Human output sent to model context is ANSI-stripped, redacted, and capped at
32 KiB even when shell-output redaction is configured off.

### Resizing a panel

`Ctrl+N` arms resize mode. `h`, `j`, `k` and `l` then drag **the surface your
keyboard is on** — each key its own border, and **outward only**:

| Keyboard is on | `h` | `l` | `j` / `k` |
|---|---|---|---|
| a panel in the **right** column | widens it | — | — |
| a panel in the **left** column | — | widens it | — |
| a panel in the **bottom** row | — | — | `k` makes it taller |
| the **main buffer** (`LLM`) | narrows a left column | narrows a right column | `j` narrows a bottom row |

A dash means that border is the frame's own edge, so there is nothing to drag —
`l` inside a right-hand column does nothing rather than shrinking it. Every
border belongs to two surfaces, so growing one shrinks the other: `h` from
inside the chat column widens it, and `l` from the transcript beside it narrows
the same column. That is why resize works from both sides.

`Esc`, `Enter`, or `Ctrl+N` again leaves. The mode stays armed while you drag
several borders in one go. Sizes are per-edge and last for the session; the
first time you drag the bottom row it switches from a percentage to exact rows,
so one press is always one row.

`Ctrl+N` used to start a new session. **New session is now the bare `n`** —
in Normal mode. It is still safe to type: bare letters go to the prompt in
Insert mode.

## Interactive terminal

`/terminal` opens a daemon-owned PTY. With nothing live behind it, it starts
your login shell (`$SHELL`, falling back to `sh`) — you do not have to name a
command first. `/terminal-create <command> [args...]` is still there when you
want to run a specific program instead.

The box opens **already taking your keys**, so you can type straight away.

Once it is taking your keys, every key is your program's — including `H`, `J`,
`K` and `L`, and `Shift`+arrows. Those are the panel-docking gesture everywhere
else, but a terminal that owns the keyboard is not a viewer, so the gesture is
turned off while you are typing. `i` is only a focus gesture while the surface is
in viewing mode (`NOR`); in the box the surface is never in `NOR` to begin with.

Output keeps up on its own: the TUI reads the PTY on its normal refresh tick, so
anything the command prints appears without you doing anything. You do not need
`/terminal-resume` to see live output — it is a recovery affordance for a view
whose read fell behind, not a step in the normal flow.

There are two modes, and each key means exactly one thing:

- **INS** — the process owns the keyboard. Everything you type, plus `Enter`,
  `Backspace`, `Tab`, the arrow keys and `Ctrl-C`/`Ctrl-D`, goes to the
  terminal. The cursor is drawn where the shell is accepting the next
  character. Press `Esc` to stop typing and read.
- **NOR** — the reader owns the keyboard. `hjkl` and the arrows move, `Ctrl+u`
  and `Ctrl+d` page, `e`/`b` jump by word, `v` selects by character, `x` selects
  whole lines, `d` deletes the selection, `space+y` copies it, `C` clears the
  screen, `D` folds in diagnostics. Press `i` or `Enter` to start typing again.
  Press `Esc` to close and detach — or to drop an armed selection first.

If you are running a full-screen program that needs its own escape (Vim, less,
tmux), press `Ctrl+]` to send it a real `Esc` byte and stay in INS. `Esc` itself
is the mode key and never reaches the child.

The screen interprets VT cursor movement, erase, SGR colors, and alternate-screen
control in bounded local state; child escape sequences are not written to the
outer terminal. Resizing the TUI resizes the PTY to the usable dialog viewport.

`/terminal` then `Shift+L` moves it into the side panel, where it behaves the
same way. You can also **click** a panel column to focus it, **drag** across one
to select its text and copy it with `Ctrl+C`, and **scroll** the wheel over one
to move that column's own content. If the process behind the panel has exited,
`i` starts a fresh shell instead — the panel never leaves you staring at a dead
one. `/terminal-clear` wipes the local scrollback and screen while the process
keeps running.

The box opens with one header line — the command, the link state, and whether
it is taking your keys. Press `D` while viewing to fold in the full diagnostics
dump (handle, workspace, attachment id, size and sequence cursors); that is the
view to paste into a bug report. A resync, a degraded screen, or an exit stays
visible either way, because those change what the rows on screen mean.

Opening the box takes an async create-and-attach round trip against the daemon.
Until it completes there is no box, so keystrokes keep going to the composer —
type the command again once the box is up rather than assuming `i` was eaten.

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

### Fast-select keys

Bare `/approval` and `/sandbox` open a selector seeded on the value currently in
force. Both answer in one keystroke:

| Key | Action |
|-----|--------|
| `j` / `k`, `↓` / `↑` | Move the cursor |
| `1` – `3` | Select that row immediately — no Enter needed |
| `Enter` | Select the highlighted row |
| `Esc` | Cancel, changing nothing |

`/approval yolo` and friends still work as the argument form and take the same
confirmation path.

The permission dialog answers the same way, with the letter keys as the primary
affordance:

| Key | Action |
|-----|--------|
| `y` / `1` | Allow once |
| `p` / `2` | Always allow — arms a confirmation, never grants outright |
| `n` / `3` | Deny once |
| `4` | Always deny — also arms a confirmation |
| `j` / `k`, `↓` / `↑` | Move the cursor |
| `Enter` | Select the highlighted row |
| `Esc` | Cancel |

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

### Approval timeouts

When a tool needs your approval, CodeGG **waits until you answer**. There is no
timeout by default: the dialog stays up and the turn stays parked until you
decide, so a request can never be denied just because you had not looked at it
yet.

To bound the wait — only worth doing if CodeGG has to fail closed unattended —
set a limit in your config:

```jsonc
{
  "permission": {
    "approval_timeout_secs": 900
  }
}
```

An unanswered request is then denied after that many seconds. The denial is
recorded as `timeout_deny`, deliberately distinct from a `no` you gave yourself,
so `/logs` can tell the two apart. Without this key the wait is unbounded.

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

## Editor language features

Opening a file puts you in `NOR`, not `INS`. It is a surface you read before
you type into, and `i` is one keystroke away. `NOR` gives the keyboard to the
Helix verbs: `hjkl`/arrows move, `Ctrl+d`/`Ctrl+u` page, `e`/`b` jump by word,
`v` selects by character, `x` selects the whole line (press again for one
more), `d` deletes the selection, and `space y` copies it — the same keys
`/terminal` uses in its viewing mode. `i`/`a`/`I`/`A`/`o`/`O` enter `INS`,
where typing goes into the file and `Esc` returns. In `NOR` anything the
buffer does not claim types into the input box at the bottom, so you can talk
to the agent without leaving the file. `Esc` never closes the editor; `:q`
does.

The cursor is a block in `NOR` and a thin bar in `INS`, so you can always see
where you are. `Ctrl+d` pages down and continues to the end of the file when a
half-page would land past it.

### Searching inside the file

`/` searches the document while the editor has focus in `NOR`. The line opens
pre-filled with the word under the cursor, and it searches as you type:
`/host` → `/host_status` → `/host_status fn`. Matches are highlighted in the
text and the count shows next to the line/column readout.

| Key | What it does |
|---|---|
| `Enter` | accept, keep the highlight |
| `Esc` | abandon the search and clear the highlight |
| `n` / `N` | next / previous match (after `Enter`) |
| `Backspace` | remove a character and re-search |

Search covers a bounded window of the file. If it stops short, the status row
says `partial` rather than implying the count is the whole story.

In `INS` a slash is just a slash — otherwise you could not type a URL.

### Syntax highlighting

Files are coloured as soon as they open, with no language server required.
When a language server is available its semantic tokens take over and stay
authoritative, because it knows the real grammar. Built-in highlighting follows
the file's extension; an extension with no bundled grammar renders plain.
Press `F9` to re-request server tokens if your language server was still
starting.

Very large files (more than 5000 lines) drop built-in highlighting rather than
freeze a frame while re-scanning; the text still edits normally.

The editor can show current-file diagnostics and request hover, definition,
references, document symbols, and workspace symbols from the daemon-owned
language server. Use F2 for diagnostics, F3
for hover, F4 for definition, F5 for references, F6 for document symbols,
and F7 to search workspace symbols for the word at the cursor. The result
panel shows unavailable and not-ready states explicitly. A failed refresh
keeps the previous complete diagnostics marked stale.

Navigation opens only locations carrying the daemon-validated
workspace-relative target; other results remain display-only. The TUI does
not start language servers itself and does not claim live diagnostic push
updates.

### Completion

Press F8 — or type `:complete` in Normal mode — to ask the language server for
completions at the cursor. A popup
lists the suggestions with their kind and detail; `↑`/`↓` (or `j`/`k` in
Normal mode) move the selection, `Enter` applies the selected item, and
`Esc` dismisses the popup without touching anything. Typing anything else
dismisses the popup first and inserts normally.

Only supported plain-text edits can be applied, and only after the editor
re-checks the document identity, revision, and edit range against the
current text. Anything else — snippets, edits needing extra changes,
server commands, stale or out-of-range results — is shown as unavailable
and refused with a message; nothing is ever inserted on your behalf, and
every acceptance is one ordinary edit with one undo step. An empty answer
("no suggestions") looks different from a warming server, a changed
document, or a server without completions, and a truncated list says so.
A reconnect never lets an old answer paint over a new request, even when
everything else lines up. On a very long single line, F8 past the first
32 KiB refuses with "Line too long for completion at this position"
instead of guessing the cursor.

### Semantic highlighting limits and truncation

Semantic highlighting covers up to 1000 tokens per file (the protocol
maximum). Ordinary files with more than 100 tokens highlight normally;
only a response beyond 1000 is rejected as a whole. Token types the theme
does not style — including ordinary `variable`, `parameter`, `property`,
and `namespace` kinds — render as plain text without erasing neighboring
highlights. One malformed span drops only itself, never the whole file.

When the language server reports more tokens than fit, the status row says
`highlighting truncated`; when some spans were dropped locally it says
`highlighting partial`. Either means the colors are incomplete coverage,
not a complete file view. An empty highlight with no such notice is a real
empty result, not a warming server. Editing, reloading, resyncing, or
reconnecting clears the highlight until the next validated read, so colors
never paint against a different revision.

### Very long lines and large file trees

One logical line is one screen row, and each visible line examines at most
32 KiB per frame (256 KiB for the whole frame). A line past 32 KiB shows
its start followed by a `…[too long]` marker, and the status row adds
`line too long — showing prefix` when the cursor is on such a line. The
cursor position itself stays exact, but horizontal scrolling and motion stop
at the end of the shown prefix; normal files are unaffected.

The file tree (Ctrl+T pane and `/files`) shows the daemon's directory
listing for the active workspace: the daemon walks at most 100,000
directory entries and keeps at most 4096, saying which limit stopped it.
`l` re-roots the listing at a directory, which is a fresh daemon walk of
that subtree rather than a filter of what is already on screen, so the
bounds above apply to the directory you are actually in. A completion
answered for a different subtree than the one requested is discarded rather
than painted.
The complete listing response (entries plus workspace/prefix, status,
and truncation reason) is capped at 512 KiB on the wire: oversized trees
are trimmed from the trailing top-level entries with an explicit reason,
down to an explicitly truncated empty listing when even one giant nested
directory cannot fit — never a silent empty project and never an oversize
payload. At most one scan runs at a time on each side; the daemon holds
its admission until the actual filesystem scan exits, so cancelling a
request cannot start a second scan behind it. A refresh that arrives while
a request is active waits as one queued latest request and the pane says
"waiting for prior scan…" instead of piling up workers. A refresh that is
superseded, or a listing answered for a different workspace, never paints
a stale or foreign listing: the pane keeps its previous state and says
why. Listings unavailable from the daemon are explicit errors — the tree
never falls back to reading the local machine's own files, so a
same-spelled local directory can never masquerade as the workspace. A busy
daemon asks for an explicit retry with `r`.

## Browsing files

`Ctrl+T` opens a tree pane beside the editor; `/files` opens the same
navigator as a box you can dock into the side panel with `Shift+L`. Both run
the same navigation, so the keys below mean the same thing in both.

| Key | What it does |
|---|---|
| `j` / `k`, `↓` / `↑` | move the cursor |
| `Ctrl+d` / `Ctrl+u` | page down / page up (two thirds of the pane) |
| `Enter` on a directory | expand or collapse it in place — the cursor stays |
| `l` / `→` | make that directory the **root** and list what's inside it |
| `h` / `←` | go back one level (at the workspace root: collapse in place, otherwise jump to the parent) |
| `Enter` on a file | open it in the editor |
| `r` | re-list the current directory |
| `Esc` | leave the pane / close the box |

`l` re-roots rather than expanding, so the listing always has a top level:
walking into a deeply nested tree is the same operation at every depth, and
`h` puts the cursor back on the directory you just came out of.

**Where you are, three ways.** The title is a breadcrumb — `workspace › src ›
tui` — and every row is drawn one level further in for each component of that
trail, with a `├─` / `└─` guide to the directory it belongs to. Enter a
directory and the rows below it visibly step in, so a scoped listing no longer
looks like the workspace root it stopped being. Guides are plain box-drawing
characters, so this still reads correctly with `CODEGG_FILE_ICONS=plain`.

### Where the keys are

**The line just above the input is where a panel's keys live — including the
keys of a panel you have docked but not yet entered.**

When `/files` is running in the side panel, that line reads

```
▌ Files · Shift+→/Shift+L in · j/k move · ^d/^u page · l/→ root at dir · …
```

and once the panel has the keyboard the front of it flips to `Shift+← back`. The
gesture is the one for wherever your panel is docked — see the table above — so
a left-column panel says `Shift+← in`. The `l/→` and `enter` you see there are
the file browser's own keys, not a generic panel description: the box, its
panel, and this line are written from one source, so they cannot drift apart.

The in-box modal keeps its own keymap along the bottom, because a modal box owns
its whole surface. The side column does not: a column too narrow for a keymap
gives its rows to the listing instead.

The columns you never go into — the session projection by default — stay quiet,
and a panel with nothing loaded in it says nothing either.

## Where to go next

- `architecture/tui.md` — module map, async command pattern, focus model
- `architecture/core.md` — core facade and transport adapters
- `docs/cli.md` — every CLI subcommand and flag
- `docs/agents-skills.md` — agents and skills from the UI
- `docs/tools.md` — what the agent can actually call
- `docs/themes.md` — theme authoring
- `docs/TROUBLESHOOTING.md` — common failures
