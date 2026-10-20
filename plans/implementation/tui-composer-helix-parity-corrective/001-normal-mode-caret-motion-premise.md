# TUI Composer Helix Parity Corrective Milestone 001 — Normal-mode Caret Motion Premise

Status: implemented

Repository baseline: `2b44be918` (branch `feat/auto-20261019-8cc543fb`)

Source roadmap: `plans/subsystems/tui-composer-helix-parity-corrective-addendum.md#c001--normal-mode-caret-motions-in-every-map`

Parent roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19) — governs
  the composer's claim set. This milestone does not change its decision.

Primary class: capability (the composer's caret is movable by letter in the default
configuration), with an invariant (the two maps must agree on caret motions) and infrastructure
(the guard's key-code-aware parser)

## 1. Objective

Make `h` and `l` move the composer caret in the **default** binding map, and turn the premise the
composer's claim table rests on — that the app already provides Helix's horizontal motions — into
a machine-checked invariant instead of a statement in prose.

## 2. Why this milestone is ready

- **No hard dependencies.** M001 and M002 are closed. The change touches one binding map, one
  guard script, one test file, and the documents that state the premise.
- **Stable interface dependencies.** `InputAction::Left` and `InputAction::Right` are the
  composer's existing caret motions, already bound to the arrow keys in both maps. The
  correction adds no action, no dispatch arm and no state.
- **Independently shippable.** It changes what a letter resolves to in one configuration. It
  adds no Helix verb, and it does not depend on M003, M004 or any other open milestone.
- **The decision is settled.** ADR-0042's decision table already records `h`/`l` as "no
  conflict — the existing action already moves the composer caret, which is Helix `h` and `l`".
  The milestone makes that decision's premise true in the map where it was false; it does not
  re-open the decision.

## 3. Current implementation evidence

- `default_bindings_internal()` (`src/tui/input.rs`) binds no bare `h` and no bare `l`. Its bare
  letters are `space`, `/`, `?`, `g`, `j`, `k` and `q`.
- `vim_bindings_internal()` binds `(NONE, 'h') → InputAction::Left` and
  `(NONE, 'l') → InputAction::Right`.
- `vim_mode` is read as `cfg.and_then(|c| c.vim_mode).unwrap_or(false)`, so the default map is
  the map a user gets with no configuration at all.
- `InputAction::Left` / `InputAction::Right` reach `TuiMsg::CursorLeft` / `TuiMsg::CursorRight`,
  which call `PromptWidget::cursor_left()` / `cursor_right()`.
- Both maps already bind `KeyCode::Left` / `KeyCode::Right` (and `Home` / `End`) to those same
  actions, so the letters and the arrow keys resolve identically.
- `InputAction::Left` and `InputAction::Right` are **not** in `is_normal_only`, and do not need
  to be: a bare printable character is skipped by the Insert-mode bare-character rule, so `h`
  and `l` stay ordinary text while composing.
- The guard's `Binding` record carried the character and **discarded** the non-character key
  code, so it could not tell `KeyCode::Left` from `KeyCode::Home`.
- M002's closure matrix (`plans/closure/tui-composer-helix-parity/002-status.md` §2) has rows for
  AC1–AC11 and none for the "`h` and `l` still move the composer caret" exit condition.

## 4. Invariants that must not regress

- **I1 — one owner per chord per surface.** `h` and `l` are owned by the app in the main
  buffer's Normal mode, and are not claimed by the composer. The claim table's 13 rows, their
  statuses and their encodings are unchanged.
- **I2 — a claimed key never falls through.** No claimed letter's reachability changes.
- **I5 — the resolved mode is the gate.** Unchanged.
- **I7 — one redraw cadence.** Unchanged; nothing here touches rendering or timing.
- **A user binding still wins.** `build_bindings` applies `KeybindConfig` overrides after the
  built-in map, so a user who bound `h` or `l` keeps their binding.
- **The editor and the terminal panel are untouched.** They own their own key authority, and
  their key paths are consulted before the binding map.
- **The composer's Insert-mode text is untouched.** `h` and `l` remain ordinary characters while
  composing.

## 5. Scope

**In scope.**

- The two bindings in `default_bindings_internal()`.
- The guard's parser (carry the non-character key code) and its new rule G9, with self-test
  cases and a run against the pre-correction tree.
- The regression tests: one at the map level, one at the dispatch level.
- The documentation corrections listed in §12.

**Out of scope.**

- Any composer verb. `w`, `b`, `e`, `v`, `x`, `d`, `p`, `u`, `U` and `o` stay claimed and
  reserved.
- The claim table's content, statuses, encodings or conditions.
- ADR-0042, whose decision is unchanged.
- The editor, the interactive terminal panel, the side panels, the dialogs, the Workspace
  dashboard, the command palette and the help box's own keymap.
- The vim map, which already binds both letters.
- The Insert-mode caret's blink, which remains the terminal's own cursor under I7.
- M003's text model, M004's motions, M005's selection and clipboard, M006's undo/redo and
  insert transitions, and M007's consistency sweep.

## 6. Required production changes

- `src/tui/input.rs` — `default_bindings_internal()`: bind `(NONE, 'h')` to
  `InputAction::Left` and `(NONE, 'l')` to `InputAction::Right`.
- `scripts/check_tui_composer_key_authority.py` — carry the non-character key code on the
  `Binding` record; add rule **G9** (the two maps must bind each caret motion to the same key
  and the same action); add the three G9 self-test cases; extend the success message.
- `tests/tui_composer_claims.rs` — the map-level premise test and the dispatch test.

## 7. Ordered work packages

**WP1 — Bind the letters in the default map.** Two `map.insert` calls, with a comment that
records why the asymmetry existed and what it cost. The comment names the defect, because a
binding whose reason is invisible is a binding the next reader will "fix" back.

**WP2 — Make the guard able to see a caret motion's key.** Extend the guard's `Binding` record
with the non-character key code and populate it in the three places the parser builds a
non-character binding. A guard that cannot name the key it is reasoning about reasons about
keys it cannot distinguish.

**WP3 — Add rule G9.** For every key bound to a caret-motion action in one map, the other map
must bind the same key to the same action. The rule is derived from the maps, not from a list
of letters, so it holds for any caret motion.

**WP4 — Prove the rule is armed.** Run the guard against the pre-correction `input.rs` and
record the two findings. A rule that has never been shown to fail is a claim, not a check.

**WP5 — Prove the behaviour.** A map-level test that both maps bind both letters, and a
dispatch test that presses `h` and `l` in the main buffer's Normal mode in both maps and
asserts the caret moved. The dispatch test is the one that matters: a binding that exists but
is shadowed would satisfy the map-level test and still leave the caret stuck.

**WP6 — Correct the documents.** See §12. The correction is that the premise is now true; the
documents that state it stop being false, and the ones that record the *baseline* say so
explicitly rather than silently changing what the baseline was.

**WP7 — Close the stale decision row.** The parent roadmap's D3 row records the blinking
cursor's source as open and owned by M001, which closed. I7 settled it: the terminal's own
cursor. The row is updated to record the decision and where it was made.

## 8. Failure, cancellation, restart, contention semantics

None. The change is a table entry, a static check and a test. There is no runtime state, no
background task, no durable record and no restart path. The guard is a lint-time check over two
source files and has no runtime behaviour.

## 9. Compatibility and migration

- **Migration.** None. Nothing is stored.
- **Compatibility.** In the default map, `h` and `l` in the main buffer's Normal mode move the
  composer caret, where previously they did nothing. That is the entire user-visible change, and
  it is the change the app's own documentation already described.
- **Users with an explicit binding.** A `KeybindConfig` entry for `h` or `l` is applied after the
  built-in map and still wins. A user who has bound `h` to something else keeps it.
- **The vim map.** Unchanged; both letters were already bound there.

## 10. Required tests

- `tests/tui_composer_claims.rs::the_caret_motions_are_bound_the_same_way_in_every_map` — the
  map-level premise, in both maps.
- `tests/tui_composer_claims.rs::h_and_l_move_the_composer_caret_in_normal_mode` — the
  behaviour, in both maps, in the main buffer.
- `tests/tui_composer_claims.rs::the_composer_claims_neither_g_h_nor_l` — unchanged in
  substance; its doc comment is corrected to say "in every map".
- `tests/tui_composer_claims.rs::every_claim_table_letter_is_typed_as_text_in_insert_mode` — the
  existing Insert-mode text sweep, which must still pass with the new bindings.
- `scripts/check_tui_composer_key_authority.py --self-test` — the three G9 cases.
- The guard run against the pre-correction tree — the two `h`/`l` findings.
- The existing suites that read the built-in maps: `tests/modifier_chords.rs`,
  `tests/tui_presentation_regressions.rs`, `tests/tui_project_picker.rs`.

## 11. Required verification commands

```bash
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_composer_key_authority.py --self-test
cargo test --test tui_composer_claims
cargo test --test modifier_chords
cargo test --test tui_presentation_regressions
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

- `src/tui/input.rs` — the comment on the two new bindings.
- `src/tui/composer_claims.rs` — the module doc's `h`/`l` sentence: it now states the fact that
  holds in every map, and names the check that keeps it true.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md` — §4's baseline statement (it records
  the baseline, so it must say which part of it was wrong and where it was corrected); the M002
  exit condition (it now holds, and the note says when and how); the §10 D3 row.
- `plans/implementation/tui-composer-helix-parity/002-normal-mode-key-ownership-contract.md` —
  the two places that state the `h`/`l` premise, and the §4 invariant, which the correction
  deviates from and must record.
- `plans/closure/tui-composer-helix-parity/002-status.md` — a factual correction recording that
  the exit condition was not met at closure, why the matrix had no row for it, and where the
  correction landed. The record is otherwise immutable.
- `architecture/tui.md` — the `h`/`l` sentence, which is now true in both maps, plus a pointer to
  the guard rule.
- `docs/tui.md` — the composer's Normal-mode keys, if that document lists them.
- `plans/registry.md` — the addendum's row and the dependency audit.

## 13. Acceptance criteria

- **AC-C1** — `h` and `l` are bound to the caret's left and right motions in **both** built-in
  maps, and a test asserts it.
- **AC-C2** — pressing `h` and `l` in the main buffer's Normal mode moves the composer caret,
  in both maps, and a test asserts the caret's position.
- **AC-C3** — the guard fails when a caret motion exists in one map and not the other, when the
  same key is bound to different actions in the two maps, and when a non-character caret key
  drifts; its `--self-test` proves all three rejections.
- **AC-C4** — the guard run against the pre-correction tree reports the two `h`/`l` findings.
- **AC-C5** — `h` and `l` are still ordinary text in Insert mode in both maps.
- **AC-C6** — the claim table is unchanged: 13 rows, one `Live` verb (`i`), no row added,
  removed or re-statused.
- **AC-C7** — no document states the corrected premise as false, and the documents that record
  a baseline say which part of it was wrong.
- **AC-C8** — the parent roadmap's D3 row records the decision that was made and where.
- **AC-C9** — `scripts/verify.sh quick` is green, and the workspace clippy gate is green on
  both the repository's minimum toolchain and the CI toolchain.

## 14. Stop conditions

Report rather than improvise if:

- The two maps cannot be made to agree on a caret motion without changing the vim map's
  behaviour — the vim map is the one that already worked, and changing it is a different
  decision.
- Binding `h`/`l` in the default map changes a surface the milestone did not consider (the
  editor route, a focused panel, the Workspace dashboard's filter) in a way that is not the
  same behaviour the arrow keys already have.
- The correction requires a second animation cadence, a timer, or any change to how the caret
  is drawn — I7 forbids it, and the correction does not need it.
- Any of the verification commands fails for a reason this milestone did not cause: report the
  failure and its evidence rather than widening the change.

## 15. Closure evidence required

- The guard's plain run and its `--self-test` output.
- The guard's run against the pre-correction tree, with its two findings.
- The focused test runs, with their counts.
- `scripts/verify.sh quick` output.
- The clippy gate on both toolchains.
- A requirement-to-evidence matrix mapping AC-C1…AC-C9 to the evidence above.
- The registry update and the dependency audit.

## 16. Handoff notes

- The composer is a frontend-local string. Nothing here touches the daemon, the editor
  document, the storage layer or the protocol.
- The change to `default_bindings_internal()` is two lines plus a comment. The comment matters:
  it is the only thing that will stop the next reader from removing the bindings as
  "duplicates of the arrow keys".
- G9 is deliberately stricter than the defect it was written for. If a future milestone wants a
  caret motion that exists in one map only, G9 is where that has to be argued, rather than
  discovered by a user.
- The correction does not make the composer feel like Helix. `w`, `b` and `e` are still claimed
  and reserved, and they land with M004, behind M003.
