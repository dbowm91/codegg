# The Terminal UI

The TUI connects to or starts CodeGG's local daemon and opens a session for the selected workspace. Use `codegg --cwd <directory>` to select an existing workspace. For all startup flags, see [docs/cli.md](cli.md).

## Contents

[Launching](#launching) · [Projects and sessions](#projects-and-sessions) · [Commands and keybindings](#commands-and-keybindings) · [Composer](#composer) · [Panels](#panels) · [Transcript](#transcript) · [Prompt queue](#prompt-queue) · [Tasks and subagents](#tasks-and-subagents) · [Approval and sandbox](#approval-and-sandbox) · [Shell and terminal](#shell-and-terminal) · [Files and editor](#files-and-editor) · [Browsing files](#browsing-files) · [Editor language features](#editor-language-features) · [Themes](#themes)

## Launching

```bash
codegg
codegg -c
codegg -s <session-id>
codegg --fork <session-id>
codegg --no-session
codegg -m <provider/model-id>
codegg -a <agent>
```

The default TUI connects to or starts the local daemon. `--standalone` runs the core in the frontend process and does not use the daemon's global scheduler. See [docs/daemon.md](daemon.md).

## Projects and sessions

Use `/projects` or `Shift+F` in Normal mode to open the project picker. `Ctrl+Alt+N` and `Ctrl+Alt+B` move between project tabs. A project tab supplies the workspace for its session.

The TUI uses the daemon's project catalog. For the separate project-local legacy store used by CLI session commands, see [docs/cli.md](cli.md). For the daemon, see [docs/daemon.md](daemon.md).

## Commands and keybindings

Type `/` to open the command palette. Press `?` or `Space` for in-app help and the active keybindings. Common commands include:

| Command | Purpose |
|---|---|
| `/help` | Show help |
| `/projects` | Choose a project |
| `/sessions` | Browse sessions |
| `/agent <name>` | Select an agent |
| `/connect`, `/connections` | Configure or select a provider connection |
| `/approval`, `/sandbox` | Select policy settings |
| `/queue` | View prompts waiting for a later turn |
| `/files` | Browse workspace files |
| `/terminal` | Open an interactive terminal |
| `/theme` | Select a theme |
| `/doctor` | Run TUI diagnostics |

Keybindings depend on the focused surface and mode. These common bindings are available:

| Key | Action |
|---|---|
| `Enter` | Send the prompt or activate the selected item |
| `Shift+Enter` | Insert a newline in the composer |
| `Esc` | Leave Insert mode or dismiss the current surface; it does not quit |
| `Ctrl+T` | Open the tasks overlay |
| `Ctrl+B` | Toggle the sidebar |
| `Shift+S` | Open the panel manager in Normal mode |
| `Ctrl+Alt+O` | Expand or collapse transcript details |
| `q` or `Ctrl+C`, twice within three seconds | Quit |

Use the live help for the full keymap. Two-modifier shortcuts use `Ctrl+Alt`.

## Composer

The composer has Insert and Normal modes. The mode badge shows which mode is active. In Insert mode, text goes into the prompt. In Normal mode, keys control the composer and the focused panel. `Enter` submits; `Shift+Enter` inserts a newline. Press `Esc` to return from Insert mode to Normal mode. Pressing `Esc` does not quit the application.

### Moving the caret in `[NOR]` mode

In `[NOR]` mode the caret is a thing you can move, and Helix's horizontal motions are the way to move it:

| Key | What it does |
|---|---|
| `h` | Move the caret one character left |
| `l` | Move the caret one character right |
| `←` / `→` | The same two moves, for people who prefer the arrows |
| `Home` / `End` | Jump to the start or the end of the line |

`h` and `l` work in both the default and vim layouts.

The word motions move it by word instead of by character:

| Key | What it does |
|---|---|
| `w` | Move to the start of the next word |
| `b` | Move to the start of the previous word |
| `e` | Move to the last character of the word at the caret |
| `j` / `k` | Move down / up a row of the composer when the composer content spans more than one row |

`j` and `k` move the caret only when the composer content spans more than one row. With one row, they scroll the conversation.

`Ctrl+u` and `Ctrl+d` page the composer when the composer content spans more than one row. Otherwise, they page the conversation, with the same page size in either mode.

### The composer's own keymap in `[NOR]` mode

This is the whole of the `[NOR]`-mode keymap, in one table. It is generated from the same source as the row above the composer and as the help box, so the three cannot disagree:

| Key | What it does |
|---|---|
| `w` | Move to the start of the next word, without selecting it |
| `b` | Move to the start of the previous word, and select the word it lands on |
| `e` | Move to last character of the word at the caret, and select the word it lands on |
| `j` / `k` | Move down / up a row of the box, only when the composer content spans more than one row |
| `v` | Toggle select mode; a caret motion then extends the selection |
| `x` | Select the whole line, and grow the selection a line at a time |
| `d` | Delete the selection |
| `p` | Paste the clipboard at the caret |
| `u` | Undo the last edit |
| `U` | Redo the last edit |
| `i` | Start typing before the caret |
| `a` | Start typing after the character under the caret |
| `o` | Open an empty line below and type in it |

Two rules cover the edges of that table:

- **`j` and `k` need a composer with more than one row.** When the composer holds one row they are the conversation's, and scroll it.
- **An edit that changes the length of the text drops the selection.** The selection remembers *positions* in the text, so once the text moves the selection would point at the wrong characters. Editing, undoing and redoing all drop it.

Text entered while a turn is running is retained for a later turn. Use `/queue` to review, edit, reorder, or remove queued prompts.

## Panels

Use `Shift+Arrow` or `Shift+H/J/K/L` in Normal mode to move between the composer and docked panels. The direction depends on panel placement. `Shift+Q` leaves a focused panel and returns to the composer. `Ctrl+B` closes the panel the keyboard is in, and the same key re-opens it with **the panel that was there** — the edge keeps its occupant. The edge keeps its panel across close and reopen. Placement and the arranged layout are saved when you dock, move or cycle a panel, so the next start lands where you left off; the default is the right column.

Use `Shift+S` (or `/panels`) to choose which panel is shown. The list holds the **full-interface panels** — Session, Chat, Terminal, Files, Sessions, Help, Agents, Tasks, Logs, Context, Review, Models, MCP, Plugins, Keys, Themes, Team and Control. Views of those are folded under them instead of taking their own rows: Goal, Plan and Memory belong to Session; Tree to Files; Cost, Usage and Stats to Context; Doctor to Logs; Shells to Terminal; Diff and Security to Review; Task View and Research to Tasks; Present to Team. Press `A` in the manager to list the folded views too (each row names its parent), or type a name into the filter — `diff`, `memory` — to reach one directly. Folding is a listing decision only: every folded panel still docks, still takes focus, and still opens from its `/` command and saved hotkey.

The project picker and panel manager have their own key hints. Press `?` or `Space` to see the current bindings.

## Transcript

Tool calls and reasoning details are collapsed by default. Select a row to expand it. Use `Ctrl+Alt+O` to expand or collapse all details.

## Prompt queue

Open `/queue` to manage prompts submitted while a turn is running. Use `j` and `k` to move, `a` to add, `e` to edit, `[` and `]` to reorder, `d` to delete, and `s` to send a selected prompt to the running turn when possible. If no turn is running, the prompt remains queued for the next turn.

## Tasks and subagents

Press `Ctrl+T` to see todos, active subagents, and live jobs. Center tabs use `Shift+N` and `Shift+P` to move between them and `Shift+X` to close the active one. The strip above the transcript carries the main conversation, every open subagent run and every open file, so a reader can move between a run and the file it is changing without leaving the center. Use the tab or task view to inspect work without treating its display name as a durable identifier.

Use `/subagent-send` to address a running subagent through the current turn. The message is sent to the agent running that turn, which decides whether to relay it; it is not delivered directly to the child.

## Approval and sandbox

Approval and sandbox are separate settings. Approval controls how a request is reviewed. The sandbox controls filesystem authority and does not by itself isolate network access.

| Setting | Behavior |
|---|---|
| `interactive` approval | Ask the user to answer escalations |
| `automatic` approval | Use the configured automatic reviewer; explicit denies remain final |
| `yolo` approval | Skip approval prompts; explicit denies and sandbox restrictions still apply |
| `read-only` sandbox | Deny filesystem writes through the selected containment backend |
| `workspace-write` sandbox | Allow writes within the authorized workspace roots when supported |
| `full-host` sandbox | Opt out of CodeGG filesystem containment |

Use `/approval` and `/sandbox` to choose these settings. The CLI equivalents are `--approval-mode` and `--sandbox`. If a required filesystem sandbox cannot be applied, CodeGG refuses to start the target. See [docs/security-semantics.md](security-semantics.md).

## Shell and terminal

Prefix a command with `!` to run it as a human shell command. A single bang keeps its output out of the model context. Double bang, `!!command`, stages bounded output for a later submitted turn. Use `/shell-include` or `/shell-ask` for the corresponding include and question workflows.

Use `/terminal` to open an interactive terminal. It uses the local daemon and can be docked as a panel. See [docs/cli.md](cli.md) for remote server and attach commands.

## Files and editor

Use `/files` or `Ctrl+T` to browse the active workspace; the keys are in [Browsing files](#browsing-files). The editor has its own Normal and Insert modes. Press `Space` in the editor for the menu of keys it offers, and `Space ?` for its command palette. LSP setup and diagnostics are covered in [docs/LSP.md](LSP.md).

## Browsing files

`Ctrl+T` opens a tree pane beside the editor; `/files` opens the same navigator as a box you can dock into the side panel with `Shift+L`. Both read the same navigation and the same selection, so the keys below mean the same thing in both. The **filter** is the one thing they do not share: the Ctrl+T pane's filter lives in the app's tree state and the `/files` box keeps its own.

| Key | What it does |
|---|---|
| `j` / `k`, `↓` / `↑` | move the cursor |
| `Ctrl+d` / `Ctrl+u` | page down / page up (two thirds of the pane) |
| `Enter` | **toggle**: a directory opens when closed and closes when open, and the **cursor never moves**; a file is opened in the editor. Stepping in and out are `l` and `h` |
| `l` / `→` | **peek into** that directory: a closed folder opens in place and the **cursor stays on the folder**; an open one is entered. On a file it opens the file |
| `h` / `←` | **close** the open directory the cursor is on; otherwise go up one level |
| `i` | type a filter into the pane, the rows narrow as you type |
| `r` | **refresh**: re-list the current directory |
| `Esc` | leave the filter mode, keeping the filter; in Normal mode, clear it |
| `q` | close the **box**, in the docked panel it does **nothing** |
| `Shift+Q` | close the box / **leave** the pane |

`l` **peeks** rather than re-rooting. A closed folder opens in place and the **cursor stays on the folder**, so the parent and its siblings stay on screen and the selection does not drift down a row. Press `l` again on a folder that is already open and the cursor steps into it, onto its first child. `Enter` is the same motion. Only when the listing has run out of room does the scope slide, and it slides to the directory's **grandparent**, the shallowest scope in which the directory's children fit back inside the listing; the cursor stays on the folder there too. The workspace root never changes: the scope is a view of the workspace, not a working directory. `h` is the mirror, so the pair reads as a horizontal motion: `l` goes right (deeper), `h` goes left (shallower). `h` closes the open directory the cursor is on, wherever it is, and the cursor stays on it; a row that is not an open directory goes up one level instead, and a top-level row of a scoped listing slides the scope back up.

The filter is **panel-local**: `i` opens it, the rows narrow as you type, and `Esc` leaves the filter mode while **keeping** the filter.

**Where you are, three ways.** The title is a breadcrumb (`workspace › src › tui`) and every row is drawn one level further in for each component of that trail, with a `├─` / `└─` guide to the directory it belongs to. The indent is drawn **relative to the scope**, not to the workspace, so a deep scope does not flatten into a wall of guides.

### Where the keys are

**The line just above the input is where a panel's keys live, including the keys of a panel you have docked but not yet entered.**

When `/files` is running in the side panel, that line reads

```
▌ Files · Shift+→/Shift+L in · j/k move · ^d/^u page · l/→ peek · …
```

## Editor language features

Opening a file puts you in `NOR`, not `INS`. It is a surface you read before you
type into, and `i` is one keystroke away. `NOR` gives the keyboard to the Helix
verbs: `hjkl` and the arrows move, `Ctrl+d`/`Ctrl+u` page, `e`/`b` jump by word,
`W`/`B`/`E` jump by WORD, `v` selects by character, `x` selects the whole line, `d`
deletes the selection, `c` deletes it and drops you into `INS`, `y` copies it into
the selected register, and `p`/`P` paste it back.

`m` is the match mode: `m m` jumps to the mate of the bracket under the cursor, `m s`
surrounds the selection, and `m a` and `m i` select a text object. `i`/`a`/`I`/`A`/`o`/`O`
enter `INS`, where typing goes into the file and `Esc` returns. `u` undoes and `U`
redoes along the branch you are on, and `Alt+u`/`Alt+U` walk the same history in
creation order. `Ctrl+s` saves and `Ctrl+r` resyncs the replica after a transport
failure. Each of those chords has exactly one owner.

In the editor, `Alt+o` expands the selection to the next enclosing syntax range and
`Alt+i` contracts it to the previous range. `Alt+.` repeats the last find or motion.

### History gestures

`Alt+u` and `Alt+U` are `NOR` gestures. In `INS` they are inert: the `INS`
catch-all covers unmodified and `Shift`-modified keys only, so an `Alt` chord
reaches the input box rather than vanishing.

### Insert-mode chords

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

To reload from disk and throw the local draft away, use `:e`. It used to be `R`,
which now means something else, see [Change verbs](#change-verbs).

### Change verbs

| Key | What it does |
|---|---|
| `"` + name | select the register the next `y`, `p`, `P` or `R` uses, see [Registers](#registers) |
| `p` / `P` | paste after / before, from the selected register |
| `y` | copy the selection into the selected register, and the system clipboard |
| `d` / `Alt-d` | delete the selection, `d` yanks into the unnamed register, `Alt-d` yanks nothing, and neither touches the clipboard |
| `c` / `Alt-c` | delete the selection, or the character under the cursor, and start typing, the same `d`/`Alt-d` split |
| `r` + char | replace the selection, or the character under the cursor |
| `R` | replace the selection, or the character under the cursor, with the selected register's text |
| `~` | flip the case of the selection, or of the character under the cursor |
| `` ` `` / `` Alt-` `` | set that text to lower / upper case |

**The delete does not touch your clipboard.** Helix's `d` yanks into a
register and `Alt-d` is the spelling that does not, and that is now the whole
truth here: `d` and `c` write the **unnamed register** (so `d` then `p` moves
the text you just cut) and `Alt-d` and `Alt-c` write nothing at all. Neither
pair ever reaches the system clipboard, so a delete cannot overwrite what you
copied for the edit you are making. Copying on every delete would clobber
it, so the two pairs were bound ahead of time. `"_d` and `"_c` are the
blackhole's spelling of "delete and yank nothing" for anyone reaching for
Vim's gesture.

`y` over several selections copies them in reading order into the clipboard as
one piece of text, so the order you see on screen is the order you get.

**Paste reads the selected register**, and the unnamed register is seeded from
the system clipboard when a document opens and kept in step by every `y`. That
means `p` pastes what you copied in another application *and* `d` then `p`
pastes the deletion, see [Registers](#registers) for the full model. The
buffer never reads the clipboard itself: the seed is one App-layer read at
open, so the paste path stays synchronous.

### Registers

| Register | What it is |
|---|---|
| `a` to `z` | a named slot; a yank writes one |
| `"` | the unnamed register, what bare `p` reads |
| `_` | the blackhole: writes are discarded and a read is refused |
| `/` | the last accepted search pattern, read-only |

**Which verbs honour a register.** `y`, `p`, `P` and `R` do, and nothing else.
`d` and `c` are deliberately **not** register-targeted (Helix's delete yanks
into the unnamed register whatever `"` selected) with one exception: `"_d` and
`"_c` are the blackhole's spelling, so a delete can be made to yank nothing.
`Alt-d` and `Alt-c` write nothing at all. The `space` family is **not**
register-aware: `space y`, `space p`, `space P` and `space R` always mean the
system clipboard, whatever `"` selected, because that family is the
cross-application path and redirecting it would make the gesture depend on
state the reader cannot see.

**The bound.** A register keeps at most **16 KiB**. A larger yank stores the
first 16 KiB (stopping on a character boundary, never inside a multi-byte
character) and says so in the toast: a paste from a truncated register is
still a paste, and the truncation was disclosed when it happened. The store is
at most 27 slots, so it is bounded at 27 × 16 KiB and can never grow into a
copy of the document.

**Clearing.** Registers are per-document presentation state, and they are
cleared by the same transitions that clear the undo history: opening a
document, a reload, a resync, a reconnect, and closing. `"ap` after one of them
is refused ("register `a` is empty") rather than pasting text from a document
you are no longer looking at.

**The blackhole refuses a read.** `"_p` says the blackhole holds nothing (`_`
discards by design) rather than inserting an empty string that would look like
a dropped keystroke. `"_y` and `"_d` discard silently, because discarding is
what they were asked for.

**The search register.** `"/p` pastes the last accepted search pattern. It is a
projection of the `/` line, not a copy: refining the line changes what the
register reads. Before a search has been accepted, `"/p` refuses and says so. A
write to `/` is refused, the search register cannot be set by hand.

**Bare `p` reads the unnamed register**, which is seeded from the system
clipboard when a document opens and kept in step by every `y`. That is what
lets `p` paste what you copied in a browser while the buffer never reads the
clipboard itself. It also means `d` then `p` pastes the deletion, the
behaviour Helix users expect, and the reason registers exist. For a `d` that
leaves the unnamed register alone, spell it `"_d` or `Alt-d`.

`y` still writes the system clipboard on every yank, so copying here and
pasting in another application is unchanged; `d` and `c` never write it.

**Insert-mode register insertion is not bound.** Helix gives `Ctrl+r` the
insert-register gesture, and `Ctrl+r` here is the mode-independent **resync**
chord. Resync is a recovery affordance that has to work from any mode, and
every register is already reachable from `NOR` with `"` followed by its name,
so the Insert-mode chord is the one Helix register gesture this surface does
not reach.

### Line-shape verbs

These reshape what you hold, and two of them rewrite the lines it covers.

| Key | What it does |
|---|---|
| `X` | widen every selection to the whole lines it touches |
| `Alt-x` | narrow every selection to its lines' own bounds |
| `_` | trim leading and trailing whitespace from every selection |
| `J` | join the lines each selection covers with a single space |
| `Alt-J` | the same join, leaving the inserted space selected |
| `>` | indent every covered line by one unit |
| `<` | unindent every covered line by one unit |

**The indent unit is read from the document, not from configuration.** The
editor scans a bounded prefix of the file and takes the smallest positive run
of leading spaces; a file whose indented lines are mostly tab-led is treated as
tab-indented; a file with no indentation at all falls back to four spaces. When
`<` finds nothing to remove it says so, and the notice names the unit and
whether it was detected from the file or defaulted: so you are never left
guessing which width the editor chose.

### Selecting more than one region

Once you can build a set, every verb above applies to all of it at once. These
are the keys that build and reshape the set.

| Key | What it does |
|---|---|
| `C` / `Alt-C` | add a cursor on the next / previous line |
| `%` | select the whole file |
| `S` | split each selection into one selection per regex match |
| `s` | the same shape, from Helix's `select_regex` spelling |
| `Alt-s` | split each selection into one selection per line |
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

`Alt-minus` and `Alt-_` differ the same way: `Alt-minus` bridges the gaps
between your selections, `Alt-_` only merges selections that are already
touching.

### The regex keys

`s` and `Alt-s` enlarge what you hold, so on a read-only buffer they are
refused with a notice, exactly as `v` and `x` are.

### The preview keys

`=` and `space r` ask the language server what it **would** change, and (when
the whole change is known) offer it for review.

| Key | What it does |
|---|---|
| `=` | the format preview for this document: the diff, and how many edits that is |
| `space r` | the rename preview for the symbol under the cursor: the old and new names, the files that would change, and the edits in each |

**A preview you can apply opens the review.** The diff in the review is the
change that would be written, and the review is where you decide:

- `[a]pply` sends the change through the daemon's checked apply path, the
  same path `/lsp-preview-apply` uses, with the same revalidation of the file
  it was computed against;
- `Esc` rejects it. Nothing is written.

After a successful apply the editor reloads the file it wrote, unless your
buffer holds edits the editor has not saved; then it keeps them and tells you
that the file changed on disk.

### Code actions: the picker is the panel

| Key | What it does |
|---|---|
| `space a` | ask the server for the code actions it would offer at the cursor |
| `j` / `k` | move the selection through the offered actions |
| `Enter` | open the change review on the selected action |
| `Esc` | close the panel |

The review is the same one the preview keys open, and the change in it is the
change that would be written. `[a]pply` sends it through the daemon's checked
apply path; `Esc` rejects it.

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

### Multi-key gestures

### Counts

What honours a count: `h`/`j`/`k`/`l` and the arrows, `w`/`b`/`e`,
`W`/`B`/`E`, `]p`/`[p`, `PageDown`/`PageUp`, `Ctrl+d`/`Ctrl+u`, `G`, `gg`, and
the goto leaves that move the cursor.

### The bottom row

The editor has **one** row of chrome, at the bottom. The file's path is on its
left; the status cluster is on its right. The cluster reads left to right:

| Part | What it says |
|---|---|
| the persistent signals | whether the file has unsaved changes, the language server's state, which root the edit is confined to (`[project]` or `[global]`), whether the file is writable, and any caveat about the rendering itself |
| the transient disclosure | what your last keystroke did, the find that landed, the search that matched, where the undo cursor is standing |
| the notice | an error, if one is being surfaced |
| the pending count and the armed prefix | the gesture you are halfway through (`3`, then `g`) at the far right of the row |

### The `:` line

```
:goto 50
```

- `:50` and `:goto 50` are the same command. The bare number is the spelling
  you reach for; the named one is there when you want to be explicit.
- The line number is one-based, `:1` is the first line. `:0` is refused
  rather than quietly taken to line one.
- As you type, the row above lists the commands that match. `Tab` or `Enter`
  takes the highlighted one.
- The match list is drawn **over** the text rather than in a row of its own,
  so opening the line does not reflow the document and move your cursor out
  from under you.

### Scrolling and pages

| Key | What it does |
|---|---|
| `PageDown` / `PageUp` | a **full** page down / up |
| `Ctrl+d` / `Ctrl+u` | **half** a page down / up |
| `zc` / `zt` / `zb` | centre the view on the cursor / put its line on the top row / on the bottom row |
| `zj` / `zk` | scroll the view a line down / up, leaving the cursor where it is |

### Select mode

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

Not bound, on purpose rather than by omission:

<!-- unbound: the keys below are deliberately not bound -->

- `gf`, `ga`, `gm`, `gn`, `gp`, files and alternate buffers. One document is
  open at a time.

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

`gy` (go to type definition) says why it cannot run rather than doing
nothing: no language server implements it here, so the editor tells you that
instead of leaving the key dead.

### The space menu

`space` is a menu, and every key in it either does something or says why it
cannot. Nothing in it is dead.

Pressing `space` draws the menu in the **lower right** of the editor, so the
keys it offers and what each one does are on screen rather than in your
memory, and the box never covers the text you are editing. A key that opens a
menu of its own shows that menu instead, titled with the level you are in
(`Space`, `Space q`), and `space` steps back out.

The menu is yours to extend: see [Configuring the editor](#configuring-the-editor).

| Key | What it does |
|---|---|
| `space f` | open the workspace file browser |
| `space ?` | open the editor's own command palette: a centred box with a filter bar |
| `space r` | open the rename prompt (the `:` line, pre-filled) and preview the rename |
| `space a` | ask what code actions the server would offer at the cursor, and pick one |
| `space y` | copy every selection |
| `space Y` | copy **only** the main selection |
| `space p` | paste from the clipboard after the selection, or after the character under the cursor |
| `space P` | paste from the clipboard before the selection, or before the character under the cursor |
| `space R` | replace the selection, or the character under the cursor, with the clipboard |
| `space j` | open the jumplist picker, the positions your last jumps started from |
| `space c` | toggle comments over the covered lines, in the buffer's own syntax |
| `space C` | the same, forcing the block pair |

These four say why they cannot run rather than doing nothing:

| Key | Why |
|---|---|
| `space F` | this build has one workspace root, and `space f` opens it |
| `space b` | one document is open at a time, so there is no list to choose from |
| `space D` | no server capability backs a separate workspace-diagnostics request; `space d` opens the panel that exists |
| `space '` | the pickers are not fuzzy, so there is no query to recall |

### The editor command palette

`space ?` opens the editor's own command palette: a box in the middle of the
screen with a filter bar, a match count, and the editor's commands.

Type to filter. `↑`/`↓` move the selection, `Enter` runs the selected command, and
`Esc` closes the box. A command that needs an argument — a line number to jump
to, a pattern to search for — opens the `:` line with the command already
written for you.

The list holds two kinds of row:

| Row | What it is |
|---|---|
| a command | runnable; `Enter` runs it |
| a key | one of the editor's own bindings, listed so you can look it up; `Enter` says so rather than pretending to run it |

Commands are filtered by their own name — `goto` finds `goto_line`. Keys are
filtered by the binding itself or by what it does — `word` finds *next word
start*.

While the palette is open it owns the keyboard, so the filter never types
into the document behind it.

### Configuring the editor

The editor's keys are configurable in the file format Helix uses: a TOML file
whose tables nest, one table per mode and one table per nested menu.

Copy the shipped defaults and edit them:

| File | Applies to |
|---|---|
| `<config dir>/codegg/editor.toml` | every project |
| `.codegg/editor.toml` | the project it sits in |

A project file is laid over the global one, so it may add a key without
restating the rest.

```toml
[keys.normal.space.q]
a = "llm_insert"
```

That reads: in Normal mode, `space` opens the space menu; inside it, `q`
opens a menu of its own; inside that, `a` runs the command named
`llm_insert`. The same binding written inline means exactly the same thing:

```toml
[keys.normal.space]
q = { a = "llm_insert" }
```

A key is written as the character you press (`a`), as a named key (`space`,
`esc`, `ret`, `tab`, `left`, `right`, `up`, `down`, `home`, `end`,
`pageup`, `pagedown`, `backspace`, `del`, `ins`), or with a modifier
prefix (`C-x`, `A-x`). A capital letter is the shifted spelling, so `A`
and `S-a` both mean shift held down with `a`.

The commands a key may name are the editor's own, in snake_case:

| Name | What it does |
|---|---|
| `llm_insert` | ask the model for an insertion at the cursor |
| `complete` | ask the language server for completions at the cursor |
| `write` | write the buffer to disk |
| `write_all` | write every open document |
| `quit` | close the document |
| `reload` | reload the buffer from disk |
| `rename_preview` | preview renaming the symbol under the cursor |
| `split_regex` | split the selection on a regex pattern |
| `keep_regex` | keep the selections that match a regex |
| `remove_regex` | remove the selections that match a regex |
| `select_regex` | select the matches inside the selection |
| `write_quit` | write the buffer and close the document |
| `force_quit` | close the document without writing |
| `new` | open a new document |
| `open` | open a document by path |
| `goto_line` | move the cursor to a line |

The `[editor]` section carries the editor's own switches:

```toml
[editor]
completion-popup = false
```

Only the space menu is configurable so far. The editor's own Normal-mode
chords are not, and a file that tries to change one says so when it loads
rather than quietly doing nothing.

### The jumplist

A jump that moves the cursor to a **named destination** records where it
started: an entry that recorded where the jump *landed* could not take you
back.

- **Records** `gg`, `G`, `:goto <n>`, `]d`/`[d` and `Enter` on a diagnostic,
  and `/`, `?`, `n`, `N`.
- **Does not record** the motions that walk the buffer: `j`, `k`, `w`, `e`,
  `Ctrl-d`, `0`/`$`, `gs`, and `gt`/`gc`/`gb`, there the screen moves to the
  cursor, and the destination depends on the viewport rather than on the text.
- An LSP result that opens **another** document is a document switch, not a
  cursor move. Only one document is attached at a time, so the new attachment
  starts its own history and the old one goes with the document it named.

| Key | What it does |
|---|---|
| `space j` | open the picker on the newest entry |
| `j` / `k` (or the arrows) | move the highlight |
| `Enter` | jump to the highlighted position |
| `Esc` | close the picker |

`space j` with nothing recorded says so, and names the gestures that fill the
list. An empty picker would be a key that appears to do nothing.

**The list is bounded at 32 positions.** A push past the bound drops the
*oldest* entry, and the picker says so: its title carries the count as `n/32`,
and at the bound it adds that the oldest entry is dropped. The disclosure is in
the title rather than under the rows because a short editor window clips a row
off the bottom of the box, and a disclosure nobody can see is not a disclosure.
A history that truncated silently would be a picker whose rows are not the
jumps you actually made.

**An entry that cannot be honoured is refused, never approximated.** The entry
is checked against the text attached *now*. A position past the end of the
document (the file is shorter than it was when the jump was recorded) is
reported as stale and the cursor does not move. Landing on a nearby line would
be worse than saying so: a reader moved to a plausible place believes they are
already there. An entry recorded against a different document is refused for
the same reason, and a refusal leaves the picker open so you can pick another
row.

**The list is frontend presentation state and nothing else.** It holds byte
offsets and a document identity, never text. It is not durable, it does not
survive a restart or a reconnect, and it is **cleared by the same transitions
that clear the undo history**: opening a document, reload, resync and close. A
reload replaces the text the entries address, so keeping them would be keeping
offsets into a document nobody is looking at any more.

**`Ctrl-i` and `Ctrl-o` are not bound to this, and that is a decision rather
than an omission.** Helix steps the jumplist with `Ctrl-i` (forward) and
`Ctrl-o` (back). This TUI never pushes the terminal's keyboard-enhancement
flags, so `Ctrl-i` is the byte `0x09` and always arrives as `Tab`, which is
already `ToggleComposerMode`; `Ctrl-o` is already `OpenWorkspaceDashboard`,
with `Shift+W` as its other spelling. An arm for either would compile and never
fire. The picker is the whole delivered gesture, and the stepping pair is
deferred with that reason on the record.

### Comment toggling

- A line token goes **after the line's existing indentation**, not at column
  zero, and not at the cursor's column.
- A block pair wraps the covered range: the start token before its first
  non-whitespace byte, the end token after its last. The text between them is
  not rewritten: no reflow, no re-indentation, no trailing whitespace.
- Blank lines are skipped. An empty line has nothing to comment, and a token
  written into one would be whitespace the toggle invented.
- A range is uncommented only when **every** non-blank line in it already starts
  with the token. A half-commented range is commented rather than
  half-uncommented, and removal takes away exactly the token that was inserted,
  so two presses restore the file to its exact bytes.
- The whole gesture is **one undo step**, however many lines it covers.

The function keys do the same jobs and keep working: F2 diagnostics, F3 hover,
F4 definition, F5 references, F6 document symbols, F7 workspace symbols.

### Match mode and text objects

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

A character outside that vocabulary is not an object, so it is handed back to
the input box rather than swallowed. `m i z` types a `z`.

**`m m` is a delimiter match, not a syntax match.** It counts brackets over the
bytes around the cursor, so it finds the mate of `(`, `[`, `{`, `<`, `'`, `"`
and `` ` ``, and it does not know what your language calls a string. When it
cannot answer it says which of the two things went wrong, **no bracket under
the cursor** when there is no delimiter there, and **no matching bracket** when
the delimiter has no mate. The two are different answers and both are worth
saying: the first means *move the cursor*, the second means *the code is
unbalanced*.

**`m r` replaces the outer pair.** On `(a(b))`, `m r ( [` gives `[a(b)]`, not
`[a(b])`, the inner pair is left alone, so the nesting you wrote survives the
edit. `m d` is the other way round: it removes the pair **immediately** around
your selection, so unwrapping `b` in `(a(b))` gives `(a)`.

### Unimpaired navigation

`[` and `]` are modes with the same arm-and-resolve behaviour as `g`. While one
is held, the status row reads `[` or `]`.

| Key | What it does |
|---|---|
| `]p` | first non-whitespace byte of the next paragraph |
| `[p` | first non-whitespace byte of the previous paragraph |
| `]Space` | insert a line below the cursor's line |
| `[Space` | insert a line above the cursor's line |

Both motions either move or say why they did not, `]p` at the last paragraph
reports that rather than standing still, because a key that quietly does nothing
is indistinguishable from a dropped keypress.

Not bound, because nothing in this codebase can answer them yet:

<!-- unbound: the keys below are deliberately not bound -->

- `[q`, `[Q`, `[F`, `[f`, `[n`, `[%`, `[i`, `[c`, `[s`, `[t`, `[b`,
  unimpaired's find family: definitions, type definitions, workspace search,
  occurrences, todos, files, configuration.
- `[a`, `[v`, `[w`, `[e`, quotes and words. A half-built bracket-word gesture
  would be worse than none, since it would look like the capital spelling was
  simply broken.
- `[D`, `]D`, workspace diagnostics. There is no server capability behind a
  separate "all diagnostics" request, so these would only be a second name for
  the diagnostics panel the space menu already opens.
- `[g`, `[G`, `]g`, `]G`, change navigation. This has no reachable data source:
  the VCS layer reports which files changed, not which lines.

`[d` and `]d` **are** bound: they step to the previous and next diagnostic and
move the cursor there, wrapping at either end.

### Long words

| Key | What it does |
|---|---|
| `W` / `B` / `E` | next / previous WORD start, next WORD end |
| `w` / `b` / `e` | next / previous word start, next word end |

### Holding your column

### Repeating the last insert

| Key | What it does |
|---|---|
| `.` | repeat the last **insert** at the cursor |
| `Alt` + `.` | repeat the last **motion**, a different verb, see [Finding a character](#finding-a-character) |

The two dots are separated by the modifier and never by what happens to be
recorded: `Alt` + `.` is Helix's *repeat last motion*, and `.` is the insert.

### Finding a character

| Key | What it does |
|---|---|
| `f` + char | jump to the next occurrence of that character |
| `F` + char | jump to the previous occurrence |
| `t` + char | as `f` |
| `T` + char | as `F` |
| `Alt` + `.` | repeat the last find |

### Searching inside the file

| Key | What it does |
|---|---|
| `Enter` | accept, keep the highlight |
| `Esc` | abandon the search and clear the highlight |
| `n` / `N` | next / previous match (after `Enter`) |
| `?` | the same search, walking **backwards** |
| `*` | search for the selected text, as literal text |
| `Alt-*` | search for the selected text, read as a regex |
| `Backspace` | remove a character and re-search |

**The grammar changed, and it changed once.** `/foo.bar` used to look for the
four literal characters `foo.bar`; now the `.` is a wildcard, so it matches
`fooXbar` too. A literal dot is spelled `/foo\.bar`. This is a behaviour change
rather than an addition, and it is documented here rather than papered over
with a second grammar: a reader who has to know which mode they are in cannot
predict what a pattern means.

`?` opens the line with the pattern you last accepted, so reversing a search
never means retyping it; with no pattern accepted yet the line opens empty
rather than the key doing nothing.

In `INS` a slash is just a slash, otherwise you could not type a URL.

### Syntax highlighting

Very large files (more than 5000 lines) drop built-in highlighting rather than
freeze a frame while re-scanning; the text still edits normally.

Navigation opens only locations carrying the daemon-validated
workspace-relative target; other results remain display-only. The TUI does
not start language servers itself and does not claim live diagnostic push
updates.

### Completion

As you type, the editor asks the language server what could follow and shows
the answers in a small list **at your cursor** — its left edge is your
cursor's own column, and it opens on the row below your cursor's line,
flipping above only when there is no room below. The list is as wide as its
widest entry, so it never becomes a panel across the middle of the screen. It
never takes a row of its own, so the document does not move under you.

The list appears only when the server actually has something to offer. An empty
answer draws nothing at all rather than a box saying that there is nothing.

An answer the editor cannot apply says **why** — `· snippet`, `· needs extra
edits`, `· has a server command` — so you can tell it apart from one that is
ready to take.

Accepting an answer inserts it at your cursor, on your cursor's line.

If you would rather have no popup at all, switch it off in the editor
configuration:

```toml
[editor]
completion-popup = false
```

With it off, no completion request is made at all.

### Inline assist

`space`, then `q`, then `a` asks the model for an insertion at the cursor. The
menu says so as you go: `space` shows the menu, `q` shows the capabilities it
holds, and `a` asks for the insertion. A small prompt appears over the text;
the document's rows do not move.

The prompt has its own `[NOR]` / `[INS]` badge, its own multi-line text,
and its own model and thinking level for that request only. `Enter` sends
the instruction, `Shift+Enter` starts a new line, `Tab` cycles the model
and `Shift+Tab` cycles the thinking level. `Esc` leaves insert mode for
the prompt's normal mode, where the Helix motions and the selection verbs
apply; a second `Esc` clears the instruction and dismisses the prompt.

The answer arrives as a marked region at the cursor. It is **not** document
text: the bytes, the file on disk and the language server see nothing until
you accept it.

| Key | What it does |
|---|---|
| `space q a` | ask the model for an insertion at the cursor |
| `Ctrl+Alt+u` | accept the generated region as one undoable edit |
| `Ctrl+Alt+v` | discard the generated region |

Move the cursor onto the region — onto the point the answer was asked for —
and a small menu appears beside it. The menu is derived from where the
cursor is, so it is there whenever the cursor is in the region and gone
whenever it is not.

| Key | What it does |
|---|---|
| `a` | accept the generated region as one undoable edit |
| `d` | deny the generated region and discard it |
| `e` | discard the region and re-open the prompt with the same instruction |

Every other key still belongs to the editor, so moving the cursor away
dismisses the menu. The menu never takes a row of its own: the document's
rows stay exactly where they were.

Neither the model nor the thinking level you pick changes the daemon's
stored selection; both are facts about that one request.

### Semantic highlighting limits and truncation

### Very long lines and large file trees

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

## Themes

Use `/theme` to choose a theme. Theme files and customization are described in [docs/themes.md](themes.md).

## See also

- [CLI reference](cli.md)
- [Providers and credentials](providers.md)
- [Tools](tools.md)
- [Security semantics](security-semantics.md)
- [Daemon](daemon.md)

## Source verification

Checked 2026-11-23 against `crates/codegg-cli/src/main.rs`, `crates/codegg-tui/src/input.rs`, `crates/codegg-tui/src/command.rs`, `crates/codegg-core/src/command_catalog.rs`, and `crates/codegg-tui/src/commands/exit_control.rs`. Session store locations were checked against `crates/codegg-core/src/storage/mod.rs`.
