# TUI editor Helix parity Milestone 007 — Multi-selection core

Status: implemented — closed 2026-10-18, see
`plans/closure/tui-editor-helix-parity/007-status.md`
Repository baseline: `84440fe7f` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 M007
Long-term requirements: `plans/000-long-term-specification.md` (TUI editor);
`plans/002-long-term-roadmap.md` (editor capability sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
Primary class: capability (enabling)

## 1. Objective

Replace the editor's single held selection with a **selection set carrying a
distinguished primary**, so that every existing selection-dependent command
applies to every member as one transaction.

This is the enabling change for the entire Helix selection vocabulary. Roughly
fifteen verbs (M008's surface, M009's match mode) are not merely unbound today —
they are *undefined*, because there is no set for them to mean anything over.

Delivered:

- `SelectionState<P>` holds `Vec<Selection<P>>` plus a primary index, while
  **every existing single-selection accessor keeps its exact signature and its
  exact meaning** (it becomes primary-scoped).
- Motions transform the whole set, preserving order.
- `v` extends all members; `x` line-growth applies per member.
- `d`, `y` / `space y`, and M004's `r` / `R` / `~` / `` ` `` / `p` / `P` emit
  **range sets** and land as **one** `apply_local` transaction.
- Rendering draws every head.
- Undo and redo restore the selection set as well as the cursor.
- Selection growth is refused on a read-only buffer, with a notice.

### 1.1 Decisions this milestone makes, stated up front

Four questions have more than one defensible answer. Recording the choice and
its reason here is cheaper than discovering the disagreement in review.

**Primary + extras, with `cursor_byte` unchanged.** `TuiDocumentPresentation`
carries `cursor_byte: usize` (`src/tui/document_session.rs:59`), which drives
the viewport, LSP reads, and undo entries. The set gains
`selections: Vec<Selection<EditorPoint>>` and `primary_index: usize`; the
primary's head **is** `cursor_byte`. No parallel cursor concept is introduced,
and every existing scalar read of `cursor_byte` stays correct. Neither new field
name contains `prefix`, so
`scripts/check_tui_editor_prefix_state.py:226-241` is unaffected.

**The single-selection API survives as primary-scoped.** This is the load-bearing
design choice. `SelectionState::selection()` keeps returning
`Option<Selection<P>>` — the primary — and `is_selected()`, `span()`, and
`line_range()` keep their signatures. That means the terminal panel's ~25 call
sites in `src/tui/interactive_terminal.rs` and the one `restore` caller at
`src/tui/editor.rs:1700` **need no mechanical rewrite**, and the rules still
run through one implementation rather than forking per surface, which is the
entire reason `helix_selection.rs` exists.

**`D` keeps its cursor-only semantics.** The roadmap's deliverable boundary says
"`d`, `space y`, `D` emit one range set". Verified: `resolve_edit`
(`src/tui/editor.rs:2525-2614`) never calls `selection_range`; `D` →
`EditorCommand::DeleteToLineEnd` (`src/tui/editor.rs:2569-2576`) reads only
`cursor_byte` and `line`. So "make `D` selection-aware" is a **new user-visible
semantic on a deliberately local extension**, not a refactor. Nothing in the
roadmap asks for it, and M004's exit conditions explicitly require the
documented interaction with the local `D` extension to be unchanged. `D` stays
cursor-only and is listed in §5 as a recorded divergence from the roadmap.

**"N cursors deletable in one gesture" is restated.** No verb that *creates* N
cursors exists until M008 (`C` / `Alt-C`, add cursor below/above). M007
therefore proves the model, not a gesture: a programmatically constructed set of
N members is deletable in one command and in one transaction. The user-facing
"select N things and delete them" gesture is M008's deliverable and cannot be
claimed here. Restated in §13 criterion 1.

## 2. Why this milestone is ready

- **M001 — closed** (`plans/closure/tui-editor-helix-parity/001-status.md`). The
  hard dependency: `check_tui_editor_key_authority.py` exists, so a binding that
  shadows a Normal-mode verb is caught before it lands rather than after.
- **Independent of M002**, per the roadmap. This milestone adds no grammar and
  no prefix.
- No ADR gates it. `SelectionState` is frontend presentation state; ADR-0011's
  text-authority boundary is untouched because every edit still lands through
  `DocumentController::apply_local`.

## 3. Current implementation evidence

### The shared model

`src/tui/helix_selection.rs` (592 lines), registered at `src/tui/mod.rs:113`:

| Item | Line |
|---|---|
| `trait SelectionPoint` (`type Line`, `fn line`) | `:31`, `:38`, `:41` |
| `enum SelectionMode { Char, Line }` | `:46` |
| `struct Selection<P>` — `anchor`, `head`, `mode`, `growing` | `:63` |
| `Selection::new` / `extends_on_move` / `line_range` / `bounds` / `contains` | `:81`, `:94`, `:99`, `:106`, `:119` |
| `struct SelectionState<P>` — `cursor: P`, `selection: Option<Selection<P>>` | `:148` |
| `new` / `restore` / `cursor` / `set_cursor` / `selection` / `is_selected` / `span` / `line_range` | `:155`, `:174`, `:179`, `:188`, `:193`, `:198`, `:203`, `:208` |
| `select_char` / `select_line` / `toggle_char` / `line_select_extends` / `grow_to` | `:213`, `:218`, `:229`, `:242`, `:250` |
| `move_to` / `repin_cursor` / `deactivate` / `translate` | `:263`, `:278`, `:291`, `:301` |

**There is no set API**: no rotate, no merge, no ordering, no multi-item
operation. M008 inherits whatever this milestone defines, so the API here is a
contract, not an implementation detail.

### Blast radius of `Option<Selection<P>>` → a set

**Breaks mechanically (8 sites assigning `None`, plus `restore`):**

- `presentation.selection = None` — `src/tui/editor.rs:1736`, `:1745`,
  `:2494`; `src/tui/commands/editor.rs:1643`, `:1759`, `:1808`, `:1881`;
  `src/tui/document_session.rs:174`.
- `SelectionState::restore(cursor, Option<Selection<P>>)` at `:174` — **one
  caller**, `src/tui/editor.rs:1700`, and it passes the stored value, never a
  self-computed one. Keeping the signature makes this a non-event.
- `src/tui/interactive_terminal.rs:1992` returns `Option<Selection<TerminalPoint>>`
  from a **panel** method; panel call sites at
  `src/tui/commands/interactive_terminal.rs:1939`, `:2710`, `:2719` depend on
  that shape. Keeping `selection()` primary-scoped preserves it exactly.

**Semantic decision, not a type error:**

- `apply_helix_verb` (`src/tui/editor.rs:1694-1785`) reads
  `state.selection()` at `:1720` to decide which head drives `x` growth, and
  writes back at `:1783`. With a set, "which head" becomes "every head".
- `selection_range()` (`src/tui/editor.rs:1658`) reads
  `presentation.selection.as_ref()` at `:1662` and returns one `Range`. It
  becomes `selection_ranges()` returning `Vec<Range<usize>>`.
- `Selection::contains` (`:119`) is per-member. With overlapping members its
  behaviour on the overlap is undefined — see §8.
- `move_to` / `repin_cursor` currently extend **one** head. They extend **every**
  member's head.

**Zero `SelectionState` accessor callers — a useful finding, stated precisely.**
`SelectionState::is_selected()`, `SelectionState::span()`, and
`SelectionState::line_range()` have **no production callers anywhere** — every
hit is inside `helix_selection.rs`'s own test module or an editor test. Do not
over-read this: the *inherent* `Selection::line_range()`
(`src/tui/helix_selection.rs:99`) **does** have two production callers, at
`src/tui/interactive_terminal.rs:766` and `:1932`. Both reach it through a
`Selection` obtained from `selection()`, so making `selection()` primary-scoped
covers them too — but the distinction between the two `line_range` methods is
the kind of thing that goes stale silently, so it is recorded rather than
collapsed. Preserving the `SelectionState` signatures is free.

### Editor storage

`src/tui/document_session.rs`: `pub cursor_byte: usize` (`:59`),
`pub selection: Option<Selection<EditorPoint>>` (`:68`),
`pub viewport_line` (`:71`), `pub desired_column` (`:96`), `undo` / `redo`
(`:151`/`:153`), reset at `:174`. Accessors at `:414` / `:418`.

`EditorPoint` — `src/tui/editor.rs:1601`:

```rust
pub struct EditorPoint { pub byte: usize, pub line: usize }
```

`impl SelectionPoint` at `:1608` with `type Line = usize`; constructed only by
`editor_point(snapshot, byte)` (`:1621`), which clamps then resolves
`cursor_line`. `Ord` is by `byte` first (`:1600`) — that is the basis for
`min`/`max` in `Selection::bounds()`, so **member ordering must be by
`EditorPoint`, not by insertion order**, or `bounds()` disagrees with the vector.

### Selection-dependent effects, today

| Verb | Path | Payload |
|---|---|---|
| `v` | `commands/editor.rs:3038` → `editor.rs:1713` `toggle_char()` | — |
| `x` | `commands/editor.rs:3042` → `editor.rs:1714-1726` | — |
| `d` | `commands/editor.rs:3048-3053` → `editor.rs:1733-1741` `selection_range()` | `BufferKey::DeleteRange(Range<usize>)` (def `:2391`), handled `:2115-2126` |
| `y` / `space y` | `commands/editor.rs:3065`, `:2767-2772` → `yank_range()` `:3288` | `BufferKey::YankRange(Range<usize>)` (def `:2394`), handled `:2127-2130` |
| `D` | `commands/editor.rs:3120` → `resolve_edit` `editor.rs:2569` | **cursor only** |
| `r`/`R`/`~`/`` ` ``/`p`/`P` | `selection_or_char_at_cursor` `editor.rs:1955`, `paste_anchor` `:1973` | via `editor_editing_verb` `commands/editor.rs:1623`; selection cleared at `:1643` |

`BufferKey` is defined at `src/tui/commands/editor.rs:~2370-2409`; the App-layer
dispatcher is `:2100-2151`.

**N edits already work — this is the good news.** `TextTransaction.edits` is a
`Vec<TextEdit>` (`crates/codegg-document/src/edit.rs:48`). `invert()`
(`src/tui/editor.rs:1854-1871`) walks **all** edits and accumulates a shift
offset, producing an N-edit inverse. `build_edit()` (`:1873`) sums bytes across
both directions and only rejects an empty transaction. `apply_local` is
transaction-level, not edit-level. So "one transaction, not a partial
multi-cursor edit" needs **no controller work at all**. The ceiling is
`MAX_DOCUMENT_EDITS = 256` (`crates/codegg-protocol/src/document.rs:7`, enforced
`crates/codegg-document/src/edit.rs:57-59`) — §8.

### Undo restores no selection

`EditorUndoEntry` is defined at `src/tui/editor.rs:1097` (not in
`document_session.rs`):

```rust
pub struct EditorUndoEntry {
    pub forward: TextTransaction,   // :1099
    pub inverse: TextTransaction,   // :1101
    pub cursor_before: usize,       // :1103
    pub bytes: usize,               // :1105
}
```

with `rebased` at `:1117` and stacks at `document_session.rs:151`/`:153`.
`editor_undo` clears the selection at `commands/editor.rs:1759` and `editor_redo`
at `:1808`. **So "undo/redo round-trips a multi-selection edit" requires a new
field** — it is not a consequence of the type change. See §6.

### Read-only refusal is missing exactly where M007 needs it

`fn editable(state)` (`commands/editor.rs:1571`) is
`is_open() && !is_busy() && is_writer() && !status.blocks_edits()`. Its call
sites are `:1595`, `:1630`, `:1710`, `:1829`, `:1848`, `:3331`, all producing
`state.set_notice(EditorNotice::from_error(&DocumentControllerError::ReadOnly))`.

**`apply_helix` (`commands/editor.rs:3275-3283`) and `helix_motion`
(`:3269-3272`) have no `editable` gate at all.** So `v`, `x`, and motions arm
and grow selections on a read-only buffer with no notice, and `d` is refused only
downstream by `controller.apply_local` returning `ReadOnly` (`:1561-1566`).
Existing read-only tests (`commands/editor.rs:6248`, `:6280`) cover only edit
verbs.

The roadmap's exit condition "read-only buffers refuse selection growth" is
therefore **genuinely new behaviour**, and the correct reading is narrow:
**growth** (`v`, `x`, and growth through motion) is refused; **plain cursor
movement is not**. A reader must still be able to move around a read-only file.

`state.notice` is a **field** — `pub notice: Option<EditorNotice>` at
`src/tui/app/state/editor.rs:274`, set via `set_notice` (`:367`), cleared via
`clear_notice` (`:373`), rendered at `src/tui/app/render.rs:995-1007`.

### Rendering has exactly two loop points

`src/tui/components/editor.rs:396` resolves `editor::selection_range(...)` once
per frame and passes `selection.as_ref()` into `line_spans` (`:474`). The
per-token selected test is `:1008-1012`, painting `theme.selection` at
`:1025-1030`. The cursor is a **single cell**: `cursor_cell` `:974`, `at_cursor`
`:1005`, style patch `:1043-1047`, definition `:1061`.

`src/tui/app/render.rs` draws **no** Helix selection; its `selection` hits are
the unrelated mouse-drag `TextSelection` band (`:516-530`) and palette rows.

So the two places that must become loops are `:474`/`:823` (paint) and
`:974`/`:1043` (heads).

### Terminal-panel parity

`src/tui/commands/interactive_terminal.rs` imports **nothing** from
`helix_selection` — it calls only the panel's own wrappers (`:1939`,
`:2060`, `:2063`, `:2070`, `:2077`, `:2710`, `:2719`).

`src/tui/interactive_terminal.rs` uses the shared type at `:110` (the `impl`),
`:534` (field `nav: SelectionState<TerminalPoint>`), `:573`/`:1547` (`new`),
`:596`/`:601`/`:611` (`cursor`/`set_cursor`/`translate`), `:763-786`
(`selection_text`), `:888`/`:915` (`styled_lines`), `:1832-1909` (the selection
verbs), `:1929-1935` (`delete_selection`), `:1955`/`:1968` (`deactivate`),
`:1985`/`:1992`/`:1998` (panel API).

Under the §1.1 design, all of these keep compiling and keep meaning what they
mean, because the panel never holds more than one member and `move_to` over a
one-member set is identical to `move_to` over an `Option`. **Parity is therefore
preserved by construction, and M007's terminal-panel obligation reduces to
running the panel's selection tests unchanged** — which is a stronger result
than a recorded exemption.

## 4. Invariants that must not regress

1. **One transaction per edit.** Every multi-selection edit lands as one
   `apply_local` call. A partial multi-cursor edit must never be observable.
2. **Text authority.** No filesystem read, no retained text buffer. Owned by
   `scripts/check_tui_editor_text_authority.py`.
3. **Insert-mode text integrity.** No new Normal-mode binding this milestone,
   but the sweep must stay green.
4. **Bounded work.** No operation may scan the whole buffer per keystroke.
   Rendering N selections is O(visible lines × members) and must stay inside the
   existing viewport bounds.
5. **Char boundary safety.** Range endpoints come from `Selection::bounds()` over
   `EditorPoint`s built by `editor_point`, which clamps. Byte-range construction
   must not split a codepoint.
6. **One prefix field.** `scripts/check_tui_editor_prefix_state.py` forbids a
   second `*prefix*` field on `TuiDocumentPresentation` (`:226-241`). The two new
   fields must not be named `*prefix*`.
7. **Terminal-panel parity.** See §3 — preserved by construction, proven by the
   panel's selection tests running unmodified.
8. **No binding moved.** This milestone adds zero chords. `R` staying
   `replace_with_yanked` and reload staying `:e` are M004's findings and must not
   be revisited here.

## 5. Scope

### In scope

- `SelectionState` set + primary, preserving the single-selection API as
  primary-scoped.
- Whole-set motion, `v` extend-all, `x` per-member growth.
- `selection_ranges()`; range-set payloads for `d` and `y` / `space y`.
- Multi-target `r` / `R` / `~` / `` ` `` / `p` / `P` through M004's existing
  funnel.
- One-transaction landing, with a cap and a report.
- Rendering every head and every member's highlight.
- `selection_before` on `EditorUndoEntry`.
- `editable` gate on selection **growth**.
- `EditorPoint`-ordered members, so `bounds()` agrees with the vector.

### Out of scope, each recorded

| Deferred | Why |
|---|---|
| `C` / `Alt-C`, `;`, `,`, `Alt-,`, `%`, `S`, `K`, `Alt-K`, `(` `)`, `&`, `Alt-;`, `Alt-:`, `Alt-(` `Alt-)`, `Alt-minus`, `Alt-_` | **M008.** Not one of these is bound here. |
| Making `D` selection-aware | §1.1. A new semantic on a local extension, unrequested. |
| `t` / `T` landing *on* the target vs `f` / `F` landing *past* it | M003's closure records this as "a selection property owed to M007". It is owed here **only** if it can be expressed without a selection set that Helix does not have. Read §16 before assuming. |
| Soft wrap, jumplist, split panes | Predecessor / roadmap non-goals. |
| Syntax-node selections | ADR-0034, proposed. |

### `t`/`T` — a correction owed from M003

M003's closure states the milestone "does not fully distinguish `t`/`T` from
`f`/`F`: per the plan's AC2 they land on the target, which at this milestone
makes them cursor-equivalent — the real distinction is a selection property and
belongs with M007."

The live keymap is unambiguous: `t` is `find_till_char` and `f` is
`find_next_char`; `T` is `till_prev_char` and `F` is `find_prev_char`. In Helix
the difference is that `t` leaves the cursor **on** the target while `f` moves
**past** it. That is a **cursor** property, not a selection property, and it
needs no set. Implementing it here is a one-line difference in the motion result.

This plan **does not silently adopt M003's deferral**, and does not silently
override it either. §14 makes it a stop condition: if the motion result type
makes cursor-landing expressible without changing `M008`'s contract, take it;
if not, report it as a one-line corrective against M003's closure. Either way
the disagreement between "owed to M007" and "needs nothing from M007" is
resolved in writing rather than inherited.

## 6. Required production changes

### `SelectionState` — the set

```rust
pub struct SelectionState<P: SelectionPoint> {
    cursor: P,
    selections: Vec<Selection<P>>,   // sorted by bounds(); never empty-but-unused
    primary: usize,                  // index into `selections`; always valid when non-empty
}
```

Invariants, all of which need a test each:

- When `selections` is empty, `primary` is meaningless and every primary-scoped
  accessor returns `None` / `false` / the previous span behaviour.
- `selections` is **sorted by `bounds()`** after every mutation. `bounds()`
  orders by `EditorPoint`'s `byte`, so sorting by `bounds().0` keeps `bounds()`
  and iteration order consistent. Insertion order is not a substitute.
- `primary` is **tracked through a sort**, not pinned to an index that a sort
  would invalidate: sorting must carry the primary member with it.
- `restore(cursor, Option<Selection<P>>)` keeps its signature and produces a
  zero- or one-member set.

### Preserved, primary-scoped

`selection() -> Option<Selection<P>>`, `is_selected()`, `span()`,
`line_range()`, `set_cursor()`, `cursor()`, `deactivate()`. Unchanged
signatures, unchanged meaning, documented as "the primary".

### Changed, whole-set

- `move_to(p)` — extend **every** member whose `extends_on_move()` holds.
- `repin_cursor(p)` — same rule, for scroll re-pinning.
- `toggle_char()` — arm on every member; when **every** member is already a char
  selection, deactivate all. The M002-era single-selection toggle becomes a
  set-wide one: `v v` on a mixed set must not disarm the char members and leave
  the line members.
- `select_line()` — arm line selection on every member, growth enabled.
- `grow_to(head)` — becomes `grow_all_to(head)`; the surface that resolves "the
  next line's point" passes a per-member head, since members on different lines
  need different points.
- `translate(map)` — already correct per member; loop instead of `if let`.
- `line_select_extends()` — true when **every** member is a growing line
  selection. A mixed set is not uniformly growable, and treating it as
  growable would make `x` swallow lines the reader did not select.

### Editor layer

- `presentation.selection: Option<…>` → `selections: Vec<Selection<EditorPoint>>`
  plus `primary_index: usize` (`document_session.rs:68`). The 8 `= None` sites
  become `.clear()` or `Vec::new()`.
- `editor::selection_range()` (`:1658`) keeps working — returning the **primary's**
  range — and `selection_ranges()` is added beside it returning every member's
  range. Keeping the old one matters: `selection_or_char_at_cursor` (`:1955`)
  and `paste_anchor` (`:1973`) are single-target helpers that §6's multi-target
  work then generalises.
- `apply_helix_verb` (`:1694-1785`) writes back the whole set.

### Payloads

`BufferKey::DeleteRange(Range<usize>)` → `DeleteRanges(Vec<Range<usize>>)`;
`BufferKey::YankRange(Range<usize>)` → `YankRanges(Vec<Range<usize>>)`. The App
layer's dispatcher (`commands/editor.rs:2100-2151`) folds the set into one
`apply_local` transaction; `yank_editor_range` (`:3300`) concatenates members
into one clipboard write.

Range sets must be **sorted and non-overlapping** before submission. Overlapping
members would double-apply or produce an invalid transaction; merging them is
cheaper than discovering the failure at the controller. When merging collapses a
set to one range, the multi-cursor read of the screen is still correct because
the ranges genuinely are contiguous.

### One transaction, with a cap

All ranges go into **one** `TextTransaction`. Above `MAX_DOCUMENT_EDITS`
(`= 256`) the command must refuse with a notice naming the limit, not silently
truncate — a partially applied multi-cursor edit is exactly the failure
§4.1 forbids.

### Undo

`EditorUndoEntry` gains `selections_before: Vec<Selection<EditorPoint>>` beside
`cursor_before`. `editor_undo` and `editor_redo` restore it instead of clearing
the selection to `None` (`:1759`, `:1808`). Cost is bounded by the member count,
which is already bounded by `MAX_DOCUMENT_EDITS`.

### Rendering

`components/editor.rs:396` resolves the range **set**; `line_spans` (`:818`)
takes `&[Range<usize>]` instead of `Option<&Range<usize>>`; the per-token test
(`:1008-1012`) asks "is this byte inside any member". `cursor_cell` (`:974`) and
the style patch (`:1043-1047`) become loops over member heads, drawing the
primary distinctly from the extras.

### Read-only

`apply_helix` (`commands/editor.rs:3275-3283`) and the growth paths gain an
`editable` check that produces the existing
`EditorNotice::from_error(&DocumentControllerError::ReadOnly)`. Cursor motion
without selection growth stays ungated.

## 7. Ordered work packages

**W1 — `SelectionState` set + primary.** The struct, the preserved
primary-scoped accessors, the new set API, sorting and primary-tracking, and the
invariants from §6. Existing `helix_selection.rs` tests should need **only**
mechanical edits where they read `selection()` — that accessor is unchanged, so
most should pass **unmodified**. That is the acceptance signal.
Acceptance: `cargo test -p codegg --lib tui::helix_selection` green; the
single-selection tests pass without behavioural rewrite.

**W2 — Whole-set rules.** `move_to`, `repin_cursor`, `toggle_char`,
`select_line`, `grow_all_to`, `line_select_extends`.
Acceptance: focused tests green; a mixed-mode `toggle_char` test exists.

**W3 — Storage and ranges.** `TuiDocumentPresentation` fields, the 8 clear
sites, `selection_ranges()`, `apply_helix_verb` write-back.
Acceptance: `check_tui_editor_prefix_state.py` exit 0 — the two new field names
must not contain `prefix`.

**W4 — Range-set payloads.** `DeleteRanges` / `YankRanges`, the dispatcher
fold, `yank_editor_range` concatenation, sort/merge, and the one-transaction
guarantee including the 256 cap.
Acceptance: a test asserting `apply_local` received **one** transaction for an
N-member delete, and a test asserting the cap refuses rather than truncates.

**W5 — Multi-target change verbs.** Generalise `selection_or_char_at_cursor`
and `paste_anchor` so `r` / `R` / `~` / `` ` `` / `p` / `P` target every member
through M004's `editor_editing_verb` funnel, in one transaction.
Acceptance: M004's single-target tests pass unmodified.

**W6 — Rendering.** Range-set highlight, per-member heads, primary
distinguished.
Acceptance: `cargo test -p codegg --lib tui::components::editor` green.

**W7 — Undo/redo selection restore.** The new field, both restore paths.
Acceptance: a multi-selection edit round-trips through `u` then `U` and returns
the selection set.

**W8 — Read-only growth refusal.** The gate, the notice, the narrow reading.
Acceptance: `v`/`x` on a read-only buffer set `state.notice` and leave the set
unchanged, while `j`/`k` still move.

**W9 — Terminal-panel non-regression.** Run the panel's selection tests
unmodified; record the parity outcome (§12).
Acceptance: `cargo test -p codegg --lib tui::interactive_terminal` and
`tui::commands::interactive_terminal` green **without editing panel tests**.

**W10 — Guards and docs.** §12.

**W11 — Full verification.** §11.

## 8. Failure, cancellation, restart, contention semantics

- **Overlapping members.** `Selection::contains` (`:119`) is per-member and its
  behaviour on an overlap is undefined. Two selections can overlap by
  construction once motions move members independently. **Decision: overlap is
  resolved at range-set construction, not in `contains`.** Ranges are merged
  before submission (§6), so the buffer never sees an overlap. `contains` keeps
  its single-member meaning. A member-level overlap test is required so the
  merge is not the only thing standing between two cursors on the same word and
  a corrupted edit.
- **Members on different lines.** Every member's point is resolved by
  `editor_point(snapshot, byte)`, which recomputes `line`. A member whose byte
  was invalidated by an earlier member's edit must be re-clamped. Ranges are
  applied in reverse document order (or rebased), never by re-resolving a stale
  byte against a mutated buffer — the M004 lesson about applying an edit built
  from one buffer to another applies directly.
- **More than `MAX_DOCUMENT_EDITS`.** Refuse with a notice naming the limit.
  Never truncate.
- **Empty set.** Every command that needs a target must degrade to its
  cursor-only behaviour (which is what M004's `selection_or_char_at_cursor`
  already does) rather than to a panic or a silent no-op.
- **Buffer switch, reload, or focus change.** The set resets exactly as the
  single selection does today, at the same 8 sites.
- **Concurrent members.** N/A — all members land in one synchronous
  `apply_local`; there is no interleaving window.

## 9. Compatibility and migration

No protocol, storage, or configuration change. `TuiDocumentPresentation` and
`SelectionState` are frontend-local. The `BufferKey` payload change is internal
to the TUI dispatch, and its handler is in the same file.

## 10. Required tests

- `helix_selection.rs`: sorting is by `bounds()`, not insertion; the primary
  survives a sort; `toggle_char` on a mixed set; `line_select_extends` on a
  mixed set is `false`; `move_to` extends every extendable member; `translate`
  maps every member; the empty set answers every accessor without panicking.
- **Existing `helix_selection.rs` tests pass unmodified except where they read a
  removed accessor.** The acceptance signal for W1.
- Editor: `selection_ranges()` for N members; cleared at every one of the 8
  sites; `apply_helix_verb` write-back round-trips.
- `d` on an N-member set: **one** transaction, members' ranges all applied,
  overlapping ranges merged, cursor and set correct afterwards.
- The 256-member cap refuses with a notice and applies nothing.
- `y` / `space y` on an N-member set: members concatenated in document order
  into **one** clipboard write.
- `r` / `R` / `~` / `` ` `` / `p` / `P` on an N-member set: every member
  targeted; M004's single-target tests unmodified.
- Undo/redo round-trip restores the set; a new edit after undo truncates the
  redo tail as today.
- Read-only: `v` / `x` / growth refused with `state.notice`; `j` / `k` /
  `h` / `l` still move.
- Rendering: N heads drawn, primary distinguished, N highlighted ranges.
- Terminal panel: all selection tests unmodified.
- Insert-mode sweep still green.

## 11. Required verification commands

```bash
cargo fmt
cargo test -p codegg --lib tui::helix_selection
cargo test -p codegg --lib tui::editor
cargo test -p codegg --lib tui::commands::editor
cargo test -p codegg --lib tui::components::editor
cargo test -p codegg --lib tui::interactive_terminal
cargo test -p codegg --lib tui::commands::interactive_terminal
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
scripts/verify.sh quick
```

`verify.sh full` is required before closure.

## 12. Documentation updates

- `docs/tui.md` — the behaviours a reader can observe today: multi-selection
  applies to all members, read-only buffers refuse growth. **Do not document a
  gesture that does not exist yet** — no key creates N cursors until M008, so
  the doc must not imply one does.
- `architecture/tui.md` — the set model, the primary-scoped accessor policy
  (and why it exists: to keep one rule set for two surfaces), the one-transaction
  rule and the 256 cap, the read-only growth boundary, and the `t`/`T` outcome
  from §5.
- `scripts/check_tui_editor_prefix_state.py` model prose — note that the
  presentation now has two **non-prefix** selection fields, so the
  one-prefix-field rule is load-bearing rather than incidental. **Update the
  prose; do not relax the assertion.**
- Terminal-panel parity: record the §3 outcome. Under the chosen design parity
  is preserved by construction, which is a stronger claim than an exemption and
  should be stated as one.

## 13. Acceptance criteria (externally observable)

1. A programmatically constructed set of N members is deleted by one command in
   **one** transaction, with every member's range applied and the set updated
   afterwards. *(Restated from the roadmap's "N cursors selectable and
   deletable in one gesture" — see §1.1. The gesture is M008's.)*
2. A motion transforms every member and preserves document order.
3. `v` extends every member; a second `v` on an all-char set clears it; a
   mixed set does not half-clear.
4. `x` growth applies per member and is refused as a uniform gesture on a mixed
   set.
5. `y`, `space y`, `r`, `R`, `~`, `` ` ``, `p`, and `P` all target every member,
   and each lands in one transaction.
6. Undo and redo restore the selection set as well as the cursor.
7. A read-only buffer refuses selection **growth** with a visible notice and
   still allows plain cursor movement.
8. Rendering shows every member's highlight and every head, with the primary
   distinguishable.
9. The terminal panel's selection tests pass **unmodified**.
10. `D` is unchanged: a held selection does not alter its meaning.
11. All three editor guard scripts and `verify.sh quick` pass.
12. No new Normal-mode binding was introduced, and the key-authority guard's
    chord counts are unchanged.

## 14. Stop conditions

Report rather than improvise if:

- Preserving the single-selection API turns out to require a *semantic* fork —
  i.e. the panel needs different rules, not just different call sites. That
  defeats §1.1's whole rationale and must be resolved in the open.
- The set cannot be ordered by `bounds()` without the primary's meaning
  changing. Ordering and primary-identity interact; if they cannot be made
  consistent, stop.
- M004's `r` / `R` / `~` / `` ` `` / `p` / `P` tests need modification for
  multi-target support. The funnel is supposed to make them fall out; if they do
  not, the funnel has a single-target assumption in it.
- The terminal panel's tests need modification. §3 claims parity is preserved by
  construction; a needed edit falsifies that claim and must be reported.
- The `t`/`T` cursor-landing distinction (§5) turns out to require changing the
  motion result type in a way M008's contract would then depend on. Report; do
  not quietly change the shape.
- The undo entry's memory bound cannot absorb a member list without exceeding
  the existing retained-payload budget.

## 15. Closure evidence required

Implementation commits; §11 output labelled local vs CI; explicit confirmation
that the existing `helix_selection.rs` tests and the terminal panel's selection
tests passed **unmodified**; the read-only boundary decision from §6; the
`t`/`T` outcome from §5; a guard demonstration that
`check_tui_editor_prefix_state.py` fails on an injected second prefix field;
and the one-transaction evidence from W4 (a test, not an assertion in prose).

## 16. Handoff notes

This is the longest pole in the roadmap and it is deliberately the
model-only milestone. A reviewer should check three things first.

**Whether the primary-scoped API is really free.** The claim in §1.1 is that
`selection()`, `is_selected()`, `span()`, and `line_range()` have zero
production callers beyond what the primary semantics already satisfy, so
keeping them costs nothing. §3 shows they are called only from tests. If that
holds, the migration is far smaller than "replacing a single selection with a
set" sounds, and the terminal panel is untouched — which is the difference
between this milestone landing and not.

**Whether W2's `toggle_char` rule is the right one.** Collapsing a mixed set on
`v` requires deciding what "in char mode" means for a set that is partly in it.
The plan's rule — arm on every member, clear only when every member is already a
char selection — is one of two defensible answers. The alternative is that `v`
always clears a mixed set, which makes `v` a reliable escape but costs the
reader their line selections. Worth an explicit decision at review.

**Whether §5's `t`/`T` correction belongs here at all.** M003's closure owes it
to M007 on the theory that it is a selection property. Reading the keymap, it
is a cursor property that needs nothing from a selection set. Rather than
inherit an assignment whose stated rationale does not hold, §14 makes it a
stop condition and requires it to be resolved either way. Leaving it unexamined
would let a closure record's reasoning propagate into a plan it does not fit.
