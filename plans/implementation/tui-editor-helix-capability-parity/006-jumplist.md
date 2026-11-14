# TUI Editor Helix Capability Parity Milestone 006 — Jumplist

Status: conditionally closed
Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-006--jumplist`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

Give the editor a bounded history of visited positions and a `space j` picker
that returns to one, replacing the "this editor keeps no visited-position
history" statement — and decide, explicitly, what to do about Helix's `Ctrl-i` /
`Ctrl-o` stepping pair, which this TUI cannot bind as Helix spells it.

## 2. Why this milestone is ready

No hard dependency. The mechanisms exist:

- `TuiDocumentPresentation` already holds bounded, lifecycle-cleared
  presentation state with an established clearing contract (the undo history and
  its cursor are the precedent).
- The editor already knows the positions worth recording: `gd`/`gi`/`gr` LSP
  jumps, `gg`/`G`/`:goto`, `/`/`n`/`N` search adoption, diagnostic stepping, and
  symbol-picker selection all land the cursor through `move_cursor_to` or an
  explicit jump.
- The picker surface exists and the space menu already routes to it (`space f` for
  files, `space s`/`space S` for symbols), so `space j` is a leaf whose action is
  a picker rather than a new surface.
- The leaf authority guard polices the six tables, and the space table already
  carries the `Unavailable` row this milestone removes.

The interface dependency is on roadmap M003 (soft): search jumps should push
entries, so landing M003 first is cheaper. M006 can land first and treat search
jumps as a follow-up, but the plan must then say so.

## 3. Current implementation evidence

At the baseline:

- `SPACE_LEAVES` contains `space j` as an `EditorPendingEffect::Unavailable` row
  with the message "jumplist: this editor keeps no visited-position history".
- `docs/tui.md` documents that row and the doc-drift guard checks the document's
  declarations in both directions.
- **`Ctrl-i` is not bindable as Helix spells it.** It is byte `0x09`; this TUI
  never pushes keyboard-enhancement flags, so the terminal delivers it as
  `KeyCode::Tab` with `KeyModifiers::NONE`. `Tab` is bound globally to
  `InputAction::ToggleComposerMode`, and `Shift+Tab` to
  `InputAction::TogglePermissionMode`. A `(CONTROL, Char('i'))` arm would
  therefore be **dead** — the exact defect class the key authority guard exists
  to catch.
- **`Ctrl-o` is bound globally** to `InputAction::OpenWorkspaceDashboard`.
  `Shift+W` is a second spelling of the same action.
- `Ctrl-b` and `Ctrl-f` are also globally bound (`ToggleSidebar`, `Search`), and
  `Ctrl-a` to `SwitchAgent`, so the neighbouring control letters are not free
  either.
- The key authority guard reports **2 / 74 / 8** with no shadowed arm.

## 4. Invariants that must not regress

- **Positions, never text.** An entry is a document identity plus a byte offset.
- **Bounded and disclosed.** The stack has a named cap and reaching it is
  reported, not silently dropping the oldest entry.
- **Cleared with the rest of the presentation** on open, reload, resync,
  reconnect and close — a jumplist entry into a document whose text has been
  replaced is a cursor into nowhere.
- **An entry that cannot be honoured is refused, not approximated.** A reloaded
  document whose length shrank may not contain the offset; the jump clamps and
  says so, or refuses and says so. It must not silently land elsewhere.
- **No dead chord.** A binding that cannot fire on this TUI is not added. This is
  the invariant the `Ctrl-i` finding is about.
- **No second text buffer**, no mutation outside `apply_local`.
- **The document and the code agree** in both directions, in the same change.

## 5. Scope

### In scope

- A bounded jumplist store on the presentation: `(document identity, byte
  offset)`, pushed on deliberate jumps, with a cursor for stepping if D1 resolves
  to "bind stepping".
- `space j` opens a picker over the list; selecting an entry jumps to it.
- Pushing on the jumps that exist today: `gd`, `gi`, `gr`, `gg`, `G`, `:goto`,
  diagnostic stepping, and symbol-picker selection. Search jumps (`/`, `n`, `N`)
  are pushed when roadmap M003 has landed, and the plan must state which of the
  two situations it is in.
- The `Unavailable` row removed; `docs/tui.md` and the keymap box updated.

### Explicitly out of scope

- A **durable** jumplist. It does not survive a restart, a reconnect or a
  document close.
- A jumplist that spans **documents**, because only one document is attached at a
  time (roadmap §3). The store still records the document identity so an entry
  can be refused rather than misapplied when the identity changes.
- `Ctrl-i` / `Ctrl-o` **unless D1 resolves to accepting the shadowing**.
- Fuzzy picker behaviour. The existing pickers are not fuzzy and this milestone
  does not change that.
- A second jumplist for the terminal panel. The terminal panel has its own
  viewing mode; adding one there is a separate surface with its own state.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None. The store is frontend presentation state.

### Protocol and DTOs

None.

### Runtime and concurrency

None. Pushing and stepping are synchronous.

### Frontend or operator surface

- `src/tui/document_session.rs`: the jumplist store, its cap, its cursor, and its
  clearing. A named constant, not a magic number.
- `src/tui/editor.rs`: the push points and the "can this entry be honoured"
  check.
- `src/tui/commands/editor.rs`: the `space j` leaf moves from `Unavailable` to the
  picker action; the push calls at each jump site; the picker selection handler.
- `src/tui/editor_effect.rs`: a new effect variant for opening the jumplist
  picker if the existing picker variants cannot express it.

### Security and authorization

No change. A jump is a cursor move, not a read and not a write.

### Documentation and static guards

- `docs/tui.md`: `space j` described as a working picker; the bound and the
  clearing rule stated; the D1 outcome recorded — including the explicit note
  that `Ctrl-i` is `Tab` on this TUI and `Ctrl-o` is the dashboard.
- The leaf authority guard's `Unavailable` set shrinks by one.

## 7. Ordered work packages

### Work package A — The store

Intent: a bounded, positions-only history with a stated clearing contract.

Required changes: the store, a cap constant, push, clear, and a membership
check. `normalize`-style ordering is **not** wanted — a jumplist is chronological,
not ordered by position, and that difference must be in the doc comment so a
later reader does not "fix" it to match the selection set.

Acceptance evidence: pushing past the cap prunes and discloses; a lifecycle
transition clears it; the store holds no text (asserted structurally by the
text-authority guard, not by inspection).

### Work package B — The push points

Intent: every deliberate jump records where it came from.

Required changes: a push at each jump site. The rule is stated once: a jump that
moves the cursor to a *named destination* pushes; a motion that walks the buffer
does not. `gg`, `G`, `:goto`, `gd`/`gi`/`gr`, diagnostic stepping and
symbol-picker selection are named destinations; `j`, `w`, `Ctrl-d` are not.

Acceptance evidence: each named jump pushes exactly one entry; a plain motion
pushes none; the entry records the position **before** the jump, which is what
makes the jump reversible.

### Work package C — The picker

Intent: `space j` delivers the capability.

Required changes: the leaf's action becomes the picker open; the picker lists the
entries newest-first with enough context to tell them apart (line number and, if
the existing picker surface supports it, the line's text); selecting an entry
jumps and reports an entry that cannot be honoured.

Acceptance evidence: `space j` opens the picker; selecting an entry returns the
cursor to the recorded position; an entry recorded before a reload that shrank
the document is refused with a notice rather than clamped silently; the leaf no
longer reports that the capability is absent.

### Work package D — D1: the stepping pair

Intent: decide, once, what to do about `Ctrl-i` / `Ctrl-o`.

**D1 — Helix's stepping chords against this TUI's global map.** Three candidates.

*(a)* Bind `Tab` / `Shift+Tab` in Normal mode on the editor route as
forward/backward, accepting that `ToggleComposerMode` and `TogglePermissionMode`
stop working on that route.
*(b)* Bind `Ctrl-o` for backward only and leave forward to the picker.
*(c)* Bind neither; the picker is the complete deliverable, and stepping is
recorded as deferred with the wire-level reason.

**Recommendation: (c).** (a) is the only option that gives Helix's two directions,
and it costs two live global actions on the editor route — one of which
(`ToggleComposerMode`) is how a user switches the composer between normal and
insert, a function with no other spelling on that screen. (b) is worse than
nothing: an asymmetric pair teaches a gesture that then does not exist in the
other direction. (c) delivers the capability — navigate to a visited position —
through a surface that is already discoverable through `space ?`, and records the
constraint honestly.
If the owner chooses (a) or (b), the plan requires: a fourth stage in the key
authority guard's model, a `--self-test` case proving the guard rejects an
injected shadow of the same chord, and `docs/tui.md` stating what the editor route
takes from the global map.

Acceptance evidence: D1 is resolved and written into this plan and the roadmap;
whichever option is chosen, no dead chord exists and the guard's model matches
the dispatch chain.

### Work package E — Guards and documentation

Intent: the document and the code agree.

Required changes: remove the `Unavailable` row, update the leaf guard's figures,
document the picker, the bound, the clearing rule and D1's outcome.

Acceptance evidence: the leaf authority guard passes with the reduced set; the
doc-drift guard passes in both directions; `docs/tui.md` no longer claims the
capability is absent.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** A picker selection whose entry cannot be honoured produces
  a notice and does not move the cursor.
- **Duplicate delivery.** None; synchronous.
- **Restart.** The store is empty after a restart, by construction.
- **Cancellation races.** None; no async work added.
- **Stale generations.** The store is cleared by the same transitions that clear
  the undo history, so an entry cannot outlive the document it names.
- **Bound reached.** The cap prunes and discloses; it does not silently drop.
- **Reload shrank the document.** The entry is refused with a notice naming the
  reason, not clamped.

## 9. Compatibility and migration

No storage, protocol, or configuration migration.

**Two compatibility surfaces.**

1. **The leaf.** `space j` changes from a statement to a picker. Additive; no
   transitional state.

2. **D1's chord question.** If D1 resolves to (a) or (b), the editor route takes
   global chords and that is a **user-visible narrowing** of the global map on one
   route. It must be documented where the global bindings are documented, not only
   in the editor's section, and the key authority guard's model must gain the
   stage that makes the narrowing explicit rather than accidental. D1(c) has no
   compatibility surface at all.

Removal criteria for a transitional state: none; there is no transitional state.

## 10. Required tests

### Focused unit tests

- Push, prune at the cap, clear, and the membership check.
- The "can this entry be honoured" check: an offset inside a shorter document, an
  offset at the exact end, and a matching document identity.

### Integration tests

- `space j` opens the picker through the real dispatch path.
- Each named jump pushes exactly one entry; a plain motion pushes none.
- Selecting an entry returns the cursor to the recorded position.
- A reload that shrank the document produces a refusal, not a clamp.

### Restart and recovery tests

None beyond the clearing assertion. The closure record must say the store is
in-memory by design rather than omit the section.

### Contention and cancellation tests

- The cap: pushing past it prunes and the disclosure is observable.

### Security and negative tests

- `check_tui_editor_text_authority.py` passes with no allowlist edit.
- The store holds no document bytes — asserted by the guard, not by inspection.

### Migration and compatibility tests

- If D1 resolves to (a) or (b): a test asserting the global action is still
  reachable by its other spelling where one exists, and a guard `--self-test`
  rejecting an injected shadow.
- The doc-drift guard as the leaf compatibility check.

## 11. Required verification commands

```bash
cargo fmt --check
cargo test -p codegg --lib tui::editor
cargo test --test tui_render
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_comment_budget.py
bash scripts/verify.sh quick
```

If D1 resolves to (a) or (b), also run
`python3 scripts/check_tui_editor_key_authority.py --self-test` and record its
output — a guard model change without a self-test is not evidence.

## 12. Documentation updates

- `docs/tui.md`: `space j` as a working picker; the bound; the clearing rule; the
  refusal when an entry cannot be honoured; D1's outcome including the note that
  `Ctrl-i` is `Tab` on this TUI because keyboard enhancement is not enabled, and
  that `Ctrl-o` is the workspace dashboard.
- The `space ?` keymap box follows.
- The store's doc comment states that the order is chronological, not positional,
  so it is not "normalized" to match the selection set.

## 13. Acceptance criteria

- `space j` opens a jumplist picker listing the positions the editor recorded.
- Selecting an entry moves the cursor to that position.
- `gg`, `G`, `:goto`, `gd`, diagnostic stepping and symbol-picker selection each
  record one entry; `j` and `w` record none.
- An entry recorded before a reload that shrank the document is refused with a
  notice and the cursor does not move.
- The store is bounded and reaching the bound is disclosed.
- The leaf authority guard passes with one fewer `Unavailable` row; the doc-drift
  guard passes in both directions.
- D1 is resolved and recorded, and no chord that cannot fire is bound.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- D1 has not been resolved by the owner. Do not bind `Tab` or `Ctrl-o` on a
  guess; the picker alone is the safe deliverable and the plan says so.
- A jump site cannot record the pre-jump position because it does not know it.
  Report the site rather than recording the post-jump position, which would make
  the entry useless.
- The existing picker surface cannot express a jumplist entry without a new
  surface. Report the shape; a new picker surface is a different change.
- The work expands into a durable jumplist, a cross-document jumplist, or fuzzy
  pickers.
- Someone asks for `Ctrl-i` to be bound as a `CONTROL` chord. It cannot fire;
  that is the finding, not an omission.

## 15. Closure evidence required

- D1's resolution and its recorded consequence.
- The push-point evidence: one entry per named jump, none per motion.
- The picker's dispatch-path test.
- The refusal evidence for an unhonourable entry, asserting the cursor did not
  move.
- The cap and disclosure evidence.
- The leaf authority guard's output with the reduced `Unavailable` set.
- A statement that no migration, protocol variant, crate, or new authority was
  added.
- The commands actually run, labelled local or CI, with unrun items named.

## 16. Handoff notes

- **`Ctrl-i` is `Tab`.** This TUI does not enable keyboard-enhancement flags, so
  the byte is `0x09` and crossterm reports `KeyCode::Tab`. A
  `(CONTROL, Char('i'))` arm would be dead code that compiles and never fires —
  the exact defect the predecessor's M001 fixed. Do not add it.
- **`Ctrl-o` is the workspace dashboard.** `Shift+W` is its other spelling.
- **The order is chronological.** Do not sort the store to match the selection
  set's document ordering; a jumplist that reorders itself is not a history.
- **Record the position before the jump.** An entry that records where the jump
  landed cannot take the reader back.
- **Refuse, do not clamp.** A jumplist entry into a shorter document is not
  approximately right; landing somewhere else is worse than saying the entry is
  stale.
- **Serial test constraint.** No environment mutation is introduced; nextest
  profile `ci` applies.
- **Preserve the user's working tree.** No generated asset is touched.
