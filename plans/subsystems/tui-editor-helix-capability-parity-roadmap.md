# TUI Editor Helix Capability Parity Roadmap

Status: active — M001–M005 implemented; M006 in flight; M007–M009 pending

Long-term references:

- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` — the frontend holds no text authority; `DocumentController::apply_local` is the only mutation path.
- `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md` — the read-only LSP surface.
- `plans/adrs/ADR-0034-editor-syntax-tree-dependency.md` — accepted **Option B**: structural editing is served by daemon-owned LSP structural reads, and **no tree-sitter dependency is added**. This roadmap inherits that decision and does not revisit it.

Authorizing predecessor: `plans/subsystems/tui-editor-helix-parity-roadmap.md`.
That roadmap is **closed** (M001–M013 and M015–M020; M014 remains its own
unimplemented syntax-node scope). Its §3 deferred a named set of Helix
capabilities to "deferred product work" rather than folding them in. This
roadmap is the successor for that set. It does **not** reopen any closed
milestone of the predecessor, and it does not absorb M014, which stays
`ready for handoff` in its own directory.

Reference dialect: <https://docs.helix-editor.com/keymap.html>. Helix is the
*normative source for the semantics this roadmap adopts*, not a dependency. No
Helix code, configuration, or asset is vendored.

## 1. Purpose and ownership boundary

Close the gap between "the editor speaks Helix's keymap" and "the editor can do
what Helix can do". The predecessor delivered the first; this roadmap delivers
the second for every capability that fits inside CodeGG's authority model.

**This subsystem owns** the frontend's normal/insert-mode *verb* semantics that
need state the predecessor deliberately left out — line-shape selection verbs,
change verbs, regex search, counts, comment toggling, the jumplist, registers,
keystroke recording, and the shape of the editor-local undo history.

**It consumes** the daemon-owned document service (canonical unsaved content),
`DocumentController` (the frontend text replica), the App layer's clipboard, and
the already-loaded `syntect` grammar set as a *comment-token* source only.

**It must not own** text authority, LSP process lifecycle, filesystem
authorization, or any mutation path other than `DocumentController::apply_local`.
That boundary is inherited from ADR-0011 and is not renegotiable here.

**It must not become** a pane-layout, multi-buffer, or renderer workstream.
Those are named in §3 with the reason each needs its own charter.

## 2. Work classification

### Invariants

- **No independent frontend text buffer.** Every text read is a scoped
  `try_snapshot()` borrow; every mutation goes through `apply_local`. Guarded by
  `scripts/check_tui_editor_text_authority.py`.
- **One owner per key chord.** The dispatch chain is ordered stages, so a chord
  declared in a later stage is unreachable the moment an earlier stage claims
  it. Guarded by `scripts/check_tui_editor_key_authority.py`, which asserts the
  guard-block chords by name so it cannot pass by being emptied.
- **Every leaf gesture is reachable and says what it does.** Guarded by
  `scripts/check_tui_editor_leaf_authority.py`.
- **The published keymap and the implemented keymap agree in both directions.**
  `scripts/check_tui_editor_doc_drift.py` compares the code's leaf gestures and
  claimed chords with `docs/tui.md`, *and* checks the document's own "not bound"
  declarations in reverse.
- **No unclaimed key silently does nothing.** A key the buffer does not own
  returns `Unhandled` and reaches the composer, which is the documented
  fallthrough contract. This roadmap therefore treats *binding a Helix verb* as
  a correctness act, not a nicety: an unbound verb types its letter into the
  prompt box.
- **Positions, never text.** Frontend-local state (selections, undo entries,
  jumplist entries, registers) holds byte offsets and identifiers. A retained
  copy of the document is the failure the text-authority guard exists to catch.
- **Every retained collection is bounded**, and the bound is disclosed when it
  is reached rather than truncating silently.

### Capabilities

- Line-shape selection verbs (`X`, `Alt-x`, `_`, `J`, `Alt-J`, `>`, `<`).
- Change verbs and deletion without a yank (`c`, `Alt-c`, `Alt-d`).
- Regex search and selection-by-regex (`/` as a regex, `?`, `*`, `Alt-*`, `s`,
  `Alt-s`).
- Counts on motions (`3w`, `5j`, `<n>G`, `<n>gg`).
- Comment toggling (`space c`, `space C`).
- The jumplist, delivered as the `space j` picker. Helix's `Ctrl-i`/`Ctrl-o`
  stepping pair is a recorded decision, not an assumption: `Ctrl-i` is `Tab` on
  this TUI and `Ctrl-o` is the workspace dashboard (§7 M006).
- Registers (`"<reg>`, `Ctrl-r` in Insert).
- Keystroke recording: macros (`Q`/`q`) and dot-repeat (`.`).
- An undo **tree** rather than a linear history.

### Infrastructure

- A comment-token source keyed by the buffer's own syntax.
- A bounded keystroke recorder that every replay consumer shares.
- A history model that can express a branch without breaking the existing
  bounded-bytes and selections-restore invariants.
- An indent-unit source for `>`/`<`.

### Polish

- `docs/tui.md` keymap tables and the editor's own `space ?` keymap box kept in
  step with the implemented set, in the same change as each binding.

## 3. Non-goals

Each item below is a real Helix capability that this roadmap **does not**
cover, with the reason. None is an oversight.

- **Shell filters** (`|`, `Alt-|`, `!`, `Alt-!`, `$`). Deferred by the
  predecessor for the reason that still holds: they introduce process execution
  into the editor surface. A filter is a CodeGG-managed process launcher and
  would need a `docs/writer-disposition.toml` disposition, a scheduler-owned
  execution path, and a sandbox decision. That is a process-execution
  workstream, not a keybinding one.
- **LSP write operations** (`=` format apply, `space r` rename apply,
  `space a` code-action apply). Already *capability-complete*: the predecessor's
  M015–M017 route preview → change review → apply through ADR-0008's accepted
  reviewable-artifact path. The gesture differs from Helix; the capability does
  not. Extending `LspReadOperation` with writes is a protocol decision beyond
  this roadmap.
- **`Ctrl-c` as `toggle_comments`.** Helix binds it; CodeGG binds `Ctrl-c` to
  `InputAction::ForceQuit` and to copy-selection. §7 M005 binds the capability
  to `space c`/`space C` instead and records the divergence rather than
  narrowing the exit gesture. A future change may revisit this as an owner
  decision, not as a milestone of this roadmap.
- **Multiple simultaneously attached documents.** `gn`/`gp`, `space b`, `gf`,
  `ga`, `gm`, and a buffer picker all require N `DocumentController`s, N undo
  histories, N LSP document synchronisations, and an answer from the
  `document.v1` surface for concurrent attachments. That is a document-session
  workstream with its own ownership boundary.
- **`Ctrl-w` window mode and splits.** The editor is a primary route that
  replaces the main buffer; pane layout is a separate workstream. The
  predecessor recorded the same non-goal.
- **Soft wrap / a display-line model.** The renderer is one screen row per
  logical line, so `j`/`k` are currently `gj`/`gk` and `z m` has no meaning.
  Splitting them is a renderer change that interacts directly with the closed
  `tui-render-cost` line's one-row-per-line assumptions.
- **Tree-sitter.** Settled by ADR-0034 Option B. Structural editing comes from
  daemon-owned LSP structural reads; M014 owns the remaining slice.
- **Unimpaired change navigation** (`]g`/`[g`/`]G`/`[G`). The predecessor's
  registry entry records that the claimed data source does not exist —
  `egggit`'s diff types carry no line ranges. Landing it needs a daemon-side
  git-derived range surface, which is its own milestone with its own deciding
  question.
- **`Ctrl-a`/`Ctrl-x` increment/decrement.** Deferred by the predecessor and
  still deferred: it needs a number scanner with a base- and sign-aware rule
  per language, and a wrong guess rewrites a value the user did not name. It is
  cheaper to add once counts (M004) give the editor a numeric-prefix grammar to
  share.
- **A picker framework change.** M006 reuses the existing picker surface; it
  does not make pickers fuzzy.

## 4. Current state

The predecessor closed with the dispatch tables machine-counted at **2
guard-block chords, 74 Normal-mode chords, and 8 Insert-mode chords**, plus 44
executable leaf gestures across the prefix tables. The numbers are the key
authority guard's own output, not a hand count, and they are the numbers every
milestone in this roadmap must keep honest.

What exists and is load-bearing for this work:

- `handle_normal_key` and `handle_insert_key` in `src/tui/commands/editor.rs`
  are the two mode tables, and `editor_buffer_key`'s guard block runs ahead of
  both. The key authority guard reads all three stages and refuses a shadowed
  arm.
- `src/tui/editor.rs` holds the mode-agnostic half: `EditorCommand`,
  `HelixVerb`, `ObjectKind`, the six prefix leaf tables, the bounded search
  scanner, and the undo/history helpers.
- `src/tui/helix_selection.rs` holds the selection algebra shared with
  `/terminal`, so the two surfaces cannot disagree about what `v` or `x` means.
- `src/tui/document_session.rs` holds `TuiDocumentPresentation`: the cursor, the
  selection set, the three single-slot repeat recorders, the notice, the command
  line, the search line and its match list, the undo history and its cursor, and
  the keymap-overlay flag.
- The clipboard is the **App layer's**, not the buffer's. The buffer reports a
  verb (`ClipboardVerb::{PasteAfter, PasteBefore, Replace}`) and the App reads
  the bytes once, at the point that owns them.
- `src/tui/editor_highlight.rs` loads `syntect`'s default syntax set and theme
  set as a static, and prefers an LSP semantic token over a `syntect` span for
  the same bytes.

What is missing, and is the reason for each milestone:

- **No line-shape verbs.** `x` selects the cursor's line and grows one line per
  press, but `X` (extend to line bounds) and `Alt-x` (shrink to line bounds) do
  not exist, `_` (trim) does not exist, `J`/`Alt-J` (join) do not exist, and
  `>`/`<` (indent/unindent) do not exist. All four are absent from the chord
  table, so each currently types its glyph into the composer.
- **No change verb.** `d` deletes the held selection and `y` yanks it, but `c`
  (delete and enter Insert), `Alt-c` (the same without a yank) and `Alt-d`
  (delete without a yank) do not exist.
- **Search is a literal substring scan.** `find_matches` scans for
  `str::match_indices` over a bounded window and returns start offsets only.
  A bounded `regex::RegexBuilder` path already exists for the `S`/`K`/`Alt-K`
  selection verbs, so the regex machinery is present but is not the search.
- **No counts.** `G` and `gg` take no numeric argument, and no motion accepts a
  repeat. The predecessor's own goto-mode documentation records this.
- **No comment syntax knowledge.** `space c` and `space C` are rows whose action
  is an explicit "this build knows no comment syntax for the edited buffer"
  statement. The tokens are obtainable without a new crate — `syntect` is
  already a dependency and its metadata feature ships line/block comment tokens
  in the default packdump — but nothing reads them today.
- **No jumplist.** `space j` is an explicit "this editor keeps no
  visited-position history" statement, and `Ctrl-i`/`Ctrl-o` are unbound.
- **No registers.** The yanked text *is* the system clipboard; there is no
  named register, no `"` prefix, and no `Ctrl-r` insert.
- **No keystroke recording.** `.` (repeat last insert) and `Q`/`q` (macros) are
  unbound. The insert session records *transactions*, not the keystrokes that
  produced them, so neither verb has a substrate.
- **Undo is one ordered list with a cursor.** An edit made after an undo
  truncates the redo tail, so the abandoned branch is unreachable. Helix keeps
  it.

## 5. Target architecture

Four shapes carry every milestone.

**A verb is a value, not an arm.** New verbs join `HelixVerb` or
`EditorCommand` and resolve through the existing `apply_helix_verb` /
`resolve_edit` funnel, so read-only refusal, undo capture, retained-bytes
accounting, LSP staleness and selection clearing apply to them for free rather
than by a second implementation that could disagree.

**Frontend-local state is bounded and positions-only.** The jumplist, the
registers and the keystroke recorder follow the undo history's established
shape: a `Vec` of positions or key events with a byte/entry cap, cleared on
open, reload, resync, reconnect and close, and never holding document text.

**One recorder, two consumers.** Macros and dot-repeat share a single bounded
keystroke log. The recorder captures *after* dispatch, so a replay reproduces
the user's keys rather than the editor's decisions, and replay goes through the
same dispatch entry the keyboard does — which is what keeps the key authority
guard's model true instead of making replay a second, unguarded keymap.

**A count is a modifier, not a prefix.** The prefix state machine is
deliberately one prefix wide. A count is accumulated beside it, consumed by the
verb it precedes, and cleared by any non-digit key and by every mode change — so
adding counts cannot widen the prefix machine or create a second grammar.

**The history becomes a tree without losing its existing invariants.** An entry
keeps both directions, never moves, and carries the selections it preceded.
Branching adds a parent link and makes the cursor a node identity rather than an
index. The memory bound changes from "trim the oldest entry" to "prune a whole
branch", because an interior node a child still needs cannot be dropped.

## 6. Dependency graph

Edges are classified: **hard** (cannot start), **interface** (a contract must be
stable, not the code), **soft** (ordering preference), **operational** (shared
verification cost).

```text
M001 selection shape verbs ────────────────────────────────┐
M002 change verbs + no-yank ──────────────interface──┐     │
M003 regex search + select-by-regex ─────soft──┐     │     │
M004 counts ───────────────────────────────────┼─────┼─────┤
M005 comment toggling ─────────────────────────┼─────┼─────┤
M006 jumplist ──────────────────────────soft───┘     │     │
M007 registers ───────────────interface──────────────┘     │
M008 keystroke recording (macros + dot-repeat) ──hard──────┘
M009 undo tree ────────────────────────────────────────────┘
```

| Edge | Class | Why |
|---|---|---|
| M006 → M003 | soft | Jumps the search makes (`/`, `n`, `N`) should push jumplist entries. M006 can land first and treat search jumps as a follow-up, but landing M003 first is cheaper. |
| M007 → M002 | interface | Both must agree what "yank" means. M002 defines deletion *without* a yank against today's system-clipboard model; M007 replaces that model with registers and must keep M002's promise true. |
| M008 → M007 | hard | A macro records to a register, so the register store must exist first. |
| M004 → prefix machine | interface | The count is accumulated beside the single-slot prefix; it must not widen it. |
| M009 → history model | interface | The tree replaces the list-and-cursor shape that M012 of the predecessor closed. Its bounded-bytes and selections-restore invariants are the contract. |

No edge in this graph is operational: the milestones touch disjoint code and
none shares a verification environment beyond the standard workspace suite.

**Already ready, not part of this roadmap.** `M014` syntax-node text objects
(`plans/implementation/tui-editor-helix-parity/014-syntax-node-text-objects.md`)
is `ready for handoff` under the predecessor. It is executable today under
ADR-0034 Option B, it shares no code with any milestone here, and this roadmap
neither blocks it nor depends on it.

## 7. Milestones

### Milestone 001 — Selection shape verbs

Class: capability.

Objective: bind `X`, `Alt-x`, `_`, `J`, `Alt-J`, `>` and `<` so a reader can
reshape a selection and a run of lines without leaving Normal mode.

Dependencies: none. Interface dependency on the existing selection set and the
shared edit funnel.

Deliverable boundary: `HelixVerb` gains the line-bounds, trim and join verbs;
`>`/`<` resolve through the edit funnel; an indent-unit source is chosen and
recorded. No new prefix, no new state, no new dependency.

Exit conditions: every chord has exactly one owner; each verb is reachable
through the real dispatch path; read-only refusal is reported rather than
silent; `docs/tui.md` and the keymap box list the new keys in the same change.

### Milestone 002 — Change verbs and no-yank deletion

Class: capability.

Objective: bind `c`, `Alt-c` and `Alt-d`, making "delete and start typing" and
"delete without touching the clipboard" available and distinct.

Dependencies: none. Interface dependency with M007, which must preserve these
semantics when it introduces registers.

Deliverable boundary: `c` deletes the held selection through the existing
delete path and enters Insert; `Alt-c` and `Alt-d` suppress the clipboard
write. The yank/no-yank distinction is explicit in the verb, not inferred from
a flag on the caller.

Exit conditions: `c` leaves the reader in Insert on a read-write attachment and
is refused with a notice on a read-only one; `Alt-d` provably leaves the
clipboard unchanged; a multi-cursor `c` is one undo step.

### Milestone 003 — Regex search and selection-by-regex

Class: capability.

Objective: make `/` a regex search, add `?` (reverse), `*` and `Alt-*`
(seed from the selection), `s` (select matches inside selections) and `Alt-s`
(split on newlines).

Dependencies: none. Soft predecessor of M006.

Deliverable boundary: `SearchOutcome` carries **spans** rather than start
offsets, so a match can be selected and highlighted exactly; the regex is
compiled under the existing bounded builder; an invalid pattern is a notice,
not a panic and not a silent empty result.

Exit conditions: a pattern that exceeds the size limit is refused with the
reason; `n`/`N` walk regex matches; `s` selects every match inside each
selection member; the literal-search behaviour change is documented.

### Milestone 004 — Counts

Class: capability.

Objective: accept a numeric prefix on the motions Helix documents it for
(`3w`, `5j`, `<n>G`, `<n>gg`) without widening the prefix state machine.

Dependencies: interface dependency on the single-slot prefix machine.

Deliverable boundary: one bounded count field on the presentation, accumulated
in Normal mode only, consumed by motions, cleared on any non-digit key and on
every mode change. Repeatable *verbs* are explicitly out of scope and recorded
as such.

Exit conditions: a count survives a prefix (`3gg`) where Helix allows it and is
refused where it does not; the count is visibly disclosed while it is being
typed; no count can be left armed across a mode change, a document switch or a
reload.

### Milestone 005 — Comment toggling

Class: capability.

Objective: make `space c` and `space C` toggle line and block comments using
the buffer's own syntax, replacing two "unavailable" statements with working
verbs.

Dependencies: none.

Deliverable boundary: a comment-token source keyed by the document's syntax; a
toggle that is one undoable transaction per gesture; a refusal with a reason
when no token is known for the buffer. `Ctrl-c` is **not** bound — see §3.

Exit conditions: a line toggle on a selection that is already commented removes
the comment rather than adding a second one; a block toggle is one transaction;
an unknown syntax says so; the two `Unavailable` rows are gone from the space
table and from `docs/tui.md` together.

### Milestone 006 — Jumplist

Class: capability.

Objective: give the editor a bounded visited-position history and the `space j`
picker over it, replacing the "this editor keeps no visited-position history"
statement.

Dependencies: soft successor of M003.

Deliverable boundary: a bounded stack of `(document, byte offset)` pushed on
deliberate jumps and cleared on open/reload/resync/reconnect/close. Positions
only, never text. The **picker** at `space j` is the delivered gesture.

**The two Helix stepping chords are not freely available, and the plan must say
so rather than bind a dead arm.** `Ctrl-i` is byte `0x09`, and this TUI never
pushes keyboard-enhancement flags, so it always arrives as `KeyCode::Tab` — which
the global map already binds to `ToggleComposerMode`. `Ctrl-o` is bound globally
to `OpenWorkspaceDashboard`. Binding either in Normal mode on the editor route
shadows a live global action there. M006's plan therefore carries this as a
decision point (D1) with the picker as the unconditional deliverable and the
stepping pair gated on it.

Exit conditions: `space j` opens the jumplist and selecting an entry returns to
that position; the stack is bounded and discloses when it prunes; the
"unavailable" row is gone from the leaf table and from `docs/tui.md` together; a
jump inside one document and a jump across a reload are both honest about what
they can restore; D1 is resolved and recorded, and no chord is bound that cannot
fire.

### Milestone 007 — Registers

Class: capability.

Objective: introduce named registers so `"<reg>` selects a target and `Ctrl-r`
inserts one, with the system clipboard as one register among several.

Dependencies: interface dependency with M002.

Deliverable boundary: a bounded register store of positions-and-text entries
that are frontend-local, cleared on document lifecycle transitions, and a
`"` prefix that is one level deep. Named `a`–`z`, the unnamed register, the
blackhole register and the search register are in scope; the file, selection
and command registers are out.

Exit conditions: yank to a named register and paste from it round-trips; the
unnamed register still reaches the system clipboard; the blackhole register
provably writes nothing; a register cannot hold more than its byte cap without
disclosing the truncation.

### Milestone 008 — Keystroke recording: macros and dot-repeat

Class: capability.

Objective: add `.` (repeat the last insert) and `Q`/`q` (record and replay a
macro) on one bounded keystroke recorder.

Dependencies: **hard** on M007.

Deliverable boundary: a bounded `Vec` of key events captured after dispatch;
replay through the same dispatch entry the keyboard uses; a replay that cannot
start a recording; an interruptible replay. Macros record to a register, so the
register store is the storage.

Exit conditions: `.` reproduces the last insert's effect including its cursor
landing; `Q`/`q` round-trips a multi-key edit; a replay is one undo step per
edit it performs and never bypasses `apply_local`; `Esc` stops a replay; a
recorder past its bound refuses rather than dropping keys silently.

### Milestone 009 — Undo tree

Class: capability. **Lowest value in this roadmap** — see §10 risk 4.

Objective: replace the linear history and its cursor with a tree so an edit
made after an undo does not destroy the abandoned branch.

Dependencies: interface dependency on the history model.

Deliverable boundary: entries gain a parent identity; the cursor becomes a node
identity; pruning operates on whole branches because an interior node a child
needs cannot be dropped. The existing invariants — both directions retained,
the entry never moves, selections restored, bounded bytes — are preserved.

Exit conditions: undo, edit, undo, `Alt-u` reaches the pre-branch state and
`Alt-U` returns to the new branch; the memory bound still holds under a
branching workload; the disclosure at a boundary still names which boundary.

## 8. Cross-cutting requirements

### Storage and migration

None. Every milestone is frontend-local presentation state. No migration, no
`STORAGE_LAYOUT_VERSION` change, and no protocol variant. A milestone that finds
it needs one has left this roadmap's boundary — see §10 stop conditions.

### Protocol and compatibility

None. The `document.v1` surface and `DocumentController` are consumed as they
are. `SearchOutcome` and the presentation struct are frontend types, so changing
their shape is not a compatibility event.

### Security and authorization

The editor gains no authority. Every writing verb goes through
`apply_local` and inherits read-only refusal. Registers, the jumplist and the
recorder hold positions and text the user already has on screen — never a
document copy, and never anything the daemon did not authorize.

### Concurrency, cancellation, and recovery

The async rules the predecessor established are unchanged: every controller
call is spawned with `spawn_scoped_registered_tui_task`, carries a `request_id`
and a `generation`, and a completion for a closed or switched document is
discarded. New frontend-local state is cleared by the same lifecycle transitions
that clear the undo history.

### Observability and audit

Each milestone's refusals are visible: a bound that is reached, a pattern that
does not compile, a syntax with no comment token, a replay that was interrupted.
The established pattern is `EditorNotice` on the banner row — never a silent
no-op, because a silent no-op is indistinguishable from a dropped keystroke.

### Performance and resource use

No wall-clock claim anywhere. Each milestone states its bound as a deterministic
count or a byte cap and reuses an existing one where it can: the undo depth and
byte caps, `MAX_EDITOR_SEARCH_MATCHES`, `MAX_EDITOR_SEARCH_BYTES`,
`MAX_EDITOR_SEARCH_LINE`, `MAX_EDITOR_PENDING_PREFIX`, and
`MAX_DOCUMENT_EDITS`. New collections get a new named constant, not a magic
number.

### Documentation and operations

`docs/tui.md` and the editor's `space ?` keymap box move in the **same change**
as each binding, because `scripts/check_tui_editor_doc_drift.py` compares both
directions and a one-sided edit fails the guard.

## 9. Verification strategy

Every milestone runs the same shape:

- `cargo fmt --check`.
- `cargo test --test tui_render` (the family selector; a member filter selects
  one member) and `cargo test -p codegg --lib tui::` for the unit surface.
- `scripts/verify.sh quick` before handoff, which runs rustfmt, the
  generated-asset checks, the guard scripts, and a workspace check.
- The four editor guards focused: `check_tui_editor_key_authority.py`,
  `check_tui_editor_leaf_authority.py`, `check_tui_editor_text_authority.py`,
  `check_tui_editor_doc_drift.py`.
- `scripts/verify.sh full` for the workspace sweep, under nextest profile `ci`.

Milestone-specific evidence is named in each plan's §11. Two rules apply
throughout: a guard change is only evidence when the guard's `--self-test`
proves it rejects an injected defect, and no milestone may claim a wall-clock
improvement.

## 10. Risks and decision points

1. **The fallthrough hazard is the whole justification for M001–M002.** An
   unbound Helix verb is not inert: `handle_editor_key` returns `false` and the
   key reaches the composer's claim table. A Helix user pressing `c` types a `c`
   into the prompt box. This is a recorded *design* property (the composer must
   stay typeable), so the fix is to bind the verbs, not to change the
   fallthrough.
2. **`Ctrl-c` is already owned.** `Ctrl-c` is `InputAction::ForceQuit` in the
   default bindings and additionally copies a live selection. Binding it to
   `toggle_comments` in the editor's Normal mode would take the exit chord and
   would need a fourth stage in the key authority guard's model. M005 binds
   `space c`/`space C` instead and records the divergence. Revisiting it is an
   owner decision about the exit gesture, not a milestone.
3. **Regex search is a behaviour change, not an addition.** Today `/foo.bar`
   matches literally; after M003 the dot is a wildcard. The plan must document
   the change and must make an invalid pattern a notice. A "literal unless
   escaped" mode is rejected as a second grammar.
4. **M009 is the weakest milestone.** The tree is only observable after
   undo-then-edit, and the linear model was deliberately designed and closed.
   The owner may drop M009 without affecting M001–M008. It is planned rather
   than hidden because leaving a known parity gap unrecorded is worse than
   recording a low-value one.
5. **Indent units are an unresolved decision inside M001.** `>`/`<` need a
   width and a tab policy. Adding an editor config key is a configuration-surface
   change; detecting from the document is bounded but can guess wrong on a
   mixed-indent file. M001 carries this as a decision point with a recommendation
   rather than settling it here.
6. **A count can collide with a real digit binding.** `0` is `LineStart`. A
   count that starts with `0` is meaningless, so `0` must remain the line-start
   motion and only `1`–`9` may begin a count. M004 must assert this explicitly.
7. **Register durability is deliberately absent.** Registers do not survive a
   document close, a reload, a resync or a reconnect, exactly as the undo
   history does not. A user who expects `"a` to survive a reload will be
   surprised; the plan must state it in the user-facing docs, not only in code.

## 11. Completion definition

This roadmap is complete when:

- Every milestone in §12 is `closed` with an accepted closure record, or is
  recorded as dropped by an explicit owner decision with its reason.
- The key authority guard's summary reports one owner for every chord, with the
  new chords counted and no shadowed arm.
- The doc-drift guard passes with the new gestures in both directions, so
  `docs/tui.md` and the code agree.
- Every new retained collection has a named bound, and reaching it is disclosed.
- No milestone introduced a migration, a protocol variant, a new crate, or a
  new filesystem or process authority. A milestone that did has left this
  roadmap's boundary and does not count toward completion.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 selection shape verbs | **conditionally closed** | `plans/implementation/tui-editor-helix-capability-parity/001-selection-shape-verbs.md` | `plans/closure/tui-editor-helix-capability-parity/001-status.md` | green `fmt --check` and `quick` on the merged tree; the red editor-path comment ceilings re-banked or the prose moved |
| M002 change verbs and no-yank deletion | **conditionally closed** | `plans/implementation/tui-editor-helix-capability-parity/002-change-verbs-and-no-yank-deletion.md` | `plans/closure/tui-editor-helix-capability-parity/002-status.md` | green `fmt --check` and `quick` on the merged tree; the red editor-path comment ceilings re-banked or the prose moved |
| M003 regex search and selection-by-regex | **conditionally closed** | `plans/implementation/tui-editor-helix-capability-parity/003-regex-search-and-selection-by-regex.md` | `plans/closure/tui-editor-helix-capability-parity/003-status.md` | green `fmt --check` and `quick` on the merged tree; the four editor-path comment ceilings re-banked; the plan's §16 consumer inventory corrected to seven consumers |
| M004 counts | **closed** (promoted 2026-11-13) | `plans/implementation/tui-editor-helix-capability-parity/004-counts.md` | `plans/closure/tui-editor-helix-capability-parity/004-status.md` | — |
| M005 comment toggling | **conditionally closed** | `plans/implementation/tui-editor-helix-capability-parity/005-comment-toggling.md` | `plans/closure/tui-editor-helix-capability-parity/005-status.md` | no `.md` block-only test; no `space C` pair-less dispatch test; the commit |
| M006 jumplist | **conditionally closed** | `plans/implementation/tui-editor-helix-capability-parity/006-jumplist.md` | `plans/closure/tui-editor-helix-capability-parity/006-status.md` | the picker-stays-open assertion and the per-jump `:goto`/`gd`/symbol-picker assertions; the commit |
| M007 registers | **conditionally closed** | `plans/implementation/tui-editor-helix-capability-parity/007-registers.md` | `plans/closure/tui-editor-helix-capability-parity/007-status.md` | its own §10 evidence and documentation items; the commit |
| M008 keystroke recording, macros and dot-repeat | **conditionally closed** | `plans/implementation/tui-editor-helix-capability-parity/008-keystroke-recording-macros-and-dot-repeat.md` | `plans/closure/tui-editor-helix-capability-parity/008-status.md` | its own §10 evidence and documentation items; the commit. M007's hard dependency is discharged. |
| M009 undo tree | **conditionally closed** | `plans/implementation/tui-editor-helix-capability-parity/009-undo-tree.md` | `plans/closure/tui-editor-helix-capability-parity/009-status.md` | Conditional on owner ratification of the single-chain pruning deviation, `full`, and the uncommitted tree; ``scripts/verify.sh quick` passed; `full` not run; worktree uncommitted; the render corpus is 1227 passed / 0 failed` |
