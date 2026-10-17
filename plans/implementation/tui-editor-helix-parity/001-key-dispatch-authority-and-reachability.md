# TUI Editor Helix Parity Milestone M001 — Key dispatch authority and binding reachability

Status: closed — closure `plans/closure/tui-editor-helix-parity/001-status.md`

Outcome: the plan's shape (a) and shape (b) both rejected; resolved as **shape
(b)**, redo on `U` (Helix's own chord — Helix binds `Ctrl+r` in *Insert* mode
only), with `Ctrl+r` keeping exactly one owner as resync in both modes. Shape
(a) was rejected because scoping the resync guard would have required inventing
a new Normal-mode resync chord. A second instance of the same class — the dead
`| (m, Char('d'))` alternative in the `D` arm — was also closed; see the closure
record's scope note.

Repository baseline: `719cfeb5d` (branch `feat/auto-20261017-80a41db7`)

Source roadmap:

- `plans/subsystems/tui-editor-helix-parity-roadmap.md#7-milestones` (M001)

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md`

Primary class: invariant

## 1. Objective

Repair the shadowed `Ctrl+r` redo binding and make an unreachable normal-mode
binding arm structurally detectable, so that no declared binding can again lose
to an earlier guard without a test failing.

## 2. Why this milestone is ready

- No hard dependency. The predecessor roadmap
  `plans/subsystems/tui-editor-correctness-usability-roadmap.md` is closed with
  individual closure records, and its post-closure corrective addendum has no
  active milestone touching key dispatch.
- ADR-0011 and ADR-0021 are accepted; neither this milestone nor M002 introduces
  a new authority boundary.
- No protocol, storage, or dependency change.
- The defect is fully characterized by reading current source: it is a local
  ordering bug in one dispatch function.

This milestone is deliberately first and deliberately small. M002 consolidates
the prefix mechanisms, and M003–M012 add roughly twenty bindings. Repairing the
authority model before any of them land is the entire point: otherwise each new
arm inherits the same unreachability hazard.

## 3. Current implementation evidence

### The defect

`src/tui/commands/editor.rs` resolves editor keys in this order:

1. `editor_buffer_key` checks `search_active`, then `command_active`.
2. It then applies **mode-independent** guards, before any mode dispatch:

   ```rust
   if key.code == KeyCode::Char('s') && key.modifiers == KeyModifiers::CONTROL {
       return BufferKey::RequestSave;
   }
   if key.code == KeyCode::Char('r') && key.modifiers == KeyModifiers::CONTROL {
       return BufferKey::RequestResync;
   }
   ```

3. Only then does it branch on mode and call `handle_normal_key`.

`handle_normal_key` contains its own arm:

```rust
(KeyModifiers::CONTROL, KeyCode::Char('r')) => {
    editor_redo(state);
    BufferKey::Consumed
}
```

That arm is **unreachable**. The step-2 guard returns `RequestResync` first.
`handle_normal_key` has exactly one call site (inside `editor_buffer_key`), so
there is no alternate path that reaches it.

Net user-visible effect: Normal mode offers undo (`u`) and has no reachable
redo. `Ctrl+r` resyncs instead.

### Why the test suite is green

Every existing redo test calls the helper directly —
`editor_redo(&mut state)` appears at three sites in the module's test code — so
no test exercises redo through the key path. The tests validate `editor_redo`
and `editor_undo`; they never validate that a key reaches them. This is the
generalizable lesson of the defect: **a binding test that bypasses dispatch
tests the helper, not the binding.**

### Related precedent

A sibling class of defect already occurred in this codebase: the Insert-mode
text-key guard was gated on `bindings.is_none()`, which never holds because
`App::on_key` always passes `Some(&ui_state.bindings)`, so a capital `F` opened
the project picker and swallowed the character in the default configuration.
The guard that caught it,
`builtin_shift_letters_are_composed_text_in_insert_mode` in `src/tui/input.rs`,
is the model for the sweep required in §7.

### Relevant symbols

- `BufferKey` — the buffer→App effect enum, including `Consumed`, `Unhandled`,
  `RequestSave`, `RequestResync`, `RequestReload`, `DeleteRange`, `YankRange`,
  `EnterInsert`, `RunCommand`.
- `handle_normal_key`, `editor_buffer_key`, `handle_insert_key`,
  `editor_redo`, `editor_undo`.
- `MAX_EDITOR_PENDING_COMMAND` (value `2`) in `src/tui/editor.rs`, bounding
  `TuiDocumentPresentation::pending_command`.

## 4. Invariants that must not regress

- **Unclaimed-key fallthrough.** In Normal mode a key the buffer does not own
  must return `BufferKey::Unhandled` so it reaches the composer. Do not convert
  an existing `Unhandled` path into a claim as part of this fix.
- **Insert-mode text integrity.** No binding may swallow a printable character
  while the route is in Insert.
- No mutation outside `DocumentController::apply_local`; the buffer keeps
  reporting effects (`DeleteRange`, `YankRange`, `RequestSave`, …) and the App
  layer keeps performing them.
- `:` and `/` remain outranking every mode-independent chord. A half-typed `:q`
  must not be rewritten by `Ctrl+s` writing the file.
- `R` (reload) stays Normal-mode-only and stays destructive-by-explicit-request.
- Rendering and remote DTO bounds are unchanged by this milestone.

## 5. Scope

### In scope

- Resolve the `Ctrl+r` conflict with one documented owner per chord.
- Add dispatch-path coverage for **every** currently declared normal-mode
  binding.
- Add a reachability guard that fails when a declared arm is shadowed.
- Add the non-vacuous Insert-mode printable-character sweep if §7 finds it is not
  already covered for this dispatch path.
- Document the chord ownership rule for `Ctrl+s` / `Ctrl+r` / `R`.

### Explicitly out of scope

- Any new binding. This milestone adds no key the user did not already have.
- The prefix state machine (M002).
- Any motion, change verb, selection, or undo work.
- The derived-keymap infrastructure, unless it can be added without expanding
  this milestone; otherwise it belongs to M002.
- `R`/reload chord relocation (considered below, not executed here).

## 6. Required production changes

### Core/domain

None. This is a frontend dispatch correction.

### Storage, protocol, runtime

None. No protocol change, no migration, no concurrency change.

### Frontend surface

Resolve the `Ctrl+r` conflict. Two shapes are acceptable; pick one and record
which:

- **(a) Mode-scoped guards.** Keep resync mode-independent for save but scope
  the resync guard so it does not shadow a Normal-mode redo chord, giving each
  chord one owner. Requires confirming `RequestResync` still has a reachable
  path in both modes.
- **(b) Rebind redo.** Give redo a chord that no guard shadows and leave
  `Ctrl+r` as resync everywhere.

Shape (a) is preferred because it preserves the Helix chord the user expects,
but shape (b) is acceptable **only** if the replacement chord is documented and
does not collide with `Ctrl+s`/`Ctrl+r`. Whichever is chosen, the invariant is
that **every declared binding has exactly one owner and one reachable path**,
and the conflict is resolved at the dispatch layer rather than by deleting an
arm.

Do **not** fix this by reordering the guards so the redo arm wins while
`RequestResync` becomes unreachable — that trades one shadowed binding for
another.

### Security and authorization

None. No new authority, no new effect type.

### Documentation and static guards

- Record the chord-ownership rule as a doc comment on the guard block, so the
  next binding author sees it before adding a colliding chord.
- If the reachability guard is implemented as a script, add it to
  `scripts/verify.sh quick` and, where CI-appropriate, to
  `.github/workflows/ci.yml`.

## 7. Ordered work packages

### Work package A — Resolve the shadow

Intent: give `Ctrl+r` and `Ctrl+s` exactly one owner each, with redo reachable.

Required changes: per §6 "Frontend surface". Add a doc comment stating the
ownership rule.

Acceptance evidence: a dispatch-path test asserting `Ctrl+r` in Normal mode
reaches redo (via observable undo/redo state transition), and a second
asserting the resync request still has a reachable path. Both must go through
`App::on_key` or `editor_buffer_key` — **not** by calling `editor_redo`.

### Work package B — Dispatch-path binding table

Intent: cover every declared normal-mode binding through the real path.

Required changes: a table-driven test enumerating each declared binding with
its expected observable effect, driven through `App::on_key` or
`editor_buffer_key`. Include at least one assertion that an unclaimed letter
still returns `Unhandled` and reaches the composer.

Acceptance evidence: the table covers every arm of `handle_normal_key` at the
baseline. A deliberate, temporary shadowing of any single arm must make the
suite fail — prove this before citing the table as a guard, then revert the
injected change.

### Work package C — Reachability guard

Intent: make shadowing statically or structurally detectable.

Required changes: choose one —

- a focused static guard under `scripts/` following the existing check-script
  conventions, wired into `verify.sh quick`; or
- a structural test asserting the guard block and the dispatch table declare
  disjoint chords.

Acceptance evidence: inject a shadowing arm, observe the guard fail, revert.

### Work package D — Insert-mode printable-character sweep

Intent: confirm no binding swallows a printable character in Insert.

Required changes: confirm whether `builtin_shift_letters_are_composed_text_in_insert_mode`
(`src/tui/input.rs`) already covers the editor route's dispatch path. If it
covers only the global keymaps, extend it to the editor route. If it already
covers it, add no duplicate — record the finding in the closure.

Acceptance evidence: the sweep is non-vacuous — assert it would fail if a
normal-mode binding were moved into the Insert path. Report truthfully whether
this package added code or confirmed existing coverage.

## 8. Failure, cancellation, restart, and contention semantics

No new lifecycle. Behavior to preserve:

- A key arriving while `search_active` or `command_active` is consumed by that
  line and never by a binding.
- Unchanged retry/idempotency semantics: this milestone adds no async work.
- No contention surface: the editor route remains single-threaded on the UI
  task.

## 9. Compatibility and migration

No storage, protocol, or configuration migration.

**User-visible behavior change:** Normal-mode `Ctrl+r` performs redo instead of
resync. If shape (b) is chosen, a different chord becomes redo. Either way this
is a deliberate, documented change to a previously mis-wired chord — record it
in `docs/tui.md` and in the closure record, and do not describe it as a new
capability without saying it replaced a resync binding.

## 10. Required tests

### Focused unit tests

- Dispatch-path test for every declared normal-mode binding (Work package B).
- `Ctrl+r` redo reachability and resync reachability (Work package A).
- Fallthrough test: an unclaimed letter returns `Unhandled`.

### Integration tests

- `cargo test --test tui_main_buffer` must pass unchanged; it is the regression
  surface for the editor route.

### Restart, contention, migration

None applicable. State explicitly in the closure that these categories are
not applicable rather than omitting them.

### Security and negative tests

- The Insert-mode printable-character sweep (Work package D), non-vacuous.
- A read-only buffer still refuses insertion entries; the fix must not widen
  any write path.

### Non-vacuity proof (required for B, C, and D)

For each guard added, inject the defect it is supposed to catch, observe the
failure, and revert. A guard never observed failing is a claim, not evidence —
record the injection and the observed failure in the closure record.

## 11. Required verification commands

```bash
# narrow first
cargo test -p codegg --lib tui::commands::editor
cargo test -p codegg --test tui_main_buffer
cargo test -p codegg --lib tui::input

# compile + guards
cargo ck
scripts/verify.sh quick

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

Run the non-vacuity injections from §10 before writing the closure record. Do
not claim commands that were not run; label local versus hosted CI truthfully.

## 12. Documentation updates

- `docs/tui.md` "Editor language features": correct the `Ctrl+r` description and
  state the chord-ownership rule.
- `architecture/tui.md`: record the dispatch authority order — modal lines, then
  mode-independent chords, then mode dispatch — and the rule that no declared
  binding may be shadowed by an earlier guard.
- `CHANGELOG.md` if the user-visible chord change warrants an entry.

## 13. Acceptance criteria

1. Redo is reachable from a chord in Normal mode, proven through the dispatch
   path.
2. Resync retains exactly one reachable chord in every mode.
3. Every declared normal-mode binding is exercised by a dispatch-path test.
4. Injecting a shadowed arm fails the suite; the guard is demonstrated, not
   assumed.
5. The Insert-mode printable-character sweep passes and is non-vacuous, or
   existing coverage is confirmed and recorded.
6. `scripts/verify.sh quick`, `cargo fmt --check`, and clippy `-D warnings` pass.
7. `docs/tui.md` and `architecture/tui.md` match the implemented chord set.

## 14. Stop conditions

Stop and report rather than improvise when:

- Resolving the conflict would change `BufferKey`'s public effect set or add a
  new App-layer effect, which is a scope expansion into M002 or beyond.
- Making redo reachable appears to require a different ownership model for
  `RequestSave`/`RequestResync` — that is a design decision for maintainers.
- The reachability guard cannot be made non-vacuous; a guard that cannot be
  shown to fail is not a guard.
- Fixing this requires touching a closed milestone's scope.
- Repository evidence contradicts ADR-0011's ownership boundary.

## 15. Closure evidence required

- `plans/closure/tui-editor-helix-parity/001-status.md` containing: the chosen
  conflict resolution shape and why; the dispatch-path binding table; the
  non-vacuity injection results for Work packages B, C, and D with observed
  failure output; exact verification commands and results with local versus
  hosted labels; an explicit statement that restart/contention/migration are
  not applicable; and the registry update including the unblock audit.

## 16. Handoff notes

- **Hazard:** the fix must not simply delete the redo arm. Deleting it would make
  the code honest and the user no better off; Normal mode would still have no
  redo.
- **Hazard:** do not reorder guards so redo wins while resync becomes
  unreachable.
- **Hazard:** tests that call `editor_redo`/`editor_undo` directly do **not**
  cover this defect and must not be counted as evidence. The existing three
  direct call sites are precisely why the suite is green today.
- **Non-vacuity is mandatory.** Each guard must be observed failing against an
  injected defect before it is cited.
- Build jobs are capped (`CARGO_BUILD_JOBS=2`); broad sweeps run under the
  nextest `ci` profile. Prefer narrow targets first.
- `verify.sh quick` is a strict superset of CI, so a green CI run does not imply
  a green `quick`.
