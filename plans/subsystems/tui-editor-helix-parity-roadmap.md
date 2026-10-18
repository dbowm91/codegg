# TUI Editor Helix Keybinding Parity Roadmap

Status: active (M001, M002, M003 closed; M004/M005 eligible for fresh planning; ADR-0034 proposed and gating M009)

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
  `LspReadOperation` is read-only by construction; `=` format, `space r` rename,
  and `space a` code action are out of scope until a protocol decision exists
  (see §10).

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
  |        |                 |
  |        |                 +--> M010 space mode + extended goto
  |        |
  |        +--> M006 unimpaired [ ] navigation (non-TS subset)
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
  M005 require M003. M008 requires M007. M009 requires M008 **and** ADR-0034
  for its syntax-node slice. All of M002's, M003's, M004's and M005's
  predecessors are now closed.
- **Interface:** M002's prefix contract is an interface dependency for every
  later milestone — changing it after they land is a breaking refactor. M003
  extended it with prefixes whose next key is *data* rather than a leaf glyph,
  which is the one shape change later milestones must build against.
- **Soft:** M004–M006, M011, M012 are mutually independent once M002 closes.
- **Operational:** terminal-panel convergence must be re-verified after M002,
  M007, and M008, because the panel shares the selection dialect.

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

Deliverable boundary: `p`/`P` paste (after/before) from the editor register;
bare `y` yank; `r` replace-with-character and `R` replace-with-yanked; `~` case
switch, backtick lowercase, `Alt-backtick` uppercase, applied to the held
selection.

User value: a complete basic edit loop without leaving Normal mode.

Exit conditions: paste round-trips with yank; `r` is refused on multi-byte
characters rather than corrupting them; case verbs operate on the whole held
selection; documented interaction with the local `D` extension is unchanged.

Deferred: `c`/`Alt-c` change, noyank variants, `>`/`<` indent, `=` format.

### M005 — Textual motions and goto mode

Class: capability

Objective: `gs` smart-home (today there is **no** way to reach first
non-whitespace), long-word `W`/`B`/`E`, textual `gj`/`gk`, and the remaining
goto-mode leaves that need no new infrastructure.

Dependencies: M003 closed (soft), M002 closed (hard — goto mode is a prefix).

Deliverable boundary: `gs`; `W`/`B`/`E`; `gj`/`gk`; goto leaves `ge`, `gt`, `gc`,
`gb`, `g.`, and `gf`.

User value: removes the single most surprising omission (no smart-home) and
completes the parts of goto mode that are pure navigation.

Exit conditions: `gs` round-trips with `0` on blank and indented lines; long-word
motions treat whitespace-separated runs as one unit; goto leaves are reachable
and documented.

Deferred: LSP-backed goto leaves (M010), `gw` label navigation, buffer/alternate
file leaves pending multi-document attachment (predecessor non-goal).

### M006 — Unimpaired navigation, non-TS subset

Class: capability

Objective: the `[`/`]` navigation layer for every entry that needs neither
tree-sitter nor an LSP write.

Dependencies: M002 closed (hard).

Deliverable boundary: `]d`/`[d` jump to next/previous diagnostic and `]D`/`[D`
first/last (F2 currently opens a panel rather than navigating); `]p`/`[p`
paragraph; `]g`/`[g` change and `]G`/`[G` first/last change, reusing the
existing diff surface; `]Space`/`[Space` add newline below/above.

User value: diagnostics become navigable rather than panel-only, and paragraph/
change jumps cover long-file traversal.

Exit conditions: every listed binding navigates or edits as documented; `[`/`]`
as literal characters remain typable in Insert; unimpaired bindings do not
shadow the existing `:` and `/` lines.

Deferred: `]f`/`[f`, `]t`/`[t`, `]a`/`[a`, `]c`/`[c`, `]T`/`[T` — ADR-0034.

### M007 — Multi-selection core

Class: capability (enabling)

Objective: replace the single held selection with a selection set and a
distinguished primary, with every existing command applying to all members.

Dependencies: M001 closed (hard). Independent of M002.

Deliverable boundary: `SelectionState` holds a set; motions transform the whole
set preserving order; `v` extends all members; line-selection growth applies
per member; the selection-dependent effects (`d`, `space y`, `D`) emit one
range set; rendering shows every head.

User value: the enabling change for the entire Helix selection vocabulary, and
the first genuinely multi-cursor editing capability in CodeGG.

Exit conditions: N cursors selectable and deletable in one gesture; undo/redo
round-trips a multi-selection edit; read-only buffers refuse selection growth;
terminal-panel parity is re-verified or an exemption is recorded.

Deferred: the manipulation verbs themselves (M008).

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

### M010 — Space mode and extended goto mode

Class: capability

Objective: replace the single `space y` leaf with the picker and LSP surface
already present, and complete LSP-backed goto leaves.

Dependencies: M005 closed (soft), M002 closed (hard).

Deliverable boundary: space-mode leaves for file/buffer/jumplist/changed-file
pickers, symbol and diagnostic pickers, global search, last-picker recall, and
the clipboard yank/paste family; goto leaves `gd`/`gy`/`gi`/`gr` mapped onto
the existing F-key LSP reads; `space c` comment toggle if a comment path exists.

User value: mouse-free access to everything the function keys already reach.

Exit conditions: leaves are consistent across both keymaps; LSP leaves degrade
visibly when no server is attached; no picker leaf steals a printable character
in Insert.

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
2. **LSP write operations.** `=` format, `space r` rename, and `space a` code
   action need a protocol extension beyond the read-only `LspReadOperation`.
   Deferred product work, not an omission.
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

- Every milestone through M012 is closed with an individual closure record, or
  the roadmap is explicitly superseded by a successor that owns its remainder.
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
| M004 | eligible for fresh planning | — | — | — |
| M005 | eligible for fresh planning | — | — | — |
| M006 | eligible for fresh planning (M002 edge discharged) | — | — | — |
| M007 | eligible for fresh planning | — | — | M001 |
| M008 | eligible for fresh planning | — | — | M007 |
| M009 | blocked | — | — | M008, ADR-0034 acceptance |
| M010 | eligible for fresh planning | — | — | M005 |
| M011 | eligible for fresh planning (M002 edge discharged) | — | — | — |
| M012 | eligible for fresh planning (M002 edge discharged) | — | — | — |

**M003 is closed** (`plans/closure/tui-editor-helix-parity/003-status.md`). It
discharged exactly one dependency edge: **M004**, whose hard dependency was the
shared motion/verb resolution path. **M005** is now dependency-clean as well —
its edges were M002 (closed) and M003 (now closed). M010 is unchanged and still
waits on M005, which the roadmap's own graph records as its predecessor; the
plan's §15 prediction that M004–M005 and M010 would all become ready together
did not match the graph, and the closure records the audit rather than the
prediction.

M006, M011 and M012 were already "M002 edge discharged" and are unaffected.
M007 remains independently available, M008 waits on M007, and M009 waits on M008
plus ADR-0034 acceptance. M004–M012 need a fresh implementation plan at their
turn; see `plans/registry.md` § "Milestones eligible for fresh planning".

One composition note for M006: the roadmap's M003 entry lists `Alt`-`.` as
repeating "the last `f`/`t`/`m`/`[`/`]` motion". The `m` and `[`/`]` leaves do
not exist until M006, so the repeatable set is the find family only today and
M006 widens it. That is the intended composition.

M001's closure is discharged — its chosen resolution was shape (b), redo on `U`
(Helix's own chord), with `Ctrl+r` keeping exactly one owner as resync in both
modes, so no chord was invented and neither `RequestSave` nor `RequestResync`
lost a path. See `plans/closure/tui-editor-helix-parity/001-status.md`.

Status labels here, in the plans, and in the registry must agree. A divergence
between them is a planning defect, not a formatting detail.
