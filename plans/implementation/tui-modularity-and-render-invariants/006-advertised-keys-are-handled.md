# TUI Modularity and Render Invariants Milestone 006 — Every advertised key is handled

Status: conditionally closed — the corrective landed: the I4 baseline is a 4-row reasoned residue (C001) read from the producer's structured items (C002); conditional on `full` on the merged tree and a commit

> **Scope amendment (2026-11-12), from the corrective research.**
> The criterion below asked for the baseline to hold "exactly the retained
> entry", i.e. **one**. Two facts make that target incoherent, so it is amended
> rather than quietly missed:
>
> 1. **There are two `Tab` conflicts, not one.** `Model` *and*
>    `WorkspaceDashboard` both advertise `Tab` and neither can receive it —
>    `FocusManager::handle_key` intercepts `Tab` before the dialog sees it
>    (`src/tui/components/component.rs`'s focus path, `focus.rs:164-170`) and
>    `handle_tab` never forwards. Reaching *one* entry would mean de-advertising
>    one of two identical conflicts; reaching *zero* needs either an ADR-0042
>    amendment (a dialog may pre-empt `Tab`) or de-advertising both.
> 2. **53 of the 61 entries were never surface defects.** The measurement could
>    not exercise the key: 46 mounted the surface with an **empty fixture** (an
>    empty session list, no providers, no receipts), 2 started at a **boundary**
>    (scroll already at 0), and 5 depended on **probe ordering** (a prior key
>    left the dialog in a mode that hides the list). Only 4 entries were a
>    missing arm and 4 were a verb that does something other than advertised.
>
> The corrective is therefore a two-milestone set — make the measurement
> inspectable and state-fresh (`plans/implementation/tui-advertised-keys-corrective/001-i4-baseline-to-its-irreducible-residue.md`),
> then convert it to the structured `(key, verb)` pairs the plan's §2 specified
> and the trait boundary currently discards (`…/002-structured-pairs-from-the-producer.md`).
> **This plan's acceptance criterion 5 is amended** to "the recorded residue,
> each with a reason", and "reach one entry" is an explicit stop condition
> requiring an owner decision on the `Tab` ownership contract.

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-006--every-advertised-key-is-handled`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — the
  interaction concepts a surface advertises are the concepts it must deliver.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — one
  owner per chord. This milestone repairs a surface that advertises a key it
  does not own, and it must not resolve an ownership conflict in the process.
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — no
  catalog change; the commands already exist.

Primary class: capability

## 1. Objective

Close invariant I4 — every key a surface advertises is a key that surface acts
on — and repair the two live instances where a footer names a verb that does
nothing.

## 2. Why this milestone is ready

Blocked on M001 (hard, verification gate) with an interface dependency on M005:
I4 needs the advertised verbs as structured `(key, verb)` pairs, and parsing
them back out of a rendered string is exactly the fragility this repository
avoids. `HintLine` supplies the pairs.

## 3. Current implementation evidence

At baseline `7642b9e38`:

- **Three gaps are already recorded**, in
  `tests/tui_render/dialog_dispatch_coverage.inc:14-26`, written as a deliberate
  record rather than a passing assertion:
  1. The Model dialog's `Tab` — `FocusManager::handle_key`
     (`src/tui/components/component/focus.rs:165`) intercepts `Tab` for its
     focus ring before the dialog sees it, so the dialog's own tabs are
     unreachable.
  2. The `/shell-show` modal's `i`/`a`/`r`/`k`/`e` verbs — "which the modal's
     own footer still advertises".
  3. The Import dialog's text input — `ImportDialog::handle_key`
     (`src/tui/components/dialogs/import.rs:310`) handles only `Esc` and
     `Enter`; `Char` and `Backspace` fall through to `_ => None`, so a typed URL
     never reaches the preview. `handle_paste` (`:324`) exists, so paste works
     and typing does not.
- **The ambiguity that makes I4 hard.**
  `Component::handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg>` returns
  `None` both for "handled, nothing to report" and for "not handled".
  `FocusManager::handle_key` (`:171-176`) therefore cannot distinguish the two,
  and no trait method answers "do you claim this key?". Adding one would create
  a second source of truth that drifts from the `match` — the anti-pattern this
  roadmap exists to remove.
- **20 files implement `keymap_hint()`**, and several are
  `Some(HINT.to_string())` over a local constant, so the advertised set is
  per-file by construction.
- **Four dialogs carry a focus dot in a border title** (`plan`, `files`, `tree`,
  `diff`), which is where a docked dialog advertises its verbs.
- `tests/tui_render/tui_info_dialog_panel_chrome.inc:721` already asserts that
  a hint never names a verb the hosted path cannot deliver, for `InfoDialog`
  only. That is the right property, applied to one dialog.

## 4. Invariants that must not regress

- **No key changes owner.** ADR-0042 governs; the two key-authority guards must
  report the same chord counts after this milestone as before it.
- **No command catalog change.** ADR-0052 governs; the slash catalog is
  unchanged and `check_command_catalog_ownership.py` passes unmodified.
- **No surface loses a working verb** in order to make its advertisement
  honest. Removing an advertisement is the fallback only where the verb cannot
  be implemented without an ownership decision (§14).
- **I1, I2, I3b, I6 stay green.**
- **The render-cost shape guard is unchanged.**
- **The comment ratchet holds.**

## 5. Scope

### In scope

- A generic I4 assertion: for every advertised `(key, verb)` pair on every
  surface, pressing that key changes the rendered frame or closes the surface.
- `/shell-show`: handle the five advertised verbs, or stop advertising them,
  with the choice recorded.
- `ImportDialog`: accept typed characters and `Backspace` so the advertised
  input path works.
- The I4 baseline from M001 driven to empty.

### Explicitly out of scope

- The Model dialog's `Tab`. That gap is a **key-ownership conflict**, not a
  missing handler: `FocusManager` legitimately owns `Tab` for its focus ring,
  and deciding that a dialog may pre-empt it is an ADR-0042 amendment. This
  milestone records the decision point and leaves the gap recorded.
- Any new command, verb or key binding.
- Making a hint honest by deleting a verb that *could* be implemented — the
  implementation is preferred and the deletion needs a recorded reason.

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

- The I4 check drives each advertised key through the production dispatch path
  and compares the rendered frame before and after. **A key whose effect is
  invisible is treated as broken**, which is the owner's bar rather than a
  stricter one: a key that produces no visible change and no close is
  indistinguishable to a user from a key that does nothing.
- `/shell-show` gains handlers for `i`/`a`/`r`/`k`/`e`, or its footer stops
  naming them.
- `ImportDialog::handle_key` gains `Char` and `Backspace` arms that edit its
  input, so the existing preview path becomes reachable by typing.

### Security and authorization

None. `ImportDialog`'s new input is bounded by the same path the paste arm
already uses; the arm must not bypass any existing length or validation limit,
and the plan requires the implementer to check whether `handle_paste` applies
one before adding a character path that does not.

### Documentation and static guards

- `architecture/tui.md` — the I4 rule: an advertised key is a promise.
- `tests/tui_render/dialog_dispatch_coverage.inc` — the three recorded gaps are
  updated: two closed, one left recorded with the ownership reason.

## 7. Ordered work packages

### Work package A — The I4 check

Intent: make "the key does something" a property rather than a review.

Required changes: for each surface, obtain its advertised pairs from
`HintLine`; for each pair, capture the frame, dispatch the key through the
production path, capture again, and assert the frame differs or the surface
closed.

Acceptance evidence: the check fails against a fixture dialog that advertises
`x expand` and ignores `x`, and passes against one that acts on it. This is the
non-vacuity proof.

### Work package B — `/shell-show`

Intent: close the recorded gap.

Required changes: either implement the five verbs or remove them from the
footer. The choice, and the reason, go in the closure record.

Acceptance evidence: the I4 check passes for `/shell-show`, and the verb set it
advertises is recorded before and after.

### Work package C — Import input

Intent: make typed input reach the preview.

Required changes: `Char` appends to the input, `Backspace` pops, both bounded
consistently with `handle_paste`.

Acceptance evidence: a test types a URL one character at a time and asserts the
input equals the typed string and that `Enter` submits a preview request; a
second test asserts the bound is enforced at the same limit paste uses.

### Work package D — Baseline to empty

Intent: leave no unhandled advertisement except the one that needs a decision.

Required changes: drive `[advertised_keys]` in the M001 baseline to contain
only the Model dialog's `Tab`, and record why that one remains.

Acceptance evidence: the guard reports exactly one remaining entry, and the
entry names the ownership reason rather than a TODO.

### Work package E — Update the recorded gaps

Intent: keep the repository's own record truthful.

Required changes: `dialog_dispatch_coverage.inc`'s header records two gaps
closed and one retained, with the ADR reference for the retained one.

Acceptance evidence: the file's assertions still pass, and its prose matches
the baseline file.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. The I4 check is synchronous and per-surface. The one hazard is
a key whose effect is deferred to an async round trip: pressing it would not
change the frame in the same turn, and the check would call it broken. Such a
surface must be listed with its reason rather than silently exempted, and the
listing is part of the closure evidence.

## 9. Compatibility and migration

No compatibility surface. `ImportDialog` gains an input path it already
advertised; `/shell-show` either gains behaviour or loses an advertisement. A
user who typed into the import dialog and saw nothing now sees the characters,
which is the defect being repaired rather than a new capability.

## 10. Required tests

### Focused unit tests

- `ImportDialog` `Char` appends and `Backspace` pops, with the same bound as
  paste.
- `ImportDialog` `Enter` in `Input` state still submits a preview request.

### Integration tests

- The I4 check over every surface with an advertised pair set.
- `/shell-show` advertises only verbs it handles.
- The I4 check is non-vacuous, proved by the failing fixture.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- An over-long typed input is refused at the same bound paste enforces.
- A surface advertising a key it ignores fails the check — the non-vacuity
  fixture is this test.

### Migration and compatibility tests

- The advertised-verb set per surface is compared to the M005 baseline; the
  only permitted difference is `/shell-show`, and it is recorded.

## 11. Required verification commands

```bash
# the I4 check and the invariant harness
cargo test --test tui_render -- tui_render_invariants

# the recorded gaps
cargo test --test tui_render -- dialog_dispatch_coverage

# the two repaired surfaces
cargo test --test tui_render -- connect_dialog_input
cargo test --lib -- components::dialogs::import

# the guard and its baseline
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_render_invariant_baseline.py --self-test

# authority must be unchanged
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_command_catalog_ownership.py

# the render-cost shape guard
python3 scripts/check_tui_render_cost.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the routine gate
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/tui.md` — the I4 rule and the one recorded exception.
- `tests/tui_render/dialog_dispatch_coverage.inc` — its header prose, as above.
- `architecture/tui.md` — the `/shell-show` outcome, whichever it is.

## 13. Acceptance criteria

1. The I4 check passes for every advertised pair on every surface, except the
   one recorded ownership conflict.
2. The check is proved non-vacuous by a failing fixture.
3. `ImportDialog` accepts typed characters and `Backspace`, bounded as paste is.
4. `/shell-show` advertises only verbs it handles, or handles all it advertises.
5. The M001 `[advertised_keys]` baseline contains the recorded residue, each entry
   carrying a reason — see the scope amendment above. Reaching a single entry is a
   stop condition, not an acceptance criterion.
6. `dialog_dispatch_coverage.inc` records two closures and one retained gap with
   its ADR reference.
7. The two key-authority guards report unchanged chord counts;
   `check_command_catalog_ownership.py` passes unmodified.
8. `scripts/verify.sh quick` is green.

## 14. Stop conditions

Report rather than improvise when:

- a repair would require changing which stage owns a chord — that is ADR-0042
  and this milestone does not amend it;
- an advertised verb cannot be implemented without a new command, which would
  be a capability addition;
- an advertised key's effect is deferred past the frame, so the check cannot
  observe it — record the surface and its reason rather than exempting it
  silently;
- `ImportDialog`'s paste path has no length bound and adding a character path
  would need one that does not exist — report the missing bound rather than
  inventing a limit;
- removing an advertisement is the only option and the verb looks implementable
  — report the choice instead of deleting the evidence.

## 15. Closure evidence required

- The I4 check, its non-vacuity proof, and its per-surface result.
- The `/shell-show` decision and its before/after verb sets.
- `ImportDialog`'s typed-input tests, including the bound.
- The baseline file containing the recorded residue, each entry with its reason.
- The updated `dialog_dispatch_coverage.inc` header.
- Key-authority guard output showing unchanged counts.
- `quick` output.
- A closure record at
  `plans/closure/tui-modularity-and-render-invariants/006-status.md`.

## 16. Handoff notes

The `Option<TuiMsg>` ambiguity is the whole difficulty. Do not add a
`handles_key()` trait method to resolve it: that is a second source of truth for
the same question, and the two will drift exactly the way the 41 hand-rolled
chrome implementations drifted. Assert the observable effect instead.

Treat "the effect is invisible" as broken. It is tempting to exempt a key whose
only effect is internal state, but a user cannot see internal state, so from the
only perspective that matters the key does nothing. If a real surface has such a
key, that is a finding worth reporting, not a reason to weaken the check.

Leave the Model dialog's `Tab` alone. It looks like the easiest of the three and
it is the only one that is a design question: `FocusManager` owns `Tab` for
every dialog, and letting one pre-empt it changes the ownership contract for
all of them. Fixing it here would be a silent ADR-0042 amendment.
