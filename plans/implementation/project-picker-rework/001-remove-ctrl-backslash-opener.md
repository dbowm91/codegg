# Project Picker Rework Milestone 001 — Remove the `Ctrl+\` opener

Status: implemented

Repository baseline: `1ae0095a` (current branch head before M001).

Source roadmap:

- `plans/subsystems/project-picker-rework-roadmap.md#7-milestones`
  (Milestone 1)

Long-term requirements:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- None required (no protocol, storage, identity, or authority change).

Primary class: polish

## 1. Objective

Make `Shift+F` (Normal mode) and `/projects` the only project-picker
openers by removing every `Ctrl+\` binding, help entry, description, and
test that references it.

## 2. Why this milestone is ready

No hard dependencies. The replacement openers exist in the worktree:
`Shift+F` Normal-only binding with both terminal encodings, `/projects`
(+ `/project` alias) dispatching to `open_project_picker`, and `\`
(vim Normal) retained. This milestone touches only the TUI key/help
surface.

## 3. Current implementation evidence

- `src/tui/input.rs`: `(CONTROL, Char('\\'))` bound in the default map
  (both modes) and the vim map, with Insert-branch carve-outs keeping it
  live while composing; help entries for Insert `Ctrl+\`, Normal `\`
  (vim), Normal `Ctrl+\`, Normal `Shift+F` (whose text cites
  "`Ctrl+\` works in every mode").
- `src/tui/command.rs`: `/projects` description cites `Ctrl+\`.
- `docs/tui.md` documents the `Ctrl+\` route; `architecture/command.md`
  mirrors picker opener descriptions.
- Tests: `shift_f_opens_picker_in_normal_only` asserts `Ctrl+\` works in
  both modes; the `modifier_chords` suite and help-audit tests pin
  documented keys resolving in the active map.

## 4. Invariants that must not regress

- `Shift+F` stays Normal-only and typeable as text in Insert mode;
  custom user bindings still win over text (existing
  `test_custom_shift_binding_wins_over_char` contract).
- `\` (vim Normal) picker access is unchanged.
- `/projects` works mid-prompt (Insert) as today.
- `Shift+H/J/K/L` panel navigation and the `Shift+S`/`Shift+F`
  Normal-only precedent are untouched.

## 5. Scope

### In scope

- Remove `Ctrl+\` from the default and vim keymaps.
- Remove or rewrite `Ctrl+\` help entries (both modes); rewrite the
  `Shift+F` help text without the `Ctrl+\` reference.
- Update `/projects` description, `docs/tui.md`, `architecture/command.md`
  (and any other doc citing the chord).
- Update/retire tests that assert `Ctrl+\` opens the picker; keep a
  negative test that it no longer does.

### Explicitly out of scope

- Picker render/filter/selection rework (M2).
- Row actions, lifecycle, destroy, rename/repath (M3/M4 — ADR-gated).
- Identity, auto-create/load, cwd switching (M5 — ADR-gated).
- Rebinding `Shift+H/J/K/L` or any split-switch chord (deferred).

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None.

### Frontend or operator surface

- Delete the `(CONTROL, '\\')` entries from the default and vim binding
  maps; confirm the Insert-branch text-key logic needs no change once the
  chord is gone (it should become dead code only for this chord — do not
  refactor the shared path beyond what removal requires).
- Rewrite help entries: drop Insert/Normal `Ctrl+\` rows; `Shift+F` row
  reads as the Normal-mode opener with `/projects` as the mid-prompt
  route.
- Update `/projects` command description.

### Security and authorization

None.

### Documentation and static guards

- `docs/tui.md`, `architecture/command.md`, roadmap milestone text if it
  cites the chord.
- No new static guard; existing key/help audit tests are updated, not
  weakened (documented-keys-resolve coverage must still pass with the
  chord gone).

## 7. Ordered work packages

### Work package A — Unbind the chord

Intent: `Ctrl+\` does nothing in either mode.

Required changes: remove both-map and vim-map `(CONTROL, '\\')`
entries; leave `Shift+F`, `\` (vim Normal), and `/projects` intact.

Acceptance evidence: focused key-dispatch test asserting `Ctrl+\` in
Normal and Insert yields no `OpenProjectPicker` action.

### Work package B — Help, descriptions, docs

Intent: no user-facing text advertises the removed chord.

Required changes: help entries, `Shift+F` help copy, `/projects`
description, `docs/tui.md`, `architecture/command.md`.

Acceptance evidence: repo-wide search for the chord spelling finds no
user-facing reference (code comments explaining the removal are
acceptable and should say where the openers live now).

### Work package C — Tests and verification

Intent: the suite pins the new opener set.

Required changes: update `shift_f_opens_picker_in_normal_only` (drop the
`Ctrl+\` assertions or convert to negative assertions), keep
`modifier_chords` green, keep documented-keys-resolve green.

Acceptance evidence: `tui::input::`, `tui::command::`, `modifier_chords`
suites green plus `scripts/verify.sh quick`.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable beyond ordinary key handling: an unbound chord is inert
in both modes. No async, storage, or cross-tab effects.

## 9. Compatibility and migration

Users with a custom `Ctrl+\` binding keep it (custom maps are unaffected;
the default entry simply disappears, so the fallback path must not
resurrect it). No config migration. Note the change in release-facing
docs.

## 10. Required tests

### Focused unit tests

- `tui::input::` dispatch: `Ctrl+\` inert both modes; `Shift+F` opens
  picker in Normal, types text in Insert; `\` still opens in vim Normal.
- `tui::command::`: `/projects` (+ alias) still resolves to the picker
  action.

### Integration tests

- `modifier_chords` suite green; documented-keys-resolve audit green.

### Restart and recovery tests

None (no durable state).

### Contention and cancellation tests

None.

### Security and negative tests

- Negative: `Ctrl+\` produces no action in either mode (regression pin
  against silent reintroduction).

### Migration and compatibility tests

- Custom-binding-wins test still green (no change to precedence logic).

## 11. Required verification commands

```bash
cargo test -p codegg --lib tui::input::
cargo test -p codegg --lib tui::command::
cargo test -p codegg --test modifier_chords
scripts/verify.sh quick
```

Do not claim commands that were not actually run in the closure record.

## 12. Documentation updates

- `docs/tui.md`: picker opener paragraph without `Ctrl+\`.
- `architecture/command.md`: `/projects` row/description without the chord.
- Roadmap §12 row for M1 moves to `active`/`closing` per actual progress
  (status edit only, no milestone rewrite).

## 13. Acceptance criteria

- Pressing `Ctrl+\` in Normal and Insert does nothing picker-related.
- `Shift+F` in Normal opens the picker; in Insert it types text.
- `/projects` opens the picker mid-prompt.
- No user-facing `Ctrl+\` reference remains; quick verification passes.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the `Shift+F` + `/projects` surface is absent at the working baseline
  (the milestone assumes it);
- removing the chord requires touching shared Insert-mode precedence
  logic beyond deleting map entries;
- repository evidence contradicts the assumed keymap locations;
- scope would expand into picker rework (M2) or ADR-gated work.

## 15. Closure evidence required

- Focused suite results (`tui::input::`, `tui::command::`,
  `modifier_chords`) and `scripts/verify.sh quick` outcome, labeled local
  vs CI truthfully.
- Repo-wide search showing no remaining user-facing `Ctrl+\` reference.
- Manual note: reinstall + daemon restart, `Shift+F` / `/projects`
  open the picker, `Ctrl+\` inert (local truth only unless CI ran).

## 16. Handoff notes

- Worktree carries uncommitted TUI picker changes at handoff time; the
  agent must rebase its baseline understanding onto the files as found,
  not onto bare `b4e9a92d`.
- Serial-test requirement: none beyond the repo's nextest `ci` profile
  for broad runs; focused lib tests are thread-safe here.
- M2 (help-style picker) is blocked on this milestone's strict closure;
  keep the dialog's key-routing seams intact rather than pre-refactoring
  toward M2.
