# TUI Editor Helix Parity M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-helix-parity/001-key-dispatch-authority-and-reachability.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-parity-roadmap.md#7-milestones` (M001)

Repository baseline reviewed: `719cfeb5d` (branch
`feat/auto-20261017-80a41db7`)

Applicable ADRs (unchanged by this milestone):

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md`

## 1. Executive finding

Normal mode had undo (`u`) and **no reachable redo**. `handle_normal_key`
declared `(KeyModifiers::CONTROL, KeyCode::Char('r')) => editor_redo`, but
`editor_buffer_key` resolves the mode-independent chords *before* it dispatches
on mode and had already claimed `Ctrl+r` for `RequestResync`. The arm was dead
code. The suite stayed green because all three redo tests called
`editor_redo(&mut state)` directly rather than pressing a key — a binding test
that bypasses dispatch tests the helper, not the binding.

The conflict was resolved by **shape (b)**: redo is now `U`, and `Ctrl+r` stays
resync in every mode. That is Helix's own chord (`u` undo, `U` redo — Helix
binds `Ctrl+r` in *Insert* mode only, as `insert_register`), so the shadowed arm
was never a Helix chord in the first place. `Ctrl+s`/`Ctrl+r` keep one owner
each and remain reachable in both modes, so `RequestSave`/`RequestResync` never
lose a path and no chord was invented to make room.

A second instance of the same class was found and fixed while building the
guard: `(m, KeyCode::Char('D')) | (m, KeyCode::Char('d')) if m == shift || m ==
none` followed `(m, KeyCode::Char('d')) if m == none` (delete the held
selection), so the `Char('d')` alternative could never match. Both encodings of
the capital are still accepted.

No protocol, storage, migration, authorization, or dependency change. No closed
milestone of `tui-editor-correctness-usability-roadmap.md` was reopened or
amended.

## 2. Work packages

### Work package A — resolve the shadow

Chosen shape: **(b) rebind redo**. Rationale, and why not shape (a):

| Option | Outcome | Verdict |
|---|---|---|
| (a) Scope the resync guard so `Ctrl+r` redo wins in Normal | Requires inventing a Normal-mode resync chord, because resync would otherwise become unreachable in Normal | Rejected: invents a chord and changes a working binding |
| (b) Rebind redo to a chord no guard shadows | `U` is Helix's own redo chord; `Ctrl+r` keeps exactly one owner (resync) in both modes | **Chosen** |

Shape (b) is also the more faithful answer to the roadmap's purpose: it moves
the binding *to* the Helix chord rather than defending a non-Helix one.

Production changes, all in `src/tui/commands/editor.rs`:

- `handle_normal_key`: `(KeyModifiers::CONTROL, KeyCode::Char('r')) => editor_redo`
  replaced by `(m, KeyCode::Char('U')) if m == none || m == shift => …`. Both
  spellings are accepted because terminals disagree on how a capital is
  encoded, the same rule `I`/`A`/`O` and `D` already follow.
- The dead `| (m, KeyCode::Char('d'))` alternative removed from the `D` arm.
- A chord-ownership doc comment on the guard block in `editor_buffer_key`,
  naming both enforcement mechanisms and the exact defect they cover.
- Doc repairs in the same functions: a duplicated summary line on
  `editor_buffer_key`, and the Helix-dialect note on `handle_normal_key` now
  naming `u`/`U` and pointing at the ownership rule.

`BufferKey`'s effect set is unchanged; no new App-layer effect was added.

### Work package B — dispatch-path binding table

Eight new tests in `src/tui/commands/editor.rs`, all driven through
`editor_buffer_key` and asserting **both** the reported `BufferKey` and the
observable state change:

| Test | Covers |
|---|---|
| `every_normal_mode_motion_is_reachable_through_dispatch` | `Ctrl+d`/`Ctrl+u`, `PageUp`/`PageDown`, `h/j/k/l` + arrows, `0`, `$`, `G`, `gg`, `w`, `b`, `e` |
| `every_normal_mode_change_verb_is_reachable_through_dispatch` | `v`, `x` (and growth), `d` (reported range, not applied), `space y`, `D` and `Shift+D` |
| `undo_and_redo_are_reachable_through_dispatch` | `u`, `U`, `Shift+U` |
| `lifecycle_chords_keep_one_owner_across_both_modes` | `Ctrl+s`/`Ctrl+r` in both modes; `U` is not a lifecycle chord |
| `every_insertion_entry_is_reachable_through_dispatch` | `i/a/I/A/o/O` → `EnterInsert`; refused on a read-only buffer |
| `modal_lines_open_and_unclaimed_keys_reach_the_composer` | `:`, `/`, `Esc`; ten unclaimed letters still `Unhandled` |
| `printable_characters_reach_the_document_in_insert_mode` | Work package D |
| `insert_mode_chords_still_reach_their_owners` | the pairing that keeps the sweep honest |

The fixture is 60 fixed-width lines, so every cursor assertion is arithmetic
(`line i` starts at byte `7 * i`) rather than a guess.

### Work package C — reachability guard

`scripts/check_tui_editor_key_authority.py`, wired into `scripts/verify.sh
quick` and the CI `verify` job (`.github/workflows/ci.yml`). It attributes
every chord to its dispatch stage and fails on two rules:

1. a mode table re-declares a chord an earlier stage already owns;
2. one table declares the same chord twice.

Design choices that make it load-bearing rather than decorative:

- An arm shape it **cannot resolve is an error (exit 2), never a pass.** A
  guard that skips input it does not understand passes by not looking.
- `handle_insert_key`'s `KeyCode::Char(ch)` catch-all is recorded as a
  **wildcard** claim over its modifier set rather than skipped, so a
  Normal-only guard that drifts into Insert is caught statically as well as at
  runtime.
- Guards are read as whole statements, not through a fixed sliding window: a
  window can hold `if key.code == Char('R')` and `&& key.modifiers == NONE`
  without the trailing `&& mode == ...::InputMode::Normal`, which reads as a
  mode-*independent* claim and produces a false positive.
- Non-vacuity: the three functions, the three `BufferKey` request variants, and
  all three tables must still exist and be non-empty.

### Work package D — Insert-mode printable-character sweep

`builtin_shift_letters_are_composed_text_in_insert_mode` in `src/tui/input.rs`
guards the **global** keymap only — a different surface with a different
failure mode. A binding added to `handle_normal_key` is invisible to it, so the
editor route needed its own proof. **This package added code; it did not
confirm existing coverage.**

`printable_characters_reach_the_document_in_insert_mode` walks every letter in
**three** encodings (lower/`NONE`, capital/`NONE`, capital/`SHIFT`) plus 13
punctuation marks — 91 assertions — and requires the document to have grown by
exactly that one character.

The capital-with-`NONE` encoding is the load-bearing one: `R` → reload is
Normal-only precisely because a legacy terminal sends `Char('R')` with `NONE`,
and a sweep testing only `Shift+R` would not notice that guard leaking into
Insert. `insert_mode_chords_still_reach_their_owners` is the pairing that stops
"Insert swallows everything" from passing the sweep.

## 3. Non-vacuity proofs

Every guard was observed failing against an injected defect, then reverted. No
injection is reported as evidence without its observed output.

### Work package B — the original defect, re-injected

Injected: the `U` arm replaced by `(KeyModifiers::CONTROL, KeyCode::Char('r'))`,
i.e. the pre-fix state.

```
test tui::commands::editor::tests::undo_and_redo_are_reachable_through_dispatch ... FAILED
assertion `left == right` failed
  left: Unhandled
 right: Consumed
test result: FAILED. 0 passed; 1 failed
```

`U` returned `Unhandled`, exactly the pre-fix symptom. Reverted.

### Work package C — four injections, and two real guard bugs found

| # | Injection | Observed |
|---|---|---|
| C1 | re-add `(KeyModifiers::CONTROL, KeyCode::Char('r')) => editor_redo` | exit 1: `claims CONTROL+'r', but the guard block at editor.rs:2557 already claims CONTROL+'r' in Normal — unreachable` |
| C2 | re-add the `\| (m, KeyCode::Char('d'))` alternative | exit 1: `re-declares NONE\|SHIFT+'d', already claimed at editor.rs:2711 … unreachable` |
| C3 | rewrite the `u` arm as `(modifier_set(m), KeyCode::Char('u'))` | exit 2: `unattributed Char arm at handle_normal_key line 2734` |
| C3b | rewrite the `u` arm as a bare `KeyCode::Char('u') =>` | exit 2: same |
| C4 | rename `fn handle_normal_key(` | exit 2: `fn handle_normal_key( not found` |

**C1 and C3 initially passed, which is the point of running them.** The first
draft of the guard had two blind spots, both of which would have made it a
claim rather than a guard:

1. Its alternative pattern could not parse `KeyModifiers::CONTROL` (the `::` was
   outside the character class), so **every control-chord arm was invisible** —
   including the exact arm this milestone is about. Fixed.
2. An unparseable `Char` arm was being skipped as "not a Char arm", so C3 was
   silently dropped rather than erroring. Fixed.

A third defect surfaced while iterating: a literal `(KeyModifiers::CONTROL, …)`
pattern was inheriting "all modifiers" from its (absent) predicate, which
produced false positives against the `NONE`-modifier `d` arm. Rust matches such
a pattern on exactly one modifier set, so literals are now resolved directly.

### Work package D — the guard leaking into Insert

Injected: removed `&& mode == crate::tui::input::InputMode::Normal` from the
`R` reload guard, making it mode-independent.

```
test tui::commands::editor::tests::printable_characters_reach_the_document_in_insert_mode ... FAILED
assertion `left == right` failed: R (capital, unshifted) must be text in Insert
  left: RequestReload
 right: Consumed
test result: FAILED. 0 passed; 1 failed
```

Caught on the capital-with-`NONE` spelling specifically. Reverted.

All injections reverted; the guard and the suite are green on the final tree
(`git diff` on `src/tui/commands/editor.rs` contains no injected arm).

## 4. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Redo reachable from a chord in Normal, through the dispatch path | `undo_and_redo_are_reachable_through_dispatch` (`u` → `U` → text restored, redo stack drained) | pass |
| Resync retains exactly one reachable chord in every mode | `lifecycle_chords_keep_one_owner_across_both_modes`; pre-existing `save_and_resync_are_requested_in_both_modes_and_reload_only_in_normal` | pass — `Ctrl+r` in both modes, unchanged |
| Every declared Normal-mode binding exercised through dispatch | the six `every_*`/`modal_lines_*` tests in Work package B | pass |
| Injecting a shadowed arm fails the suite | §3 C1 (guard) and B (test) | pass — both observed failing |
| Insert-mode sweep passes and is non-vacuous | `printable_characters_reach_the_document_in_insert_mode` (91 assertions, asserted count) + §3 D | pass |
| A read-only buffer still refuses insertion entries | `every_insertion_entry_is_reachable_through_dispatch` (read-only arm) | pass — no write path widened |
| Unclaimed keys still reach the composer | `modal_lines_open_and_unclaimed_keys_reach_the_composer` (10 letters `Unhandled`) | pass |
| `:`/`/` still outrank the mode-independent chords | pre-existing `esc_closes_the_command_line_without_running_anything` | pass |
| `R` stays Normal-only | pre-existing `save_and_resync_…` + sweep's capital assertions | pass |
| No mutation outside `DocumentController::apply_local` | `d` asserts `DeleteRange(0..14)` **and** that the text is unchanged; buffer still only reports effects | pass |
| Rendering and remote DTO bounds unchanged | no render or DTO code touched; `tui_render` 111 passed | pass |
| `docs/tui.md` / `architecture/tui.md` match the implemented chord set | §5 | pass |

## 5. Documentation updates

- `docs/tui.md` ("Editor language features"): states `u`/`U`, states that each
  chord has exactly one owner, and separates the three round-trip chords
  (`Ctrl+S`, `Ctrl+R`, `R`) from the mode verbs.
- `architecture/tui.md`: new subsection **"Key dispatch authority: ordered
  stages, one owner per chord"** recording the three-stage order, the rule, the
  defect, the chosen fix, and both enforcement mechanisms.
- `architecture/overview.md`: `CI guard scripts` 49 → **52**, verified against
  `ls scripts/check_*`. The documented 49 was already stale at baseline (51
  existed before this milestone); the count now matches its stated source, and
  the adjacent guard inventory sentence was updated.
- `CHANGELOG.md`: two `Fixed` entries (redo reachability; the dead `d`
  alternative).

### Correction to an earlier finding

An earlier pass of this research reported that *the help overlay* listed both
`Ctrl+R` (resync) and `Ctrl+R (redo)`, and treated it as user-visible evidence.
That was wrong on the second half. `src/tui/components/help_overlay.rs` is
**not** a live surface: it is absent from `components/mod.rs`, so it is never
compiled, and its `editor_bindings()` table is not rendered anywhere. The
shipped `/help` surface is `components::dialogs::help::HelpDialog`, fed by
`input::default_help_entries`, which lists global bindings only and names **no
editor chord at all**.

Consequences, applied rather than papered over:

- The edits made to `help_overlay.rs` during this milestone were **reverted**.
  They were out of scope (the file is not compiled), and the test added there
  would never have run — citing it as evidence would have been false.
- The `Ctrl+R`/`Ctrl+R (redo)` contradiction in that file is still real and does
  corroborate the shadow, but it is dead code, not a user-facing bug.
- `architecture/tui.md` now records explicitly where the editor keymap is
  published (`docs/tui.md` + the architecture table) and where it is **not**,
  naming the dead file so a future reader does not mistake it for the
  implemented keymap.

The authoritative user-facing keymap was updated and is verified by tests.

## 6. Restart, contention, and migration

**Not applicable**, stated explicitly rather than omitted:

- **Restart** — this milestone adds no durable state, no queue, no attempt, and
  no reconciliation path. Undo/redo history behaviour is unchanged (open,
  reload, resync, reconnect, close still clear it; save does not).
- **Contention** — the editor route remains single-threaded on the UI task. No
  new concurrency, no new shared state, no lock ordering.
- **Migration** — no storage layout, protocol, configuration, or persisted
  format change. Nothing to migrate and nothing to version.

## 7. Verification

All runs are **local** on this worktree. No hosted CI run was performed, so no
CI result is claimed.

| Command | Result |
|---|---|
| `python3 scripts/check_tui_editor_key_authority.py` | pass — 3 guard-block chords, 30 Normal-mode chords, 1 Insert-mode chord, no shadowed arm |
| `cargo test -p codegg --lib tui::commands::editor` | **53 passed, 0 failed** |
| `cargo test -p codegg --lib tui::input` | 49 passed, 0 failed |
| `cargo test -p codegg --test tui_main_buffer` | 44 passed, 0 failed |
| `cargo test -p codegg --test tui_render` | 111 passed, 0 failed |
| `cargo fmt --check --all` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `scripts/verify.sh quick` | **passed** (exit 0) — every guard plus `cargo check --workspace --all-targets --locked` (`Finished dev profile … in 2m 00s`) |

Not run: `cargo nextest run --workspace` (`verify.sh full`). It is a broad
sweep and no focused target in this milestone's blast radius failed; it remains
the maintainer's pre-handoff gate.

## 8. Scope note

One change sits slightly outside the plan's literal text and is recorded here
rather than buried: the dead `Char('d')` alternative in the `D` arm was removed.
The plan said the milestone "adds no key the user did not already have" and was
about reachability; this is the same defect class (a declared alternative that
can never match), the new guard reports it as a violation, and leaving it would
have meant shipping a guard that fails on the tree it was written for. No
behaviour changed: bare `d` still deletes the held selection, and `D` still
deletes to end of line in both capital encodings.

## 9. Registry update and unblock audit

- `plans/registry.md` §"Dependency-ready implementation plans": the M001 row
  moves from *ready for handoff* to **closed**, citing this record.
- §"Blocked work": M002's stated hard dependency (M001) is satisfied, so M002 is
  unblocked and moves to *dependency-ready*. **M003 stays blocked** — its hard
  predecessor is M002, which is not closed.
- Unblock audit: M002 depends on M001 only. M001's own prerequisites were
  ADR-0011 and ADR-0021, both accepted and untouched, so the dependency edge is
  discharged with no residual condition. No other row changes status.
- M009's ADR-0034 gate is unaffected and still open.

## 10. Residual findings not acted on

Recorded so they stay owned rather than being rediscovered later:

1. **`src/tui/components/help_overlay.rs` is unreferenced dead code.** It still
   advertises `Ctrl+R (redo)` and a `dd dw` row for bindings that do not exist.
   Deleting it, or wiring it up and making it the real help surface, is a
   separate decision — it is not part of key dispatch authority.
2. **The shipped `/help` lists no editor chords at all.** A user cannot discover
   `u`/`U`, `space y`, `gg`, or the `:`/`/` lines from `/help`; only `docs/tui.md`
   carries them. Publishing editor chords in `default_help_entries` is a genuine
   gap, and out of scope here.
3. **`D` is a non-Helix binding retained deliberately** (documented at its arm),
   and `x` then `d` is the Helix gesture. Noted so a future Helix-parity pass
   does not "fix" `D` into something else without a decision.
4. **`handle_editor_key`'s Normal-only pane gestures can shadow buffer chords.**
   `Shift+L`/`Shift+H` are the live instance and are deliberate. This stage sits
   above the chain the new guard covers; extending the guard to it would require
   resolving the global binding table and `is_normal_only`, which is a larger
   scope than M001 authorised.