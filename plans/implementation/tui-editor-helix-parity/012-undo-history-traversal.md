# TUI editor Helix parity Milestone 012 — Undo history traversal

Status: ready for handoff
Repository baseline: `a8778e87c` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 M012
Long-term requirements: `plans/000-long-term-specification.md` (IDE-grade
language intelligence); `plans/002-long-term-roadmap.md` (editor capability
sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
Primary class: capability

## 1. Objective

Replace the two undo/redo stacks with **one history and a time cursor**, and
bind Helix's history gestures.

| Chord | Helix command | Effect |
|---|---|---|
| `Alt-u` | `earlier` | move the history cursor **back** one entry |
| `Alt-U` | `later` | move the history cursor **forward** one entry |

`u` (undo) and `U` (redo) keep their meanings and become the same cursor
movements, because that is what they already are in the two-stack model.

The deliverable is the **model**. A cursor that nothing can observe is a
rename, so the milestone also makes the cursor's position **visible** and
makes the traversal boundaries **report** rather than do nothing silently.

## 2. Why this milestone is ready

- **Its dependency is discharged.** The roadmap's M012 entry lists no hard
  dependency; the registry records it as *"eligible for fresh planning after
  M002 (may run in parallel)"*, and M002 is closed
  (`plans/closure/tui-editor-helix-parity/002-status.md`).
- **The model it replaces is entirely frontend-local.** The module doc at
  `src/tui/commands/editor.rs:13-15` states: *"Undo is frontend-local. An
  undo step is the inverse `TextTransaction` applied through `apply_local` —
  never a protocol operation."* There is no daemon, no protocol and no
  storage change.
- **The entry already retains both directions.**
  `EditorUndoEntry` (`src/tui/editor.rs:1306-1324`) holds `forward` and
  `inverse` `TextTransaction`s, and its doc says so: *"An entry retains
  **both** directions so moving it between the undo and redo stacks is
  exact"* (`:300-304`). A cursor model is strictly simpler than moving
  entries between two stacks, because **an entry never moves at all**.
- **The chords are free.** `Alt-u` and `Alt-U` are claimed by nothing
  anywhere in `src/` (§3.4).

## 3. Current implementation evidence

Verified by reading the tree at `a8778e87c` with the M013 work applied.

### 3.1 The model: two stacks, two byte counters

`TuiDocumentPresentation` (`src/tui/document_session.rs:209-216`) holds:

```rust
pub undo: Vec<EditorUndoEntry>,
pub redo: Vec<EditorUndoEntry>,
pub undo_bytes: usize,
pub redo_bytes: usize,
```

`EditorUndoEntry` (`src/tui/editor.rs:1306-1324`):

```rust
pub struct EditorUndoEntry {
    pub forward: TextTransaction,   // redo applies this
    pub inverse: TextTransaction,   // undo applies this
    pub cursor_before: usize,
    pub selections_before: Vec<Selection<EditorPoint>>,
    pub bytes: usize,
}
```

`EditorUndoEntry::rebased(self, cursor_before)` (`:1334-1339`) — the two
transactions are **direction-labelled, not stack-labelled**; they are never
swapped.

### 3.2 The API is stack-shaped

| Function | Location | Shape |
|---|---|---|
| `push_undo(presentation, forward, inverse, bytes, cursor_before, selections_before)` | `src/tui/editor.rs:4292-4311` | push one entry, **clear the redo stack** |
| `take_undo(presentation)` | `:4315-4319` | pop the newest undo entry |
| `take_redo(presentation)` | `:4322-4326` | pop the newest redo entry |
| `restore_undo(presentation, entry)` | `:4329-4335` | put an entry back on the undo stack |
| `record_redo(presentation, entry)` | `:4338-4342` | put an entry on the redo stack |
| `trim_history(presentation)` | `:4344-4373` | evict from the bottom of each stack |
| `clear_history(presentation)` | `:4400-4415` | drop everything, **and the armed prefix** |

`push_undo` has **exactly one production call site**:
`src/tui/commands/editor.rs:1566`, inside `submit_edit`. Every one of the
seven edit producers funnels through `submit_edit`.

### 3.3 The memory bound — and a doc/code divergence

```rust
// src/tui/editor.rs:50-58
pub const MAX_EDITOR_UNDO_DEPTH: usize = 256;
pub const MAX_EDITOR_UNDO_BYTES: usize = 4 * 1024 * 1024;
```

`trim_history` (`:4344-4373`) **evicts the oldest entry** from the bottom of
each stack; payloads are never truncated. The bound is **per stack**, so the
worst case today is 2 × (256 entries, 4 MiB).

`architecture/tui.md:3210-3212` describes it as *"256 entries and 4 MiB
**across both directions**"* — the documented contract is one cap, and the
two-stack implementation doubled it. A single history restores the documented
reading.

`bytes` is computed by `build_edit` (`:3280-3303`) as the forward inserts'
bytes **plus** the inverse inserts' bytes — retained payload bytes across
both directions.

### 3.4 The chords

| Chord | Owner | Citation |
|---|---|---|
| `u` | `editor_undo` | `src/tui/commands/editor.rs:3572-3575` |
| `U` (`NONE` or `SHIFT`) | `editor_redo` | `:3560` |
| `Ctrl+r` | resync (mode-independent guard block) | `:3100-3102` |
| `Ctrl+s` | save (mode-independent guard block) | `:3097-3099` |
| `Ctrl+u` | half-page up | `:3168-3171` |
| `Ctrl+d` | half-page down | `:3164-3167` |
| **`Alt-u`** | **nobody** | no arm matches `(ALT, Char('u'))`; `handle_normal_key` ends with `_ => BufferKey::Unhandled` (`:3653`) |
| **`Alt-U`** | **nobody** | same |

`handle_normal_key` already has the modifier aliases (`:3154-3156`):
`let none = KeyModifiers::NONE; let shift = KeyModifiers::SHIFT; let alt =
KeyModifiers::ALT;` — and eleven existing `Alt-*` arms (`Alt-.`, `Alt-;`,
`Alt-:`, `Alt-,`, `Alt-C`, `Alt-(`, `Alt-)`, `Alt-K`, `Alt-_`, `Alt-minus`,
``Alt-` ``).

**There is no `Alt-` prefix in the editor.** `EditorPrefix`
(`src/tui/editor.rs:96-123`) has exactly nine variants — `Idle`, `Goto`,
`Space`, `OpenBracket`, `CloseBracket`, `FindNext`, `FindPrevious`,
`FindTillNext`, `FindTillPrevious`, `ReplaceChar` — and no `Alt` variant.
The M006 `Alt-.` repeat slots (`last_find`, `last_selection_motion`,
`last_unimpaired`) are explicitly **not** prefix state
(`scripts/check_tui_editor_prefix_state.py:47-50`). `Alt-u` is simply an
unclaimed chord, so this milestone **adds two arms** and changes no prefix.

**The capital-encoding problem is real here.** `Alt-U` is a capital, and
terminals disagree about how they encode one: some send `Char('U')` with no
`SHIFT`, others send `Char('u')` plus `SHIFT`. M013 hit this exactly — the
`leaf_lookup_char` doc (`src/tui/commands/editor.rs:2885-2899`) records that
`space S` and `space F` *"would have been live on one terminal and dead on
the next"*. The `Alt-U` arm must accept **both** spellings, exactly as the
`U` arm accepts `NONE` and `SHIFT` today (`:3560`).

### 3.5 The apply path

`editor_undo` (`src/tui/commands/editor.rs:1762`) and `editor_redo`
(`:1850`) apply through `controller.apply_local(transaction)` (`:1807` and
`:1859`) — the **same** call `submit_edit` makes at `:1530`. Undo and redo
are not a special transaction path; they are local edits.

A **refused** apply must leave the history unchanged — pinned today by
`rejected_undo_entry_returns_to_the_history_unchanged`
(`src/tui/editor.rs:6078`).

`EditorCommand::Undo` and `EditorCommand::Redo` exist as variants
(`src/tui/editor.rs:1408-1409`) but are **never constructed**:
`grep -rn "EditorCommand::Undo\|EditorCommand::Redo" src/` finds no matches,
and `resolve_edit` ends in `_ => None` (`:4262`). They are dead code today.

### 3.6 The guards

**`scripts/check_tui_editor_key_authority.py`** (in `verify.sh quick:85-86`
and CI `:121`):

- `MODIFIERS` (`:106`) includes **`ALT`**.
- `ALT_RE` (`:139-142`) matches a `(modifier_token, code_expr)` pair, and
  `LET_BINDING_RE` (`:146-148`) + `resolve_modifier` (`:303`) resolve the
  `let alt = KeyModifiers::ALT;` binding. The M001-era defect — an
  `ALT_RE` that could not parse `KeyModifiers::CONTROL` — is fixed, so an
  `Alt` arm **is** attributed.
- Rule 1 (`:563-581`): a chord a mode table declares that the guard block
  already claims is a violation. The guard block claims only `Ctrl+s` and
  `Ctrl+r`; an `ALT` chord does not intersect `CONTROL`, so it will not trip
  this rule.
- Rule 2 (`:583-601`): two arms of the same table claiming the same
  `(modifier, char)` is a violation. `ALT+u` and `NONE+u` are different
  keys, so `Alt-u` does not collide with `u`.
- Current counts (measured): **2 guard-block, 66 Normal-mode, 1
  Insert-mode** chords. Two `Alt` arms raise Normal to **68**. The guard
  prints the counts but does not pin them.
- Unparseable arm shapes are **exit 2**, never a pass (`:538-545`).

**`scripts/check_tui_editor_prefix_state.py`** (quick `:88-89`, CI `:124`):

- `TuiDocumentPresentation` must declare **exactly one** field whose name
  contains `prefix` (`:251`, `:260-266`). The new cursor field must
  therefore **not** contain "prefix" — `history_cursor` is fine; `undo_prefix`
  would fail the guard.
- Forbidden anywhere under `src/`, including tests: `pending_command`,
  `yank_prefix_pending`, `MAX_EDITOR_PENDING_COMMAND`, `bounded_pending`,
  `push_pending` (`:111-117`).

**`scripts/check_tui_editor_text_authority.py`** (quick `:82-83`, CI `:118`):

- `REQUIRED_DECLARATIONS` (`:164-171`) includes `("tui/editor.rs", "pub
  struct EditorUndoEntry")` and `("tui/editor.rs", "pub fn
  clear_history")`. **A new history model must keep these names**, or the
  guard must be updated in the same change.
- A `TextTransaction` in a struct field is permitted by design (`:52-55`); a
  whole-document text container is not.

### 3.7 The tests that pin the current shape

| Test | Location | Note |
|---|---|---|
| `undo_and_redo_are_reachable_through_dispatch` | `src/tui/commands/editor.rs:5360` | **dispatch path** — `normal(&mut state, …)` goes through `editor_buffer_key` |
| `editor_keeps_edit_through_undo_redo_save_and_reopen` | `:4384` | **helper path** — calls `editor_undo`/`editor_redo` directly |
| `a_local_edit_undo_and_redo_clear_stale_semantic_state` | `:4845` | **helper path** |
| `a_multi_selection_edit_is_undone_and_redone_as_one_step` | `:6975` | |
| `an_edit_after_undo_truncates_the_redo_tail_and_drops_the_set` | `:7009` | |
| `undo_history_does_not_survive_reload_resync_or_close` | `:4751` | |
| `a_divergent_resync_keeps_the_draft_and_clears_undo` | `:4790` | |
| `undo_history_pushes_caps_and_records_bytes` | `src/tui/editor.rs:6026` | |
| `undo_history_drops_the_oldest_entry_past_the_depth_cap` | `:6043` | asserts `undo.len() == MAX_EDITOR_UNDO_DEPTH` |
| `undo_history_drops_the_oldest_entry_past_the_byte_cap` | `:6060` | asserts `undo_bytes <= MAX_EDITOR_UNDO_BYTES` |
| `rejected_undo_entry_returns_to_the_history_unchanged` | `:6078` | |
| `clear_history_drops_undo_redo_and_the_pending_prefix` | `:6091` | |
| `reset_for_open_clears_history_and_viewport` | `:6683` | |
| `the_eight_alt_chords_are_inert_in_insert_mode` | `src/tui/commands/editor.rs:7806-7836` | enumerates the Alt glyphs — its name says eight and its list holds **nine**; **must be extended** |
| `all_eighteen_chords_are_claimed_in_normal_mode` | `:7838-7880` | asserts `claimed.len() == 22` — **must become 24**, and the number must be **24 for a stated reason**: the `Alt-U` verb has two spellings (`Char('U')` and `Char('u')`+`SHIFT`), so the second spelling is exercised by a **separate** test rather than added to this list. Adding it here would make the number 25. Do not let it be 24 by accident. |
| `none_of_the_eighteen_new_chords_are_captured_in_insert_mode` | `:7812` | |
| `a_modified_key_never_resolves_a_leaf` | `:8736-8745` | asserts `Alt+k` resolves no leaf |

**The roadmap's §9 claim is only half true.** *"Calling `editor_redo`
directly, as the existing tests do, is what let the `Ctrl+r` shadow
survive."* M001 added a dispatch-path test (`:5360`) but did **not** convert
the helper-path ones — `:4409`, `:4862` and `:7003` still call the helpers
directly. **New M012 bindings must be driven through `editor_buffer_key` or
`App::on_key`**, and the helper-path tests are left as they are.

### 3.8 A correction to the roadmap's pointer

The roadmap's M012 entry points at `TextTransaction::invert()` around
`src/tui/editor.rs:1854-1871`. There is **no such method**. The real
function is a **free function**:

```rust
// src/tui/editor.rs:3261-3278
pub fn invert(snapshot: &DocumentSnapshot, transaction: &TextTransaction) -> Option<TextTransaction>
```

It walks the edits in order, tracking a `shift: isize` so each inverse
edit's range is expressed in **post-transaction** coordinates, and reads the
removed bytes out of the pre-transaction snapshot. The roadmap's pointer is
corrected in the amendment.

## 4. Invariants that must not regress

1. **An entry retains both directions**, and it is never swapped or
   copied. `src/tui/editor.rs:300-304`.
2. **Reconciliation clears history.** Open, reload, resync, reconnect and
   close all clear it; save does not. `architecture/tui.md:3214-3218`.
3. **A refused apply leaves the history unchanged.** The cursor must not
   move when `apply_local` returns `Err`.
4. **Undo and redo are local edits.** They go through
   `controller.apply_local` — the same call a normal edit makes — and never
   through a protocol operation.
5. **The selection set is restored.** `EditorUndoEntry.selections_before`
   carries it, and `a_multi_selection_edit_is_undone_and_redone_as_one_step`
   (`:6975`) pins that a multi-member edit undoes as one step.
6. **`EditorUndoEntry` and `clear_history` keep their names** — the
   text-authority guard requires them.
7. **One `prefix` field.** The new cursor field must not be named with
   "prefix" in it.
8. **Insert mode never captures an `Alt` chord.** The catch-all covers
   `NONE`/`SHIFT` only (`:2796-2801`), and the Alt-inert test pins it.
9. **A new edit truncates the redo tail.** Today `push_undo` clears the
   redo stack (`:4300-4310`); with a cursor, everything after the cursor is
   truncated and the cursor moves to the tip.

## 5. Scope

### In scope

- The **model**: one `history: Vec<EditorUndoEntry>`, one `history_bytes`,
  one `history_cursor`.
- The cursor's **observable** position in the editor's status surface.
- `Alt-u` and `Alt-U`, in **Normal** mode only.
- The traversal **boundaries** reporting rather than doing nothing.
- The consolidation of the memory bound to one cap, matching
  `architecture/tui.md:3210-3212`.
- The dead `EditorCommand::Undo` / `EditorCommand::Redo` variants: wired or
  deleted, never left dead.
- The doc surfaces in §12.

### Out of scope, each with a named owner

| Deferred | Why | Owner |
|---|---|---|
| **M011's insert-session coalescing** | M011 owns the *granularity* of an entry; M012 owns the *traversal* of entries. §8 sequences them | **M011** (see §14) |
| A **history panel / timeline view** | A cursor nobody can see is a rename; a cursor the reader can *navigate* is a panel. That is a larger surface with its own selection model | A future milestone, now unblocked by this one |
| **Named registers** (`"<reg>"`) | Roadmap non-goal; the clipboard is the only buffer | The deferred named-registers workstream |
| `Alt-u` / `Alt-U` in **Insert** mode | The `Alt` chords are Normal-mode gestures; the catch-all's `NONE`/`SHIFT` guard is what keeps Insert inert | — |
| **Durable** history across sessions | History is frontend presentation state; persisting it is a storage and schema decision | — |
| The **helper-path** undo/redo tests | They are pre-existing and pass; converting them is not this milestone's work | A test-hygiene milestone |

### The divergence this milestone records

**`Alt-u` / `Alt-U` report the history boundary; `u` / `U` stay silent.**
Helix's `u`/`U` and `Alt-u`/`Alt-U` are the same movement — `undo`/`redo`
versus `earlier`/`later` are two spellings of one history cursor in Helix's
model. CodeGG's editor rule is that a bound key states its outcome (the
M002 failure mode is a key that silently does nothing), so the **explicit
history gestures report** and the **undo/redo gestures keep their current
silence**. This is a CodeGG convention, not a Helix behaviour, and it is
recorded as such.

## 6. Required production changes

### 6.1 The model

`TuiDocumentPresentation` (`src/tui/document_session.rs:209-216`) replaces
four fields with three:

```rust
/// The document's edit history, oldest first.
///
/// Entries `[0..history_cursor)` are applied and undoable; entries
/// `[history_cursor..]` are undone and redoable. An entry is never moved
/// between containers — it retains both directions and the cursor says
/// which side of it the document is on.
pub history: Vec<EditorUndoEntry>,
pub history_bytes: usize,
/// Index into `history`: how many entries are applied.
///
/// A *time* cursor, not a stack boundary: it names a position in one
/// ordered history, so "earlier" and "later" are the same movement in
/// opposite directions.
pub history_cursor: usize,
```

`history_cursor` is the number of **applied** entries. `history_cursor == 0`
means "at the beginning" (everything is redoable);
`history_cursor == history.len()` means "at the tip" (nothing is redoable).

`EditorUndoEntry` is **unchanged** — same fields, same doc. A cursor model
never needs to move an entry, so the "both directions" invariant becomes
structural rather than a discipline.

### 6.2 The API

The stack-shaped functions in §3.2 become cursor-shaped ones. The names
`push_undo` and `clear_history` stay (the text-authority guard requires
`clear_history`; `push_undo` is referenced by the closure records and by
the call site at `src/tui/commands/editor.rs:1566`).

| Function | New shape |
|---|---|
| `push_undo(presentation, forward, inverse, bytes, cursor_before, selections_before)` | **truncate** `history[history_cursor..]`, adjust `history_bytes` by the truncated entries' bytes, push, set `history_cursor = history.len()`, trim |
| `undo_entry(presentation) -> Option<&EditorUndoEntry>` | the entry at `history_cursor - 1`, if any |
| `redo_entry(presentation) -> Option<&EditorUndoEntry>` | the entry at `history_cursor`, if any |
| `move_history_cursor_back(presentation) -> bool` | decrement if `history_cursor > 0` |
| `move_history_cursor_forward(presentation) -> bool` | increment if `history_cursor < history.len()` |
| `trim_history(presentation)` | **one** cap across the whole history |
| `clear_history(presentation)` | drop the history, the byte counter, the cursor, and the armed prefix |

The `take_undo` / `take_redo` / `restore_undo` / `record_redo` quartet
disappears: with a cursor, "take" and "put back" are **cursor movements**,
and the refusal case (§4.3) is "the cursor does not move" rather than "the
entry goes back on a stack".

**The truncation arithmetic must be exact.** Truncating
`history[history_cursor..]` must subtract exactly those entries' `bytes`
from `history_bytes`, using `saturating_sub` as `trim_history` already
does (`:4352-4360`).

### 6.3 The apply side

`editor_undo` (`src/tui/commands/editor.rs:1762`) and `editor_redo`
(`:1850`) keep their shape — take the entry, apply its inverse/forward
through `controller.apply_local` — with one change: the cursor moves **only
after** the apply succeeds.

Today `take_undo` pops and `restore_undo` puts back on failure
(`rejected_undo_entry_returns_to_the_history_unchanged`,
`src/tui/editor.rs:6078`). With a cursor, the refusal is: **the cursor does
not move**. That test's assertion becomes "the cursor is where it was", and
the test must be updated to say so — its *name* stays, or is renamed to
match what it now asserts.

`Alt-u` and `Alt-U` use the **same** two functions, with the difference that
they report the boundary (§6.5).

### 6.4 The chords

Two arms in `handle_normal_key` (`src/tui/commands/editor.rs:3138`):

- `(alt, KeyCode::Char('u'))` → move the cursor back one entry.
- `(alt, KeyCode::Char('U'))` **and** `(alt | shift, KeyCode::Char('u'))` →
  move the cursor forward one entry.

**The capital must be accepted in both spellings** (§3.4). The `U` arm
already does this for `NONE`/`SHIFT` (`:3560`); the `Alt-U` arm must do
the same for `ALT` and `ALT|SHIFT`. A test drives both spellings, exactly as
M013's `leaf_lookup_char` test does for `space S`.

**Both are Normal-mode only.** Insert mode's catch-all covers `NONE`/`SHIFT`
only, so an `Alt` chord is inert there — the property
`the_eight_alt_chords_are_inert_in_insert_mode` (`:7806-7836`) pins, and
that test must be extended with `u` and `U`.

`handle_normal_key` already has the `alt` let-binding (`:3156`), and eleven
existing `Alt-*` arms use it.

### 6.5 The boundary report

`Alt-u` at the oldest entry reports **"no earlier change"**; `Alt-U` at the
tip reports **"no later change"**. `u` and `U` keep their current silence.

The report uses the editor's existing notice surface, and it must be
**readable after the key is consumed** — a toast disappears; a status line
stays. The M013 precedent is the LSP panel's status line
(`EditorPendingEffect::Unavailable` renders its own line, and `NotReady` /
`Ambiguous` / `Unsupported` each render their own).

### 6.6 The observable cursor

The cursor's position must be **visible**, or "time cursor" is a rename.
The editor's status surface reports the position when the cursor is **not
at the tip** — for example `history 3/12` — and says nothing when it is at
the tip, so the common case is unchanged.

This is the smallest thing that makes the model observable, and it is the
precondition a future history panel would need. The panel itself is out of
scope (§5).

### 6.7 The consolidated bound

`trim_history` enforces **one** cap: 256 entries and 4 MiB for the history
**as a whole**, which is what `architecture/tui.md:3210-3212` already says.
The worst case today is twice that.

**This tightens the bound.** The closure must state it plainly: the maximum
retained history **halves**, and the retained bound now matches the
documented contract instead of exceeding it.

`MAX_EDITOR_UNDO_DEPTH` and `MAX_EDITOR_UNDO_BYTES` keep their names and
their values.

### 6.8 The dead `EditorCommand` variants

`EditorCommand::Undo` and `EditorCommand::Redo` (`src/tui/editor.rs:1408-1409`)
are never constructed, and `resolve_edit` ends in `_ => None` (`:4262`).

They are either **wired** — as the `PrefixAction::Edit` path for an undo
verb, if `resolve_edit` can build the inverse transaction from the snapshot
it already has — or **deleted**. A variant with no producer is a claim, not
a hook, and this milestone touches the file they live in. The closure states
which of the two happened.

### 6.9 Guard changes

- **The key-authority guard needs no change.** `ALT` is in `MODIFIERS`,
  `ALT_RE` parses the `alt` let-binding, and an `ALT` chord cannot collide
  with the guard block's `CONTROL` chords. The Normal-mode count goes
  **66 → 68**, and the closure records it.
- **The prefix-state guard needs no change**, provided the new field is not
  named with "prefix" in it (§4.7).
- **The text-authority guard needs no change**, provided `EditorUndoEntry`
  and `clear_history` keep their names (§4.6).
- **The milestone must extend two tests** that pin a *count*:
  `the_eight_alt_chords_are_inert_in_insert_mode` (`:7834`) and
  `all_eighteen_chords_are_claimed_in_normal_mode` (`:7866`, asserting
  `claimed.len() == 22` → **24**). A count that is not updated is a test
  that fails for the wrong reason.

## 7. Ordered work packages

- **W1 — The model.** The three fields, the cursor-shaped API, the
  truncation arithmetic, the consolidated bound (§6.1, §6.2, §6.7). The
  existing undo/redo behaviour must be **preserved**: `u` and `U` still
  undo and redo, still restore the selection set, still truncate the redo
  tail on a new edit, still survive no reconciliation. The existing tests
  are the evidence, and they must be updated only where the *shape* they
  assert changed (the stack fields, the counts).
- **W2 — The apply side.** The cursor moves only after a successful apply
  (§6.3), and the refusal test is updated to assert the cursor's position.
- **W3 — The chords.** `Alt-u` and `Alt-U`, both spellings of the capital
  (§6.4), Normal-mode only.
- **W4 — The reports.** The boundary report (§6.5) and the observable
  cursor (§6.6).
- **W5 — The dead variants.** `EditorCommand::Undo` / `Redo` wired or
  deleted (§6.8).
- **W6 — Docs and the sweep.** The doc surfaces in §12; the
  `CHANGELOG.md` entry; the extended count tests.

## 8. Failure, cancellation, restart, contention semantics

- **A refused apply does not move the cursor** (§4.3). The document and the
  history are both untouched.
- **Reconciliation clears the whole history and resets the cursor to 0**
  (§4.2). `clear_history` is called from `src/tui/document_session.rs:239`
  and `src/tui/commands/editor.rs:1411, 4770, 4804, 6183`.
- **A new edit after undoing truncates the tail** (§4.9): everything after
  the cursor is dropped, the cursor moves to the tip. This is today's
  behaviour (`push_undo` clears the redo stack) and
  `an_edit_after_undo_truncates_the_redo_tail_and_drops_the_set` (`:7009`)
  pins it.
- **An undo that reaches the beginning and is applied again** is a no-op
  with a report; it must not wrap around. There is no "undo the oldest
  entry's inverse" case to handle — the boundary is a boundary.
- **No daemon, no protocol, no transport.** The whole model is frontend
  presentation state (§2).
- **The cursor is per document attachment.** A new attachment starts at
  cursor 0 with an empty history.

## 9. Compatibility and migration

- **No protocol change.** `crates/codegg-protocol` is untouched.
- **No storage change.** No migration, no schema bump.
- **No daemon change.**
- **The `TuiDocumentPresentation` field shape changes** — `undo`/`redo`/
  `undo_bytes`/`redo_bytes` become `history`/`history_bytes`/
  `history_cursor`. The struct is internal to the TUI crate; the
  `codegg-client` crate and the WS protocol are unaffected.
- **The retained bound tightens** (§6.7): the worst case halves, and the
  code now matches `architecture/tui.md:3210-3212`.
- **Two new chords** are added, and **no chord is removed or rebound**.
- **Two test counts change**: the Alt-inert list gains `u` and `U`, and the
  claimed-chord count goes 22 → 24.

## 10. Required tests

Every new binding is driven through the **real dispatch path** —
`editor_buffer_key` (the `normal` / `press` helpers at
`src/tui/commands/editor.rs:4884-4903` already do this) or `App::on_key`.
The roadmap's §9 names the reason, and §3.7 records that the existing
helper-path tests are why the `Ctrl+r` shadow survived.

- **The model**:
  - `u` still undoes, `U` still redoes, and both still restore the
    selection set;
  - a multi-member edit still undoes as **one** step (`:6975` unmodified);
  - an edit after an undo still truncates the tail (`:7009` unmodified);
  - the cursor's position is `0` at the beginning and `history.len()` at
    the tip;
  - a **refused** apply leaves the cursor where it was;
  - the history is cleared, and the cursor reset, on reload, resync and
    close (`:4751`, `:4790` unmodified).
- **The chords**:
  - `Alt-u` moves the cursor back one entry and the document reflects it;
  - `Alt-U` moves it forward;
  - `Alt-U` works in **both** terminal spellings — `(ALT, Char('U'))` and
    `(ALT|SHIFT, Char('u'))`;
  - `Alt-u` at the oldest entry reports "no earlier change" and does not
    move;
  - `Alt-U` at the tip reports "no later change" and does not move;
  - `u` and `U` at their boundaries stay **silent** (the divergence in §5);
  - `Alt-u` and `Alt-U` in **Insert** mode are inert and type nothing.
- **The observable cursor**: with the cursor not at the tip, the status
  surface reports the position; at the tip, it says nothing.
- **The bound**: the history never exceeds `MAX_EDITOR_UNDO_DEPTH` entries
  or `MAX_EDITOR_UNDO_BYTES` bytes, **in total** (the existing
  `:6043` / `:6060` tests are updated to assert against the single
  history).
- **Guard non-vacuity**: an injected `Alt-u` that duplicates another arm
  makes the key-authority guard fail. Prove it by injection before citing
  the guard as evidence.

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
| `docs/tui.md` | `## Editor language features` (`:715-731`) | `Alt-u` / `Alt-U`; the distinction from `u` / `U` |
| `docs/tui.md` | `### Multi-key gestures` (`:845-859`) | the `Alt-u` / `Alt-U` gesture |
| `architecture/tui.md` | `#### Document editor (M006-A)` (`:3191-3218`) | the **Presentation record** paragraph — the history model, the cursor, and the bound (which the code now matches) |
| `architecture/tui.md` | the mode table (`:3230-3233`) | the new Normal-mode chords |
| `architecture/tui.md` | the dispatch-authority narrative (`:3300-3341`) | the `Alt-u` / `Alt-U` arms and the capital-encoding rule |
| `CHANGELOG.md` | `## [Unreleased]` → `### Added` / `### Changed` | the chords, the model change, and the **tightened** bound under `### Changed` |
| `.opencode/skills/tui/SKILL.md` | the editor-grammar bullet | the history model, if the skill names editor chords |

`AGENTS.md` requires the skill and the `architecture/` doc to move together.

`src/tui/components/help_overlay.rs` is **unreferenced dead code** and is
not a doc surface; it is not edited.

## 13. Acceptance criteria (externally observable)

1. `u` and `U` still undo and redo, still restore the selection set, still
   truncate the redo tail on a new edit, and still survive no
   reconciliation.
2. `Alt-u` and `Alt-U` move the history cursor, and the document follows.
3. `Alt-U` works on a terminal that reports capitals as `Char('U')` and on
   one that reports `Char('u')` + `SHIFT`.
4. `Alt-u` at the oldest entry and `Alt-U` at the tip say so; `u` and `U`
   stay silent.
5. The history's position is visible when the cursor is not at the tip.
6. The retained history never exceeds 256 entries or 4 MiB **in total**.
7. A refused apply leaves the cursor where it was.
8. `Alt-u` and `Alt-U` are inert in Insert mode.
9. The key-authority guard reports **68** Normal-mode chords, and the two
   count tests are updated to 24 and the extended Alt list.
10. `EditorCommand::Undo` / `Redo` are either wired or deleted — no dead
    variant survives.
11. `scripts/verify.sh quick` passes.

## 14. Stop conditions

Stop and report rather than continue if any of these holds.

- **The model change cannot preserve the existing behaviour.** If `u`/`U`
  stop restoring the selection set, stop truncating the tail, or start
  surviving a reconciliation, the model is wrong — the two-stack model is
  the baseline, not a suggestion.
- **The observable cursor cannot be added to the status surface** without a
  broader status-model change. A cursor nobody can see is a rename; that
  is a reason to stop and re-scope, not to ship the rename.
- **A refused apply would move the cursor.** That is the one behaviour the
  milestone must not get wrong.
- **The single cap would evict entries the reader still needs** in a way
  the two caps did not. The bound tightening is planned and recorded; an
  *unbounded* eviction is not.
- **M011's coalescing has not landed and M012's design assumes one entry
  per keystroke.** M012 must tolerate any entry granularity; if it cannot,
  the two milestones must be re-sequenced (§16).
- **The `Alt-U` capital cannot be accepted in both terminal spellings.** A
  chord that is live on one terminal and dead on the next is the M013
  failure mode; the plan's test must drive both.

## 15. Closure evidence required

- A closure record at
  `plans/closure/tui-editor-helix-parity/012-status.md` with: the baseline
  SHA, the work packages landed, the **new** key-authority chord counts,
  the guard outputs, the test names and counts for every suite in §11, and
  the exit conditions with their evidence.
- The **bound tightening** stated as a change: what the worst case was, what
  it is now, and the doc reference that now matches the code.
- The **existing-test disposition**: which of the undo/redo tests passed
  unmodified, and which were updated, with the reason for each.
- The **capital-encoding evidence**: the test that drives both spellings.
- The guard-injection proof from §10, with the command and the observed
  failure.
- An explicit statement of whether `EditorCommand::Undo` / `Redo` were
  wired or deleted.
- The **M011 sequencing note**: whether M011's coalescing had landed when
  this milestone did, and what that means for the entry granularity this
  milestone traversed.

## 16. Handoff notes

- **The honest framing matters here.** In Helix, `u`/`U` and `Alt-u`/`Alt-U`
  are the same movement — `undo`/`redo` and `earlier`/`later` are two
  spellings of one history cursor. This milestone does **not** claim a new
  capability it did not deliver. What it delivers is the **model** (one
  history and a cursor, so an entry never moves between containers), the
  **consolidated bound** that finally matches the doc, and the
  **observability** that makes a cursor worth having.
- **The one thing that is genuinely new to a reader** is that `Alt-u` /
  `Alt-U` report their boundary while `u` / `U` stay quiet. That is a
  CodeGG convention, and it is recorded as one.
- **The bound tightening is a real change.** The worst case retained
  history halves. That is a deliberate consequence of one history, and it
  brings the code into line with `architecture/tui.md:3210-3212`.
- **M011 and M012 share the undo model.** M011 changes the *granularity* of
  an entry; M012 changes the *traversal*. M011 should land first, or M012
  must tolerate any granularity. This is the one sequencing constraint this
  plan imposes, and it is mirrored in M011 §6.2 and §16.
- **The `Alt-U` capital is the trap.** A terminal that reports
  `Char('U')` with no modifier and one that reports `Char('u')` + `SHIFT`
  are both real. M013 hit this with `space S` and `space F`; the same
  failure here would make `Alt-U` live on one machine and dead on the next.
