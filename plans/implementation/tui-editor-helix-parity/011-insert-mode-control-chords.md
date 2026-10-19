# TUI editor Helix parity Milestone 011 — Insert-mode control chords

Status: implemented - closed 2026-10-19
Repository baseline: `a8778e87c` (`feat/auto-20261018-12255c45`); W1 was
implemented against the same baseline (uncommitted at the time of writing)
Closure: `plans/closure/tui-editor-helix-parity/011-status.md`
Implementation decisions and recorded deviations: §17 below

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 M011
Long-term requirements: `plans/000-long-term-specification.md` (IDE-grade
language intelligence); `plans/002-long-term-roadmap.md` (editor capability
sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
Primary class: capability

## 1. Objective

Make Insert mode a **real mode**: the control chords Helix binds there work
here too, every chord has exactly one owner, and nothing that should type a
character is stolen by a panel.

The chords this milestone binds, all in **Insert** mode only:

| Chord | Helix command | Effect here |
|---|---|---|
| `Ctrl-x` | `completion` | open the LSP completion popup (today `F8` / `:complete` only) |
| `Ctrl-w` | `delete_word_backward` | delete the previous word |
| `Alt-Backspace` | `delete_word_backward` | same |
| `Alt-d`, `Alt-Delete` | `delete_word_forward` | delete the next word |
| `Ctrl-u` | `kill_to_line_start` | delete to the start of the line |
| `Ctrl-k` | `kill_to_line_end` | delete to the end of the line |
| `Ctrl-h` | `delete_char_backward` | same as `Backspace` |
| `Ctrl-d` | `delete_char_forward` | same as `Delete` |
| `Ctrl-j` | `insert_newline` | same as `Enter` |

Two Helix insert-mode chords are **not** bound here, and §5 says why with the
evidence: `Ctrl-s` (commit undo checkpoint) and `Ctrl-r` (insert register
content).

## 2. Why this milestone is ready

- **Its only hard dependency is closed.** M001 (key dispatch authority) and
  M002 (the unified prefix state machine) are closed, and the guard
  (`scripts/check_tui_editor_key_authority.py`) that enforces one owner per
  chord is in `verify.sh quick` and in CI.
- **The roadmap records no unresolved dependency.** The roadmap's M011 row
  lists no blocking dependency; it records a *conflict* to be decided, which
  §5 and §6 decide.
- **The completion path it needs already exists and is authorized.** M004
  built the `F8` completion path, M006-B fixed its reachability, and M013
  extended the LSP read surface. `start_editor_completion`
  (`src/tui/commands/editor.rs:933`) and `accept_selected_completion`
  (`:1125`) are the code `Ctrl-x` reuses.
- **The edits it needs already exist in part.** `EditorCommand::DeleteToLineEnd`
  (`src/tui/editor.rs:4219`) is exactly a `Ctrl-k`; `EditorCommand::DeleteWord`
  (`:4251`) is close to a `Ctrl-w`. Two of the seven effects need new
  builders, not new machinery.

## 3. Current implementation evidence

All claims were verified by reading the tree at `a8778e87c` with the M013
work applied.

### 3.1 The dispatch stages

`editor_buffer_key` (`src/tui/commands/editor.rs:3051-3117`) is five stages
in a fixed order:

1. the search line owns every key (`:3061-3068`);
2. the `:` command line owns every key (`:3069-3075`);
3. the **mode-independent guard block** (`:3097-3102`) — `Ctrl+s` →
   `BufferKey::RequestSave`, `Ctrl+r` → `BufferKey::RequestResync`;
4. the **Insert** branch (`:3109-3115`) → `handle_insert_key`;
5. the **Normal** branch (`:3116`) → `handle_normal_key`.

The in-code contract at `:3076-3092` states: *"every chord has exactly one
owner, and that owner is the first stage that claims it"*, and names the M001
`Ctrl+r`/redo shadow as the precedent for the rule.

**The guard block runs before the Insert branch.** So a `Ctrl+s` or `Ctrl+r`
arm added to `handle_insert_key` would be **dead code**, and the key-authority
guard fails the build on that shape (rule 1, `:566-581`).

### 3.2 The Insert table is a `match key.code`, with the catch-all first

```rust
// src/tui/commands/editor.rs:2766
fn handle_insert_key(state: &mut EditorState, key: KeyEvent, visible_lines: usize) -> bool {
    match key.code {
        KeyCode::Char(ch)
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        { editor_insert_literal(state, &ch.to_string()); true }   // :2796-2801
        KeyCode::Backspace => { editor_delete_backward(state); true }
        KeyCode::Delete    => { editor_editing_command(state, EditorCommand::DeleteChar); true }
        KeyCode::Left | Right | Up | Down => { editor_motion(...); true }
        KeyCode::Enter     => { editor_insert_literal(state, "\n"); true }
        KeyCode::Tab       => { editor_insert_literal(state, "\t"); true }
        _ => false,                                                  // :2834
    }
}
```

Two consequences:

- The **catch-all is the first arm** and it matches only `NONE | SHIFT`. A
  new chord arm must sit **before** it, or the catch-all's guard must
  exclude `CONTROL`/`ALT`.
- `handle_insert_key` matches on `key.code` alone, while
  `handle_normal_key` (`:3110`) matches on `(key.modifiers, key.code)`. **The
  key-authority guard cannot parse the Insert table's shape for a `Char`
  chord** — see §6.5.

### 3.3 An unclaimed Insert chord is not swallowed — it becomes a *global* action

`handle_insert_key` returns `false` → `BufferKey::Unhandled` (`:3113`) →
`handle_editor_key` returns `false` (`:2180`) → the **global** keymap runs
(`src/tui/app/mod.rs:3699`) → if the global map also declines, a printable
becomes composer text (`src/tui/input.rs:2148-2154`).

In Insert mode the global map suppresses only bare/text-modifier characters
(`src/tui/input.rs:2096-2154`); **CONTROL and ALT chords still fire**. So the
global bindings are what an unclaimed chord reaches:

| Chord | Global action | Source |
|---|---|---|
| `Ctrl+w` | `CloseSession` | `src/tui/input.rs:816-819` |
| `Ctrl+k` | `ClearSession` | `:749-752` |
| `Ctrl+d` | `PageDown` | `:879-882` |
| `Ctrl+u` | `PageUp` | `:883-886` |
| `Ctrl+s` | `StashPrompt` | `:829-832` |
| `Ctrl+r` | `RestorePrompt` | `:833-836` |
| `Alt+w` | `CloseProjectTab` | `:945-948` |

**`Ctrl+w` typed in the editor's Insert mode closes the session today.** A
test pins that behaviour: `bare_ctrl_w_still_closes_the_session`
(`src/tui/input.rs:2337-2347`).

`Ctrl+x`, `Alt+d`, `Alt+Backspace`, `Alt-Delete`, `Ctrl+h`, `Ctrl-j` are
**unbound in both** the default map (`src/tui/input.rs:703-973`) and the vim
map (`:979-1176`).

### 3.4 The mode-independent guard block, and what it costs

| Chord | Action | Consumed at | Side effect while typing |
|---|---|---|---|
| `Ctrl+s` | `RequestSave` | `:2181-2184` → `save_editor` (`:1255`) | starts a controller save; clears the completion popup (`:1342`); sets `EditorStatus::Flushing`; **does not** leave Insert mode; **does not** clear undo history |
| `Ctrl+r` | `RequestResync` | `:2185-2188` → `resync_editor` (`:1288`) | a daemon round trip that installs a fresh snapshot and **clears the undo history** (`src/tui/editor.rs:4400`); disruptive mid-typing |

Both are documented as mode-independent in two places:
`architecture/tui.md:3302-3303` (*"Mode-independent chords … Both in every
mode"*) and `docs/tui.md:728-731` (*"Two chords work in either mode, because
they are round trips rather than verbs"*).

Three live tests pin that promise:
`insert_mode_chords_still_reach_their_owners` (`:5618-5638`),
`lifecycle_chords_keep_one_owner_across_both_modes` (`:5401-5425`), and
`save_and_resync_are_requested_in_both_modes_and_reload_moved_to_the_colon_line`
(`:5688-5720`).

### 3.5 There is **no register** of any kind

- `grep -rn "register\|Register" src/ --glob '*.rs'` finds no editor
  register; the hits are unrelated prose.
- `yank_editor_ranges` (`:3872-3907`) writes to the **OS clipboard**
  (`crate::util::clipboard::copy_to_clipboard`, `:3897`).
- The code says so in prose at `:1714-1717`: *"There is no local register —
  `space y` writes to the OS clipboard and nothing else."*
- `plans/closure/tui-editor-helix-parity/004-status.md:85-102` records the
  same as a **structural answer, not an oversight**.
- The roadmap's non-goals list named registers (`"<reg>"`) as **deferred**.

So Helix's `Ctrl-r` = *insert a register's content* has **no substrate** here.
The only bytes available are the OS clipboard, and `p`/`P` already paste it in
Normal mode.

### 3.6 The undo model: one keystroke, one entry, no coalescing

```
handle_insert_key → editor_insert_literal (:1897) → editor::insert_text
  (src/tui/editor.rs:3314) → submit_edit (:1496) → controller.apply_local
  (:1530) → editor::push_undo (:1566 → src/tui/editor.rs:4292)
```

`EditorUndoEntry` (`src/tui/editor.rs:1306`):

```rust
pub struct EditorUndoEntry {
    pub forward: TextTransaction,
    pub inverse: TextTransaction,
    pub cursor_before: usize,
    pub selections_before: Vec<Selection<EditorPoint>>,
    pub bytes: usize,
}
```

`push_undo` (`:4292-4311`) unconditionally pushes **one** entry and clears
the redo stack. `grep -rni "coalesc" src/tui/` finds coalescing only in the
interactive terminal and in persistence debounce — **none in the editor**.

Bounds: `MAX_EDITOR_UNDO_DEPTH = 256` (`:51`), `MAX_EDITOR_UNDO_BYTES =
4 MiB` (`:58`), trimmed oldest-first by `trim_history` (`:4344-4373`).

**A 256-keystroke insert session consumes the entire undo depth.** That is
the concrete cost of the missing coalescing, and it is Helix's own mechanic
that is missing: *"Changes to the text are only saved for undos when escaping
from insert mode to normal mode."* (Helix keymap, Insert mode section.)

### 3.7 The word and line helpers

All in `src/tui/editor.rs`, all byte-offset based on the controller's own
`&DocumentSnapshot`:

| Helper | Line | Scope |
|---|---|---|
| `word_forward` / `word_backward` / `word_end` | `:1939` / `:1963` / `:2001` | **buffer-wide** — they cross line boundaries |
| `long_word_forward` / `_backward` / `_end` | `:2025` / `:2052` / `:2077` | buffer-wide, whitespace-delimited |
| `is_word_byte` | `:1901` | ASCII alnum, `_`, or `>= 0x80` |
| `is_long_word_byte` | `:1913` | "not ASCII whitespace" |
| `leading_byte` | `:1930` | char-safe byte read |
| `next_char_boundary` / `prev_char_boundary` | `:1572` / `:1584` | `pub` |
| `line_start` / `line_content_end` / `first_non_whitespace` | `:1605` / `:1652` / `:1707` | line-scoped |
| `cursor_line` | `:1596` | byte offset → line index |
| `buffer_content_end` | `:1674` | end of document content |

**The word motions are buffer-wide, and Helix's insert-mode word deletions
are not.** A Helix `Ctrl-w` stops at the line boundary; `word_backward`
(buffer-wide) at column 0 would delete the **previous line's** last word.
That is a real behavioural difference and §6.4 resolves it.

Existing commands: `EditorCommand::DeleteToLineEnd` (`:4219-4226`) deletes
`cursor..line_content_end` — **exactly** a `Ctrl-k`.
`EditorCommand::DeleteWord` (`:4251-4261`) deletes backward to
`word_backward(next_char_boundary(cursor))` — close to a `Ctrl-w`, but it
deletes the word **plus the following whitespace** because it starts from
`next_char_boundary`. **No kill-to-line-start and no delete-word-forward
exist.**

### 3.8 The completion path, and a mode-blind popup

- **Trigger today:** `F8` → `handle_completion_key` (`:1076-1080`) →
  `start_editor_completion` (`:933`); and `:complete` in Normal mode
  (`:2349-2350`).
- **Scope:** `current_completion_scope` (`:893-925`) builds an
  `EditorCompletionScope { read_scope, document_id, revision, line, character }`
  (`src/tui/editor_completion.rs:60-66`); the column is resolved to
  **UTF-16** via `snapshot.utf16_column_bounded` (`:909-917`), and an
  over-budget line is refused with `CompletionScopeError::LineTooLong`
  (`:874-881`).
- **Popup state:** `EditorCompletionState` (`src/tui/editor_completion.rs:70-93`)
  on `EditorState.completion` (`src/tui/app/state/editor.rs:303`).
- **Rendering:** `EditorWidget::render_completion_popup`
  (`src/tui/components/editor.rs:798`) is called unconditionally from
  `render` (`:152`) — **the popup already renders in Insert mode**.
- **Acceptance:** `accept_selected_completion` (`:1125`) re-validates scope,
  generation, attachment identity and revision against *current* state, and
  submits one transaction through the shared edit path — one undo unit.

**The defect this milestone must fix.** `handle_completion_key` (`:1085-1114`)
claims `Enter` → accept, `Up` → `select_previous`, `Down` → `select_next`
**with no mode check**; only `j`/`k` are gated on `normal_mode`
(`:1102-1109`). So in Insert mode, with the popup open, **`Enter` accepts
the completion instead of inserting a newline**, and `Up`/`Down` move the
popup selection instead of the cursor. The catch-all (`:1110-1113`) dismisses
the popup on any other key, so a printable both dismisses and types — that
part is right.

Today this is reachable only through `F8` in Insert mode. A `Ctrl-x` in
Insert mode makes it routine.

### 3.9 A second mode-blind panel: the LSP diagnostics panel

`handle_lsp_panel_key` (`src/tui/commands/editor.rs:2411-2445`) claims
`KeyCode::Char('j')`, `KeyCode::Char('k')`, `KeyCode::Char('r')` and
`KeyCode::Enter` **with no mode check**. With the panel open, `j`/`k`/`r`
in Insert mode move the panel instead of typing, and `Enter` jumps to a
diagnostic. That breaks the documented promise in `docs/tui.md:997-1016`
(*"In `INS` a printable is always text, never a search target"*).

`handle_lsp_panel_key` is stage 3 of `handle_editor_key` (`:2056-2058`),
**above** the buffer dispatch, so it wins.

### 3.10 The key-authority guard, and why it must be taught

`scripts/check_tui_editor_key_authority.py` attributes every chord to one of
three stages: the guard block, `handle_normal_key`, `handle_insert_key`. It
currently reports (verified by running it):

```
2 guard-block chord(s), 66 Normal-mode chord(s), 1 Insert-mode chord(s), no shadowed arm
```

The one "Insert chord" is the **wildcard** `KeyCode::Char(ch)` catch-all.
Every `Backspace`/`Delete`/`Enter`/`Tab`/arrow arm is **out of scope** for
the guard (`NON_CHAR_KEYCODE_RE`, `scripts/check_tui_editor_key_authority.py:243`).
  **W1 corrected this:** the tuple branch creates a `KeyCodeClaim` for
  non-`Char` codes and rule 2 applies to them, so `Alt-Backspace` and
  `Alt-Delete` are attributed rather than out of scope.

- **Rule 1** (`:566-581`): a chord declared in a mode table that the guard
  block already claims in a shared mode is **unreachable → violation**.
  So `Ctrl+s`/`Ctrl+r` in an Insert table is a build failure, by design.
- **Rule 2** (`:584-601`): two arms of one table claiming the same
  `(modifier, char)` → violation.
- **The guard cannot parse an Insert-mode `Char` chord as the table is
  written today.** `match_table` locates the Insert table with
  `re.compile(r"match\s+key\.code\s*\{")`. An arm spelled
  `KeyCode::Char('w') if key.modifiers == KeyModifiers::CONTROL =>` yields no
  `(modifiers, code)` alternative for `ALT_RE` (`:139-142`), so
  `CHAR_LITERAL_RE` matches → `ParseError`, **exit code 2** (`:443-450`,
  `:538-545`).
- **Restructuring to `match (key.modifiers, key.code)` also fails today**,
  because `match_table` cannot find `match key.code {`
  (`match_table_any`, `scripts/check_tui_editor_key_authority.py:444`) *and*
  the `Char(ch)` catch-all would be skipped by `CATCHALL_CHAR_RE` (`:240`),
  silently dropping the wildcard from the model.
  **W1 corrected this**, and also found that `arm_blocks` cut an arm at its
  `=>`, so the real Insert table's catch-all handed its orphan braces to
  the next arm. See §6.5.
- The guard models a guard chord as mode-independent or **Normal-only**
  (`NORMAL_ONLY_RE`, `:126`; `collect_guard_chords`, `:403-411`). **There is
  no Insert-only class.**

The script's own doctrine (`:71-77`, `:541-543`): *"Teach the guard the new
shape rather than skipping it"*, and an unresolvable arm shape is exit 2,
never a pass.

### 3.11 The insert-mode sweep

`printable_characters_reach_the_document_in_insert_mode`
(`src/tui/commands/editor.rs:5541-5613`) sweeps 26 letters × 3 encodings
(78) + 15 punctuation = **93** cases, and asserts for each that the key was
`Consumed` **and** that the document text grew by exactly that one character
(`:5609-5612` asserts `inspected == 93`).

> The M001 closure says "91 assertions"; the count moved to **93** when `[`
> and `]` joined in M006 (`src/tui/commands/editor.rs:5576-5581`). The
> current code asserts 93 and the closure record is stale on this number.

The sweep exercises only `NONE`/`SHIFT` modifiers, so a `CONTROL`/`ALT`
chord cannot break it — **unless** the catch-all's guard is widened, which
would both insert a literal instead of claiming the chord *and* make every
new arm unreachable.

### 3.12 The `help_overlay.rs` drift is **not** a doc surface

`src/tui/components/help_overlay.rs` still advertises `("Ctrl+R (redo)", …)`
(`:213`) and `("R", "Editor: reload from disk…")` (`:214`). Both are wrong
today (redo is `U`; `R` is `replace_with_yanked`; reload is `:e`).

**This file is unreferenced dead code** — it is absent from
`components/mod.rs`, so it is never compiled. `architecture/tui.md:3621-3623`
says so explicitly, and the M001/M002/M003/M013 closures all carry it as a
recorded residual. **It is not edited by this milestone and it is not a
documentation surface.** The live surfaces are `docs/tui.md`,
`architecture/tui.md`, and the shipped `/help`, which
(`architecture/tui.md:3619-3621`) **lists global bindings only and names no
editor chord at all**.

## 4. Invariants that must not regress

1. **One owner per chord, in stage order.** The guard block owns
   `Ctrl+s`/`Ctrl+r` in both modes, forever, unless §6.1 says otherwise.
2. **One prefix field.** Unchanged by this milestone, but the guard stays
   green.
3. **In Insert mode a printable is text.** No panel may claim a bare
   character. This is the invariant §6.3 and §6.6 restore.
4. **The Insert-mode sweep stays at 93.** The catch-all's guard must keep
   excluding `CONTROL` and `ALT`.
5. **The undo entry keeps both directions.**
   `src/tui/editor.rs:300-304` (*"An entry retains **both** directions so
   moving it between the undo and redo stacks is exact"*) and
   `architecture/tui.md:3208-3218`.
6. **No retained text in the editor path.** `scripts/check_tui_editor_text_authority.py`
   forbids a struct field named `text`/`content`/`lines`/`body`/`buffer`/
   `rope`/`source`/`document`, forbids an owned `DocumentSnapshot` or
   `DocumentBuffer`, and forbids any filesystem read in the editor path.
   A coalescing mechanism must not retain a `String` of the checkpointed
   text; bounded `TextTransaction`s are permitted (`:52-55`).
7. **One mode axis, not two.** `architecture/tui.md:3220-3233`. The editor
   has `InputMode` (the composer's axis) and `EditorMode` (the editor's),
   kept in sync at `:2227`, `:1946-1950` and `src/tui/editor.rs:3950`. This
   milestone adds **no** third axis.

## 5. Scope

### In scope

- The nine chords in §1, bound in **Insert** mode only.
- The decision on `Ctrl+s` and `Ctrl+r` (§6.1), and its documentation.
- The insert-session **undo coalescing** that delivers Helix's checkpoint
  capability without the chord (§6.2).
- The completion popup's **mode-aware** key behaviour (§6.3).
- The LSP panel's **mode-aware** key behaviour (§6.3).
- The key-authority guard taught to parse the new arm shape (§6.5).
- The two new deletion builders: kill-to-line-start, delete-word-forward.
- The doc surfaces in §12.

### Out of scope, each with a named owner

| Deferred | Why | Owner |
|---|---|---|
| Helix's `Ctrl-x c` / `Ctrl-x f` / `Ctrl-x l` **sub-chord** completion menu | The milestone's deliverable is the *trigger*; a sub-chord grammar needs a second capture and a second popup mode | A later editor milestone; recorded as a divergence |
| `Ctrl-r` insert register content | **No register exists** (§3.5); the roadmap defers named registers; the OS clipboard is already reachable via `p`/`P` | The deferred named-registers workstream |
| An explicit mid-insert-session undo checkpoint chord | `Ctrl+s` is the app's save chord; see §6.1 | This milestone declines it and delivers the capability by coalescing instead |
| Named registers (`"<reg>"`) | Roadmap non-goal | Deferred, as the roadmap already records |
| Rebinding the **global** `Ctrl+w` / `Ctrl+k` / `Ctrl+d` / `Ctrl+u` | An app-wide ergonomics change that reaches session close, session clear and paging — outside an editor-keymap milestone | A session-lifecycle milestone |
| The `help_overlay.rs` drift | Unreferenced dead code; explicitly not edited | Carried as a recorded residual |
| `Ctrl+Shift+<letter>` and `Ctrl+Alt+<letter>` chords | Unreachable in legacy xterm encoding (`src/tui/input.rs:779-800`); `Ctrl+Alt+s` is already `FocusSidebar` (`:803-809`) | — |

### The Helix divergences this milestone records

- `Ctrl-s` and `Ctrl-r` keep their app meanings in Insert mode.
- `Ctrl-x` opens the existing LSP completion directly, not a sub-chord menu.
- `Ctrl-w` / `Ctrl-u` / `Ctrl-k` / `Ctrl-d` are **editor-route-scoped** in
  Insert mode; the same chords keep their global meanings everywhere else.

All three are recorded in `docs/tui.md`, `architecture/tui.md` and the
`CHANGELOG.md` entry — not left to be discovered.

## 6. Required production changes

### 6.1 The `Ctrl+s` / `Ctrl+r` decision

**Decision: `Ctrl+s` stays `RequestSave` and `Ctrl+r` stays
`RequestResync`, in every mode, unchanged.** The Helix insert-mode meanings
are declined.

The reasons, in the order they matter:

1. **`Ctrl+s` = save is a documented user-facing promise** in two places
   (`architecture/tui.md:3302-3303`, `docs/tui.md:728-731`) and is pinned by
   three live tests (§3.4). Re-scoping it to Normal-only would break a
   promise and three tests to buy a chord whose capability is delivered
   better in §6.2.
2. **The checkpoint capability does not need the chord.** Helix's own
   documentation says the insert-mode mechanic is *"Changes to the text are
   only saved for undos when escaping from insert mode to normal mode."*
   That is §6.2's coalescing, and it is **strictly more useful** than a
   manual checkpoint: the reader gets one undo step for the whole typing
   burst without pressing anything.
3. **`Ctrl-r` has no substrate.** There is no register (§3.5). Whatever the
   chord, there is nothing to insert that is not the OS clipboard, and
   `p`/`P` already paste that.

The **alternative** — making the two guard chords Normal-only, which the
guard script can already model (`NORMAL_ONLY_RE`, `:126`) — is recorded in
the closure as the option rejected, with its cost: three live tests change,
and a documented promise is narrowed. If a maintainer prefers it, that is a
**separate decision** with its own test changes, not a silent edit inside
this milestone.

### 6.2 Insert-session coalescing — delivering the checkpoint capability

When Insert mode is entered, the editor starts an **insert session**. Every
edit inside it is recorded as one growing entry rather than one entry per
keystroke. On leaving Insert mode (Escape, or any command that returns to
Normal), the session is **committed** as a single undo entry.

- The merged entry needs **one forward transaction** covering all the
  session's insertions and **one inverse** covering their removal.
  `EditorEdit.inverse` is currently computed per edit by `invert(snapshot,
  &transaction)` (`src/tui/editor.rs:3289`), so the session needs a merged
  inverse builder, or the session's edits must be retained as bounded
  transactions and merged at commit.
- `push_undo` clears the redo stack on every push (`:4300-4310`). The
  session must clear it **once**, on the first edit of the session — a
  reader who undoes and then types must not have the redo tail resurrected.
- `clear_history` runs on reconciliation (open/reload/resync/close) at
  `src/tui/document_session.rs:239` and `src/tui/commands/editor.rs:1412,
  4742, 4776, 6155, 8494` — **five** sites, not four. An **open** session
  must be discarded there, not
  committed: a reload must not resurrect the pre-reload text as an undo
  step.
- The session's retained transactions must be **bounded** by
  `MAX_EDITOR_UNDO_BYTES` and must not become a retained-text field
  (invariant 6).
- `EditorUndoEntry.bytes` accounting and `trim_history` (`:4344-4373`) must
  treat a session as one entry with the summed bytes.

**Interaction with M012.** M012 owns the undo *history* (a time cursor over
the stacks). Coalescing changes the **granularity** of an entry. M012 must
therefore not assume "one entry per keystroke", and M011's work package must
land **before or with** M012's traversal work, or M012 must be re-derived.
This is recorded as a **sequencing note** in §14 and in the registry.

### 6.3 Mode-aware panels

**Completion popup.** `handle_completion_key` (`:1085-1114`) must become
mode-aware:

| Key | Normal mode, popup open | Insert mode, popup open |
|---|---|---|
| `Enter` | accept the candidate | **insert a newline** |
| `Up` / `Down` | move the candidate selection | **move the cursor** |
| `Tab` / `Shift-Tab` | — | move the candidate selection |
| `Esc` | dismiss | dismiss, **staying in Insert** |
| any printable | dismiss, then the key is handled by the table | dismiss, then the key **types** |

`Ctrl-x` in Insert mode opens the popup and **stays in Insert**. The
candidate selection is moved with `Tab` and cycled backwards with
`Shift-Tab`; `Enter` always inserts a newline in Insert mode — that is the
hard, testable rule. (This paragraph originally said the candidate was
*accepted* with `Tab`, which contradicted the table above it and acceptance
criterion 4, and contradicted Helix's own completion menu, which binds `Tab`
to *Next entry*. See §17.1.)
The implementation must check Helix's own insert-mode completion keys and
**record** any divergence from the table above.

**LSP diagnostics panel.** `handle_lsp_panel_key` (`:2411-2445`) must not
claim a bare character or `Enter` in Insert mode. The panel's `j`/`k`/`r`/
`Enter` claims are gated on Normal mode, exactly as the completion popup's
`j`/`k` already are (`:1102-1109`).

Both changes are required by invariant 3 and by the documented promise in
`docs/tui.md:997-1016`.

### 6.4 The two new deletion builders, and the line-boundary rule

| Chord | Builder |
|---|---|
| `Ctrl-k` | **existing** `EditorCommand::DeleteToLineEnd` (`src/tui/editor.rs:4219-4226`) |
| `Ctrl-u` | **new** — delete `line_start(snapshot, cursor_line(snapshot, cursor))..cursor` |
| `Ctrl-w`, `Alt-Backspace` | **new** — delete the previous word, **stopping at the line boundary** |
| `Alt-d`, `Alt-Delete` | **new** — delete the next word, **stopping at the line boundary** |
| `Ctrl-h` | **existing** `editor_delete_backward` (`:1916-1936`) |
| `Ctrl-d` | **existing** `EditorCommand::DeleteChar` (`:4211-4218`) |
| `Ctrl-j` | **existing** `editor_insert_literal(state, "\n")` (`:2826-2829`) |

**The line-boundary rule.** `word_backward` / `word_forward`
(`src/tui/editor.rs:1963` / `:1939`) are **buffer-wide** (§3.7). Helix's
insert-mode word deletions stop at the line boundary. A Helix `Ctrl-w` at
column 0 deletes nothing; the buffer-wide helper would delete the previous
line's last word. The new builders therefore use a **line-clamped** word
scan: the scan is bounded by the current line's start/end, not the
document's.

The scan must reuse `is_word_byte` (`:1901`) / `is_long_word_byte` (`:1913`)
so the word definition matches the `w`/`b`/`e` motions, and must step by
`next_char_boundary` (`:1572`) so a multi-byte character is never split.
`EditorCommand::DeleteWord` (`:4251-4261`) is **not** reused as-is: it
deletes the word *plus the following whitespace*, which is not Helix's
`Ctrl-w`.

The existing `EditorCommand::DeleteWord` keeps its own behaviour (it is
M008's `dw`); the new builders are separate.

### 6.5 Teaching the key-authority guard

`scripts/check_tui_editor_key_authority.py` **must** be taught the new
shape. Three changes:

> **W1 LANDED — and it also found a parser defect that had to be fixed
> first.** The guard's `arm_blocks` cut an arm at its `=>`, so the real
> Insert table's catch-all — whose `=>` and `{` sit on separate lines —
> handed its orphan braces to the *next* arm: `KeyCode::Backspace` parsed as
> `'{ editor_insert_literal(...); true } KeyCode::Backspace'`. The arms now
> wait for their body to close, and two loud checks (a brace in a pattern, a
> range pattern) make any future mis-split exit `2` rather than silently
> vanish. Without that fix the guard was not reading the Insert table it
> claimed to police.

1. **Locate the Insert table by its new form.** After the restructure below,
   the Insert table matches `(key.modifiers, key.code)` like the Normal
   table. `match_table` (`:263-265`) must find it, and `CATCHALL_CHAR_RE`
   (`:470-471`) must still recognise the `Char(ch)` wildcard in the tuple
   form so the wildcard is not silently dropped from the model.

   > **W1 resolution.** The lookup is now `match_table_any(insert_body,
   > (TUPLE_TABLE_RE, LEGACY_TABLE_RE))` — it accepts **both** shapes,
   > because the production code still uses the legacy form until W4 lands.
   > The two headers are mutually exclusive by construction: `LEGACY` needs
   > `key.code` straight after `match`; `TUPLE` needs `(`, `key.modifiers`,
   > `,`, `key.code`. The Normal lookup stays tuple-only, so the two tables
   > cannot be confused.

2. **Recognise `Char` arms with a modifier predicate** in the Insert table,
   so `KeyCode::Char('w') if key.modifiers == KeyModifiers::CONTROL` is
   attributed to `(CONTROL, 'w')` instead of raising `ParseError`.

   > **W1 resolution.** The code alternative now accepts a *binding*
   > (`KeyCode::Char(ch)`), which it previously truncated to a bare
   > `KeyCode::Char` — that truncation is exactly why the tuple wildcard was
   > silently dropped from the model. A literal becomes a chord; a binding
   > becomes a wildcard over the spelled-out modifier, or over the arm's
   > predicate for an `m` binding, with one arm's alternatives merged into a
   > single wildcard chord. Both shapes now yield the identical model:
   > `NONE|SHIFT` plus any printable character.

3. **Recognise non-`Char` key codes with modifiers** — `Alt-Backspace`,
   `Alt-Delete`. Today the guard treats every `Backspace`/`Delete`/`Enter`/
   `Tab`/arrow arm as out of scope (`:462-464`), so `Alt-Backspace` would be
   unguarded. The plan requires the guard to attribute them, or — if that is
   not feasible — the milestone must add an explicit check that
   `Alt-Backspace` and `Alt-Delete` are claimed exactly once.

   > **W1 resolution — the plan's second option was taken.** Folding
   > key-code claims into the counted totals would change what "66
   > Normal-mode chord(s)" means, and that line is quoted in the closure, so
   > non-`Char` codes are a separate `KeyCodeClaim` that feeds duplicate
   > detection (rule 2) but is not counted. Rule 3 is an explicit check that
   > the Insert table claims `Alt-Backspace` and `Alt-Delete` **exactly
   > once** each. The check reads the same in both shapes, because a
   > modifier-less legacy arm really does claim `Alt-Backspace`.

**The restructure.** `handle_insert_key` becomes `match (key.modifiers,
key.code)`, with the `Char(ch)` wildcard spelled as an arm that keeps
`NONE | SHIFT`:

```rust
match (key.modifiers, key.code) {
    (KeyModifiers::NONE, KeyCode::Char(ch))
    | (KeyModifiers::SHIFT, KeyCode::Char(ch)) => { editor_insert_literal(state, &ch.to_string()); true }
    (KeyModifiers::CONTROL, KeyCode::Char('x')) => { … }
    …
}
```

The catch-all keeps its `NONE | SHIFT` guard, so **invariant 4 holds** and
the 93-case sweep is unaffected.

**An Insert-only guard class is not needed** for this milestone: no guard
chord changes (§6.1). If a later milestone makes a guard chord mode-specific,
the guard gains an `INSERT_ONLY_RE` counterpart to `NORMAL_ONLY_RE`.

**Non-vacuity is required.** The closure must show the guard **failing** on
(a) a duplicated Insert chord and (b) an Insert chord that shadows a
guard-block chord, by injection — not merely passing.

### 6.6 Where the chords live

The new Insert arms go in `handle_insert_key`
(`src/tui/commands/editor.rs:2766`), which is stage 4 of
`editor_buffer_key` — **after** the guard block. A `Ctrl+s`/`Ctrl+r` arm
there is unreachable, which is why §6.1 declines them.

`Ctrl-x` does **not** belong in `handle_insert_key`: the completion popup's
keys are owned by `handle_completion_key` (stage 2 of `handle_editor_key`,
`:2059`), which runs **above** the buffer dispatch and already claims `F8`.
A `Ctrl-x` arm belongs there, next to `F8`, and it must open the popup
**without** entering Normal mode — `handle_completion_key` already carries
a `normal_mode` flag (`:1084`).

## 7. Ordered work packages

- **W1 — The guard, first. LANDED.** Teach the guard the new arm shape (§6.5)
  and prove it by injection. Doing this **before** the chords exist means the
  guard is ready to catch them, not after.

  W1 delivered the teaching plus a `--self-test` mode with **9 cases**: a
  clean legacy table; a clean tuple table whose wildcard survives; a
  duplicated Insert chord in each shape (rule 2); an Insert chord shadowing
  the guard block's `Ctrl+s` in each shape (rule 1); a duplicated
  `Alt-Backspace`; a missing `Alt-Delete`; and a shape the guard cannot
  resolve. Every synthetic source goes through the **same** `analyse()`
  function that reads the real tree — not a reimplementation of it.

  The guard's output is unchanged and byte-exact:
  `2 guard-block chord(s), 66 Normal-mode chord(s), 1 Insert-mode chord(s),
  no shadowed arm`.

  **Non-vacuity is proven, not asserted.** Neutering rule 3 makes the
  self-test exit `2` with exactly the two rule-3 cases failing; neutering
  rule 1 does the same for the two shadow cases. The main guard *still
  passed* on the real tree with rule 1 neutered — which is exactly why the
  self-test is required rather than a green run.

  **One pre-existing hole is recorded, not closed:** a character chord
  sharing a modifier with a table's own wildcard catch-all is still not
  flagged as shadowed. The legacy shape's wildcard covers `NONE|SHIFT` only,
  so today's tree is unaffected. It is out of scope for W1.
- **W2 — The mode-aware panels** (§6.3). The completion popup and the LSP
  panel, with the Insert-mode rules. This is a **prerequisite** for W4: a
  `Ctrl-x` that opens a popup which then eats `Enter` would be worse than no
  `Ctrl-x`.
- **W3 — The deletion builders** (§6.4): kill-to-line-start,
  delete-word-backward, delete-word-forward, with their line-clamped scans
  and their unit tests.
- **W4 — The chords** (§6.6): the nine arms in `handle_insert_key`, plus
  `Ctrl-x` in `handle_completion_key`.
- **W5 — Coalescing** (§6.2). The largest package; it changes the undo
  model, so it lands with its own tests and its own doc updates.
- **W6 — Guards, sweep and docs** (§12). The sweep stays at 93; the doc
  surfaces; the `CHANGELOG.md` entry.

## 8. Failure, cancellation, restart, contention semantics

- **No daemon and no protocol change** for the chords themselves. `Ctrl-x`
  reuses the existing authorized completion read.
- **A refused completion** (no session scope, no read scope, a cold server)
  must surface the existing reason and leave the document untouched. The
  popup's error row already carries it.
- **Coalescing is frontend-local.** It is presentation state, cleared by
  `clear_history` on reconciliation (§6.2). A daemon round trip cannot
  observe it, and must not.
- **A reload or resync mid-insert-session discards the session** (§6.2).
  A resync clears the undo history anyway (`src/tui/editor.rs:4400`).
- **A `Ctrl-x` request in flight when the buffer changes** must not apply a
  stale candidate. `accept_selected_completion` re-validates scope,
  generation, attachment identity and revision against *current* state
  (`:1125`); that check is load-bearing and must not be relaxed.
- **The mode-independent chords keep their side effects.** `Ctrl+s` still
  clears the completion popup (`:1342`); `Ctrl+r` still clears the undo
  history.

## 9. Compatibility and migration

- **No protocol change.** `crates/codegg-protocol` is untouched.
- **No storage change.** No migration, no schema bump.
- **No daemon change.**
- **Two user-visible behaviour changes inside the editor route**, both
  documented:
  1. In Insert mode, `Ctrl+w` / `Ctrl+k` / `Ctrl+u` / `Ctrl+d` no longer
     reach the global session actions. The global bindings are unchanged
     **outside** the editor route.
  2. The completion popup no longer steals `Enter` / `Up` / `Down` in Insert
     mode.
- **The undo granularity changes** with §6.2: a typing burst becomes one
  undo step instead of one per keystroke. This is a user-visible change to
  `u` / `U` behaviour and is recorded in the `CHANGELOG.md`.
- **`src/tui/input.rs` is not changed.** The global keymap, the vim map and
  `default_help_entries` stay as they are; the editor route simply claims
  these chords first. The shipped `/help` lists global bindings only, so it
  remains accurate as a *global* list — `docs/tui.md` carries the editor
  route's table.

## 10. Required tests

Every chord is driven through the **real dispatch path** — `App::on_key` →
`handle_editor_key` → `editor_buffer_key` → `handle_insert_key` — not
through a helper. The roadmap's §9 names the reason: *"Calling `editor_redo`
directly, as the existing tests do, is what let the `Ctrl+r` shadow
survive."*

- **Chord-level** (`src/tui/commands/editor.rs`), each asserting the
  reported `BufferKey` **and** the observable document change:
  - `Ctrl-w` deletes the previous word and **stops at the line start**;
  - `Alt-Backspace` is the same gesture;
  - `Alt-d` deletes the next word and **stops at the line end**;
  - `Ctrl-u` deletes to the line start; `Ctrl-k` to the line end;
  - `Ctrl-h` deletes one character backward; `Ctrl-d` one forward;
  - `Ctrl-j` inserts a newline;
  - `Ctrl-x` opens the completion popup and the mode is **still Insert**.
- **Negative chords**, the ones §6.1 declines:
  - `Ctrl+s` in Insert still yields `BufferKey::RequestSave` and does not
    edit the document (the existing test at `:5618` covers this; it must
    still pass);
  - `Ctrl+r` in Insert still yields `BufferKey::RequestResync` and does not
    edit the document;
  - `Ctrl+s` / `Ctrl+r` are **not** present in the Insert table.
- **Mode-blindness regressions** (§6.3):
  - with the completion popup open in Insert mode, `Enter` **inserts a
    newline** and does not accept a candidate;
  - with the popup open in Insert mode, `Up`/`Down` move the cursor;
  - with the LSP panel open in Insert mode, `j`/`k`/`r` **type** and do not
    move the panel;
  - with the LSP panel open in Insert mode, `Enter` inserts a newline.
- **Coalescing** (§6.2):
  - typing five characters then leaving Insert mode produces **one** undo
    entry, and one `u` reverts all five;
  - a second `u` reverts the edit before the session;
  - a `Ctrl+r`-style resync or a reload **discards** an open session and
    does not resurrect the pre-reload text;
  - typing, undoing, then typing again leaves the redo stack empty.
- **Guard non-vacuity**: injecting a duplicated Insert chord, and injecting
  an Insert chord that shadows `Ctrl+s`, each make
  `scripts/check_tui_editor_key_authority.py` fail. Prove it by injection
  before citing the guard as evidence.
- **The sweep stays at 93.** The existing assertion is the non-vacuity
  check; it must not be edited to a new number.

## 11. Required verification commands

```bash
cd /Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261018-12255c45

cargo fmt && cargo fmt --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 cargo build --locked --bin codegg-sandbox-helper
CARGO_BUILD_JOBS=2 cargo test -p codegg --lib --no-fail-fast

python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_text_authority.py
bash scripts/verify.sh quick
```

`verify.sh full` is the wider sweep; the pre-existing 69 failing `tests/`
integration binaries are **not** this milestone's and are not fixed here (see
`plans/registry.md`, "Residual registered, not discharged by M013"). A
closure that claims a green full sweep must have one.

## 12. Documentation updates

| File | Section | What changes |
|---|---|---|
| `docs/tui.md` | `### Where your input goes` (`:243-294`) | the Insert-mode chord table |
| `docs/tui.md` | `## Editor language features` (`:715-738`) | the *"Two chords work in either mode"* paragraph (`:728-731`) — qualify it with the declined Helix meanings |
| `docs/tui.md` | `### Completion` (`:1062-1083`) | `Ctrl-x`; the Insert-mode popup rules |
| `docs/tui.md` | the Insert-mode printable promise (`:997-1016`, `:1035`) | unchanged in wording, but the panel fix makes it true |
| `architecture/tui.md` | `#### Key dispatch authority: ordered stages, one owner per chord` (`:3293-3340`) | the stage list, the guard-block chord set, and the `Ctrl+s`/`Ctrl+r` decision — the doc already notes at `:3320-3322` that *"Helix binds `Ctrl+r` in **Insert** mode only"*, which is exactly the tension this milestone resolves |
| `architecture/tui.md` | `#### One mode axis, not two` (`:3220-3233`) | the Insert chord list |
| `architecture/tui.md` | `#### Presentation record` (`:3208-3218`) | the coalescing contract |
| `architecture/tui.md` | `#### Normal returns BufferKey::Unhandled…` (`:3245-3254`) | the Insert-side fallthrough statement |
| `CHANGELOG.md` | `## [Unreleased]` → `### Added` / `### Fixed` | the chords, the coalescing, the panel fixes, and the three divergences |
| `.opencode/skills/tui/SKILL.md` | the editor-grammar bullet | the Insert-chord ownership rule and the guard-teaching requirement |

`AGENTS.md` requires the skill and the `architecture/` doc to move together.

**`src/tui/components/help_overlay.rs` is not a doc surface** (§3.12) and is
not edited.

## 13. Acceptance criteria (externally observable)

1. Each of the nine chords in §1 does its documented thing in Insert mode,
   and does **nothing** in Normal mode.
2. `Ctrl+s` in Insert still saves and `Ctrl+r` still resyncs; the Helix
   insert-mode meanings are declined and documented.
3. A typing burst is **one** undo step, and a reload discards an unfinished
   burst.
4. With the completion popup open in Insert mode, `Enter` inserts a
   newline, `Up`/`Down` move the cursor, and `Tab` moves the candidate
   selection.
5. With the LSP panel open in Insert mode, `j`/`k`/`r` type and `Enter`
   inserts a newline.
6. In Insert mode, a printable character always reaches the document,
   whatever panel is open.
7. `Ctrl-w` at the start of a line deletes nothing; it does not reach into
   the previous line.
8. The insert-mode sweep still asserts `inspected == 93`.
9. The key-authority guard **fails** on an injected duplicate Insert chord
   and on an injected shadow of `Ctrl+s`.
10. `scripts/verify.sh quick` passes, and the closure states the new chord
    counts.

## 14. Stop conditions

Stop and report rather than continue if any of these holds.

- **Coalescing (§6.2) turns out to require changing `EditorUndoEntry`'s
  shape** rather than its construction. The entry's "both directions"
  invariant is load-bearing for M012; a shape change is an architecture
  decision, and the plan's alternative — no coalescing, and a documented
  divergence from Helix — must be taken explicitly rather than by drift.
- **The key-authority guard cannot be taught the new shape** without
  weakening what it catches. The guard's value is that an unparseable arm is
  exit 2, never a pass; a weakened guard is worse than no new chords.
- **A chord cannot be given exactly one owner.** A chord reachable from two
  stages, or from a stage that runs after another stage already claims it,
  is the M001 defect class.
- **The insert-session coalescing and M012's history traversal cannot both
  hold.** M012 must not assume one entry per keystroke; if M011's entry
  granularity makes M012's design materially different, the two milestones
  must be re-sequenced (§16) rather than one being bent to fit.
- **A panel's key claims cannot be made mode-aware** without changing the
  panel's Normal-mode behaviour.
- **The `Ctrl+w` / `Ctrl+k` route-scoped shadow is judged unacceptable.**
  That is a product decision for the maintainer; the alternative is
  `Alt-Backspace` / `Alt-d` only, with a recorded partial parity.

## 15. Closure evidence required

- A closure record at
  `plans/closure/tui-editor-helix-parity/011-status.md` with: the baseline
  SHA, the work packages landed, the **new** key-authority chord counts
  (the guard's printed "2 / 66 / 1" line changes), the guard outputs, the
  test names and counts for every suite in §11, and the exit conditions
  with their evidence.
- The **`Ctrl+s` / `Ctrl+r` decision**, with the rejected alternative
  recorded and the three tests that continue to pin the mode-independent
  promise.
- The three recorded divergences, each with its doc reference.
- The **guard-injection proof** from §10, with the command and the observed
  failure.
- An explicit statement of whether the **undo granularity change** landed,
  and what `u` / `U` now do across an insert session.
- A note on the `help_overlay.rs` residual, carried forward unchanged.

## 16. Handoff notes

- **The interesting part of this milestone is not the chords — it is the
  three conflicts.** The roadmap names one (`Ctrl+s` / `Ctrl+r`); the code
  shows three more (`Ctrl+w`, `Ctrl+k`, `Ctrl+u` against global session
  actions), and one of them (`Ctrl+w`) is a live test today. A reviewer
  should check §3.3 and §5 rather than trust that §1's nine chords are
  free.
- **The panel blindness is the defect most likely to bite.** `Ctrl-x` in
  Insert mode is a *routine* path to a popup that currently eats `Enter`.
  W2 lands before W4 for that reason.
- **M011 and M012 share the undo model.** Coalescing changes the
  granularity of an entry; M012 traverses the entries. M011 should land
  first, or M012 must be re-derived against the coalesced entries. This is
  the one sequencing constraint this plan imposes on another milestone.
- **The register question is settled by absence, not by a decision.**
  There is no register, the roadmap defers named registers, and `p`/`P`
  already paste the OS clipboard. `Ctrl-r` is declined because there is
  nothing to insert — not because the chord was contested.

## 17. Implementation decisions and recorded deviations (2026-10-19)

Recorded here rather than left implicit, per the planning rule that a plan
which disagrees with the code is a defect.

### 17.1 The plan's own prose contradicted its own table, and the table won

§6.3's prose says *"The candidate is accepted with `Tab` (and `Shift-Tab`
cycles)."* §6.3's **table** says `Tab` / `Shift-Tab` **move the candidate
selection** in Insert mode, and acceptance criterion 4 says the same. The
prose is the outlier: Helix's completion menu binds `Tab` to *Next entry* and
`Shift-Tab` to *Previous entry*, not to accept.

**Decided:** the table and criterion 4 are implemented. `Tab` moves the
candidate selection forward, `Shift-Tab` backward, in Insert mode. The prose
in §6.3 is corrected to match.

### 17.2 The catch-all character arm is last, not first

§6.6's snippet puts `(NONE|SHIFT, Char(ch))` first. It is implemented **last**.
The tuple match makes the two orders equivalent today — `(CONTROL,
Char('d'))` and `(NONE, Char(ch))` cannot both match one key — but a table
whose correctness depends on the reader reading it in the right order is a
table that will not stay correct. The guard does not care about the order.

### 17.3 The insert table's plain keys accept `NONE` and `SHIFT` only

Before this milestone the insert table matched `KeyCode::Backspace`,
`KeyCode::Delete`, the arrows, `Enter` and `Tab` **regardless of modifier**,
so `Ctrl+Backspace` and `Ctrl+Left` were the editor's. They are not any
more: the plain arms spell out `NONE` and `SHIFT` (the latter is how a
terminal reports `Shift-Backspace`), and a modified plain key is now an
unclaimed insert chord — which §3.3 says is a **global** action, and which is
Helix's arrangement. No Helix insert-mode binding is lost: Helix binds
`Ctrl+Backspace` to nothing.

### 17.4 The word deletions remove the separating whitespace too

§6.4 says "delete the previous word" and "delete the next word" without
saying what happens to the whitespace between the word and the cursor.

**Decided:** the deleted range is the word *and* the whitespace that separated
it from the cursor — the range a shell's `Ctrl-w` removes, and the range that
makes `Alt+d` at the end of a word take the next word whole. "the word" alone
would leave `Ctrl+w` removing `world` and leaving a trailing space, which
reads as a chord that half-worked.

### 17.5 The coalesced run is one **contiguous insertion**, and only that

§6.2 offers two shapes: a merged inverse builder, or the session's
transactions retained and merged at commit. The second was taken, and it
constrains what may join the run.

A `TextTransaction`'s edits are **all expressed in the coordinates of one
base text** (`DocumentBuffer::apply` walks them in reverse order against the
original). Two insertions at the same base place therefore cannot both be
expressed in one transaction, and the application order of same-place
insertions is not the order they were typed in. A run that is not one
contiguous insertion has no single merged form.

**Decided:** an edit joins the run only when it is a pure insertion at the
end of the run so far. A deletion, a replacement, or an insertion anywhere
else **ends** the run: the run is committed as one entry and the new edit
starts a fresh one. This is exact by construction, needs no inverse builder,
and keeps `EditorUndoEntry`'s shape — the stop condition in §14 was not
reached, because the constraint was met by narrowing what a run is rather
than by changing the entry.

### 17.6 The guard's Insert chord count is 8, not 9

The roadmap's §13 stage table says the Insert chord count goes `1 → 9`. It
goes **`1 → 8`**.

The guard counts a chord as a *character* claim. The nine chords of §1 are
seven character chords (`Ctrl+h`, `Ctrl+d`, `Ctrl+j`, `Ctrl+k`, `Ctrl+u`,
`Ctrl+w`, `Alt+d`), two **key-code** claims (`Alt+Backspace`, `Alt-Delete`,
which the guard checks under rule 3 for exactly-once ownership and does not
count), and `Ctrl+x`, which lives in `handle_completion_key` — a different
dispatch stage, and one the guard does not scan. With the wildcard
character arm, the count is `1 + 7 = 8`.

**The roadmap's 9 is corrected by amendment**; the Normal-mode count is
unaffected, and M012's acceptance criterion pins that one, not this one.

### 17.7 Helix's insert-mode behaviour, checked and recorded

§6.3 requires the implementation to check Helix's own insert-mode completion
keys and record the divergences. Helix's published keymap
(`docs.helix-editor.com/keymap.html`, "Insert mode" and "Completion Menu"):

| Helix | CodeGG | Verdict |
|---|---|---|
| `Ctrl+w`, `Alt+Backspace` → delete word backward | same | match |
| `Alt+d`, `Alt+Delete` → delete word forward | same | match |
| `Ctrl+u` → kill to line start | same | match |
| `Ctrl+k` → kill to line end | same | match |
| `Ctrl+h`, `Backspace` → delete char backward | same | match |
| `Ctrl+d`, `Delete` → delete char forward | same | match |
| `Ctrl+j`, `Enter` → insert newline | same | match |
| `Ctrl-x` → completion | same | match |
| `Ctrl-s` → commit undo checkpoint | `Ctrl+s` saves | **declined** (§6.1) |
| `Ctrl-r` → insert register | `Ctrl+r` resyncs | **declined** (§6.1) |
| completion menu: `Tab` → next entry | same | match |
| completion menu: `Shift+Tab` → previous entry | same | match |
| completion menu: `Enter` → accept the completion | `Enter` is a newline in `INS` | **divergence** |
| completion menu: `↑`/`↓` → previous/next entry | `↑`/`↓` move the cursor in `INS` | **divergence** |
| completion menu: "any other keypresses result in the completion being accepted" | a printable **types** and dismisses the popup | **divergence** |

The three divergences are all the same decision, and it is the plan's
invariant 3: **in Insert mode a printable is text.** Helix's completion menu
takes the keypress, and the character the reader typed is lost with it. The
divergence is recorded in `docs/tui.md` and `architecture/tui.md`.

