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

**`Esc` does not leave a panel.** It is the mode switch: it leaves Insert mode
(keeping whatever you typed) and, in Normal mode, it does nothing. A panel
holds the keyboard and its selection until you point somewhere else with the
gestures above. `Shift+Q` is the panel's own exit — it hands the keyboard back
to the composer and leaves the panel where it is, which is what `Esc` used to
do. It is how the `/files` box closes. The line above the input says so while
a panel holds the keyboard (`⇧Q leave · …`).

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
| `/sessions` | Session browser — the session list plus **Subagents** and **Jobs** sections for live work (`j`/`k` move across all three, skipping the headings; `Enter` opens a session or a subagent transcript; job rows are read-only). The dialog labels live-work loading or unavailable state separately from an authoritative empty result. |
| `/subagent-send` | Point the next prompt at one running subagent (`/subagent-send off` clears it); the message goes to the agent running the current turn, which decides whether to relay it |
| `/queue` | Prompts waiting for this session's next turn (`j`/`k` move, `[`/`]` reorder, `a` add, `e` edit, `d` delete, `s` send now, `r` refresh) |
| `/policy` | Current approval and sandbox policy detail |
| `/approval` | Approval-mode fast-select (`/approval-mode` is an alias) |
| `/sandbox` | Sandbox-profile fast-select (`/sandbox-profile` is an alias) |
| `/themes` | Theme picker (`/theme` is an alias) |
| `/connect`, `/connections` | Provider connection management |
| `/doctor` | In-app diagnostics for the search backend, MCP servers, and providers |
| `/nodes` | Host telemetry of the configured remote Eggwork nodes — state, freshness, and the dimensions each node actually reported |
| `/reload` | Refresh project runtime assets — `/reload skills`, `/reload agents` |
| `/init` | Review a bounded root `AGENTS.md` proposal; press `a` in the preview to publish |
| `/exit` | Quit (`quit` and `q` are aliases) |

To leave CodeGG: `q` in Normal mode, `/exit`, or `Ctrl+C`. **`Esc` never
quits.** It backs out one layer at a time — close a dialog, dismiss the
completion popup, return from Insert to Normal mode, or discard a half-written
prompt — and then stops.

`Esc` never hands a **panel's** selection back to the conversation either. It
is the mode switch: it leaves Insert mode (keeping whatever you typed), and in
Normal mode it does nothing. A panel keeps the keyboard and the selection
until you move the panel selection with `Shift+H/J/K/L`; `Shift+Q` is how you
leave it, and it is how the `/files` box closes.

**Leaving takes two deliberate presses.** `q` and `Ctrl+C` each need a second
press of the same key within three seconds. The first `Ctrl+C` clears the text
you were typing and arms the exit; the line above the input then says which
key to press again (`press ctrl+c again to exit`, `press q again to quit`). If
you do not press it again the message goes away on its own and nothing
happens. A dialog is still dismissed by the first `Ctrl+C`, so a dialog takes
two presses to leave.

Registry metadata — domain, scope, and source exist for palette and help
presentation only. It does not bypass dispatch, observer mode, daemon
authorization, or permission gates.

### The cursor in the composer

The composer shows a caret, and what that caret tells you is where the
keyboard is:

| Where you are | What you see |
|---|---|
| Typing in the composer, `[INS]` mode | A **blinking** cursor at the caret |
| The composer is in `[NOR]` mode | A **solid block** at the caret |
| Another panel has the keyboard | **No cursor at all** in the composer |

The blinking cursor is your terminal's own cursor, so it blinks at whatever
rate your terminal is configured for — some terminals show it steady, and the
`[INS]`/`[NOR]` badge is what tells the two modes apart.

When you select a side panel, the composer stops showing a caret entirely.
That is deliberate: a caret sitting in a box you are not typing in is a lie
about where your keystrokes go, and the `▸` marker on the panel is the
honest one.

### Moving the caret in `[NOR]` mode

In `[NOR]` mode the caret is a thing you can move, and Helix's horizontal
motions are the way to move it:

| Key | What it does |
|---|---|
| `h` | Move the caret one character left |
| `l` | Move the caret one character right |
| `←` / `→` | The same two moves, for people who prefer the arrows |
| `Home` / `End` | Jump to the start or the end of the line |

`h` and `l` work in **both** layouts — the default one and the vim one — so
they are there whichever you use.

The word motions move it by word instead of by character:

| Key | What it does |
|---|---|
| `w` | Move to the start of the next word |
| `b` | Move to the start of the previous word |
| `e` | Move to the last character of the word at the caret |
| `j` / `k` | Move down / up a line, keeping the column |

`j` and `k` only move the caret when your prompt spans **more than one
line**. On a single line they scroll the conversation instead, which is what
they have always done.

### The composer's own keymap in `[NOR]` mode

This is the whole of the `[NOR]`-mode keymap, in one table. It is generated
from the same source as the row above the composer and as the help box, so the
three cannot disagree:

| Key | What it does |
|---|---|
| `w` | Move to the start of the next word |
| `b` | Move to the start of the previous word |
| `e` | Move to the last character of the word at the caret |
| `j` / `k` | Move down / up a line — only when the prompt spans more than one line |
| `v` | Start selecting characters; a caret motion then extends the selection |
| `x` | Select the whole line, and grow the selection a line at a time |
| `d` | Delete the selection |
| `p` | Paste the clipboard at the caret |
| `u` | Undo the last edit |
| `U` | Redo the last edit |
| `i` | Start typing before the caret |
| `a` | Start typing after the character under the caret |
| `o` | Open an empty line below and type in it |

Two rules cover the edges of that table:

- **`j` and `k` need more than one line.** On a single-line prompt they are
  the conversation's, and scroll it.
- **An edit that changes the length of the text drops the selection.** The
  selection remembers *positions* in the text, so once the text moves the
  selection would point at the wrong characters. Editing, undoing and
  redoing all drop it.

### The row above the composer

The one-line row above the composer names whoever holds the keyboard and what
their keys do. When the composer has it, the row is the composer's:

```
a · b · d · e · i · j · k · o · p · u · U · v · w · x · space help
```

- The letters are the composer's own keys, and they are the ones available
  **right now** — `j` and `k` are absent while your prompt is one line long,
  because there they scroll the conversation.
- `space help` opens the help box, where every key above is described in
  full.
- The row does **not** repeat the mode. The badge immediately above the input
  box already says `[NOR]`, `[INS]`, or `[SEL]`, and a second copy of it one
  line below is the same fact said twice.
- In `[INS]` mode the row says `type · Esc normal` instead: the composer's
  keys are not available while you are typing — the letters are text.
- The row is **present** in both modes, so `Esc` does not move the composer
  up or down. Only its text changes.

When a **panel** has the keyboard, the row names that panel's keys instead.

When the **editor** has the keyboard the row is **absent**. The editor has a
keymap of its own — `space ?` opens it in the editor's bottom-right corner —
and a row of keys in two places at once is how a keymap becomes a lie. The
space that row would have taken is given back to the document.

### Undoing, and the two insert transitions

`[NOR]` mode can undo what you typed, and redo it:

| Key | What it does |
|---|---|
| `u` | Undo the last edit |
| `U` | Redo the last edit |

Both say what they did, in both directions: "Undid the last edit" when there was
one, and "Nothing to undo" when there was not. A key that does nothing is never
a mystery.

The undo covers **the composer's own text**. Sending the prompt empties the
composer and clears the history, so an undo can never bring back something you
have already sent. A command you are still typing is ordinary text, and it is
undoable while you are typing it.

A large paste is still undoable. The history is bounded so it cannot grow
without limit, and the bound drops *older* edits — never the newest one, which
is the one you are most likely to want back.

Two keys put the caret somewhere new and start you typing:

| Key | What it does |
|---|---|
| `i` | Start typing **before** the caret, where it already is |
| `a` | Start typing **after** the character under the caret |
| `o` | Open an empty line below and type in it |

`a` moves the caret one character forward — never past the end of its own line
— and `o` opens a new line below the one you are on. `o` is an edit, so `u`
takes the new line back; `a` only moves the caret, so there is nothing to undo.

While a character selection is held, `a` **extends** it, like the other caret
motions do.

### Selecting and the clipboard in `[NOR]` mode

`[NOR]` mode can select text in the composer, and the selection is what the
delete and copy verbs work on:

| Key | What it does |
|---|---|
| `v` | Start selecting characters at the caret; press it again to stop |
| `x` | Select the caret's line; press it again to take the next line too |
| `d` | Delete what is selected |
| `p` | Paste the clipboard's contents at the caret |

While a character selection is held, the caret motions **extend** it — move
with `w`, `b`, `e`, `h` or `l` and the selection grows with you. A line
selection does not: `x` takes one line, and a second `x` takes the next.

To **copy** the selection, press `Space` and then `y`. That is the whole
gesture: `Space` opens the help menu, and `y` in that menu copies what you
have selected to the system clipboard. The `/help` command's menu does not
copy — it is a menu you open to type a query in, so `y` there filters the
list, and so does `y` in the `Space` menu once you have switched it to
filtering with `Enter`.

Every one of these says what it did: the composer tells you how many
characters it copied or pasted, and it says so rather than failing quietly,
so a key that does nothing is never a mystery. The text itself is not
echoed back, because your prompt may hold something you would rather not
see in the log.

### Four keys moved to chords

In `[NOR]` mode the composer now owns a set of letters for its own Helix-style
editing, and four app-level actions that used to live on a letter moved to a
chord. The capabilities are unchanged — only the spelling moved:

| What it does | Was | Is now |
|---|---|---|
| Stop or resume the running turn | `x` | `Ctrl+Alt+X` |
| Open the diff view | `d` (vim layout) | `Ctrl+Alt+D` |
| Open the command palette from Normal mode | `i` (vim layout) | `Ctrl+Alt+E` |
| Focus the sidebar | `a` (vim layout) | `Ctrl+Alt+S` |

All four chords work in both layouts, so you do not have to know which one is
active. The keys `x`, `d`, `i` and `a` still type normally in `[INS]` mode —
they are only spoken for in `[NOR]` mode.

`a` was the one that lost a spelling rather than a capability: the sidebar was
already reachable on `Ctrl+Alt+S` in both layouts, so the vim layout's bare `a`
was a second way to do something you could already do. It is now the composer's
insert-after key.

`i` still takes you from `[NOR]` back to typing. That was already true in the
default layout; in the vim layout it is now the same gesture instead of the
command palette, which moved to `Ctrl+Alt+E`. The Helix editing verbs above —
motions, selection, delete and paste, undo and redo — are all in place.

The slash command is unaffected: typing `/` at the start of the composer still
opens the command palette, exactly as before, in both layouts.

Help is unaffected too: `Space` still opens the help menu, and `?` still does
as well. That menu is also where the copy gesture lives — `Space`, then `y`.

The copy gesture belongs to **that** box and to no other one. The box `/help`
opens is a different box: it is opened to type a search, so `y` there is a
letter you type into the filter, and it is not listed as a verb in it. Both
boxes list the composer's own keys — those are documentation, and a reader who
opens `/help` is exactly the reader who needs to know what `w` does.
### Typing while the agent is already working

You can keep typing during a turn, and pressing Enter does not throw the text
away. If the agent is still working, CodeGG says so and **puts your prompt back
in the composer** for you to send again once the turn ends — the running turn
keeps going, and the status line above the composer keeps showing it (spinner,
elapsed time) rather than flipping to an error.

Nothing is sent on your behalf. The text comes back and you press Enter when you
want it sent. Anything you typed while the submission was in flight is kept
behind it in the draft stash.

A lost connection is reported as a connection problem, not as "the agent is
busy", and it restores your prompt the same way.

## The prompt queue

A prompt you send while a turn is running is kept instead of dropped, and it
goes to the next turn for the same session. `/queue` shows what is waiting.

| Key | What it does |
|-----|--------------|
| `j` / `k`, `↓` / `↑` | Move the selection |
| `r` | Re-read the queue |
| `a` | Add a prompt |
| `e` | Edit the selected prompt in place |
| `[` / `]` | Move the selected prompt earlier / later |
| `d` | Delete the selected prompt |
| `s` | Send the selected prompt to the turn running now |
| `Esc` | Close the box |

The list is in the order the queue is in. Each row shows its status, so a
prompt whose turn is running right now says so rather than looking like it is
still waiting. A prompt you gave a due time carries an `@` marker with the time
left (`@2h`, `@30m`), so you can see at a glance which prompts are waiting for
a clock and which are waiting for a turn. Once a due time arrives the marker
becomes `@·`: the time has passed and the prompt is waiting for the next turn.

### Adding and correcting

`a` opens a line under the list. Type the prompt, then `Enter` — that moves to a
second line where you say **when** it should run. Press `Enter` there to queue
it, or `Esc` to go back to the prompt text without losing it. Leaving the delay
empty queues the prompt for the next turn, which is what almost every prompt
wants. The box is navigable again the moment you commit; you do not have to wait
for the save to finish.

`e` opens the same line with the selected prompt's text already in it, so you
correct a typo instead of retyping. Editing never changes which prompt it is
or where it sits; only the words change.

### Setting a due time

The delay line takes a number and a unit: `30s`, `5min`, `2h`, `1d`. There is no
bare number — `5` is refused rather than guessed, because five seconds and five
minutes are very different instructions to give a machine. The hint on that line
lists the same forms, so you do not have to leave the box to learn them.

A delay that lands at or after the moment the prompt would expire is refused
rather than quietly shortened. Prompts stop being eligible after seven days; the
box tells you the two times rather than scheduling something that can never run.

A due time is a floor, not a promise. The prompt is delivered at the first turn
boundary at or after that moment, and if the daemon is not running when the time
passes the prompt goes on the next turn after it starts. Nothing here claims the
prompt reaches the model at any particular second.

### Typing safely

While a line is open, every letter goes into it. Typing a prompt that contains
`a`, `e`, `d`, `j`, `k`, or `s` is completely safe — the line's hint reads
`Enter next · Esc cancel` precisely so you can see the box has switched into
typing mode. The delay line is guarded the same way, so typing `2d` cannot
delete the prompt you were writing.

If a prompt is too long or the queue is full, the box says so and **keeps what
you typed**, so you can shorten it and save again. It never quietly cuts text
off.

### Reordering

`[` and `]` swap the selected prompt with its neighbour. At either end there is
nothing to swap with, and the box tells you which end you are at rather than
appearing to do nothing.

A prompt whose turn has already started cannot be moved — it is mid-delivery.
The same is true of editing it.

Reordering the queue moves the prompts that are still waiting. One whose turn is
running holds its place while it runs, and if that turn is interrupted the prompt
resumes **where you put it** — it does not jump to the end.

### Sending now

`s` hands the selected prompt to the turn that is running **right now**, instead
of waiting for that turn to finish. It is the one place where the queue reaches
into a live turn rather than starting a new one.

It is a handover, not an override. The prompt goes into the running turn's stream
of messages; the agent reads it the way it reads anything else that arrives
mid-turn, and it may already be part-way through answering something else. What
`s` does **not** do is interrupt, restart, or force anything — if the turn is
already finishing, or has no room for another message, the box says so and your
prompt stays queued exactly where it was.

When the running turn accepts the prompt, the box removes it from the list and
records it as delivered to that turn, so it will not also arrive at the next one.
If the queue cannot record that, the box tells you so plainly rather than
leaving you to guess whether to send it again — check before you do.

If nothing is running, `s` tells you the prompt stays queued for the next turn.
That is not a refusal you need to work around; it is what an idle queue is for.

### What it still cannot do

- **It cannot delete, edit, reorder, or send now a prompt whose turn has already
  started as its own.** The queue refuses it and the box says the prompt is
  being delivered — it does not tell you the prompt expired, because it has not.
  Close the box and the running turn keeps its text.
- **It cannot send a prompt to a turn that has already ended**, and it cannot
  send one for another frontend's turn. Both are refused with the reason.
- **It cannot schedule a prompt for later** or claim a queue that has no owner.
  Those are separate, explicit operations outside this box.

The waiting count also appears on the status line above the composer
(`· 2 queued`), so a queue with nothing running is still visibly not empty.

The box is one session's queue. With another frontend attached to the same
session, that session's queue belongs to whoever claimed it, and a change made
there cannot be made here; the box says so if you try.

## Subagents, jobs, and the tasks overlay

When a turn fans out — several subagents, background jobs — CodeGG gives each
subagent a short friendly name so you can refer to it, and shows **the same
name** in every place it appears.

Names are temporary. One is assigned while the subagent is running and released
when it finishes, so two concurrent runs never share a name, and a name is not
a durable handle for that run. Read it as "the subagent I was watching a minute
ago", not as an identifier you can rely on later.

Each child also has a generation-safe internal identity independent of its
display name and numeric worker task id. The daemon keeps live child state with
its session; durable AgentRuns and jobs are associated through their explicit
run/job IDs. After daemon restart, durable work is rebuilt from its stores and
ephemeral workers are not shown as still running.

**Every named agent also has a character.** The same curated pool each name
comes from describes it — Greg is a confident generalist who changes three
unrelated files before finding the missing environment variable; Susan is blunt
about when she is the wrong tool. That description rides along with the name
onto the row wherever it appears, so a fan-out reads as a team rather than as a
list of `explore` calls.

A one-line row has room for the character *or* the task, not both, so it shows
the character and keeps the task as its fallback. Opening the subagent gives you
both: the tab shows the **Task** and the **Agent** side by side. Subagents started
before this existed have neither, so they show the task exactly as they always
did.

Five surfaces show this, and all five read one projection:

| Surface | Open it with | What it shows |
|---|---|---|
| Tasks overlay | `Ctrl+T` in `INS` or `NOR` | Todos, subagents, and jobs together |
| Session panel | the sidebar — `Ctrl+B` toggles it, `Shift+L` / `Shift+→` in `NOR` gives it the keyboard on the default layout | `Subagents` and `Jobs` sections under Todos |
| Sessions dialog | `/sessions` | The session list plus its live `Subagents` and `Jobs` sections |
| Working strip | always on, above the input box | One row per in-flight subagent |
| Buffer tabs | `Shift+N` / `Shift+P` / `Shift+X` | One subagent's transcript per tab, in the main buffer |

`Ctrl+T` works in **both** modes — the app boots in `[INS]`, and a chord that
first needs an `Esc` is a chord most people never find.

`Shift+N`, `Shift+P`, and `Shift+X` are `[NOR]`-only. In `[INS]` they are
ordinary capital letters, so composing a word containing `N`, `P`, or `X` never
triggers them — and neither does any other capital letter. `Shift+F` (project
picker) and `Ctrl+O` (Workspace view) work the same way round: the shifted
letter is `[NOR]`-only, the chord is live in both modes.

### The tasks overlay

`Ctrl+T` opens a single overlay listing **todos, then subagents, then jobs**,
each under its own heading. A group with nothing in it is left out entirely —
there are no empty headings. `j`/`k` move, `Enter` opens a subagent as a tab,
`Enter` opens a child transcript or job detail; `i` inspects a durable child run or linked job by its stable ID. The dialog shows job attempt history, and `Esc` closes it. With nothing in any group the overlay says
`Nothing pending` rather than opening to a blank strip, so a quiet session still
confirms the key did something.

It shows what the turn is *waiting on*, not what it has done: finished
subagents and completed jobs are filtered out. When a job reports a state this
build does not recognise it is shown rather than hidden, because showing
finished work costs a glance and hiding live work costs you the thing you
asked for.

Job detail is keyed by `job_id`. It shows the current daemon record and bounded
attempt metadata; press `x` to request cancellation when the job is live. The
dialog refreshes from the daemon after the request and reports refusal or an
uncertain outcome without claiming the job stopped. `/sessions` job rows and
the sessions sidebar open the same detail surface. On a child with a linked
run or job, `i` opens that linked detail while `Enter` keeps opening the child
transcript. Read-only observers can inspect jobs but do not get the cancel
key. `/sessions` opens on the
active session and clears work rows when you select a different session whose
projection is not loaded, showing that live work is unavailable.

Opening it from `[INS]` switches to `[NOR]`, because the overlay's own keys —
`j`/`k`, `Enter`, `Esc` — are `[NOR]` gestures and must never be claimed while
you are composing. Closing it hands `[INS]` straight back, so peeking at pending
work never ends composition mode. You can also press `i` to go back to typing
with the overlay left open.

Above the input, the one-line todo strip stays on screen at all times. It has no
expanded form of its own any more: the full list is the overlay's first group,
next to the subagents and jobs, because "what is this session waiting on" is one
question and answering it in two places meant the two could disagree.

### When the agent is waiting on other work

Press `Ctrl+T`. The overlay lists the todos, the subagents that are still
running, and the live jobs the session is waiting on, in that order — so a long
fan-out looks like a wait rather than a hang. Live jobs count here too, which
is why a job running under a finished turn is still visible.

That is the only place this state appears. The main buffer has no status row
of its own: the line above the input carries the composer's `space help`
chord, a latched failure, and an armed exit confirmation — nothing else.

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

### Talking to a subagent

`/subagent-send` points your next prompt at one running subagent.

```
/subagent-send            address the subagent in the tab you are looking at
/subagent-send Sally      address the running subagent named Sally
/subagent-send 42         address task 42 — exact, and never ambiguous
/subagent-send off        clear it again
```

While it is armed the badge beside the input reads `[TO Sally]` instead of
`[LLM]`, and the input placeholder says what the prompt is for. The tab strip
stays where it is — this does not take the keyboard away from the input box.

**What actually happens:** your message is handed to **the agent running this
turn**, addressed to Sally by name, with the task she was given. That agent
decides whether to pass it on. Nothing is delivered to the subagent directly,
and the confirmation toast says exactly that rather than telling you it was
"sent". If you need a subagent to actually do something, that is what the
parent agent is for.

Two rules worth knowing:

- **A reply can only be addressed while a turn is running**, because the turn
  is what carries it. With nothing running, the prompt stays in the box and you
  are told why — it never quietly starts a new turn instead.
- **A finished subagent drops the address.** If Sally finishes while you are
  typing, the badge goes back to `[LLM]`, so what you have written is treated as
  an ordinary prompt instead of being steered at work that is already done.

If a reply is refused, your text comes back to the input box so you can send it
again — nothing durable keeps it for you.

### Session panel sections

The Session panel lists `Todos`, then `Subagents`, then `Jobs`, so the three
"what is this waiting on" surfaces sit together. `j`/`k` and the arrow keys
move through them, `Space` collapses a section, and `Enter` (or a click) acts on
the selected row:

- a **subagent** row opens that subagent's transcript as a tab in the main
  buffer, the same tab `Ctrl+T` would have opened;
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

**Terminals need the local frontend.** They work when CodeGG's TUI is talking
to your daemon — plain `codegg`, or `codegg daemon attach` over the local
socket. They do not work through the remote WebSocket attach
(`codegg attach http://<host>:<port> --token …`): that connection carries chat,
but no interactive-process traffic, so every terminal command there answers
with a message naming the transport rather than pretending the daemon is down.
Everything else on that frontend — chatting, tools, permissions — is unaffected.
To open a shell, run `codegg` (or `codegg daemon attach`) on the machine running
the daemon.

The box opens **already taking your keys**, so you can type straight away.

Once it is taking your keys, every key is your program's — including `H`, `J`,
`K` and `L`, and `Shift`+arrows. Those are the panel-docking gesture everywhere
else, but a terminal that owns the keyboard is not a viewer, so the gesture is
turned off while you are typing. `i` is only a focus gesture while the surface is
in viewing mode (`NOR`); in the box the surface is never in `NOR` to begin with.

Output keeps up on its own: the TUI reads the PTY on its own refresh tick while the
terminal is on screen, so anything the command prints appears without you doing
anything. That includes after you hand the keyboard back to the main buffer with
`Shift`+an arrow — a docked terminal keeps producing output while it is merely
visible. You do not need `/terminal-resume` to see live output — it is a recovery
affordance for a view whose read fell behind, not a step in the normal flow.

The `[NOR]` / `[INS]` badge above the main input shows where your keystrokes are
actually going, whichever surface has them. It reads `[INS]` while a terminal is
taking your keys — in the box or in the docked panel — and `[NOR]` once `Esc` has
handed them back, so the badge never claims you are reading a shell you are
typing into.

There are two modes, and each key means exactly one thing:

- **INS** — the process owns the keyboard. Everything you type, plus `Enter`,
  `Backspace`, `Tab`, the arrow keys and `Ctrl-C`/`Ctrl-D`, goes to the
  terminal. The cursor is drawn where the shell is accepting the next
  character, and it blinks while it is there — so a glance is enough to see
  that the terminal is live rather than frozen. Press `Esc` to stop typing and
  read.
- **NOR** — the reader owns the keyboard. `hjkl` and the arrows move, `Ctrl+u`
  and `Ctrl+d` page, `e`/`b` jump by word, `v` selects by character, `x` selects
  whole lines, `d` deletes the selection, `space+y` copies it, `C` clears the
  screen, `D` folds in diagnostics. Press `i` or `Enter` to start typing again.
  Press `Esc` to close and detach — or to drop an armed selection first.

While reading, the cursor sits on the last line the process actually printed —
the prompt after a short command, the last line of the output after a long one.
`Esc` puts it there for you, so the first `k` moves from where you were working
rather than from the bottom of an empty screen. The reader cursor does not
blink: it is what `x`, `v` and `d` act on, so it stays put and stays legible.

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
one.

The line of keys above the composer describes whatever is holding your keyboard,
so it appears while the terminal panel has it and leaves with it: once you have
handed the keyboard back to the conversation, the terminal's own verbs are gone
from the frame, and the row they occupied goes back to the transcript. Press
`Shift`+`←` (or whichever edge the panel is on) to take the keyboard back, and
the line returns.

A new shell shows its prompt as soon as it prints one — you do not have to press
a key to see it. `C` clears the screen by asking the shell to clear itself, so
what comes back is a fresh prompt from *your* shell, with your `PS1`, rather than
an empty panel; the process keeps running and so does anything it was doing.
`/terminal-clear` does the same. A terminal whose process has already exited has
no prompt left to print, so clearing one just empties it.

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
`W`/`B`/`E` jump by WORD, `v` selects by character, `x` selects the whole line
(press again for one more), `d` deletes the selection, `y` and `space y` copy
it, and `p`/`P` paste it back — the same keys `/terminal` uses in its viewing
mode. `m` is Helix's match mode: `m m` jumps to the mate of the bracket under
the cursor, `m s`/`m d`/`m r` surround the selection, and `m a`/`m i` select a
text object — see [Match mode and text objects](#match-mode-and-text-objects).
`i`/`a`/`I`/`A`/`o`/`O` enter `INS`, where typing goes into the file and
`Esc` returns. `u`/`U` undo and redo, the Helix pair, and `Alt+u`/`Alt+U` walk
the same history one entry at a time — see
[History gestures](#history-gestures). In `NOR` anything the
buffer does not claim types into the input box at the bottom, so you can talk
to the agent without leaving the file. `Esc` never closes the editor; `:q` does.

Two chords work in either mode, because they are round trips rather than
verbs: `Ctrl+S` saves and `Ctrl+R` resyncs the replica after a transport
failure. Each chord has exactly one owner. In particular `U` is redo and
`Ctrl+R` is resync; they are not two spellings of the same thing. Helix gives
`Ctrl+s` and `Ctrl+r` different Insert-mode meanings — an undo checkpoint and
a register prompt — and CodeGG declines both; see
[Insert-mode chords](#insert-mode-chords).

### History gestures

`u` undoes and `U` redoes. `Alt+u` and `Alt+U` are Helix's `earlier` and
`later`: they walk the same history, one entry back and one entry forward.

The difference is what happens at the edge. `u` with nothing to undo, and `U`
with nothing to redo, do nothing and say nothing — they always have. `Alt+u`
at the oldest change says **no earlier change**, and `Alt+U` at the newest says
**no later change**, in the status row. A key that silently does nothing is
the one failure this editor is built to avoid, and these are the keys where
you are most likely to be at the edge without knowing it.

The history is **one** list with a cursor in it, not two stacks. An entry
never moves to a different pile, so undo and redo are the same movement in
opposite directions. While the cursor is not at the newest change, the status
row says where it is — `history 1/3` — so you can see that you are standing
in the middle of your own history. At the newest change it says nothing,
because there is nothing to disclose.

`Alt+u` and `Alt+U` are `NOR` gestures. In `INS` they are inert: the `INS`
catch-all covers unmodified and `Shift`-modified keys only, so an `Alt` chord
reaches the input box rather than vanishing.

### Insert-mode chords

`INS` has chords of its own, and they are the Helix ones. A chord may mean one
thing in `NOR` and another in `INS` — `Ctrl+d` pages down in `NOR` and deletes
one character forward in `INS` — and each still has exactly one owner.

| Chord | What it does in `INS` |
|---|---|
| `Ctrl+h` | delete one character backward |
| `Ctrl+d` | delete one character forward |
| `Ctrl+j` | insert a newline |
| `Ctrl+k` | delete to the end of the line |
| `Ctrl+u` | delete to the start of the line |
| `Ctrl+w`, `Alt+Backspace` | delete the previous word |
| `Alt+d`, `Alt+Delete` | delete the next word |
| `Ctrl+x` | open the completion popup, staying in `INS` |

The two word deletions stop at the line boundary: `Ctrl+w` in column zero
deletes nothing rather than reaching into the line above, and `Alt+d` at the
end of a line does not eat the newline.

Everything you type in one go in `INS` is **one** undo step. Leaving `INS`
closes the burst, so a single `u` takes back the whole run rather than its
last keystroke. A run ends early if you delete something, or move the cursor
and type somewhere else — then the parts are separate steps, because they are
separate edits. A reload throws the burst away with the rest of the local
draft.

To reload from disk and throw the local draft away, use `:e`. It used to be `R`,
which now means something else — see [Change verbs](#change-verbs).

The cursor is a block in `NOR` and a thin bar in `INS`, so you can always see
where you are. `Ctrl+d` pages down and continues to the end of the file when a
half-page would land past it.

### Change verbs

| Key | What it does |
|---|---|
| `p` / `P` | paste after / before the cursor or selection |
| `y` | copy the selection |
| `r` + char | replace the selection, or the character under the cursor |
| `R` | replace the selection, or the character under the cursor, with the clipboard |
| `~` | flip the case of the selection, or of the character under the cursor |
| `` ` `` / `` Alt-` `` | set that text to lower / upper case |

`y` and `p` together are a complete move-text loop: select, `y`, move, `p`.
`y` behaves exactly like `space y`, including the toast — it is the same code
path, not a second implementation.

`r`, `R` and the three case keys all act on *the selection* when one is held
and on *the character under the cursor* when none is. On a single character
the two are the same thing, which is why `~` flips one letter when nothing is
selected and a whole phrase when something is.

Every one of these verbs acts on **all** held selections at once, and does it
as a single undoable change. See [Selecting more than one region](#selecting-more-than-one-region)
for how you get more than one in the first place.

`y` over several selections copies them in reading order into the clipboard as
one piece of text, so the order you see on screen is the order you get.

A buffer you cannot write refuses to **grow** a selection: `v` and `x` report
that they were refused instead of arming a highlight you cannot do anything
with. It also refuses the verbs that would rewrite text — `&`, `Alt-(`, `Alt-)`,
`o` and `O`. Moving the cursor still works, as do `v`, `x`, `C` and the merge
keys: you can always read and select in a read-only file, you just cannot change
it.

`o` opens a line **below** the cursor's line and `O` opens one **above**, and
both hand you the keyboard: you are typing into the line they opened, with the
cursor on it. The line is a real document edit, so a read-only buffer refuses
both and says so.

The line `o` opens and the word you type into it are **two** undo steps: `u`
takes back the word, and a second `u` takes back the line. The newline that
ends the opened line sits *after* the text you type, so the two are not one
contiguous change and cannot be one step. Everything you type in one go in
`INS` is still **one** step — see
[Insert-mode chords](#insert-mode-chords).

`~` flips per character, so `~` on `abC` gives `Abc` and a second `~` gives
back `abC`. `` ` `` and `` Alt-` `` **set** rather than flip: `` ` `` on `Abc`
gives `abc` and pressing it again does nothing, which is what tells the two
apart. Characters with no case — digits, punctuation, CJK — are left alone by
all three, and text that is already the case you asked for reports nothing
rather than pushing an undo step for no change.

`r` shows its indicator while waiting, like `f` does, because the character it
wants is the next keystroke. A key that cannot be that character — an arrow,
`Esc`, `:` — is passed through and does its normal job instead of being eaten.

**Paste reads the system clipboard**, and CodeGG keeps no editor-local register.
`y` and `space y` are the same write. That means `p` pastes whatever is on the
OS clipboard, including text you copied from another application — which is
Helix's own behaviour, and is why `R` is "replace with the clipboard" here
rather than "replace with a register". A clipboard the system refuses, and a
clipboard holding nothing, are reported differently: with nothing on the
clipboard, `p` says so rather than doing nothing in silence.

The space menu carries the same family, so a reader looking for "the
clipboard keys" finds them in one place: `space y` copies every selection,
`space Y` copies **only** the main selection, and `space p`, `space P` and
`space R` are the paste and replace verbs. `space Y` is the one that differs
from the bare keys: with several regions held, `y` takes all of them and
`space Y` takes just the main one. With nothing selected, `space Y` says
"Nothing to yank" rather than putting an empty string on the clipboard.

### Selecting more than one region

Once you can build a set, every verb above applies to all of it at once. These
are the keys that build and reshape the set.

| Key | What it does |
|---|---|
| `C` / `Alt-C` | add a cursor on the next / previous line |
| `%` | select the whole file |
| `S` | split each selection into one selection per regex match |
| `K` / `Alt-K` | keep / drop the selections whose text matches a regex |
| `;` | collapse to a single cursor at the main selection's head |
| `,` / `Alt-,` | keep only / remove the main selection |
| `(` / `)` | move which selection is the main one |
| `Alt-(` / `Alt-)` | rotate the selections' **contents** around the set |
| `Alt-minus` | merge every selection into one |
| `Alt-_` | merge only selections that touch; gaps stay |
| `&` | pad every selection out to the widest one's width |
| `Alt-;` | flip each selection's start and end |
| `Alt-:` | make every selection run forwards |

`(`/`)` move the main selection and change no text. `Alt-(`/`Alt-)` move the
text and leave every selection where it is. They are the easiest pair in this
list to confuse, and they are the only two pairs here where a mistake corrupts
the buffer rather than just surprising you.

`Alt-minus` and `Alt-_` differ the same way: `Alt-minus` bridges the gaps
between your selections, `Alt-_` only merges selections that are already
touching.

`C` adds a cursor rather than moving one, so one press gives you two, two give
you four, and the cursor stays on the main selection — pressing it near the end
of a file adds only the cursors that had a line to move to, and says so.

`&` measures in **display columns**, not bytes, so a tab and a CJK character
count for what they occupy on screen. It pads after each selection, which
equalises their widths and lines up whatever comes next. A selection wider than
the 32 KiB measurement budget is measured over a prefix, so its padding is
approximate — and `&` tells you that when it happens rather than leaving you to
guess why one line looks wrong.

### The regex keys

`S`, `K` and `Alt-K` need a pattern, so they open the `:` command line already
filled in with `split-regex `, `keep-regex ` or `remove-regex `. Type the
pattern, press `Enter`, and `Esc` abandons it with nothing changed.

They are commands rather than a private prompt because the `:` line already
knows how to take text, how to show you what you are typing, and how to abort.
The line you get is the line you could have typed by hand, and the pattern is
listed in the palette. A pattern that will not compile, and one that matches
nothing, are both reported and both leave your buffer alone.

### The preview keys

`=` and `space r` ask the language server what it **would** change, and — when
the whole change is known — offer it for review.

| Key | What it does |
|---|---|
| `=` | the format preview for this document: the diff, and how many edits that is |
| `space r` | the rename preview for the symbol under the cursor: the old and new names, the files that would change, and the edits in each |

`space r` needs a new name, so it opens the `:` command line already filled
in with `rename-preview `. Type the name, press `Enter`, and the preview
arrives. `Esc` abandons it with nothing changed. The line is the same one you
could have typed by hand — `:rename-preview newname` — and it is in the
palette like the others. An empty name is refused before a request is sent,
and so is one longer than the server's limit; the refusal names the limit.

**A preview you can apply opens the review.** The diff in the review is the
change that would be written, and the review is where you decide:

- `[a]pply` sends the change through the daemon's checked apply path — the
  same path `/lsp-preview-apply` uses, with the same revalidation of the file
  it was computed against;
- `Esc` rejects it. Nothing is written.

A preview that **cannot** be applied stays a preview and says so: a truncated
change, a change with no edits, and a change too large to apply are all
previews still. The daemon's own refusals — unsaved edits in the file, or a
file that changed since the preview was built — appear verbatim in the review.

After a successful apply the editor reloads the file it wrote, unless your
buffer holds edits the editor has not saved; then it keeps them and tells you
that the file changed on disk.

`space a` — the code action — asks the server what code actions it would offer
**at the cursor**, and shows each one with the diff of its own edit. It is a
**choice**, not a single change, so the panel is the picker: `j` and `k` move
the selection, `Enter` opens the change review on the action you selected,
and `Esc` closes the panel. See
[Code actions: the picker is the panel](#code-actions-the-picker-is-the-panel).

### Code actions: the picker is the panel

`space a` asks the server what code actions it would offer at the cursor, and
the **panel** is where you choose one. It is a list, not a single change, so it
cannot be a preview the way `=` and `space r` are.

| Key | What it does |
|---|---|
| `space a` | ask the server for the code actions it would offer at the cursor |
| `j` / `k` | move the selection through the offered actions |
| `Enter` | open the change review on the selected action |
| `Esc` | close the panel |

The review is the same one the preview keys open, and the change in it is the
change that would be written. `[a]pply` sends it through the daemon's checked
apply path; `Esc` rejects it.

The panel shows eight actions at a time and the window follows the selection,
so the action you are about to apply is always the one you can read. When the
server offered more, the panel says how many.

**One request, many previews.** The editor asks the server once, and computes
the diff of each action's own edit locally. It does not ask once per action,
and it does not ask for a list and then ask again for the one you picked: an
action's position in a list is only meaningful for the list the server
returned in that moment, and a second request may return a different list. The
action you pick is therefore the action that was previewed.

**Not every action can be applied, and the panel says which and why.**

| The action… | What the panel says |
|---|---|
| carries an edit | *ready to apply* |
| carries a command | the server attaches a command, and this editor does not run commands |
| carries no edit at all | the server offered no edit, so there is nothing to apply |
| is disabled by the server | the server disabled this action, with the server's own reason |
| has a change too large to apply | the change is too large to apply |
| has a change that could not be previewed completely | the preview is incomplete, so there is nothing to apply |
| arrives after the result reached its preview bound | the result reached its preview bound, so this action has no preview |

An action that carries a **command** is never applied here, even one that also
carries an edit. The LSP specification has the client apply an action's edit
*and then* its command; applying the edit alone would apply half of what the
action means, so the whole action is refused and the reason is on the screen.

### Multi-key gestures

`gg` (buffer start) and `space y` (copy the selection) each take two keys. While
the first one is held, the status row shows which one: `g` or `space`. The
indicator is there because a prefix that shows nothing is a prefix you cannot
predict. `r` is a gesture too, for the same reason: it shows `r` while it waits
for the character to write.

`m` is the one mode that takes **three** keys: `m` names a command, and the
command may name an argument. It shows `m`, then `m s`, then `m r (` — the last
of which is the *second* half of the replace, and it has to say so, because the
two halves are otherwise the same word on screen. See
[Match mode and text objects](#match-mode-and-text-objects).

The two gestures finish differently, on purpose. After `g`, any key belongs to
the chord — `g j` does nothing, because `g j` is not a gesture. After `space`,
an unmapped key is just a normal-mode key: `space j` moves down. After `m`, an
unmapped key is a normal-mode key too: `m z` is not a gesture, so `z` is
what you asked for. `[` and `]` are Helix's unimpaired prefixes; they show their
indicator too, and any key after them still does what it normally does. A
prefix never survives leaving the editor, opening a different file, reloading,
or moving focus to the file tree.

`space` is a whole menu, and it is the one prefix whose second level is a
list rather than a single key: the file browser, the editor's command
palette, the clipboard family, and the keys that say why they cannot run. See
[The space menu](#the-space-menu). Every key in it is either bound or states
its reason, so there is no Helix space gesture that does nothing at all.

### The bottom row

The editor has **one** row of chrome, at the bottom. The file's path is on its
left; the status cluster is on its right. The cluster reads left to right:

| Part | What it says |
|---|---|
| the persistent signals | whether the file has unsaved changes, which root the edit is confined to (`[project]` or `[global]`), whether the file is writable, and any caveat about the rendering itself |
| the transient disclosure | what your last keystroke did — the find that landed, the search that matched, where the undo cursor is standing |
| the notice | an error, if one is being surfaced |
| the armed prefix | the key you are halfway through, at the far right of the row |

The armed prefix is **last** on purpose. It is the one thing that comes and
goes on its own, so it gets the one place on the row nothing else competes
for, and "did my `g` register?" is a question that has to be answered at a
glance.

When the row is too narrow for all of it, something has to give — and what
gives is decided by how much it would cost to lose. The busy marker goes
first, then the unsaved-changes word, then the writability marker, then the
scope badge, and only then the disclosure. A **caveat** — "line too long",
"highlighting truncated" — outranks all of them, because dropping a caveat is
not a smaller truth but a misleading one: a truncated line that reads as the
whole line is worse than a missing footnote. A part that is still too wide on
its own is cut from the **right**, so the part that answers the question
survives.

### The `:` line

`:` opens a command line **on the bottom row of the editor**. The file's path
is erased while the line is open, because you are looking at what you are
typing rather than at where the file is, and the commands that match what you
have typed are drawn on the row directly above.

```
:goto 50
```

- `:50` and `:goto 50` are the same command. The bare number is the spelling
  you reach for; the named one is there when you want to be explicit.
- The line number is one-based — `:1` is the first line. `:0` is refused
  rather than quietly taken to line one.
- As you type, the row above lists the commands that match. `Tab` or `Enter`
  takes the highlighted one.
- The match list is drawn **over** the text rather than in a row of its own,
  so opening the line does not reflow the document and move your cursor out
  from under you.

### Scrolling and pages

The editor keeps a **five-line margin**: the page stands still while the
cursor walks the last few rows, and moves only when the cursor would leave
the margin. A view that scrolls the instant the cursor touches the bottom row
is a view you cannot read — the text you are on moves out from under you
mid-sentence. At the very top and the very bottom of the file the margin
gives way, so the cursor can sit on the first and last lines.

| Key | What it does |
|---|---|
| `PageDown` / `PageUp` | a **full** page down / up |
| `Ctrl+d` / `Ctrl+u` | **half** a page down / up |
| `zc` / `zt` / `zb` | centre the view on the cursor / put its line on the top row / on the bottom row |
| `zj` / `zk` | scroll the view a line down / up, leaving the cursor where it is |

`z` is **view mode**: the page moves and the cursor stays where it is, so you
can look at another part of the file without losing your place. `zc`, `zt` and
`zb` align the view; `zj` and `zk` scroll it a line at a time. The next cursor
movement re-attaches the view.

The **scrollbar** on the right edge of the editor shows where you are in the
file. Its thumb is sized by how much of the file is visible, so it maps the
document rather than pointing at the cursor.

### Select mode

`v` enters **select mode**, and the badge above the input box reads `[SEL]`
while you are in it. In select mode a movement **grows** the selection;
outside it, a movement **drops** it. A selection that outlives the movement
that made it reads as "something is still selected" long after you thought it
was gone. `v` again, or `Esc`, leaves select mode and drops the selection.

`e` and `b` **select the word** they land on, so holding `e` walks the file
with each word picked out in turn — and `d` right after deletes that word. `w`
does not select: it is a positioning motion.

### Goto mode

`g` is a mode, not one chord. Its status row reads `g` while it is armed:

| Key | What it does |
|---|---|
| `gg` | start of the file |
| `gs` | first non-whitespace on the line |
| `ge` | end of the file |
| `gh` | start of the line |
| `gl` | end of the line |
| `gj` / `gk` | down / up one line |
| `gt` / `gc` / `gb` | top / middle / bottom **of the screen** |

`gs` is the one worth learning: `0` is the only key that reaches a line start, so
in an indented file every placement went through column zero.

`gt`, `gc` and `gb` move the **caret** to the top, the middle and the bottom
of your screen. They know where your screen ends because the editor measures
its own pane and publishes the height, rather than guessing at it.

`gj`/`gk` are Helix's *textual* line motions, against `j`/`k`'s *visual* ones.
CodeGG wraps no lines — one screen row is one buffer line — so the two are the
same move, and `gj`/`gk` exist for the muscle memory rather than for a
difference this surface can show.

Not bound, on purpose rather than by omission:

<!-- unbound: the keys below are deliberately not bound -->

- `gf`, `ga`, `gm`, `gn`, `gp` — files and alternate buffers. One document is
  open at a time.

A list like this is **checked**. The build compares the keys declared here
against the editor's own tables, so a key that gets implemented without leaving
this list is a defect the build reports. It cannot go stale in the direction
that matters — a document that says a key does not work is worse than one that
omits it.

The language-server jumps are bound, through the space and `g` menus:

| Key | What it asks the language server |
|---|---|
| `space k` | hover at the cursor |
| `space s` | this document's symbols |
| `space S` | workspace symbols, searched for the word at the cursor |
| `space d` | open the diagnostics panel |
| `space h`, `gr` | references |
| `gd` | go to definition |
| `gi` | go to implementation |

`gy` — go to type definition — says why it cannot run rather than doing
nothing: no language server implements it here, so the editor tells you that
instead of leaving the key dead.

Three more language-server features are bound as **previews** rather than
jumps: `=`, `space r` and `space a` show what a format, a rename or a code
action *would* change, and open the review when the whole change is known and
can be applied. See [The preview keys](#the-preview-keys).

### The space menu

`space` is a menu, and every key in it either does something or says why it
cannot. Nothing in it is dead.

`space ?` opens the editor's **keymap box** — the editor's own catalogue of
its keys, in its bottom-right corner. It is a box rather than a row because
the keys are a list, and a list on a row is a list that has to be truncated. It
covers the corner of the text rather than taking rows away from it, so
opening it does not move the document under you, and `Esc` closes it.

| Key | What it does |
|---|---|
| `space f` | open the workspace file browser |
| `space ?` | open the editor's own command palette — the `:` list, filtered as you type |
| `space r` | open the rename prompt — the `:` line, pre-filled — and preview the rename |
| `space a` | ask what code actions the server would offer at the cursor, and pick one |
| `space y` | copy every selection |
| `space Y` | copy **only** the main selection |
| `space p` | paste after the selection, or after the character under the cursor |
| `space P` | paste before the selection, or before the character under the cursor |
| `space R` | replace the selection, or the character under the cursor, with the clipboard |

`space f` opens the same browser `/files` opens, and the browser names the
workspace root it is browsing. If the active tab has no workspace root — or
no canonical project/workspace binding, which is what the daemon resolves the
listing from — the editor says so instead of opening a browser with nothing
in it. `space f` used to be a second name for `space s`; it is now the
browser, and the two are different requests.

`space ?` is the editor's own list of what it can do. Bare `?` still opens
the general help; `space ?` is the editor's catalogue.

`space r` and `=` show what the language server **would** change, and open the
review when the change can be applied: the rename preview lists the files a
rename would touch, and the format preview shows the diff.
See [The preview keys](#the-preview-keys).

These seven say why they cannot run rather than doing nothing:

| Key | Why |
|---|---|
| `space F` | this build has one workspace root, and `space f` opens it |
| `space b` | one document is open at a time, so there is no list to choose from |
| `space j` | the editor keeps no visited-position history |
| `space D` | no server capability backs a separate workspace-diagnostics request; `space d` opens the panel that exists |
| `space c` | nothing here knows a comment syntax for the buffer you are editing |
| `space C` | the same, for block comments |
| `space '` | the pickers are not fuzzy, so there is no query to recall |

The function keys do the same jobs and keep working: F2 diagnostics, F3 hover,
F4 definition, F5 references, F6 document symbols, F7 workspace symbols.

### Match mode and text objects

`m` is a mode, and it is the only **two-level** one: `m` names a command, and
for most of those commands the key after it names an argument. The status row
says which half you are in — `m`, then `m s`, then `m r (` for the two halves
of the replace.

| Gesture | What it does |
|---|---|
| `m m` | jump to the mate of the bracket under the cursor |
| `m s (` | surround the selection with a pair |
| `m d (` | remove the pair around the selection, keeping the contents |
| `m r ( [` | replace the outer pair around the selection with another pair |
| `m a w` | select **around** the word under the cursor |
| `m i w` | select **inside** the word under the cursor |
| `m i (` | select the inside of the pair under the cursor |
| `m i m` | select the inside of the innermost pair under the cursor |

The objects are `w` (word), `W` (WORD — the whitespace-delimited run), `p`
(paragraph), a delimiter pair such as `(`, and `m` (the innermost pair of any
kind). `a` takes the object **with** its boundary and `i` takes it **without**:
`m a w` is the word plus the space after it, `m i w` is the word alone. `m i (`
is the text between the brackets; `m a (` is the brackets too.

A character outside that vocabulary is not an object, so it is handed back to
the input box rather than swallowed. `m i z` types a `z`.

**`m m` is a delimiter match, not a syntax match.** It counts brackets over the
bytes around the cursor, so it finds the mate of `(`, `[`, `{`, `<`, `'`, `"`
and `` ` ``, and it does not know what your language calls a string. When it
cannot answer it says which of the two things went wrong — **no bracket under
the cursor** when there is no delimiter there, and **no matching bracket** when
the delimiter has no mate. The two are different answers and both are worth
saying: the first means *move the cursor*, the second means *the code is
unbalanced*.

**`m r` replaces the outer pair.** On `(a(b))`, `m r ( [` gives `[a(b)]`, not
`[a(b])` — the inner pair is left alone, so the nesting you wrote survives the
edit. `m d` is the other way round: it removes the pair **immediately** around
your selection, so unwrapping `b` in `(a(b))` gives `(a)`.

The three surround gestures work on your selection, or on the character under
the cursor when nothing is selected. `m s` leaves the surrounded text selected,
so you can go on to delete or replace it as one thing. Each is one undo step,
and each is refused on a file you cannot write.

### Unimpaired navigation

`[` and `]` are modes with the same arm-and-resolve behaviour as `g`. While one
is held, the status row reads `[` or `]`.

| Key | What it does |
|---|---|
| `]p` | first non-whitespace byte of the next paragraph |
| `[p` | first non-whitespace byte of the previous paragraph |
| `]Space` | insert a line below the cursor's line |
| `[Space` | insert a line above the cursor's line |

A *paragraph* is a run of non-blank lines; a *blank* line is one holding nothing
but spaces and tabs. So `]p` crosses the blank run and lands on the next
paragraph's first non-blitespace byte, skipping indentation. Runs of several
blank lines are one separator, not several.

Both motions either move or say why they did not — `]p` at the last paragraph
reports that rather than standing still, because a key that quietly does nothing
is indistinguishable from a dropped keypress.

`Alt-.` repeats them. Helix documents `Alt-.` as "repeat last motion (`f`, `t`,
`m`, `[` or `]`)", so `]p` is repeatable and `]Space` deliberately is **not**:
an edit is not a motion, and `]Space` followed by `Alt-.` inserts one line, not
two. The useful pairing is `]Space` then `]p`, which splits the paragraph you
are standing in.

`]` `Space` is Helix's `add_newline_below`, not `o`. The difference is visible:
`o` opens the line *and* hands you the keyboard, while `]Space` inserts the line
and leaves the cursor exactly where it was, so you keep reading from the same
place. On a read-only buffer the two `Space` keys are refused — they are the
only unimpaired keys here that write — while `]p` and `[p` still work.

Not bound, because nothing in this codebase can answer them yet:

<!-- unbound: the keys below are deliberately not bound -->

- `[q`, `[Q`, `[F`, `[f`, `[n`, `[%`, `[i`, `[c`, `[s`, `[t`, `[b` —
  unimpaired's find family: definitions, type definitions, workspace search,
  occurrences, todos, files, configuration.
- `[a`, `[v`, `[w`, `[e` — quotes and words. A half-built `[w` would be worse
  than none, since it would look like `[W` was simply broken.
- `[D`, `]D` — workspace diagnostics. There is no server capability behind a
  separate "all diagnostics" request, so these would only be a second name for
  the panel `space d` already opens.
- `[g`, `[G`, `]g`, `]G` — change navigation. This has no reachable data source:
  the VCS layer reports which files changed, not which lines.

`[d` and `]d` **are** bound: they step to the previous and next diagnostic and
move the cursor there, wrapping at either end.

### Long words

| Key | What it does |
|---|---|
| `W` / `B` / `E` | next / previous WORD start, next WORD end |
| `w` / `b` / `e` | next / previous word start, next word end |

A *word* is alphanumerics, `_`, or any non-ASCII character. A *WORD* is
everything up to whitespace, punctuation included. On `foo.bar` the two differ:
`w` stops at `bar`, `W` skips to the next space. Use `W`/`B`/`E` to cross code
without stopping at every dot and bracket.

### Holding your column

`j` and `k` remember the column you started from. Move down onto a short line
and back up and you land where you began, rather than at that short line's end.
Any horizontal move re-seeds the column; anything that places the cursor
somewhere else — `G`, `gg`, `w`, `e`, `$`, `gs`, a find, a page, a search jump —
ends the run, and the next `j` starts a new one from where the cursor is.

### Finding a character

| Key | What it does |
|---|---|
| `f` + char | jump to the next occurrence of that character |
| `F` + char | jump to the previous occurrence |
| `t` + char | as `f` |
| `T` + char | as `F` |
| `Alt` + `.` | repeat the last find |

These search the whole file, not just the line the cursor is on — `f` jumps
across line breaks. The cursor lands on the character itself, and the scan
starts strictly past the cursor, so `f x` while already sitting on an `x` finds
the *next* one.

`f`, `F`, `t` and `T` show their indicator while waiting, so you can see that
the editor is waiting for a character rather than having dropped the keypress.
`:`, `/` and any non-character key are not swallowed: they still open the
command line, open the search line, or do their normal job. In `INS` a
printable is always text, never a search target.

Search covers a bounded window from the cursor. If the character is not in the
window the status row says so — `no 'z' within the 2 MiB scan bound` — rather
than reporting it as absent, which is a different fact and needs a different
response.

`Alt` + `.` repeats the last find, and any other movement replaces it: after
`f x`, then `j`, then `Alt` + `.` there is nothing to repeat. With no find
recorded it does nothing at all and the keystroke stays available to the
composer.

### Searching inside the file

`/` searches the document while the editor has focus in `NOR`. The line opens
pre-filled with the word under the cursor, and it searches as you type:
`/host` → `/host_status` → `/host_status fn`. Matches are highlighted in the
text and the count shows on the editor's bottom status row.

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
completions at the cursor. In `INS`, `Ctrl+x` asks the same question and
leaves you in `INS`. A popup
lists the suggestions with their kind and detail; `↑`/`↓` (or `j`/`k` in
Normal mode) move the selection, `Enter` applies the selected item, and
`Esc` dismisses the popup without touching anything. Typing anything else
dismisses the popup first and inserts normally.

In `INS` the popup never takes a key that means text. `Enter` inserts a
newline, `↑`/`↓` move the cursor, and `Tab`/`Shift+Tab` move the candidate
selection. Helix's completion menu accepts the candidate on any other
keypress; CodeGG does not, because a printable in `INS` is text, and losing
one to a popup would be a quiet way to lose a character.

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

### Editing in the buffer

The Editor route owns its own keys rather than routing them through the global
binding table, because what a key means depends on whether the buffer has focus
and which mode it is in.

| Key | What it does |
|---|---|
| `Ctrl+E` | focus the buffer (`Esc` returns to the composer) |
| `Esc` | leave insert mode, then drop buffer focus, then leave the view |
| `Ctrl+S` | save the document through the controller |
| `Ctrl+R` | resync the replica after a transport failure |
| `Ctrl+R` (redo) | redo the last undone local edit |
| `R` | reload from disk, discarding the local draft |
| `h` `j` `k` `l` | move by character or line |
| `0` / `$` | start or end of line |
| `gg` / `G` | start or end of buffer |
| `w` / `b` | move by word |
| `PgUp` / `PgDn` | move by half a page |
| `i` `a` `I` `A` | enter insert mode at the cursor or line end |
| `o` / `O` | open a line below or above |
| `x` / `D` | delete a character, or to end of line |
| `dd` / `dw` | delete a line or a word |
| `u` | undo the last local edit |

`Ctrl+S` is the one chord whose meaning changes with the route: on the composer
it stashes the prompt, and here it saves the document. `Ctrl+T` is the other —
it toggles this route's file tree rather than opening the tasks overlay.

## Browsing files

`Ctrl+T` opens a tree pane beside the editor; `/files` opens the same
navigator as a box you can dock into the side panel with `Shift+L`. Both run
the same navigation, so the keys below mean the same thing in both.

| Key | What it does |
|---|---|
| `j` / `k`, `↓` / `↑` | move the cursor |
| `Ctrl+d` / `Ctrl+u` | page down / page up (two thirds of the pane) |
| `Enter` | **the same as `l`**: a directory is entered, a file is opened in the editor |
| `l` / `→` | **peek into** that directory: it opens in place and the cursor moves into it. On a file it opens the file |
| `h` / `←` | go back one level (at the workspace root: collapse in place, otherwise jump to the parent) |
| `i` | type a filter into the pane — the rows narrow as you type |
| `r` | re-list the current directory |
| `Esc` | leave the filter mode, keeping the filter |
| `Shift+Q` | close the box / leave the pane |

`l` **peeks** rather than re-rooting. The directory opens in place and the
cursor moves into it, so the parent and its siblings stay on screen and you
can see where you are. `Enter` is the same motion — it used to expand a
directory in place and leave the cursor alone, which at the edge of the
listing showed an "empty" directory instead of a move.

Only when the listing has run out of room — three levels deep is as far as the
daemon's listing reaches — does the scope slide, and it slides to the
directory's **grandparent**: the shallowest scope in which the directory's
children fit back inside the listing, so you keep the widest context and the
directory's parent stays visible beside its siblings. The path down to the
directory opens with it, so the directory you asked for is on screen with its
own children under it. The workspace root never
changes; the scope is a view of the workspace, not a working directory, so the
panel can go as deep as the tree goes. `h` is the mirror: from inside a
directory it moves the cursor to the parent row and collapses it; from a
top-level row it slides the scope up one level. The pair reads as a horizontal
motion: `l` goes right (deeper), `h` goes left (shallower).

The filter is **panel-local**: `i` opens it, the rows narrow as you type, and
`Esc` leaves the filter mode while **keeping** the filter, so you can go back
to moving through the rows you narrowed down. The mode indicator in the
composer does not change — the filter belongs to the pane, not to the app.

**Where you are, three ways.** The title is a breadcrumb — `workspace › src ›
tui` — and every row is drawn one level further in for each component of that
trail, with a `├─` / `└─` guide to the directory it belongs to. Enter a
directory and the rows below it visibly step in, so a scoped listing no longer
looks like the workspace root it stopped being. Guides are plain box-drawing
characters, so this still reads correctly with `CODEGG_FILE_ICONS=plain`.

The indent is drawn **relative to the scope**, not to the workspace: a scoped
listing draws its own top level at the left edge, and the breadcrumb carries
the absolute depth. A deep scope therefore does not flatten into a wall of
guides.

### Where the keys are

**The line just above the input is where a panel's keys live — including the
keys of a panel you have docked but not yet entered.**

When `/files` is running in the side panel, that line reads

```
▌ Files · Shift+→/Shift+L in · j/k move · ^d/^u page · l/→ open · …
```

and once the panel has the keyboard the front of it flips to `Shift+← back`. The
gesture is the one for wherever your panel is docked — see the table above — so
a left-column panel says `Shift+← in`. The `l/→` and `enter` you see there are
the file browser's own keys, not a generic panel description: the box, its
panel, and this line are written from one source, so they cannot drift apart.

That source is the box itself. Every box that can dock says its own keys now, not
just the information panels: `/files` says `l/→ open`, `/mcps` says
`j/k move · enter actions`, `/review` says `j/k move · enter diff`, and `/diff`
says `j/k scroll · ^d/^u page · s side-by-side`. A box whose meaning changes with
what it is showing says the right thing in each case — `/theme` says
`j/k preview · enter commit` once you have navigated and `j/k navigate · enter
apply` before that, and `/security-review` drops its `H copy hunk` verb from the
panel for a reason given below. `/cost`, `/usage`, `/logs`, `/doctor` and the
rest of the information panels are all one widget, so a single panel of theirs
says `j/k scroll` and `/project-init` adds `a publish`, because that is the one
of them that has an approve key. Nothing advertises a key the box does not have.

**A docked box never sees the arrow keys, `Esc`, or uppercase `H`/`J`/`K`/`L`.**
While a panel has the keyboard, the host answers those before the box is asked:
the arrows and bare uppercase `H`/`J`/`K`/`L` move *between panels*, and `Esc`
returns the keyboard to the input. So a docked box's line spells its scrolling
verbs `j/k` and never `↑/↓`, and `/tasks` does not offer `J/K reorder` in a
column — the same `J` that reorders modally moves panels when docked. This is
why those verbs are missing rather than forgotten.

**A docked box has no `Esc`.** `Esc` returns the keyboard to the input, and
`Shift+←` brings the box back out of the column as a window you can close. So
when you want the box *gone* rather than just stepped out of, press `Shift+←`
first. The in-box window keeps `Esc`/`Enter` closing it exactly as before.

Two boxes are worth knowing the shape of. `/diff` and `/plan` have no close key
in a column at all — their `Enter` is wired to close the window, and a docked box
has no window left to close, so `Shift+←` is the way out. `/keybinds` in export
or import mode is the same shape: `Esc` is the only exit from those two modes,
and a column does not deliver it, so use `Shift+←` and come back to the list.

The in-box modal keeps its own keymap along the bottom, because a modal box owns
its whole surface. The side column does not: a column too narrow for a keymap
gives its rows to the listing instead. A docked information panel keeps one
line at the bottom instead, reading `Showing 12-40 of 214`, because its scroll
keys do not stop at the end of the page — and so does a docked plugin box, which
keeps its `12-30/30` indicator for the same reason: that number is where you are,
not a keymap, and it is the only thing telling you the listing ends.

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
