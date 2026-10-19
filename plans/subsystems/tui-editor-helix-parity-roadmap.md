# TUI Editor Helix Keybinding Parity Roadmap

Status: active (M001–M013 and M015 closed; M014 blocked on ADR-0034)

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md`
- `plans/adrs/ADR-0034-editor-syntax-tree-dependency.md` (proposed; gates M009 and the tree-sitter milestone slices)

Authorizing predecessor: `plans/subsystems/tui-editor-correctness-usability-roadmap.md` §3
Non-goals names "full vim/Helix parity" an **explicitly deferred successor workstream**.
This roadmap is that successor. It does not reopen that roadmap's closed milestones.

Reference dialect: <https://docs.helix-editor.com/keymap.html>. Helix is the
*normative source for the key semantics this roadmap adopts*, not a dependency.
No Helix code, config, or asset is vendored.

## 1. Purpose and ownership boundary

Give the CodeGG TUI editor a complete, discoverable, internally consistent
modal editing vocabulary so a user who knows Helix can edit in CodeGG without
relearning, and so the documented keymap and the implemented keymap cannot
diverge.

**This subsystem owns** the frontend normal/insert-mode key→intent resolution,
the pending-prefix state machine, the held-selection and multi-selection
model, the editor-local undo/redo traversal surface, the in-buffer search
line, and the user-facing keymap documentation.

**It consumes** the daemon-owned document service (canonical unsaved content),
`DocumentController` (the frontend text replica), and the daemon-owned
language-server service (read-only LSP operations).

**It must not own** text authority, LSP process lifecycle, filesystem
authorization, or any mutation path other than `DocumentController::apply_local`.
This boundary is inherited from ADR-0011 and is not renegotiable here.

The terminal panel (`src/tui/commands/interactive_terminal.rs`) already speaks a
deliberate subset of the same dialect for selection and navigation. This
roadmap must not let the two diverge: a change to selection or navigation
semantics either lands in the shared state machine or explicitly records why
the terminal panel is exempt.

## 2. Work classification

### Invariants

- No independent frontend text buffer; no mutation outside
  `DocumentController::apply_local`; no app-side LSP startup (ADR-0011).
- **Unclaimed-key fallthrough.** In Normal mode, any key the buffer does not own
  must return `Unhandled` so it reaches the composer. Adding a binding narrows
  the composer's keyspace; that narrowing is intended, but the fallthrough
  contract must remain true for everything not deliberately claimed.
- **Insert-mode text integrity.** No normal-mode or global binding may swallow a
  printable character while the route is in Insert. This already failed once in
  production (`Shift+W` / capital `F` opened pickers and ate the character in the
  default configuration) and must be guarded by a non-vacuous test, not by
  inspection.
- **Binding reachability.** Every declared normal-mode binding must be reachable
  through the real dispatch path. An arm shadowed by an earlier guard is a
  silent capability loss — see §4.
- No path- or current-directory-derived authority; all rendering and remote
  DTOs stay bounded.

### Capabilities

- Character-finding motions (`f`/`F`/`t`/`T`, `Alt-.`), long-word motions
  (`W`/`B`/`E`), and smart-home (`gs`).
- A single coherent pending-prefix grammar (`g`, `space`, `[`, `]`, `Alt-*`)
  replacing per-feature ad-hoc prefix flags.
- Core change verbs: paste (`p`/`P`), bare yank (`y`), replace (`r`/`R`), case
  switching (`~`, backtick, `Alt-backtick`).
- Textual goto mode and the unimpaired `[`/`]` navigation layer.
- Multi-selection: a selection set, with every existing command applying to all
  members, plus the selection-set manipulation verbs.
- Match mode and text objects; surround add/replace/delete.
- Space mode and extended goto mode over the existing picker surface.
- Insert-mode control chords that do not collide with already-bound app chords.
- Undo history traversal (`Alt-u` / `Alt-U`) beyond the linear two-stack model.

### Infrastructure

- One pending-prefix state machine with a single resolution path.
- A binding reachability guard (static or test-based) proving no declared arm
  is shadowed.
- Non-vacuous Insert-mode printable-character sweep across both keymaps.
- A dispatch-level keymap table that documentation and tests can both read, so
  the published keymap is derived rather than hand-maintained.

### Polish

- Help-overlay entries and `docs/tui.md` keymap tables kept in step with the
  implemented set.

## 3. Non-goals

- No CRDT/OT collaboration, soft wrap, or several simultaneously open document
  attachments (inherited non-goals from the predecessor roadmap).
- No `Ctrl-w` window mode or arbitrary pane/split layouts. The editor has no
  splits today; that is a pane-layout workstream, not a keybinding workstream.
- No macros (`Q`/`q`), named registers (`"<reg>`), shell filters (`|`, `!`,
  `$`), `.` repeat-insert, or `Ctrl-a`/`Ctrl-x` increment/decrement in this
  roadmap. Individually cheap, collectively a different vocabulary; each is
  registered as deferred product work rather than silently folded in.
- No vendored Helix code, configuration, or assets.
- No relaxation of sandbox, `file.read`, or `file.modify` authorization.
- No code-action execution, automatic import edits, or LSP write operations.
  `LspReadOperation` stays read-only by construction; M013 extends it only with
  **read** variants plus rename/format **preview**. `=` format, `space r`
  rename apply, and `space a` code-action apply remain out of scope. **M015
  binds the two previews** (`=`, `space r`) and binds `space a` to a statement,
  and all three still apply nothing. See §10 risk 2 — the apply itself needs
  the **bridge** from the editor's preview to the registered reviewable
  artifact ADR-0008's accepted write path consumes, not a new ADR.

## 4. Current state

Complete normal-mode keymap implemented by `handle_normal_key` in
`src/tui/commands/editor.rs`:

- Motion: `h`/`j`/`k`/`l` and arrows, `w`, `b`, `e`, `0`, `$`, `G`, `gg`,
  `Ctrl+d`/`Ctrl+u`, `PageUp`/`PageDown`.
- Selection: `v` (toggle character), `x` (select line / grow one line).
- Change: `d` (delete selection), `D` (delete to line end, a deliberate
  non-Helix local extension), `u` (undo), `space y` (yank to clipboard).
- Insert entry: `i`, `a`, `I`, `A`, `o`, `O`.
- Lines and modes: `:` command line, `/` in-document search, `n`/`N`.
- Mode-independent: `Ctrl+s` save, `Ctrl+r` resync, `R` reload.

Everything else returns `BufferKey::Unhandled`.

Insert mode handles printable characters, `Backspace`, `Delete`, arrows,
`Enter`, and `Tab`. There are **no** insert-mode control chords.

The `:` palette exposes exactly nine commands (`EditorCommandName::ALL`): `w`,
`q`, `wq`, `q!`, `e`, `wa`, `new`, `open`, `complete`. LSP is reachable only
through function keys F2–F9 and is read-only.

**That F2–F9 surface is not reliably delivering today.** Verified at baseline
`84440fe7f`: the diagnostics publisher has no production driver, and warm-client
resolution returns a false `NotReady` whenever more than one server shares a
workspace root. Both are recorded and owned by M013, and both must be fixed
before LSP leaves are bound — a key that opens an always-`NotReady` panel is
harder to diagnose than an absent key.

### Known defects and structural limits found during the parity survey

1. **`Ctrl+r` redo is unreachable.** The mode-independent guard in
   `editor_buffer_key` returns `RequestResync` for `Ctrl+r` before dispatch
   reaches the `editor_redo` arm inside `handle_normal_key`.
   `handle_normal_key` has exactly one caller, so the redo arm is dead in the
   production key path. Every existing redo test calls `editor_redo` directly
   and therefore bypasses dispatch — which is why the suite is green. Normal
   mode currently has undo and no reachable redo.
2. **Two bespoke prefix mechanisms.** `TuiDocumentPresentation::pending_command`
   is a `String` bounded by `MAX_EDITOR_PENDING_COMMAND = 2` (used for `gg`);
   `yank_prefix_pending` is an independent `bool` (used for `space y`). Adding
   Helix's `[`/`]` unimpaired layer and `Alt-*` chords as-is would add a third
   and fourth mechanism.
3. **Single selection.** `SelectionState` holds `Option<Selection<P>>` — one
   held selection, never a set. Roughly fifteen Helix selection-manipulation
   verbs are therefore unimplementable as specified, not merely unbound.
4. **No tree-sitter anywhere in the workspace.** Every Helix binding marked
   `TS` (syntax-node selections, `]f`/`[f`, `]t`/`[t`, `]a`/`[a`, `]c`/`[c`,
   `]T`/`[T`) is blocked at the dependency level. See ADR-0034.
5. **No paste binding.** Yank exists (`space y`); there is no `p`/`P`.
6. **No syntax-aware layer at all.** No surround, text objects, or
   match-bracket symbols exist in `src/tui/editor.rs` or
   `src/tui/helix_selection.rs`.
7. **Undo is two linear stacks** (`undo`/`redo` in `src/tui/document_session.rs`),
   with no `earlier`/`later` traversal.

## 5. Target architecture

A single **pending-prefix state machine** owns multi-key grammar. It holds at
most one pending prefix, resolves exactly one terminal key against it, and
disarms on any non-matching or terminal key. Every mode that needs a prefix
(`g`, `space`, `[`, `]`, `Alt-`) registers its leaf table against it rather than
owning a flag.

A single **dispatch function** resolves `(mode, modifiers, code)` to a typed
`EditorIntent`, with a total, table-driven match. Every declared binding is
reachable from exactly one place, and a guard proves no arm is shadowed.

Above that sits a **selection set**: `Vec<Selection>` with a distinguished
primary, replacing the current single `Option<Selection>`. Motions transform the
set (each member moves, preserving relative order); `v` extends all members; a
`Moved` intent leaves a line selection alone as it does today. All existing
commands then apply to every member, which is what makes the Helix
selection-manipulation verbs implementable rather than special-cased.

The undo model extends from two linear stacks to a persistent history with a
time cursor, so `Alt-u`/`Alt-U` can walk it without abandoning redo.

The published keymap in `docs/tui.md` and the help overlay are **derived from
the binding table**, not hand-maintained beside it.

## 6. Dependency graph

```text
M001 dispatch authority + reachability guard
  |
  +--> M002 unified prefix state machine
  |        |
  |        +--> M003 character-finding motions (f/F/t/T, Alt-.)
  |        |        |
  |        |        +--> M004 core change verbs (p/P, y, r/R, case)
  |        |        |
  |        |        +--> M005 textual motions + goto mode (gs, W/B/E)
  |        |                 |    [also delivers the leaf-table shape]
  |        |                 |
  |        |                 v
  |        |        M013 LSP read delivery + reachability  <-- corrective, see below
  |        |                 |
  |        |                 +--> M010 space mode + extended goto
  |        |
  |        +--> M006 unimpaired [ ] navigation (non-TS subset)
  |                 |
  |                 +--> [unimpaired change nav — NOT planned; no data source]
  |                 `--> [unimpaired diagnostic nav — folded into M013]
  |        |
  |        +--> M011 insert-mode control chords
  |        |
  |        +--> M012 undo history traversal
  |
  `--> M007 multi-selection core  (longest pole; independent of M002)
           |
           +--> M008 multi-selection command surface
                    |
                    `--> M009 match mode + text objects
                             ^
                             |
                     ADR-0034 accepted (gates syntax-node slices)
```

Classification:

- **Hard:** M002 requires M001. M003–M006, M011, M012 require M002. M004 and
  M005 require M003. **M010 requires M013** (hard). M008 requires M007. M009
  requires M008 **and** ADR-0034 for its syntax-node slice. All of M002's,
  M003's, M004's and M005's predecessors are now closed.
- **Hard, added 2026-10-18:** **M006 owns no LSP edge at all.** Its original
  boundary claimed `]d`/`[d`/`[D`/`]D`; those bindings are now **M013's**, for
  the two reasons in M006 amendment A1 — M013's boundary already named them, and
  the read path they need is broken until M013 repairs it. M013 therefore gains a
  binding obligation it already carried in prose but which no plan had allocated.
  The unimpaired **change** quartet (`]g`/`[g`/`]G`/`[G`) has no owner yet and is
  registered as deferred product work, because no reachable data source exists for
  it (M006 amendment A1.3). Neither obligation is a new blocker on M006: M006
  stays `ready` with a **smaller** boundary.
- **Interface:** M002's prefix contract is an interface dependency for every
  later milestone — changing it after they land is a breaking refactor. M003
  extended it with prefixes whose next key is *data* rather than a leaf glyph.
  **M005 replaces the single-glyph `EditorPrefix::leaf` assumption with an
  explicit per-prefix leaf table**, which M010 and M013 then build on; this is
  the second shape change later milestones must build against, and the reason
  M013 is sequenced after M005 rather than in parallel.
  **A third interface shape is now frozen by M007's plan §5**: the selection-set
  API (`set_members`, `selectors`, merge/rotate/split primitives) that M008 is
  written against. It is specified in M007's plan precisely because M008 depends
  on it and neither milestone is closed.
- **Soft:** M004–M006, M011, M012 are mutually independent once M002 closes.
  M013 has no dependency on them beyond M005's leaf table.
- **Operational:** terminal-panel convergence must be re-verified after M002,
  M007, and M008, because the panel shares the selection dialect. M007's plan
  predicts parity is preserved **by construction** if the single-selection
  accessors are kept as primary-scoped rather than replaced — see its §1.1.

M001 is deliberately first and small. Fixing the shadowed-binding defect
before adding twenty more bindings is the point: otherwise each new arm carries
the same unreachability hazard, and M002's consolidation would be built on an
authority model already known to be lossy.

## 7. Milestones

### M001 — Key dispatch authority and reachability

Class: invariant

Objective: repair the `Ctrl+r` shadow and make unreachable normal-mode arms
structurally detectable.

Dependencies: none (predecessor roadmap closed; ADR-0011/0021 accepted).

Deliverable boundary: the resync/redo conflict resolved with one documented
owner for each chord; every declared normal-mode binding exercised through the
real dispatch path rather than by calling a helper directly; a guard that fails
when a declared arm becomes unreachable.

User value: normal mode gains working redo, and the keymap stops being able to
lie about what it binds.

Exit conditions: `Ctrl+r` reaches redo in Normal mode and resync retains a
distinct, documented chord; a dispatch-level test table covers every declared
binding; a shadowing regression test is non-vacuous; the Insert-mode
printable-character sweep covers both keymaps.

Deferred: the derived-keymap infrastructure is introduced here only if it does
not expand this milestone; otherwise it lands with M002.

### M002 — Unified prefix state machine

Class: infrastructure

Objective: one pending-prefix mechanism replacing `pending_command` and
`yank_prefix_pending`, with `g`, `space`, and the unimpaired `[`/`]` grammars
registered against it.

Dependencies: M001 closed (hard).

Deliverable boundary: a single prefix type with arm/disarm semantics; the `gg`
and `space y` gestures expressed through it; `[` and `]` armed and consuming,
with the unimpaired leaf table arriving in M006; the two legacy fields removed
and their render-path plumbing updated.

User value: multi-key editing verbs stop needing one bespoke flag each, and the
pending-indicator render stays correct as the grammar grows.

Exit conditions: both legacy prefix mechanisms deleted (not merely bypassed);
`gg` and `space y` behavior preserved exactly; the pending indicator renders
correctly for a multi-character prefix; prefix bound is enforced.

Deferred: unimpaired leaf bindings, space-mode leaves, goto-mode leaves.

### M003 — Character-finding motions

Class: capability

Objective: `f`/`F`/`t`/`T` find-until-char motions plus `Alt-.` repeat, which
together are Helix's highest-frequency motion family.

Dependencies: M002 closed (hard — `f` and `Alt-.` consume a following key).

Deliverable boundary: motion resolution that is **not** confined to the current
line (Helix's documented difference from vi); `t`/`T` semantics that leave the
cursor on the target character; `Alt-.` repeating the last `f`/`t`/`m`/`[`/`]`
motion; the last-motion record cleared by any other motion.

User value: the single largest ergonomic gap closes; navigation no longer
requires O(n) `j`/`k` for a symbol on a nearby line.

Exit conditions: all four motions work across line boundaries; `Alt-.` repeats
the last find motion and is disarmed by a different motion; UTF-8 boundaries
respected; tests cover motion past end-of-file and multi-byte targets.

### M004 — Core change verbs

Class: capability

Objective: close the missing change verbs that make an edit-then-paste loop
impossible today.

Dependencies: M003 closed (hard — shares the motion/verb resolution path).

Deliverable boundary: `p`/`P` paste (after/before) from the system clipboard;
bare `y` yank; `r` replace-with-character and `R` replace-with-yanked; `~` case
switch, backtick lowercase, `Alt-backtick` uppercase, applied to the held
selection.

User value: a complete basic edit loop without leaving Normal mode.

Exit conditions: paste round-trips with yank; `r` is refused on multi-byte
characters rather than corrupting them; case verbs operate on the whole held
selection; documented interaction with the local `D` extension is unchanged.

Deferred: `c`/`Alt-c` change, noyank variants, `>`/`<` indent, `=` format.
`space p`/`space P` are left to the milestone that owns `SPACE_LEAVES`.

### M005 — Textual motions and goto mode

Class: capability

Objective: `gs` smart-home (today there is **no** way to reach first
non-whitespace), long-word `W`/`B`/`E`, textual `gj`/`gk`, and the remaining
goto-mode leaves that need no new infrastructure.

Dependencies: M003 closed (soft), M002 closed (hard — goto mode is a prefix).

Deliverable boundary: `gs`; `W`/`B`/`E`; `gj`/`gk` as goto leaves; goto leaves
`gs`, `ge`, `gh`, `gl` plus the existing `gg`.

User value: removes the single most surprising omission (no smart-home) and
completes the parts of goto mode that are pure navigation.

Exit conditions: `gs` round-trips with `0` on blank and indented lines; long-word
motions treat whitespace-separated runs as one unit and are observably distinct
from `w`/`b`/`e`; goto leaves are reachable through two real keystrokes and
documented; `gg` and `space y` behave exactly as before.

Deferred, each recorded rather than left silent: LSP-backed goto leaves (M010,
gated on M013); `gt`/`gc`/`gb`, which are **screen**-relative and need the real
viewport height rather than the fixed `NOMINAL_VISIBLE_LINES` guess; `gw` label
navigation; `g|` and buffer/alternate-file leaves (`gf`, `ga`, `gm`, `gn`,
`gp`, `g.`) pending multi-document attachment (predecessor non-goal).

### M006 — Unimpaired navigation, non-TS subset

Class: capability

Objective: the `[`/`]` navigation layer for every entry that needs neither
tree-sitter nor an LSP write.

Dependencies: M002 closed (hard).

Deliverable boundary *(amended — see A1)*: `]p`/`[p` paragraph; `]Space`/`[Space`
add newline below/above; `Alt-.` widened to repeat the unimpaired motions, which
this roadmap's M003 entry already composes. The `[`/`]` grammar itself lands here
— M002 armed both prefixes with an empty leaf table, and this milestone is what
gives the table rows.

User value: paragraph and whitespace jumps cover long-file traversal, and the
`[`/`]` layer becomes a real grammar rather than two inert prefixes.

Exit conditions *(amended)*: every binding in the amended boundary navigates or
edits as documented; `[`/`]` as literal characters remain typable in Insert —
**with a test, which did not previously exist**; unimpaired bindings do not
shadow the existing `:` and `/` lines, and in particular `]` then `Space` resolves
to the `]Space` leaf rather than arming space mode; the diagnostic and change
quartets are **asserted unbound** so a later milestone cannot land them by
accident.

Deferred: `]f`/`[f`, `]t`/`[t`, `]a`/`[a`, `]c`/`[c`, `]T`/`[T` — ADR-0034.

> **Amendment A1 (2026-10-18).** The original boundary above claimed six
> families: `]d`/`[d`, `]D`/`[D`, `]p`/`[p`, `]g`/`[g`, `]G`/`[G`, and
> `]Space`/`[Space`. Three independent checks — the live Helix keymap, a
> call-graph trace of the diagnostics read path, and a search for any reachable
> change/hunk data — showed two of the six cannot be delivered here and one was
> described backwards. Full reasoning in
> `plans/implementation/tui-editor-helix-parity/006-unimpaired-navigation-non-ts-subset.md`
> §1.1. Summary:
>
> 1. **`]D`/`[D` and `]G`/`[G` were inverted.** This roadmap read "`]D`/`[D`
>    first/last" and "`]G`/`[G` first/last change". The live keymap reads
>    `]D` = `goto_last_diag`, `[D` = `goto_first_diag`, `]G` = `goto_last_change`,
>    `[G` = `goto_first_change`. `]` is *forward* throughout this layer (`]d` =
>    next, `]p` = next, `]g` = next), so `]D` is last and `[D` is first. The same
>    rule applies when those bindings do land — Helix is normative, and this
>    prose was not.
> 2. **The diagnostic quartet moves to M013.** Two reasons, either sufficient.
>    **This roadmap already claims those bindings twice** — M013's deliverable
>    boundary names "`]`/`[` diagnostic navigation", so M006's claim was an
>    overlap, not a division of labour. And the read path is broken: at this
>    baseline `publish_changed_diagnostics` and `DIAGNOSTICS_POLL_INTERVAL`
>    (`src/core/daemon_lsp.rs:555`, `:570`) have **zero callers**, so
>    `daemon.lsp_diagnostics` never receives a production write, and
>    `find_existing_client_for_root_hint` (`:213-215`, `:334-336`) returns
>    `NotReady` whenever more than one client shares the root. A `]d` that jumps
>    nowhere is harder to diagnose than an absent key. This is the same reasoning
>    already applied to M010 in §7.
> 3. **The change quartet needs a milestone of its own.** The claim that it works
>    by "reusing the existing diff surface" is **false**. `crates/egggit/src/diff.rs`
>    exposes `ChangedFile { path, kind }` and `FileDiff { path, patch }`, neither
>    of which carries line ranges; the only parsed hunk type, `DiffHunk`
>    (`src/tui/components/diff.rs:27`), is produced by the **private**
>    `DiffViewer::compute_diff` (`:66`) inside a widget; and `handle_normal_key`
>    receives `&mut EditorState`, which has no git handle and no base copy of the
>    document. Delivering `]g` would mean a new App-level request, a daemon
>    response type, parsed-hunk presentation state, and a new IPC surface — a
>    second ownership boundary inside a capability milestone, which §5 sizing
>    forbids. The three options for that milestone, and the constraint that only
>    one of them keeps the editor free of filesystem authority, are recorded in
>    the M006 plan §5. **It needs fresh planning and is registered as deferred
>    product work, not as an orphan.**

> **Amendment A2 (2026-10-19).** M006 is implemented and closed
> (`plans/closure/tui-editor-helix-parity/006-status.md`). The boundary above
> was delivered as written: `]p`/`[p`, `]Space`/`[Space`, and `Alt-.` widened to
> the unimpaired motions. Five things the planning pass could not have known, and
> which a successor milestone would otherwise re-derive:
>
> 1. **`PrefixAction` needed a third arm, `Edit`.** `]Space` inserts a newline,
>    and `editor_motion` is the one function in the dispatch whose contract is
>    that it never touches text. The `Motion` arm was not an option; the leaf has
>    to say what kind of thing it is. It routes through `editor_editing_command`,
>    so the read-only gate, the undo entry and the document transaction are the
>    ones `x`/`d`/`y` already use.
> 2. **A paragraph scan has *three* answers, not two.** `Landed`, `NoParagraph`,
>    and `Bounded` for a walk that ran out of byte budget. The plan described the
>    disclosure but not the type; `Option` cannot carry the third, and collapsing
>    it into `None` would report "there is none" about a document that has one —
>    the one answer a reader has no way to distrust. `MAX_PARAGRAPH_SCAN_BYTES` is
>    the whole-walk budget (the frame's own `MAX_FRAME_SOURCE_BYTES`), because
>    `bounded_prefix_end`'s per-line cap alone still leaves a 200 MB walk on a
>    200 MB file with no blank lines.
> 3. **Deliberate divergence from Helix, on the degenerate case.** With no blank
>    line anywhere, Helix's `move_next_paragraph` skips every remaining line and
>    lands on the end of the buffer — evidence in
>    `test_behaviour_when_moving_to_next_paragraph_single`,
>    `helix-core/src/movement.rs`. CodeGG reports "no next paragraph" instead: an
>    end-of-buffer jump looks like the key did something and hides that it found
>    nothing. Recorded rather than matched.
> 4. **`Alt-.` had a pre-existing slot defect, found here and fixed.** A find
>    returned early from `note_motion` without clearing the other repeat slot, so
>    `]p`, `f x`, `Alt-.` repeated the *older* `]p` backwards instead of `f x`
>    forwards. Harmless-looking while there were two slots and `Alt-.` checked
>    one of them first; wrong the moment a third existed. Every recorder now
>    closes the other two doors, which is the single-slot rule the function's
>    comment already claimed.
> 5. **`o`/`O` do not insert a line, and that predates M006.** `enter_insert`
>    maps `OpenLineBelow`/`OpenLineAbove` to "the cursor", so `resolve_edit`'s
>    open-line arms are reachable only from their own unit test. `o` enters
>    Insert mode on the current line rather than opening a new one below it.
>    **Found, recorded, not fixed** — changing a pre-existing milestone's
>    behaviour is not this one's to decide. Owner: whoever next touches the
>    insert-entry chords.
>
> The terminal panel is **explicitly exempt**. It has no unimpaired layer, no
> `helix_selection` import in `src/tui/commands/interactive_terminal.rs`, and no
> leaf table to extend; `src/tui/interactive_terminal.rs` is **unmodified** by
> this milestone, which is the evidence for the exemption rather than an
> assertion of it.

### M007 — Multi-selection core

Class: capability (enabling)

Objective: replace the single held selection with a selection set and a
distinguished primary, with every existing command applying to all members.

Dependencies: M001 closed (hard). Independent of M002.

Deliverable boundary: `SelectionState` holds a set with a distinguished primary;
motions transform the whole set preserving order; `v` extends all members;
line-selection growth applies per member; the selection-dependent effects emit
one range set — `d`, `y`/`space y`, and M004's `r`/`R`/`~`/`` ` ``/`p`/`P`; the
set lands as **one** transaction; rendering shows every head; undo/redo restores
the set; read-only buffers refuse selection **growth**.

**`D` is deliberately excluded from that list** (amendment A1 below).

User value: the enabling change for the entire Helix selection vocabulary, and
the first genuinely multi-cursor editing capability in CodeGG.

Exit conditions *(amended)*: a programmatically constructed set of N members is
deleted by one command in one transaction; undo/redo round-trips a multi-selection
edit; read-only buffers refuse selection growth; terminal-panel parity is
re-verified or an exemption is recorded.

Deferred: the manipulation verbs themselves (M008).

> **Amendment A1 (2026-10-18).** Two claims in the original entry were checked
> against the code and corrected. Full reasoning in
> `plans/implementation/tui-editor-helix-parity/007-multi-selection-core.md`.
>
> 1. **"`d`, `space y`, `D` emit one range set" is wrong about `D`.**
>    `resolve_edit` (`src/tui/editor.rs:2525-2614`) never calls `selection_range`;
>    `D` resolves to `EditorCommand::DeleteToLineEnd` (`:2569-2576`), which reads
>    only `cursor_byte` and `line`. `D` is **not** selection-dependent today, so
>    including it would be a *new user-visible semantic on a deliberately local
>    extension*, not a refactor — and M004's exit conditions require the
>    documented interaction with the local `D` extension to stay unchanged.
>    `D` stays cursor-only. The set-scoped effects are `d`, `y`/`space y`, and
>    M004's six verbs, which already route through a single funnel.
> 2. **"N cursors selectable and deletable in one gesture" is not available
>    here.** No verb that *creates* N cursors exists until M008 (`C` / `Alt-C`).
>    M007 proves the **model** — a programmatically constructed N-member set is
>    deleted by one command in one transaction — and M008 delivers the gesture.
>    Claiming the gesture here would be a user-visible claim with no key behind
>    it.
>
> Two findings from the same pass are worth recording, because both make this
> milestone cheaper than its title suggests. **N edits already work**:
> `TextTransaction.edits` is a `Vec<TextEdit>`, `invert()`
> (`src/tui/editor.rs:1854-1871`) walks every edit with a shift offset, and
> `apply_local` is transaction-level — so "one transaction, not a partial
> multi-cursor edit" needs no controller work. And **terminal-panel parity is
> preserved by construction** if the single-selection accessors are kept as
> primary-scoped rather than replaced: `SelectionState::is_selected()`,
> `SelectionState::span()` and `SelectionState::line_range()` have **no
> production callers at all**, so keeping them reading as the primary is free,
> and the panel's two production uses of the *inherent* `Selection::line_range()`
> (`src/tui/interactive_terminal.rs:766`, `:1932`) both reach it through
> `selection()`. That is the difference
> between this milestone landing and not, and it is why the M007 plan makes that
> choice explicitly.
>
> One finding is a genuine gap this milestone creates work for:
> `apply_helix` (`src/tui/commands/editor.rs:3275-3283`) and `helix_motion`
> (`:3269-3272`) have **no `editable` gate**, so `v`, `x`, and growth arm
> selections on a read-only buffer today with no notice. The exit condition
> above is therefore new behaviour, and its correct reading is narrow: **growth**
> is refused, plain cursor movement is not. A reader must still be able to move
> around a read-only file.
>
> M003's closure records that `t`/`T` are "a selection property and belongs with
> M007". That reasoning does not hold against the keymap: Helix's `t`/`f`
> difference is whether the cursor lands **on** the target or **past** it, which
> is a cursor property needing no set. The M007 plan makes this a stop condition
> rather than silently adopting or silently overriding M003's assignment.

> **Amendment A2, 2026-10-18 — M007 closed; M008 unblocked.** Closure record:
> `plans/closure/tui-editor-helix-parity/007-status.md`. Three things the
> roadmap's entry did not anticipate, recorded here rather than only in the
> closure because they are roadmap-level facts:
>
> 1. **A pre-existing correctness defect was found and fixed on the delete
>    path.** `selection_range` resolved a character selection's end with
>    `next_char_boundary(..).unwrap_or(last.byte)`; at the end of a document
>    `next_char_boundary` is `None`, so the fallback set `end == last.byte` and
>    **dropped the head character**. `v` on the final character of a file
>    resolved to an empty range, so `d` on it deleted nothing. Fixed to use
>    `char_end`. This is why the M007 boundary included rewriting
>    `selection_range` at all.
> 2. **The primary's head is not painted with cursor styling when it differs
>    from the cursor.** A head that is not the cursor draws with the accent. On
>    every Helix path `cursor_byte` *is* the primary's head, so this only
>    changes the state a goto leaves behind — but painting a second
>    reverse-video block would read as "the keyboard is here".
> 3. **Selection consumption moved from five verbs to one funnel.** `submit_edit`
>    now records the set into the undo entry and clears it for *every* accepted
>    edit, because that is what lets `u` restore the set. This is a visible
>    behaviour change for edit verbs that previously kept the selection —
>    `D` is the one reachable from Normal mode — and
>    `apply_helix_verb` correspondingly no longer clears it when reporting a
>    delete or yank effect. The geometry layer reports; the App layer consumes.
>
> **M007's §5 set API contract shipped as specified**, so M008 needs no
> corrective pass: `SelectionState::{restore_set, selections, primary,
> primary_index, into_parts, clear, grow_all_to}`,
> `TuiDocumentPresentation::{selections, primary_index, clear_selections,
> restore_selections}`, `HelixVerbOutcome::{DeleteRanges, YankRanges}`,
> `RangeSetEdit`, `BufferKey::{DeleteRanges, YankRanges}`, and
> `editor::{selection_ranges, merge_ranges, delete_ranges}`. One addition
> beyond the plan, which M008 must know: `RangeSetEdit` distinguishes `Empty`
> from `TooMany` precisely so a refusal can be *shown* — the 256-edit cap
> (`MAX_DOCUMENT_EDITS`) refuses rather than truncates, because a truncated
> multi-cursor edit is a state neither `u` nor redo can describe.

### M008 — Multi-selection command surface

Class: capability

Objective: the Helix verbs that only make sense over a selection set.

Dependencies: M007 closed (hard).

Deliverable boundary: `;` collapse, `Alt-;` flip, `Alt-:` ensure-forward, `%`
select all, `,` keep primary, `Alt-,` remove primary, `C`/`Alt-C` add cursor
below/above, `S` split on regex, `K`/`Alt-K` keep/remove by regex, `(` `)` and
`Alt-(` `Alt-)` rotate selections and contents, `&` align in columns,
`Alt-minus`/`Alt-_` merge and merge-consecutive.

User value: completes Helix's core differentiator.

Exit conditions: each verb is correct for heterogeneous selection sets (mixed
empty/non-empty, mixed modes, different lines); regex verbs handle a
no-match set without corrupting the buffer; every verb is documented.

Deferred: `v`-as-extend-mode unification (fold into M008 if it proves bounded,
otherwise a corrective addendum).

> **Amendment A1 (2026-10-18).** The plan is written and registered
> **`blocked` on M007**, not `ready`: M007's hard dependency is unmet, and
> `plans/003-planning-process.md` §4 makes `ready` conditional on that. It is
> written early for one reason — **M008's verbs are undefined until the set API
> they operate on exists**, so §5 of the M008 plan pins the exact contract M007
> must expose. Two plans written months apart against an imagined interface is
> how `EditorPrefix::leaf()` → `leaves()` had to be corrected once already.
>
> **Amendment A3 (2026-10-19) — M008 closed.** All eighteen verbs are
> bound; the key-authority guard moved from 48 to **66 Normal-mode chords**, the
> count this milestone's plan pinned. Three roadmap-level facts did not survive
> contact with the implementation:
>
> 1. **Terminal-panel parity is broken on exactly one chord, and it is
>    recorded rather than fixed.** §12 warned that eighteen new chords were the
>    first thing in this roadmap able to break it, and the prediction was right:
>    the panel's own Normal-mode table claims `C`/`c` for
>    `TerminalNavAction::Clear`, while the editor uses `C` to add a cursor. The
>    other seventeen are unclaimed by the panel and are swallowed by it. The
>    divergence follows from the two surfaces having different subjects — the
>    panel has no document to place a cursor in — so neither surface could have
>    chosen differently. Closure finding 1.
> 2. **The plan's preferred prompt design held, and its §6 claim is now
>    verified rather than argued.** Reusing the `:` command line meant **no new
>    prefix field and no new input mode**: `check_tui_editor_prefix_state.py`
>    emits byte-identical output before and after this milestone. The plan's own
>    concern — that a dedicated prompt is easy to add and hard to disarm — turned
>    out to be the right concern.
> 3. **Two additions beyond the plan's §5 contract.** `RangeSetEdit::Edit`
>    carries an optional note, because `&` must disclose a member it could only
>    measure in part; and `HelixVerb` lost `Copy`, because the three regex verbs
>    carry a `String` and a verb that owns text is not a unit value. Neither
>    changes what M009 consumes — M009's input is the set API.
>
> **Unblock audit.** M009 was blocked on this milestone **and** on ADR-0034
> acceptance. This milestone is discharged; ADR-0034 is still `proposed` and is
> **not** discharged here, so M009 stays blocked on the ADR alone and its row is
> annotated accordingly rather than flipped to `ready`. A closure must not
> quietly discharge a blocker it did not touch.
>
> **Deferred, unchanged.** `v`-as-extend-mode unification was **not** folded in:
> it did not prove bounded. Unimpaired change navigation (`]g`/`[g`/`]G`/`[G`)
> remains deferred, unregistered product work with no assigned milestone.
>
> **Amendment A2 (2026-10-18) — M007 closed, so this milestone is `ready`.**
> M007's hard dependency is satisfied and its §5 set API contract shipped
> **exactly as pinned** (see the M007 entry's A2). The early write-out paid
> off: no interface drift, and no corrective pass is needed. M008's remaining
> blocker list is unchanged otherwise — M009 still waits on this milestone
> *and* on ADR-0034 acceptance.
>
> Two findings from the verification pass change the sizing estimate. **No chord
> collides**: at this baseline the guard block
> (`src/tui/commands/editor.rs:2795`) claims exactly two chords and
> `handle_normal_key` claims 48, and none of these eighteen — bare `c`, `s`, `C`,
> `S`, `K`, `;`, `,`, `%`, `&`, `(`, `)` nor any `ALT` variant — is claimed. This
> is the second-cleanest keymap addition in the roadmap after M004's, and no
> existing binding moves. **And `S`/`K`/`Alt-K` need no new dependency**:
> `regex = "1"` is already in the root package (`Cargo.toml:213`) and already
> used under `src/tui/` (`src/tui/components/messages.rs:26-29`,
> `src/tui/command.rs:1317`). What remains expensive in that third of the
> milestone is the **argument prompt** and boundedness, not the matching — which
> is why the plan recommends reusing the `:` command line rather than building a
> new input mode.
>
> All eighteen bindings were quoted from the live keymap's Normal mode →
> Selection manipulation table rather than from this entry, in the same spirit as
> M004's `R` correction. Two Helix labels are easy to invert and are called out in
> the plan: `)`/`(` rotate the **primary selection**, while `Alt-)`/`Alt-(` rotate
> the **selection contents** — one moves the cursor, the other moves bytes.
>
> Seven real Helix selection verbs sit outside this entry's boundary: `s`
> (select all regex matches), `Alt-s` (split on newlines — *cheaper* than `S`),
> `_` (trim), `X`/`Alt-x` (extend/shrink to line bounds), `J`/`Alt-J` (join),
> and `Ctrl-c` (comment). They are enumerated with reasons in the M008 plan §7.
> `Alt-s` in particular is the natural next slice once `S` ships.

### M009 — Match mode and text objects

Class: capability

Objective: `m m` matching bracket, `m s`/`m r`/`m d` surround, and `m a`/`m i`
text objects.

Dependencies: M008 closed (hard); **ADR-0034 accepted** for the syntax-node
slice.

Deliverable boundary: pair-aware bracket matching; surround add/replace/delete;
inner and around objects for word, pair, and (if ADR-0034 admits it) block.

User value: structural editing without syntax awareness becomes possible.

Exit conditions: bracket matching survives unbalanced and multi-byte input;
surround replace preserves nesting; objects resolve against a bounded window
and never scan the whole buffer per keystroke.

> **Amendment A3 (2026-10-19) — the ADR-0034 gate is narrower than this entry
> says, and the milestone splits in two.**
>
> 1. **The byte-derived boundary is not gated on ADR-0034.** The ADR's own
>    decision text says so: *"Word and delimiter-pair text objects are
>    implemented frontend-side from bytes and are explicitly **not** gated on
>    this decision — they do not need a parser."* Its Option C is written as
>    *"Land M001–M008, M010–M012 and their non-TS slices"*, and its Context
>    section says the same: *"word and delimiter-pair objects are computable
>    from bytes — but block objects and syntax-node-aware selection need
>    one."* Helix's own text-object table agrees independently: `w`, `W`, `p`,
>    the specified surround pairs and `m` are parser-free, while `f`, `t`,
>    `a`, `c`, `T` and `x` need a tree-sitter grammar. This entry's own
>    deliverable boundary already names word and pair as the objects, with
>    block as the conditional one, and its exit conditions are all
>    parser-free. **M009 therefore delivers the byte-derived boundary and is
>    `ready`**, and the syntax-node slice moves to a new milestone, **M014**,
>    which is blocked on ADR-0034 acceptance.
> 2. **Helix marks `m m` as `TS`.** The live keymap lists *"Goto matching
>    bracket (**TS**)"*. CodeGG's `m m` will be a **delimiter-pair table**
>    match — a depth counter over bytes, not a syntax-tree query. That is a
>    deliberate local divergence, recorded in the plan and in the docs
>    rather than left to be discovered, because the alternative is a key that
>    does nothing. A syntax-aware `m m` is part of M014.
> 3. **Helix's `m` is a two-level grammar and this editor's machine is
>    one-prefix-one-terminal-key.** `m` + command, then command + argument
>    (`m s (`, `m r ([`, `m a w`) does not fit a single leaf table. The
>    resolution keeps **one** presentation field: the sub-state rides in the
>    `EditorPrefix` variant, so the prefix-state guard stays green. A
>    reviewer should check that claim rather than trust it.
>
> Plan: `plans/implementation/tui-editor-helix-parity/009-match-mode-and-text-objects.md`

> **Amendment A4 (2026-10-19) — M009 is closed, and the `PrefixAction`
> question is answered by a third answer.**
>
> 1. **M009 is `closed`.** Closure record:
>    `plans/closure/tui-editor-helix-parity/009-status.md`. The `m` grammar
>    landed whole: `m m` (bracket match), `m s` / `m d` / `m r` (surround,
>    unsurround, re-surround), and `m a` / `m i` (the word, WORD, paragraph,
>    delimiter-pair and closest-pair text objects). The status row shows
>    which half of the grammar is pending, and a bracket match that cannot
>    land says **which** of the two things went wrong — "no bracket under
>    the cursor" and "no matching bracket" are different answers.
> 2. **§13.4 question 2 is answered, and neither option (a) nor option (b)
>    was taken.** The leaf table does not need to carry a *verb*: the
>    two-level commands' leaves all say the same thing — **which argument is
>    being waited for** — so the leaf carries `PrefixAction::Capture(MatchArg)`,
>    whose payload is a `Copy` enum, and the verb is produced when the
>    *capture* resolves. `Copy` was never lost, the leaf tables stayed
>    `const`, and §14's stop condition never fired. Recorded in the plan's
>    §17.1.
> 3. **§13.4 question 4 is answered: the indicator renders on the
>    presentation's own banner row**, beside the history cursor and the
>    pending find, and the two halves of `m r` are told apart by the
>    **character already captured** — `"m r"` and `"m r ("`. That required
>    `EditorPrefix::display()` to return a `Cow<'static, str>` rather than a
>    `&'static str`. `"m r ("` is five bytes, which is exactly
>    `MAX_EDITOR_PENDING_PREFIX`, so the indicator bound is unchanged. See the
>    plan's §17.2.
> 4. **The Normal-mode chord count moves 69 → 70**, one chord for the `m`
>    arming arm. The final figure is still 71, recomputed once at the end.
> 5. **The byte-derived boundary is what landed, and it is the whole of this
>    milestone's scope.** The syntax-node object slice (`f`, `t`, `a`, `c`,
>    `T`, `x`) and a syntax-aware `m m` remain M014's, blocked on ADR-0034.

### M013 — LSP read delivery and editor reachability

Class: invariant (delivery) + enabling capability (reach)

Objective: make the LSP read surface the editor already calls actually deliver,
then extend it to the read-only capabilities `egglsp` implements but the editor
cannot invoke.

This milestone is a **corrective insertion**, not a successor. A survey at
baseline `84440fe7f` verified two defects that make the F2–F9 path return
`NotReady` in ordinary configurations:

1. **Diagnostics are never driven.** `publish_changed_diagnostics` and
   `DIAGNOSTICS_POLL_INTERVAL` (`src/core/daemon_lsp.rs`) have zero callers.
   The single production write to `daemon.lsp_diagnostics` lives inside that
   uncalled function, so the store stays empty and every diagnostics read is
   permanently `NotReady`. `NotReady` is explicitly not an empty clean set, so
   the UI cannot tell "not ready" from "broken".
2. **Warm-client resolution is ambiguous on polyglot roots.** The read path
   calls `find_existing_client_for_root_hint(Some(&root), None)`; the resolver
   returns `Err` when **more than one** client shares the root, and that `Err`
   is mapped flatly to `NotReady`. Any workspace with two language servers
   attached has F3–F9 silently dead while the same project with one server
   works.

Dependencies: M005 closed (hard, for the leaf table), M002 closed (hard).

Deliverable boundary: a daemon-owned diagnostics publisher cadence; an optional
`server_id` on the read request with explicit → extension-matched → unique →
**typed ambiguity** resolution; new read-only `LspReadOperation` variants for
implementation, declaration, document highlight, signature help, and
formatting/rename **preview**; Helix bindings (`space k`/`s`/`S`/`d`/`h`/`f`,
`gd`/`gy`/`gi`/`gr`, `]`/`[` diagnostic navigation) mapped onto them.

User value: the LSP features already documented as reachable become reachable in
practice, and the `NotReady` state stops being the permanent answer.

Exit conditions: a warm server returns `Ready` with contents; a two-server root
resolves or reports a distinct ambiguity; a cold root still returns `NotReady`
and starts no process; every LSP leaf degrades visibly; F2–F9 unchanged.

Non-goal: LSP **write** authorization. Rename/code-action apply and `=` format
need an accepted ADR; only preview is delivered here (see §10 risk 2).

Plan: `plans/implementation/tui-editor-helix-parity/013-lsp-read-delivery-and-reachability.md`

> **Amendment A1 (2026-10-19) — three further defects found while implementing.**
> The two defects above were found by inspection at baseline. These three were
> found by *running* the surface, and each one is a delivery failure of the
> same class: correct code that is never reached.
>
> 3. **The read surface is unreachable for any team principal.**
>    `LspReadGet` is `ViaSession + FileRead`, but the daemon's session-locator
>    match had **no `LspReadGet` arm** — only `LspCompletionGet` and
>    `LspPreviewApply`, under a comment saying the M006-B reads were
>    "deliberately untouched". With no locator, no project resolved, so every
>    read failed closed with `authorization_scope_required` *before* the
>    warm-client resolver ran. A socket-connected client is a team principal,
>    so the entire M006-B editor read surface was dead in production while
>    reading as "you may not read this". It stayed invisible because M006-B's
>    own tests called the daemon as LocalOwner, which bypasses scope
>    resolution. This is the same "delivered symbol with no driver" shape the
>    M013 plan was written to hunt, one layer above the two it recorded.
> 4. **"Warm-only" was a claim about two different servers.** The gate
>    resolved a client for the *session's workspace root*; the query then
>    discarded it and re-resolved through `get_or_create_client`, which derives
>    its own root from the target file and its own server id from the file's
>    language. The second resolution could disagree with the first and **could
>    start a language server**, which ADR-0012 §3 forbids. `workspace_symbols`
>    was worse: it took no path at all and routed to `first_client_key()`, so
>    its answer depended on hash-map iteration order rather than on the
>    request.
> 5. **Two smaller mis-reports.** Diagnostics were keyed by `file_name()`, so
>    `src/lib.rs` and `tests/lib.rs` were one tracker entry and the second
>    silently overwrote the first. And every point read mapped a *server*
>    error to `LspReadDenial::OutsideWorkspace`, telling a user their path was
>    unavailable when the server had simply failed; `ServerFailed` already
>    existed for exactly this and the M006-B path was not using it.
>
> Consequence for the deliverable boundary: the read path had to be made
> warm-only **structurally** (one resolved key, threaded through
> `*_for_warm_client` entry points) rather than by adding a check, and the
> `server_id` resolution order has to be applied to the *query*, not only to
> the gate.

> **Amendment A2 (2026-10-19).** M013 is implemented and closed
> (`plans/closure/tui-editor-helix-parity/013-status.md`). Amendment A1 stopped
> at "the read path had to be made warm-only *structurally*". Doing that turned
> out to be most of the work, and it moved this milestone's boundary in three
> ways a successor would otherwise re-derive:
>
> 1. **Structural warm-only is not a daemon change.** Threading one resolved key
>    from the gate into the query required key-addressed bodies and
>    `*_for_warm_client` entry points in **fourteen `egglsp` operations** plus the
>    service's own document-sync helpers (`ensure_file_open_from_disk`,
>    `open_file`, `update_file`, `set_managed_document`), each of which resolved
>    a client of its own. Probing — which §14 requires rather than reading
>    signatures — is what found this: `signature_help_typed`,
>    `format_preview_typed` and `rename_preview_typed` are real, working, and
>    **not** warm-only, three layers down from the operation you call.
> 2. **The keys are bound here, not deferred to M010.** This entry's deliverable
>    boundary already named them; the plan deferred them to M010 on the grounds
>    that binding before delivery "ships keys that open always-`NotReady`
>    panels". Once delivery was trustworthy, the remaining gap was the
>    `pending_leaf` hop between the prefix resolver (which holds
>    `&mut EditorState`) and the App (which holds the core client), and M010 is
>    better spent on the pickers and the clipboard family it uniquely owns.
>    M010's dependency line is updated accordingly.
> 3. **One unplanned read landed on the same seam.** `Unsupported` is a
>    *status*, not a denial: a warm server that never advertised
>    `renameProvider` has still answered, and mapping that to `ServerFailed`
>    tells a user their working server broke. It needed the same status path
>    the new capabilities required, so it came along rather than being
>    deferred to a later milestone.
>
> Unchanged, and still binding on whoever plans M010: `space r`, `space a` and
> `=` remain unbound, because applying an LSP-driven mutation needs an accepted
> ADR. The format and rename **previews** delivered here are the delivery; the
> apply is not.

### M010 — Space mode and extended goto mode

Class: capability

Objective: replace the single `space y` leaf with the picker and LSP surface
already present, and complete LSP-backed goto leaves.

Dependencies: M013 closed (hard, discharged 2026-10-19), M005 closed (hard),
M002 closed (hard).

Deliverable boundary: space-mode leaves for file/buffer/jumplist/changed-file
pickers, symbol and diagnostic pickers, global search, last-picker recall, and
the clipboard yank/paste family; `space c` comment toggle if a comment path
exists; and `space r` / `space a` / `=` **only if** an LSP write authorization
ADR has been accepted by then — M013 delivered their previews, never their
apply.

**Amended 2026-10-19 (M013 closure).** The original boundary also named the
LSP goto leaves (`gd`/`gy`/`gi`/`gr`) and the `space` LSP leaves
(`k`/`s`/`S`/`d`/`h`/`f`), and those are **delivered** — M013 bound them in the
M005 leaf tables once the read surface was trustworthy, rather than leaving them
for a successor. `gd`, `gi`, `gr`, `space k`, `space s`, `space S`, `space d`,
`space h` and `space f` are live; `gy` and `space F` are bound to a visible
statement of why they cannot run. What this milestone uniquely owns is
therefore the **pickers** and the clipboard family, not the LSP leaves.

User value: mouse-free access to everything the function keys already reach.

Exit conditions: leaves are consistent across both keymaps; LSP leaves degrade
visibly when no server is attached; no picker leaf steals a printable character
in Insert.

> **Amendment A1 (2026-10-19) — the `space f` binding is reclaimed, and the
> preview leaves move to a new milestone.**
>
> 1. **`space f` is bound to a duplicate of `space s`.** M013 bound it to
>    `DocumentSymbols` with the comment *"Helix's file picker. Multi-document
>    attachment is out of scope, so the closest honest reading is this
>    document's symbols."* That reasoning was correct **then** — the
>    workspace file browser (`Dialog::Files`) existed but was not reachable
>    from the editor route, because `/files` is untypeable there (the editor
>    claims `/` for its search line). The browser exists; the *route* was the
>    problem; this milestone is the route. **`space f` becomes the file
>    picker**, which is a **breaking change to a live M013 binding** and is
>    recorded in the `CHANGELOG.md` under `### Changed`.
> 2. **`space s` and `space S` are as the code has them.** The live Helix
>    keymap binds `space s` to the **document** symbol picker and `space S` to
>    the **workspace** symbol picker. The code is right; this entry's
>    amendment listed the two without saying which was which, and the
>    correction is recorded here so the next reader does not invert them.
> 3. **The preview leaves leave this milestone.** M013 delivered the format
>    and rename **previews** as protocol operations, daemon arms and panel
>    renderings — and **no key reaches either of them**. A preview is a
>    *read*, so it needs no LSP-write ADR; only the **apply** does. Binding
>    them here would contradict this entry's own A2 amendment, which says
>    this milestone owns "the **pickers** and the clipboard family, not the
>    LSP leaves". They therefore move to a new milestone, **M015**, which
>    binds `space r` (rename **preview**) and `=` (format **preview**), and
>    leaves `space a` unbound with a visible reason because no code-action
>    *read* exists to preview.
> 4. **Seven space leaves have no substrate here** — buffer picker,
>    jumplist picker, changed-file picker, global search, last-picker recall,
>    and the three comment leaves. They are bound to a **visible statement of
>    why they cannot run**, the M013 `space F` precedent, so that no space key
>    is silently dead. Two of them — the changed-file picker and global
>    search — have **no reachable data source at all** in the TUI and are
>    recorded as deferred product work.
> 5. **The pending-effect mechanism is generalized.** M013's
>    `EditorLspState::pending_leaf` is LSP-typed, and a file picker is not an
>    LSP fact. The one-hop mechanism (the resolver parks the effect; the App
>    drains it on the next statement) is preserved exactly; the effect's
>    *type* and *home* widen.
>
> Plan: `plans/implementation/tui-editor-helix-parity/010-space-mode-pickers-and-clipboard-family.md`

> **Amendment A5 (2026-10-19) — M010 is `closed`, and two of its boundaries
> moved.**
>
> 1. **M010 is `closed`.** Closure record:
>    `plans/closure/tui-editor-helix-parity/010-status.md`. The space menu
>    landed whole: the file picker (`space f`), the editor's own command
>    palette (`space ?`), the clipboard family (`space p`/`space P`/
>    `space R`/`space Y`), and the seven visible statements (`space F`,
>    `space b`, `space j`, `space D`, `space c`, `space C`, `space '`).
>    **No space key is silently dead**: every Helix space glyph is bound, bound
>    to a statement with its reason, or documented as out of scope with its
>    reason.
> 2. **The picker browses the *tab's* workspace, and the editor's part is to
>    check the binding first.** This entry's A1 amendment said the browser
>    would be "seeded from the **session's** workspace root". The document
>    attachment cannot supply one — `DocumentScope` carries a `workspace_id`
>    and a relative path, and **no root path** — and the daemon resolves the
>    listing from the **tab's** canonical project/workspace binding
>    (ADR-0022, C005), with the local root as fencing state only. So the
>    browser browses the active tab's workspace, and `space f` refuses
>    **before** opening when that binding is missing, rather than opening a
>    browser with nothing in it. The seeding decision moved into a new
>    `App::open_file_picker`, and `Dialog::Files` delegates to it, so `/files`
>    and `space f` cannot disagree — which also **fixed** `/files`, which used
>    to show a tree belonging to another workspace under this one's name.
> 3. **The seven statements are seven, not six-plus-one.** A1's point 4
>    listed "buffer picker, jumplist picker, changed-file picker, global
>    search, last-picker recall, and the three comment leaves" — and the
>    changed-file picker and global search have **no reachable data source at
>    all**. They are therefore **not bound to a statement**: a key that says
>    "there is no data source" is a sentence about a feature that was never
>    scoped, and the honest answer is that the chord does not exist. What
>    ships is a statement for each glyph that **is** a Helix chord this
>    editor recognises — `space F`, `space b`, `space j`, `space D`,
>    `space c`, `space C`, `space '` — and the two with no data source are
>    recorded here as deferred product work, not as keys.
> 4. **The leaf tables are now policed.** `scripts/check_tui_editor_leaf_authority.py`
>    is a **new** guard, wired into `verify.sh quick` and the CI `verify` job,
>    that fails on a duplicated glyph in one table, on a glyph that is not
>    printable ASCII, on one glyph claimed by two tables of the same prefix,
>    on a leaf behind a guard-block chord, and on a table no prefix binds. It
>    reads the guard-block chords through
>    `scripts/check_tui_editor_key_authority.py` rather than re-parsing the
>    dispatch chain. The plan's own rule 4 ("no leaf glyph is a guard-block
>    chord's **glyph**") was **wrong as written** — the guard block's glyphs
>    are `s` and `r`, and `space s` is a live leaf — and is implemented
>    instead as the **chord** a leaf can resolve to. The plan's rule 3 was
>    likewise implemented as the prefix's **union** across its tables.
> 5. **The M013 leaf test lost exactly one row, by design.** This roadmap
>    and the plan require the M013 leaf tests to pass **unmodified**, and
>    A1's point 1 reclaims `space f` from the document-symbol read. As
>    written the two conflict, because the `f` row of the M013 leaf test
>    asserted the old binding. The row was **removed** — `space f` is no
>    longer an LSP read at all — and the other thirteen M013 leaf tests and
>    the remaining rows are unmodified. The closure record says so in those
>    words rather than claiming fourteen.
> 6. **What M015 now owns is unchanged**: `space r` (rename **preview**),
>    `=` (format **preview**), and `space a`, which stays unbound with a
>    visible reason because no code-action *read* exists to preview. They
>    were separated rather than folded in because applying an LSP-driven
>    mutation has no accepted authorization ADR, and a **preview** does not
>    need one — so the two were kept apart from the apply. *(M015, 2026-10-19:
>    the ADR framing in this point was wrong — ADR-0008 is accepted — and the
>    separation still holds, because the gap is the bridge to a reviewable
>    artifact. See Amendment A2.)*
> 7. **The key-authority guard's chord counts are unchanged at 2 / 70 / 8.**
>    That is the point, and it is worth stating: the space leaves are not
>    Normal-mode or Insert-mode chords, so growing the space menu from eight
>    leaves to nineteen moved **no** chord count at all. The leaf table is
>    policed by its own guard, which is exactly why that guard was needed —
>    the key-authority guard scopes itself to the three dispatch stages and
>    never saw a leaf table.

### M011 — Insert-mode control chords

Class: capability

Objective: the missing insert-mode chords, excluding the two already owned by
the app.

Dependencies: M002 closed (soft). No hard dependency; may run in parallel.

Deliverable boundary: `Ctrl-x` completion (today F8/`:complete` only),
`Ctrl-r` register insert, `Ctrl-w`/`Alt-Backspace` delete-word-backward,
`Alt-d` delete-word-forward, `Ctrl-u` kill-to-line-start, `Ctrl-k`
kill-to-line-end, `Ctrl-s` undo checkpoint.

**Binding conflicts to resolve explicitly, not silently:** `Ctrl+s` (save) and
`Ctrl+r` (resync) are already mode-independent and currently intercept in
Insert. Helix's insert-mode meanings of those two chords cannot be adopted
without a documented decision about the app chords.

User value: keyboard-only line and word editing without leaving Insert.

Exit conditions: each chord is implemented or explicitly declined with a
recorded rationale; the mode-independent save/resync chords keep their current
Insert behavior unless the conflict decision says otherwise.

> **Amendment A1 (2026-10-19) — the conflict list was incomplete, and the
> decision is now made.**
>
> 1. **The roadmap named two conflicts; the code has four.** `Ctrl+w` is
>    bound **globally** to `CloseSession` (`src/tui/input.rs:816-819`, pinned
>    by `bare_ctrl_w_still_closes_the_session`), `Ctrl+k` to `ClearSession`
>    (`:749-752`), and `Ctrl+u`/`Ctrl+d` to `PageUp`/`PageDown` (`:883-886`,
>    `:879-882`) — the last two already shadowed in Normal mode by the
>    half-page motions. **`Ctrl+w` typed in the editor's Insert mode closes
>    the session today.** The editor route is consulted before the global
>    keymap, so these become **route-scoped** shadows rather than global
>    ones, and the plan records that as a deliberate divergence.
> 2. **The decision: `Ctrl+s` stays save and `Ctrl+r` stays resync, in every
>    mode.** Both are a documented user-facing promise
>    (`architecture/tui.md:3302-3303`, `docs/tui.md:728-731`) and are pinned
>    by three live tests. Re-scoping them to Normal-only would break that
>    promise to buy a chord whose capability is delivered better another way.
>    The rejected alternative is recorded in the plan.
> 3. **The checkpoint capability does not need the chord.** Helix's own
>    documentation says *"Changes to the text are only saved for undos when
>    escaping from insert mode to normal mode."* That is **insert-session
>    coalescing**, and it is strictly more useful than a manual checkpoint:
>    the reader gets one undo step for a typing burst without pressing
>    anything. `Ctrl+s` therefore does not become an undo checkpoint; the
>    capability is delivered by coalescing instead.
> 4. **`Ctrl-r` has no substrate.** There is **no register** of any kind:
>    `y`/`space y` both write the OS clipboard, `p`/`P` read it, and the
>    roadmap already defers named registers. "Insert a register's content"
>    has nothing to insert, so it is declined for that reason as well as the
>    chord one.
> 5. **Two panels are mode-blind, and the milestone fixes them.** The
>    completion popup claims `Enter`/`Up`/`Down` in **every** mode, so with
>    the popup open in Insert, `Enter` accepts a completion instead of
>    inserting a newline. The LSP diagnostics panel claims `j`/`k`/`r`/
>    `Enter` with no mode check, so those letters do not type in Insert.
>    Both break the documented promise that *"In `INS` a printable is
>    always text"*, and a `Ctrl-x` that opens a popup which then eats
>    `Enter` would be worse than no `Ctrl-x`.
> 6. **The key-authority guard must be taught.** It cannot parse an
>    Insert-mode `Char` chord as the table is written today (it exits 2
>    rather than passing), and it has no **Insert-only** class for guard
>    chords. The plan teaches the new arm shape, and requires the
>    non-vacuity proof by injection.
> 7. **M011 and M012 share the undo model.** Coalescing changes the
>    *granularity* of an entry; M012 changes the *traversal*. M011 should
>    land first, or M012 must tolerate any granularity. This is the one
>    sequencing constraint either milestone imposes on the other.
>
> **Amendment A2 (2026-10-19) — M011 landed; §13's chord count was wrong.**
>
> 1. **The Insert-mode chord count is 8, not 9.** §13's stage table said the
>    count goes `1 → 9`. It goes **`1 → 8`**, and the reason is arithmetic
>    rather than a defect: the key-authority guard counts *character* claims,
>    and of the nine chords two are **key-code** claims (`Alt-Backspace` and
>    `Alt-Delete`, which it checks under rule 3 for exactly-once ownership and
>    does not count as chords) and one (`Ctrl+x`) lives in
>    `handle_completion_key`, a different dispatch stage the guard does not
>    scan. With the wildcard character arm, `1 + 7 = 8`. **The Normal-mode
>    count is unchanged at 66**, and that is the number M012's acceptance
>    criterion pins.
> 2. **The plan's §6.3 prose contradicted its own table about `Tab`.** The
>    prose said the candidate is *accepted* with `Tab`; the table and
>    acceptance criterion 4 said `Tab` *moves the candidate selection*; and
>    Helix's completion menu binds `Tab` to *Next entry*. The table was
>    implemented and the prose corrected (plan §17.1).
> 3. **The insert table's plain keys now accept `NONE` and `SHIFT` only.**
>    `Ctrl+Backspace` and `Ctrl+Left` are no longer the editor's in Insert
>    mode; an unclaimed insert chord is a **global** action, which is the
>    plan's §3.3 finding and Helix's arrangement (plan §17.3).
> 4. **The coalesced run is one contiguous insertion.** A `TextTransaction`'s
>    edits are all expressed in the coordinates of one base text, so two
>    insertions at the same place cannot both be expressed in one
>    transaction. An edit joins the run only when it is a pure insertion at
>    the end of the run so far; a deletion, a replacement, or an insertion
>    elsewhere ends it. This kept the plan's stop condition from being
>    reached — `EditorUndoEntry`'s shape is unchanged (plan §17.5).
> 5. **Three divergences from Helix are recorded**, all the same decision: in
>    Insert mode a printable is text, and Helix's completion menu takes the
>    keypress. `Enter` is a newline, `↑`/`↓` move the cursor, and a printable
>    types instead of accepting the completion (plan §17.7).
> 6. **The `Ctrl+w` / `Ctrl+k` / `Ctrl+u` route-scoped shadow remains open for
>    the maintainer.** It is not a defect — it is Helix's arrangement, scoped
>    to Insert mode — but the plan flags it as a product decision, and
>    `Alt+Backspace` / `Alt+d` are offered as the alternative spelling.
>
> Plan: `plans/implementation/tui-editor-helix-parity/011-insert-mode-control-chords.md`
> Closure: `plans/closure/tui-editor-helix-parity/011-status.md`

### M012 — Undo history traversal

Class: capability

Objective: `Alt-u`/`Alt-U` `earlier`/`later` over a persistent history.

Dependencies: M002 closed (soft). No hard dependency.

Deliverable boundary: history with a time cursor rather than two stacks; each
entry retains both directions per the existing `EditorUndoEntry` contract; new
edits truncate the redo tail as they do today.

User value: undo across a long exploratory sequence without losing the redo
branch, which the current two-stack model cannot express.

Exit conditions: traversal is bounded and memory-capped; undo/redo round-trip
still exact; a new edit after traversal truncates correctly; the existing memory
bound on retained payload bytes is preserved.

> **Amendment A1 (2026-10-19) — the roadmap's pointer is wrong, and two
> decisions follow from the model change.**
>
> 1. **There is no `TextTransaction::invert()` method.** The roadmap points
>    at one "around `src/tui/editor.rs:1854-1871`". The real function is a
>    **free function**, `pub fn invert(snapshot: &DocumentSnapshot,
>    transaction: &TextTransaction) -> Option<TextTransaction>` at
>    `src/tui/editor.rs:3261-3278`, and it walks the edits in order,
>    tracking a `shift: isize` so each inverse edit's range is expressed in
>    post-transaction coordinates.
> 2. **The memory bound is currently applied twice, not once.**
>    `architecture/tui.md:3210-3212` says *"256 entries and 4 MiB **across
>    both directions**"*, but `trim_history` enforces the cap **per stack**,
>    so the worst case is twice the documented bound. A single history
>    restores the documented reading — and **tightens** the worst case. The
>    closure states the tightening rather than letting it pass as a
>    side-effect.
> 3. **`Alt-u`/`Alt-U` report the boundary; `u`/`U` stay silent.** In
>    Helix, `u`/`U` and `Alt-u`/`Alt-U` are the same movement — `undo`/
>    `redo` and `earlier`/`later` are two spellings of one history cursor.
>    This milestone does **not** claim a capability it did not deliver: what
>    it delivers is the model (one history, one cursor, an entry that never
>    moves between containers), the consolidated bound, and the
>    **observability** that makes a cursor worth having. The boundary
>    reporting is a CodeGG convention, recorded as one.
> 4. **`Alt-U` is a capital, and terminals disagree about how they encode
>    one.** Some send `Char('U')` with no modifier; others send `Char('u')`
>    plus `SHIFT`. M013 hit this exactly with `space S` and `space F` — a
>    key that is live on one terminal and dead on the next. The `Alt-U` arm
>    must accept **both** spellings, and a test drives both.
> 5. **`Alt-u` and `Alt-U` are claimed by nothing**, in the editor route or
>    anywhere else in `src/`. There is no `Alt-` prefix in this editor; the
>    M006 `Alt-.` repeat slots are explicitly not prefix state. The
>    milestone adds two arms and changes no prefix.
>
> Plan: `plans/implementation/tui-editor-helix-parity/012-undo-history-traversal.md`

> **Amendment A2 (2026-10-19) — the milestone landed; five corrections to
> this entry and to the plan's own figures.**
>
> 1. **The guard's Normal-mode count is 69, not 68.** The plan counted the
>    two *gestures* — `Alt-u` and `Alt-U` — and the guard counts *chords*.
>    The two terminal spellings of `Alt-U` differ in the **key code**, not
>    only in the modifier: a terminal that reports the shifted glyph sends
>    `(ALT, Char('U'))`, and one that reports the modifier sends
>    `(ALT | SHIFT, Char('U'))` or `(ALT | SHIFT, Char('u'))`. Two spellings
>    with different codes cannot share one arm, so the two gestures are
>    **three** arms and the count is `66 → 69`. The Insert-mode count is
>    unaffected at 8. The plan's §6.9, §13.9 and the stage table below are
>    corrected to 69, and the plan's §17.2 carries the reasoning.
> 2. **The key-authority guard's conflict model had to change.** The plan's
>    §6.9 said the guard "needs no change". It did: rule 2 keyed a claim by
>    its **modifier names**, so an arm accepting exactly `ALT` and one
>    accepting exactly `ALT | SHIFT` looked like a duplicate — both carry the
>    name `ALT`. Rust's `==` on the bitflags says they are disjoint, so the
>    old model would have reported a false positive and the source would have
>    had to be contorted to avoid it. The rule now compares **exact modifier
>    bitmasks**, and the guard learned to read a multi-modifier binding
>    (`let alt_shift = KeyModifiers::ALT | KeyModifiers::SHIFT;`). A tenth
>    self-test case pins the new model.
> 3. **A redo restores the same recorded position an undo restores.** The
>    two-stack model gave the redo stack its own copy of the entry's cursor
>    (`EditorUndoEntry::rebased`), so a redo landed where the reader was when
>    they pressed `u`. One history and one entry carry one recorded
>    position, so a redo restores `entry.cursor_before` — the same value the
>    undo restores — and `rebased` is deleted. The change is visible after
>    `u` then `U`, and it is *more* symmetric: both directions restore the
>    state the entry names.
> 4. **The dead `EditorCommand::Undo` / `Redo` variants were deleted, not
>    wired.** They were never constructed, and an undo cannot be built from a
>    snapshot the way `resolve_edit` builds an edit — the inverse of an edit
>    is a property of the *entry*, and the entry is not an `EditorEdit` — so
>    `PrefixAction::Edit` could not carry them either.
> 5. **The observable cursor is on the editor's banner row.** The plan
>    pointed at the fallback string in `src/tui/app/render.rs`, which is the
>    *no-snapshot* shell rather than the status surface a reader looks at.
>    The banner row — the one that carries the mode, the status word, the
>    find disclosure and the pending indicator — is where the position goes.
>    It reads `history <at>/<of>` while the cursor is not at the tip, and
>    nothing at all when it is, so the ordinary case is unchanged.
>
> The plan's §17 records each of these in full, with the reasoning.

### M014 — Syntax-tree text objects and structural selection

Class: capability

Objective: the syntax-node slice of M009's match-mode grammar — `m a` / `m i`
for Helix's tree-sitter-only text objects (`f` function, `t` type, `a`
argument/parameter, `c` comment, `T` test, `x` element), and a syntax-aware
`m m` in place of the delimiter-pair match M009 delivers.

Dependencies: **ADR-0034 accepted** (hard). M009 closed (hard).

Deliverable boundary: the syntax-node object kinds; a structural read
path that answers them; the `m m` upgrade.

User value: object selection that follows the language's structure rather
than a delimiter table.

Exit conditions: every object kind resolves against a real syntax tree; an
unavailable grammar is reported as such rather than silently falling back to
the byte-derived object; the byte-derived objects are unchanged.

**No implementation plan exists yet, deliberately.** ADR-0034 **recommends
Option B** (extend the daemon-owned, read-only LSP surface with structural
reads) but records that *"no option is selected by this record"*, and Option
A (a frontend tree-sitter dependency) and Option B produce **different
contracts** and would need different plans. Writing this plan now would be
writing a plan against a decision nobody has made. The plan is written
immediately after the ADR is accepted, against the option it selects.

### M015 — LSP preview leaves

Class: capability

Objective: give the two LSP previews M013 delivered a key.

M013 shipped `LspReadOperation::FormatPreview` and
`LspReadOperation::RenamePreview` as read-only operations, with daemon
dispatch, warm-client plumbing, protocol bounds and panel rendering — and
**no key reaches either of them**. They are capabilities a reader cannot
invoke.

Dependencies: M013 closed (hard). M010 closed (hard, for the generalized
pending-effect mechanism — but the milestone is written to work without it).

Deliverable boundary: `space r` — the rename **preview**, with the new name
collected through the `:` command line; `=` — the **format preview**;
`space a` — unbound with a visible reason, because no code-action *read*
exists to preview.

User value: a reader can see exactly what a rename or a format would change
before anything changes.

Exit conditions: both previews are reachable and show their result; a
preview **applies nothing**; a server without the capability answers
`Unsupported` and the panel names it; an empty or over-long new name issues
no request.

**The preview/apply split is the design.** Helix's `space r` renames and
Helix's `=` formats; CodeGG's versions **show what would change and apply
nothing**. A preview is a read, so it needs no LSP-write ADR — only the
apply does. When an LSP-write ADR is accepted, these two chords become the
apply step, and that **meaning change is registered now** so the later
milestone does not change a chord silently.

> **Amendment A2 (2026-10-19) — the ADR framing above was wrong, and the
> code said so.**
>
> 1. **ADR-0008 is accepted** (2026-09-20) and owns the LSP write path:
>    `CoreRequest::LspPreviewApply` is authorized `via_session` +
>    `file.modify`, and it applies a **registered, still-current reviewable
>    preview** by id through `src/lsp/mutation.rs::apply_preview()` — from
>    the TUI's change-review surface or from the model-facing preview-apply
>    tool. So "an LSP-write ADR" was never missing, and this entry's
>    sentence above is corrected accordingly.
> 2. **What the editor lacks is the *bridge*.** The editor's `=` and
>    `space r` produce a **read result** — no preview id, no patches, no
>    `register_preview_artifact` call anywhere in the editor path, and no
>    reference to `change_review` in `src/tui/commands/editor.rs` or
>    `src/tui/editor.rs`. The apply path consumes a registered preview
>    artifact, and the editor's preview is not one. So the chords cannot
>    reach it, and they apply nothing — an **implementation** gap, not a
>    decision gap.
> 3. **The transition is registered against the bridge, not the ADR.** When
>    the editor's preview becomes a reviewable artifact, `=` and `space r`
>    become the apply step. The registration happens now, while the preview
>    is what a reader learns, so the later change is a documented upgrade
>    rather than a surprise. `architecture/lsp.md`, `docs/LSP.md`,
>    `docs/tui.md`, `CHANGELOG.md` and the two skill files were corrected to
>    the same wording in M015's commit.

Plan: `plans/implementation/tui-editor-helix-parity/015-lsp-preview-leaves.md`

> **Amendment A1 (2026-10-19) — M015 is `closed`, and one of its three
> deliverables moved shape.**
>
> 1. **The milestone landed whole.** `=` shows the format preview, `space r`
>    collects a new name and shows the rename preview, and `space a` states
>    that no code-action request exists. The closure record is
>    `plans/closure/tui-editor-helix-parity/015-status.md`.
> 2. **`=` is a Normal-mode chord and is reported through `BufferKey`,
>    not through `pending_effect`.** The milestone's own boundary text said
>    `=` becomes a `PrefixAction::Effect(...)`, but `=` has no prefix and
>    `EditorState::pending_effect` is documented as *a **prefix leaf's**
>    effect*. The implementation uses the new
>    `BufferKey::LspRead(LspReadOperation)` arm — the buffer decides **what**
>    was asked for, the App layer does **how** — which is why the
>    key-authority guard attributes `=` as a Normal-mode chord and the count
>    moves **70 → 71**. §13.2's stage-E row predicted exactly that figure.
> 3. **`space r` is a leaf that names a *step*, not a read.** The leaf parks
>    `EditorPendingEffect::RenamePrompt`, which opens the `:` command line
>    pre-filled with `rename-preview `; the read is issued when the line is
>    submitted. A preview is still a read — it just happens after the reader
>    has supplied the name the request needs.
> 4. **The preview/apply transition is registered, in three places.** This
>    entry already said the meaning change would be registered when the write
>    ADR lands; it is registered **now** — in this entry, in the closure
>    record and in `CHANGELOG.md` — so the change arrives as a documented
>    upgrade rather than as a chord that silently means something else.
> 5. **The over-long-name refusal is real but unreachable through the `:`
>    line.** The command line is bounded at `MAX_EDITOR_COMMAND_LINE` (64)
>    characters, so a 201-character name cannot be typed into it. The check
>    is kept as defence in depth and is unit-tested directly rather than
>    through the prompt.
> 6. **The `:` palette grew by one command**, so the palette's row count is
>    **13**. `docs/tui.md` has no enumerated palette table — the palette is
>    generated from `EditorCommandName::ALL` — so the new command is
>    documented in the new "The preview keys" section instead.

## 8. Cross-cutting requirements

### Storage and migration

None. Selection set, prefix state, and history live in frontend presentation
state; no migration, no durable schema change.

### Protocol and compatibility

No protocol change in M001–M008 or M011–M012. M009's syntax slice and any
`=` format / `space r` rename / `space a` code action would require extending
`LspReadOperation`, which is read-only by construction — out of scope here and
gated behind a future protocol decision.

### Security and authorization

No relaxation. Paste and register insertion consume already-authorized
frontend content; shell filters (`|`, `!`, `$`) are explicitly deferred
precisely because they introduce process execution into the editor surface.

### Concurrency, cancellation, and recovery

The prefix state machine must disarm on pane focus change, route change, and
document open — a stale pending prefix must never apply to a newly opened
buffer. Multi-selection edits must land as **one** controller transaction so a
partial multi-cursor edit is not observable.

### Observability and audit

No new audit surface. Key handling is frontend-local.

### Performance and resource use

`f`/`F`/`t`/`T` must search a bounded window and report a visible truncation,
consistent with the existing bounded-search contract. Text objects must not scan
the whole buffer per keystroke. All of this stays inside the bounds already
established by the predecessor roadmap's viewport work.

### Documentation and static guards

`docs/tui.md` "Editor language features" is the user-facing keymap and must be
updated per milestone, not at roadmap close. `architecture/tui.md` records the
dispatch authority. The Insert-mode printable-character sweep and the
binding-reachability guard run in `verify.sh quick`.

## 9. Verification strategy

- **Focused:** in-module `#[cfg(test)]` coverage in `src/tui/commands/editor.rs`
  and `src/tui/editor.rs`; `cargo test --test tui_main_buffer`.
- **Dispatch-path, not helper-path:** every new binding is exercised through
  `App::on_key` or `editor_buffer_key`. Calling `editor_redo` directly, as the
  existing tests do, is what let the `Ctrl+r` shadow survive — that shape is
  prohibited for new binding tests.
- **Non-vacuity:** sweep tests must assert they would fail if a binding were
  wrong, following the existing
  `builtin_shift_letters_are_composed_text_in_insert_mode` precedent.
- **Static guards:** `cargo ck` plus `scripts/verify.sh quick`; any new guard
  script is added to `quick` and, where CI-appropriate, to
  `.github/workflows/ci.yml`.
- **Terminal-panel convergence:** after M002, M007, and M008, re-verify that
  `/terminal` selection and navigation still behave, or record the exemption.
- **Labels:** local runs are labeled local. Hosted CI evidence is claimed only
  when the closure record carries the run id.

## 10. Risks and decision points

1. **Syntax awareness (ADR-0034, proposed).** Whether to adopt tree-sitter, or
   a narrower alternative, gates the `TS`-marked slice of M006 and most of
   M009. Roughly twenty Helix bindings are unreachable without it. This is a
   durable dependency decision and requires an ADR, not a milestone choice.
2. **LSP write operations.** `=` format, `space r` rename apply, and `space a`
   code-action apply need an authorization contract beyond the read-only
   `LspReadOperation`. M013 delivers rename and format **preview** only
   (`egglsp`'s `preview_workspace_edit` / `preview_text_edits_for_file` already
   exist), so the binding can ship without a mutation path. The apply decision
   remains deferred product work, not an omission. **M015 (2026-10-19)
   corrected this entry's framing: the authorization contract is not missing —
   ADR-0008 is accepted and owns `CoreRequest::LspPreviewApply` (`via_session`
   + `file.modify`, applying a *registered, still-current reviewable preview*
   by id). What the editor lacks is the **bridge** from a rendered preview to
   such an artifact. M015 binds the two previews and still applies nothing.
3. **`Ctrl+s` / `Ctrl+r` in Insert.** M011 cannot adopt Helix's insert-mode
   meanings for these two chords while the app owns them mode-independently.
   Needs an explicit decision recorded in the M011 plan.
4. **Insert-mode text starvation.** Every new Normal-mode binding shrinks the
   set of keys that fall through to the composer in Normal mode, and every new
   global chord risks the capital-letter swallowing class of bug. The
   printable-character sweep is the mitigation and must not be treated as
   optional.
5. **Multi-selection blast radius.** M007 touches the selection model that the
   terminal panel also depends on. M002/M007/M008 each require explicit
   terminal-parity verification.
6. **Scope creep toward a second editor.** The milestone ladder is already at
   the edge of "implement Helix". M001–M006 and M011–M012 are cheap and
   high-value; M007–M009 are the expensive half. Sequencing is deliberate, and
   the deferred list in §3 is the pressure valve.

## 11. Completion definition

This roadmap closes when:

- Every milestone through M015 is closed with an individual closure record, or
  the roadmap is explicitly superseded by a successor that owns its remainder.
  **M014 is the exception, and it is a named one:** it is blocked on
  ADR-0034, and the completion definition below already says the
  ADR-0034-gated slices remain owned elsewhere. M014 closing is not a
  precondition for this roadmap closing.
- The normal-mode keymap, the `:` palette, the help overlay, and
  `docs/tui.md` describe one identical set of bindings, and that set is
  derived from the binding table rather than hand-maintained beside it.
- No declared binding is unreachable, and the reachability guard is in the
  routine verification set.
- The Insert-mode printable-character sweep passes non-vacuously across both
  keymaps.
- Terminal-panel selection/navigation parity is verified or an exemption is
  recorded with rationale.

Roadmap closure does **not** require full Helix parity. The deferred list in
§3 and the ADR-0034-gated slices remain explicitly owned elsewhere.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | **closed** | `plans/implementation/tui-editor-helix-parity/001-key-dispatch-authority-and-reachability.md` | `plans/closure/tui-editor-helix-parity/001-status.md` | — |
| M002 | **closed** | `plans/implementation/tui-editor-helix-parity/002-unified-prefix-state-machine.md` | `plans/closure/tui-editor-helix-parity/002-status.md` | — |
| M003 | **closed** | `plans/implementation/tui-editor-helix-parity/003-character-finding-motions.md` | `plans/closure/tui-editor-helix-parity/003-status.md` | — |
| M004 | **closed** | `plans/implementation/tui-editor-helix-parity/004-core-change-verbs.md` | `plans/closure/tui-editor-helix-parity/004-status.md` | — |
| M005 | **closed** | `plans/implementation/tui-editor-helix-parity/005-textual-motions-and-goto-mode.md` | `plans/closure/tui-editor-helix-parity/005-status.md` | — |
| M006 | **closed** | `plans/implementation/tui-editor-helix-parity/006-unimpaired-navigation-non-ts-subset.md` | `plans/closure/tui-editor-helix-parity/006-status.md` | — |
| M007 | **closed** | `plans/implementation/tui-editor-helix-parity/007-multi-selection-core.md` | `plans/closure/tui-editor-helix-parity/007-status.md` | — |
| M008 | **closed** | `plans/implementation/tui-editor-helix-parity/008-multi-selection-command-surface.md` | `plans/closure/tui-editor-helix-parity/008-status.md` | — |
| M009 | **closed** | `plans/implementation/tui-editor-helix-parity/009-match-mode-and-text-objects.md` | `plans/closure/tui-editor-helix-parity/009-status.md` | Byte-derived boundary only; the syntax-node slice moved to M014 (Amendment A3). Closed 2026-10-19 (Amendment A4): the `m` grammar landed whole — bracket match, three surrounds, five text objects — and the guard's Normal chord count is **70**, one for the `m` arming arm |
| M010 | **closed** | `plans/implementation/tui-editor-helix-parity/010-space-mode-pickers-and-clipboard-family.md` | `plans/closure/tui-editor-helix-parity/010-status.md` | M013 closed; M005 closed earlier — unblocked. Closed 2026-10-19 (Amendment A5): the space menu landed whole — the file picker (`space f`, reclaimed from a duplicate of `space s`), the editor's command palette (`space ?`), the clipboard family (`space p`/`space P`/`space R`/`space Y`) and seven visible statements — and a **new** leaf-authority guard polices the leaf tables |
| M011 | **closed** | `plans/implementation/tui-editor-helix-parity/011-insert-mode-control-chords.md` | `plans/closure/tui-editor-helix-parity/011-status.md` | M002 edge discharged; the `Ctrl+s`/`Ctrl+r` conflict is decided (Amendment A1). Closed 2026-10-19 (Amendment A2): the nine insert-mode chords are bound with one owner each, the two mode-blind panels are fixed, a typing burst is one undo step, and the guard's Insert chord count is **8** — not the 9 §13 originally recorded |
| M012 | **closed** | `plans/implementation/tui-editor-helix-parity/012-undo-history-traversal.md` | `plans/closure/tui-editor-helix-parity/012-status.md` | M002 edge discharged; sequenced after M011 (Amendment A1). Closed 2026-10-19 (Amendment A2): the history is one list with a cursor in it, `Alt-u`/`Alt-U` walk it and report their boundary, the position is observable on the banner row, the memory bound is one cap, and the guard's Normal chord count is **69** — not the 68 the plan recorded, because the two spellings of the capital differ in the key code |
| M013 | **closed** | `plans/implementation/tui-editor-helix-parity/013-lsp-read-delivery-and-reachability.md` | `plans/closure/tui-editor-helix-parity/013-status.md` | — |
| M014 | **blocked** | — | — | ADR-0034 acceptance; its plan is written after the ADR selects an option |
| M015 | **closed** | `plans/implementation/tui-editor-helix-parity/015-lsp-preview-leaves.md` | `plans/closure/tui-editor-helix-parity/015-status.md` | M013 closed; the previews are delivered and unreachable until this lands. Closed 2026-10-19 (Amendment A1): the two previews M013 shipped now have keys — `=` shows the format preview and `space r` collects a new name on the `:` line and shows the rename preview — both apply nothing, `space a` states that no code-action request exists, and the guard's Normal chord count is **71**, one for the `=` arm |

**M013 is closed** (`plans/closure/tui-editor-helix-parity/013-status.md`).
It discharges **M010**'s last dependency edge, and it found **six** further
"correct code never reached" defects beyond the two its plan recorded — all the
same class, and all invisible to the tests that covered them. The most serious
is that `CoreRequest::LspReadGet` had **no session-locator arm** at all: every
M006-B read failed `authorization_scope_required` *before* the resolver ran, so
the whole read surface was dead in production while its 14 tests passed, because
those tests called the daemon as `LocalOwner` and bypassed scope resolution. No
guard script inspects that match; a guard for locator-arm completeness is the
highest-value follow-up this milestone produced.

The deliverable boundary also moved during implementation, by **Amendment A2**
(below). M013 was planned as *delivery only* with the LSP leaves deferred to
M010; it could not close the delivery defects without making warm-only
structural, and that meant threading the resolved client key through eleven
`egglsp` operations and the service's document-sync helpers. Once that was
done, binding the keys was a smaller change than leaving them unbound behind a
surface that now answers. **`space r`, `space a` and `=` remain unbound** —
applying an LSP-driven mutation still has no accepted ADR, and no amount of
delivery work changes that. **M015 (2026-10-19) bound the two previews, and
corrected the reason:** ADR-0008 *is* accepted, and what the editor lacked was
the bridge to a reviewable artifact (Amendment A2).

**M003 is closed** (`plans/closure/tui-editor-helix-parity/003-status.md`). It
discharged exactly one dependency edge: **M004**, whose hard dependency was the
shared motion/verb resolution path.

**M004 is closed** (`plans/closure/tui-editor-helix-parity/004-status.md`). The
basic edit loop exists: `y` then `p` moves text, which was impossible before.
It delivered `p`/`P`, bare `y`, `r`, `R`, `~`, and `` ` ``/`` Alt-` `` — the last
two because *this* roadmap's deliverable boundary named them and Helix binds
them, where the implementation plan's §5 had not. `r` is the second **capture**
grammar after M003's find family, and the dispatch now resolves every capture
through one branch classified by `EditorPrefix::is_capture`.

One correction was made against the live Helix keymap: the implementation plan
had `R` as vi's "replace to end of line", entering Insert mode with a retained
tail. Helix binds it to `replace_with_yanked` — no argument, no Insert mode, no
retention — and **this** roadmap's deliverable boundary already said so. The
plan's entire retained-tail design was dropped (amendment A1).

**A user-visible binding moved as a result.** `R` was already reload, as a
Normal-only guard-block chord — the exact arrangement
`check_tui_editor_key_authority.py` rejects as shadowing a Normal-mode verb while
surviving into Insert. The chord is gone; reload survives as `:e`. There is also
**no local register**: `y`/`space y` both write the OS clipboard, so `p` pastes
whatever is on it, and that is precisely why Helix's `R` is
`replace_with_yanked`. M004 discharges no dependency edge of its own.

**M005 is closed** (`plans/closure/tui-editor-helix-parity/005-status.md`). It
delivered `gs`, `W`/`B`/`E` and `gj`/`gk`, and discharged its own edges — M002
and M003. It also supplied the per-prefix **leaf table** that M010 and M013
build on, replacing M002's single-glyph `EditorPrefix::leaf`, so **M010's M005
edge is now discharged**. Two corrections were made against the live Helix
keymap during implementation and recorded in the closure: `gl` is
`goto_line_end` (not "last goto"), and `gt`/`gc`/`gb` are **screen**-relative
and are therefore deliberately unbound, because motions resolve against a fixed
`NOMINAL_VISIBLE_LINES` guess rather than the real viewport.

**M010's block is discharged.** It was blocked, at one point, on **M013
alone**: a survey at baseline `84440fe7f` found that the LSP read surface the
F2–F9 keys already call cannot deliver — the diagnostics publisher has no
production driver, and a workspace with two warm servers resolves to a false
`NotReady`. Those defects were owned by **M013**, which is now **closed**, so
M010 is `ready` with a plan.

M013 is numbered as a corrective insertion rather than continuing the sequence,
so it sorts after the closed 001–003 without implying a dependency on the
un-planned M004–M012. It depended on M005 only for the leaf table M005 W1
introduces, and **that edge is now discharged**; M013's remaining work is
independent of M005.

M006, M007 and M008 now have implementation plans, written 2026-10-18 at
baseline `84440fe7f`. **All three are now `closed`** — M007 and M008 on
2026-10-19, and M006 on 2026-10-19 after Amendment A2. (M008 was registered
**blocked on M007**, because a milestone is dependency-ready only when every hard
dependency is closed; that edge was discharged by M007's closure.) The three
plans displaced three rows of this roadmap's status table and produced **nine
corrections against this roadmap's own prose**, each recorded as an amendment on
the milestone entry rather than absorbed.

The three corrections worth carrying forward:

- **M006's boundary was claiming bindings twice and described two of them
  backwards.** `]D`/`[D` and `]G`/`[G` are inverted here — `]` is *forward*
  throughout the layer, so `]D` is **last** and `[D` is **first**. The
  diagnostic quartet belongs to **M013**, which already names "`]`/`[` diagnostic
  navigation" in its own boundary *and* owns the pipeline repair that
  `]d`-would-need. The change quartet has **no reachable data source** — the
  claim that it reuses "the existing diff surface" is false, because
  `egggit`'s diff types carry no line ranges and the only parsed hunk type lives
  behind a private widget method. It needs a milestone of its own.
- **M007's entry named `D` as selection-dependent.** It is not: `resolve_edit`
  never calls `selection_range`, and `D` resolves against `cursor_byte` and
  `line` only. Making it selection-aware would be a new semantic on a
  deliberately local extension. M007 also cannot claim "N cursors deletable in one
  gesture" — no verb *creates* N cursors until M008.
- **M008's eighteen chords are all unclaimed and its regex verbs need no new
  dependency.** `regex = "1"` is already in the root package and already used
  under `src/tui/`. The real cost there is the argument prompt, not the matching.

**Every remaining milestone now has a plan, and the status table matches.**
M009, M010, M011 and M012 were planned on 2026-10-19 against the live Helix
keymap and the tree at `a8778e87c`, and M015 was planned on the same tree once
M013 closed. **All five are now `closed`** — M009, M010, M011, M012 and M015 all
on 2026-10-19 — each with a plan path and a closure record in §12. Two
milestones were **added** by that planning, both from gaps the plans
exposed rather than from preference:

- **M014** carries the syntax-node slice of M009's match-mode grammar.
  ADR-0034's own decision text says the word and delimiter-pair objects are
  *"explicitly **not** gated"* on the ADR, and Helix's text-object table
  agrees, so M009's byte-derived boundary is not blocked and the blocked
  remainder needed its own entry rather than staying an orphan. **M014 has
  no plan, deliberately**: ADR-0034 records that *"no option is selected by
  this record"*, and Option A (a frontend tree-sitter dependency) and
  Option B (daemon-owned structural reads) produce different contracts and
  would need different plans.
- **M015** gives the two LSP previews M013 delivered a key. M013 shipped
  `FormatPreview` and `RenamePreview` with daemon arms, protocol bounds and
  panel renderings — and **no key reaches either**. A preview is a *read*,
  and it needs no LSP-write ADR — the **apply** does, and ADR-0008 already
  owns that path (see Amendment A2 and §13.4 question 7). M010's A2 amendment
  reserves M010 for "the **pickers** and the clipboard family, not the LSP
  leaves". **M015 is now closed** (Amendment A1): `=` and `space r` reach the
  two previews, both apply nothing, and the preview/apply transition is
  registered while the preview is what a reader learns.

Nine corrections were produced against this roadmap's own prose by the
four plans, each recorded as an amendment on the milestone entry rather
than absorbed: M009's ADR gate is narrower than stated; Helix marks `m m` as
`TS`; M010's `space f` is bound to a duplicate of `space s` and is reclaimed
as the file picker; M010's `space s`/`space S` mapping is as the code has
it; M011's conflict list omitted three global chords; M011's `Ctrl+s`/
`Ctrl+r` decision is made; M012's `TextTransaction::invert()` pointer names
a method that does not exist; M012's memory bound is applied twice rather
than once; and `Alt-U` must accept both terminal spellings of a capital.

See `plans/registry.md` § "Milestones eligible for fresh planning".

One composition note for M006: the roadmap's M003 entry lists `Alt`-`.` as
repeating "the last `f`/`t`/`m`/`[`/`]` motion". **M006 discharged this.** The
`m` and `[`/`]` leaves now exist, so the repeatable set is the find family, the
selection verbs and the unimpaired **motions** — and specifically *not*
`]Space`/`[Space`, because `Alt-.` repeats motions and an edit is not one. That
is the intended composition, and the asymmetry is now stated rather than left to
be discovered. Recording the `Alt-.` slot defect A2.4 describes is what proved
the three slots are mutually exclusive rather than merely consulted in order.

M001's closure is discharged — its chosen resolution was shape (b), redo on `U`
(Helix's own chord), with `Ctrl+r` keeping exactly one owner as resync in both
modes, so no chord was invented and neither `RequestSave` nor `RequestResync`
lost a path. See `plans/closure/tui-editor-helix-parity/001-status.md`.

Status labels here, in the plans, and in the registry must agree. A divergence
between them is a planning defect, not a formatting detail.

## 13. Implementation order for the planned milestones (2026-10-19)

The five plans in `plans/implementation/tui-editor-helix-parity/` share three
core files, one guard script, and one set of counters. They cannot be
implemented as five independent efforts. This section is the **total order**
that a single core owner follows, and it is derived from a source-verified
audit of all five plans against the tree at `a8778e87c` with M013 applied.

**W1 of M009, M010 and M011 has already landed** (2026-10-19) and is excluded
from the order below. Those three packages were file-disjoint and ran
concurrently: M009's byte foundation in `src/tui/editor_match.rs`, M010's
pending-effect generalization in `src/tui/editor_effect.rs`, and M011's
key-authority guard teaching.

### 13.1 The mandatory constraints

| # | Constraint | Source |
|---|---|---|
| C1 | M011's insert-session coalescing must land **before or with** M012's traversal work, or M012 must be re-derived | M011 §6.2; M012 §5; Amendment A1 |
| C2 | M012's chords must land while the Normal-chord base is still 66, because M012's acceptance criterion 9 pins the guard's report to **69** | M012 §13.9; Amendment A2 |
| C3 | M010 must land before M015, because M015's acceptance criterion 8 requires `space r` and `space R` to be **distinct and both reachable** — `space R` is M010's binding | M015 §13.8 |
| C4 | M011's mode-aware panels must land before its chords — a `Ctrl-x` popup that then eats `Enter` is worse than no `Ctrl-x` | M011 §7 W2 |
| C5 | M012's chords must land after M012's model; M009's grammar before its resolvers; M010's W2 before W7; M015's W1 before W4 | each plan's §7 |

**C2 is the one constraint that moves a milestone out of roadmap order.**
`m` (M009) and `=` (M015) each add one Normal-mode chord; `Alt-u` and
`Alt-U` add **three** — the two terminal spellings of the capital differ in
the key code, so they need two arms, and `Alt-u` is the third. If M009 landed
first, M012's report would be 70 and its own
acceptance criterion would fail. The roadmap does not order M009 against
M011/M012 — they sit in different branches of the dependency graph — so
placing M012 first violates nothing and preserves M012's hard number. The
cost is one sentence: **M009's and M015's count statements are deltas, not
absolutes**, and only the final figure is recorded once, at the end.

### 13.2 The order

| Stage | Work packages | Normal chords | Insert chords |
|---|---|---|---|
| A | **M011** W1–W6: the guard (landed), the mode-aware panels, the deletion builders, the nine Insert arms, the coalescing, the docs | 66 (unchanged — M011 adds no Normal-mode chord) | 1 → **8** |
| B | **M012** W1–W6: the cursor model, the apply side, the `Alt-u`/`Alt-U` arms, the boundary report and the observable cursor, the dead variants, the docs | 66 → **69** | 8 |
| C | **M009** W2–W6: the grammar, the pair matching, the text objects, the surround verbs, the guards and the docs | 69 → **70** | 8 |
| D | **M010** W2–W7: the file picker, the clipboard family, the command palette, the visible statements, the leaf-table guard, the docs | 70 | 8 |
| E | **M015** W1–W4: the format preview, the rename prompt, the `space a` statement, the docs | 70 → **71** | 8 |

**Final figure: `2 guard-block chord(s), 71 Normal-mode chord(s), 8
Insert-mode chord(s)`.** The count is recomputed **once**, at the end.

> **Correction (2026-10-19, stage A landed).** This table originally said the
> Insert chord count goes `1 → 9`. It goes **`1 → 8`**, and the reason is
> arithmetic rather than a defect. The key-authority guard counts *character*
> claims; of M011's nine chords, two are **key-code** claims (`Alt-Backspace` and
> `Alt-Delete`, which the guard checks under rule 3 for exactly-once ownership
> and does not count as chords) and one (`Ctrl+x`) lives in
> `handle_completion_key`, a different dispatch stage the guard does not scan.
> With the wildcard character arm, `1 + 7 = 8`. The **Normal-mode** count is
> unaffected, and it is the one M012's acceptance criterion pins. See M011's
> Amendment A2.

### 13.3 What must not be broken by the order

- **The `all_eighteen_chords_are_claimed_in_normal_mode` list is M008's
  eighteen verbs across twenty-two spellings** — it is not "every Normal
  chord". M009's `m` and M015's `=` must **not** be added to it, or M012's
  `claimed.len() == 24` breaks.
- **The insert-mode sweep stays at 93.** All five plans reaffirm it; only a
  new *claim* moves it.
- **One history, one bound.** M011's coalescing changes the *granularity* of
  an undo entry; M012 changes the *traversal*. M012 must assume the history
  holds **coalesced** entries, that the redo tail is truncated **once** per
  session, that a coalesced session counts as **one** entry with the
  **summed** bytes, and that an **open** session's bytes are not yet in the
  history — M012 must either add them to the bound or record that M011 bounds
  them separately.
- **The doc writes are serialized** — one per plan, in its own package's
  last step — and a skill file and its `architecture/` doc move **in the same
  commit**.

### 13.4 The open questions this order exposes

1. ~~**Where does the insert session's state live?**~~ **Answered by M011
   (2026-10-19).** On the presentation, beside `pending_prefix`, as
   `TuiDocumentPresentation::insert_session` — so `clear_history` discards it
   in the same place that clears the stacks, and there is no second discard
   path to forget. The field name does not contain `prefix`, so the
   prefix-state guard's one-field rule does not fire. On `EditorState` it
   would have been wrong: `EditorState` is not where the history lives, and
   a session that had to be discarded from two places would eventually be
   discarded from one.
2. ~~**M009's `PrefixAction` payload: option (a) or (b)?**~~ **Answered by
   M009 (2026-10-19), by a third answer.** Option (a) drops `Copy` from
   `PrefixLeaf` and constrains every future leaf to a const-constructible
   payload; option (b) costs one mapping arm and keeps the M005-frozen
   `leaves()` interface. Neither was taken: the leaf carries
   `PrefixAction::Capture(MatchArg)`, whose payload is a `Copy` enum, and
   the verb is produced when the **capture** resolves. `Copy` was never
   lost, the leaf tables stayed `const`, and §14's stop condition never
   fired. See Amendment A4 and the plan's §17.1.
3. ~~**Is the count test 24 or 25?**~~ **Answered by M012 (2026-10-19).** It
   is **24**, with `Alt-U`'s second spelling exercised by a **separate** test
   — `alt_u_accepts_both_terminal_spellings_of_the_capital` — so the number
   is not 24 by accident. The list holds the two *gestures* (`Alt-u` and
   `Alt-U` in the `Char('U')` spelling), and the `Char('u')` + `ALT | SHIFT`
   spelling is driven by its own test because it is a different chord, not a
   different spelling of the same chord. The guard's own count is **69**,
   not 68: it counts *chords* (arms), and the two spellings of the capital
   differ in the key code, so they need two arms. See Amendment A2.
4. ~~**Where does M009's `m`-prefix indicator render, and what does
   `MatchArg` display?**~~ **Answered by M009 (2026-10-19).** The indicator
   renders on the presentation's own banner row, beside the history cursor
   and the pending find. `MatchArg::SurroundReplaceFrom` and
   `::SurroundReplaceTo(ch)` are told apart by the **character already
   captured**: the second half renders `"m r ("`, which required
   `EditorPrefix::display()` to return a `Cow<'static, str>` rather than a
   `&'static str`. See Amendment A4 and the plan's §17.2.
5. **Does M011's `Ctrl-x` need a guard?** `handle_completion_key` is outside
   the key-authority guard's scope, so the chord is invisible to it — the
   same blind spot the plans note for the leaf tables.
6. ~~**Which invariants does M010's new leaf-table guard police?**~~ **Answered
   by M010 (2026-10-19).** Five, and the plan's own rule 4 was **wrong as
   written** — it forbade a leaf glyph equal to a guard-block chord's
   *glyph*, and the guard block's chords are `Ctrl+s` and `Ctrl+r`, so their
   glyphs are `s` and `r`: the rule would have forbidden `space s`, which is
   a live leaf. It is implemented as the **chord** a leaf can resolve to
   (`(NONE, glyph)`, and for a capital `(SHIFT, lower-case glyph)`), so the
   guard fails only when one of those chords is a guard-block chord. The
   invariants are: **1** no duplicated glyph inside one table; **2** every
   glyph is printable ASCII; **3** one glyph per prefix across that prefix's
   tables (the *union*, not a second within-table check — the plan's own
   parenthetical contradicted its heading); **4** no leaf behind a
   guard-block chord; **5** no leaf table that no prefix binds (added — a
   grammar written and never wired is how a leaf ends up "documented" and
   absent). The guard is a **new** script,
   `scripts/check_tui_editor_leaf_authority.py`, and it reads the guard-block
   chords **through** `scripts/check_tui_editor_key_authority.py` rather than
   re-parsing the dispatch chain. See Amendment A5 and the plan's §17.7–§17.10.
7. ~~**Is the LSP apply waiting on an ADR?**~~ **Answered by M015
   (2026-10-19): no — the ADR is there, the *bridge* is not.** This roadmap,
   the M015 plan and `architecture/lsp.md` all said the apply needs "an
   accepted LSP-write ADR, which does not exist". **ADR-0008 is accepted**
   (2026-09-20) and owns that path: `CoreRequest::LspPreviewApply`, authorized
   `via_session` + `file.modify`, applying a **registered, still-current
   reviewable preview** by id through `src/lsp/mutation.rs::apply_preview()`,
   reachable from the change-review surface and from the model-facing
   preview-apply tool. The editor's `=` and `space r` produce a **read
   result** — no preview id, no patches, no `register_preview_artifact` in the
   editor path — so they cannot reach it. The open work is therefore an
   **implementation** task, not a decision. Every surface the M015 commit
   touched was corrected to this wording. See Amendment A2 and the plan's
   §17.15.

### 13.5 The pre-existing failures that are not this work's

The 69 failing `tests/` integration binaries, the two load-sensitive
`interactive_terminal` library failures, and the `workspace-authority-and-
session-navigation` residuals are **registered, not discharged here**. A
closure record states the **new** failure set, not the total: if the failing
count is not exactly 69 with the same names, that is a regression of this
work and must be fixed before closure.
