# TUI Composer Helix Parity Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-composer-helix-parity-corrective/001-normal-mode-caret-motion-premise.md`

Source corrective addendum:

- `plans/subsystems/tui-composer-helix-parity-corrective-addendum.md` (C001)

Parent roadmap:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md`

Repository baseline reviewed: `2b44be918` (branch `feat/auto-20261019-8cc543fb`)

Implementation commits or pull requests:

- *uncommitted at the time of writing* — the working tree carries the two bindings, the guard's
  key-code-aware parser and rule G9, the regression tests, the documentation corrections, this
  record, and the addendum's own plan. No hosted CI run is claimed.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **accepted** 2026-10-19. This
  milestone does not change its decision. It corrects a false statement *about the binding maps*
  that the decision's `h`/`l` row rests on, and the fix makes that statement true.

## 1. Executive finding

The composer's Normal-mode caret is now movable by letter in **every** configuration the app can
be configured with, and the premise the composer's claim table rests on — that the app already
provides Helix's horizontal motions — is now a machine-checked invariant rather than a statement
in prose.

At the baseline, `h` and `l` moved the composer caret in the **vim map only**. The default map
bound neither, and `vim_mode` defaults to `false`, so in the app's default configuration a
Normal-mode composer painted a solid block at a caret that **no letter could move**: `h` and `l`
were unbound, while `w`, `b` and `e` are claimed by the composer and consumed as pending verbs
under the fallthrough rule. M002's exit condition "`h` and `l` still move the composer caret"
was therefore false when M002 closed, and M002's closure matrix had no acceptance-criterion row
for it, which is why it closed green.

The fix is two bindings. The evidence that the fix cannot rot is a guard rule: **G9** fails if
the two built-in maps ever disagree about a caret motion, and the guard's own `--self-test`
proves the rejection.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| **AC-C1 — `h` and `l` are bound to the caret's horizontal motions in both maps** | `tests/tui_composer_claims.rs::the_caret_motions_are_bound_the_same_way_in_every_map`; guard rule G9 (both maps agree on every caret motion) | pass | `default_bindings_internal()` now binds `(NONE, 'h') → InputAction::Left` and `(NONE, 'l') → InputAction::Right`, the same two actions the arrow keys already use. |
| **AC-C2 — pressing `h` and `l` in Normal mode moves the caret** | `tests/tui_composer_claims.rs::h_and_l_move_the_composer_caret_in_normal_mode` | pass | Exercised through `App::on_key` in the main buffer in Normal mode, in both maps. The caret's byte position is asserted, not a side effect. |
| **AC-C3 — the guard rejects a caret motion that drifts between the maps** | `scripts/check_tui_composer_key_authority.py --self-test`: "a caret motion bound in the vim map only", "the same caret key bound to a different action in the two maps", "an arrow caret motion bound in the vim map only" | pass | The third case needs the guard's parser to carry the **non-character key code**, which it previously discarded — a binding with no character and no code could not be told apart from any other non-character key. |
| **AC-C4 — the guard fails on the pre-correction tree** | The guard run against `2b44be918:src/tui/input.rs` with the current `composer_claims.rs` | pass | Two findings: the vim map binds `'h'` under NONE to `Left` and the default map does not bind that key at all; the same for `'l'`/`Right`. This is the evidence the rule is armed rather than merely present. |
| **AC-C5 — `h` and `l` remain ordinary text in Insert mode** | `tests/tui_composer_claims.rs::every_claim_table_letter_is_typed_as_text_in_insert_mode`; `InputAction::Left`/`Right` are not in `is_normal_only` | pass | A bare printable character is skipped by the Insert-mode bare-character rule, so the letters stay ordinary text while composing. |
| **AC-C6 — the claim table is unchanged** | `src/tui/composer_claims.rs` — 13 rows, one `Live` verb (`i`); `tests/tui_composer_claims.rs::the_claim_table_is_not_empty_and_has_no_duplicate_letters` | pass | No row added, removed or re-statused. The table's *premise* changed; the table did not. |
| **AC-C7 — no document states the corrected premise as false** | The corrections in §9 | pass | The parent roadmap's §4 and M002's exit condition, the M002 plan, the M002 closure record, `architecture/tui.md` and the `composer_claims.rs` module doc. ADR-0042 needed no correction: its decision is unchanged and its `h`/`l` row is now true. |
| **AC-C8 — the parent roadmap's D3 row records the decision that was made** | `plans/subsystems/tui-composer-helix-parity-roadmap.md` §10 | pass | D3 (the blinking cursor's source) was **settled in M001** by invariant I7 — the terminal's own cursor. The row said "Open" with the owner "Maintainers, in M001", a milestone that had closed. |
| **AC-C9 — the gates are green** | §4 | pass | `verify.sh quick` green; workspace clippy green on 1.89.0 (the repository's minimum) and on 1.99.0 (the CI toolchain); `cargo fmt --all --check` clean. |

## 3. Production implementation evidence

### 3.1 The two bindings — `src/tui/input.rs`

`default_bindings_internal()` binds `(NONE, 'h') → InputAction::Left` and
`(NONE, 'l') → InputAction::Right`, immediately after the `j`/`k` transcript-navigation
bindings. The comment on them records why the asymmetry existed and what it cost, because a
binding whose reason is invisible is a binding the next reader will "fix" back.

Nothing else in the app changed. `InputAction::Left` and `InputAction::Right` are the composer's
existing caret motions — they reach `TuiMsg::CursorLeft` / `TuiMsg::CursorRight`, which call
`PromptWidget::cursor_left()` / `cursor_right()` — and both maps already bound the arrow keys and
`Home`/`End` to the same two actions. The letters add no action, no dispatch arm and no state.

### 3.2 The guard's rule G9 — `scripts/check_tui_composer_key_authority.py`

**The parser change.** The guard's `Binding` record carried the character and **discarded** the
non-character key code, so it could not tell `KeyCode::Left` from `KeyCode::Home`. G9 reasons
about caret motions, and the arrow and Home/End keys *are* caret motions, so the record now
carries the key code and exposes a `key_id` (modifiers, character, key code) for comparison.

**The rule.** For every key bound to a caret-motion action in one map, the other map must bind
the same key to the same action. The rule is **derived from the maps**, not from a list of
letters: it holds for every caret motion, not only for the two this milestone happened to find.
A caret motion that exists in one configuration and not the other is a capability the user has
or does not have depending on which keymap they picked.

**The self-test.** Three cases: a caret motion bound in the vim map only; the same key bound to
different actions in the two maps; and a non-character caret key (an arrow) bound in one map
only. All three are detected.

### 3.3 The regression tests — `tests/tui_composer_claims.rs`

- `the_caret_motions_are_bound_the_same_way_in_every_map` — the map-level premise, in both maps.
- `h_and_l_move_the_composer_caret_in_normal_mode` — the behaviour, through `App::on_key` in the
  main buffer's Normal mode, in both maps. A binding that exists but is shadowed by a surface
  that owns the keyboard first would satisfy the map-level test and still leave the caret stuck,
  so the behaviour is asserted directly.

## 4. Verification executed

All commands were run locally in the worktree. **No hosted CI run is claimed for this
milestone.**

| Command | Result |
|---|---|
| `python3 scripts/check_tui_composer_key_authority.py` | pass — "the claim table is closed, single-owner, leaks no key to an app-level action, and every caret motion is bound the same way in both maps" |
| `python3 scripts/check_tui_composer_key_authority.py --self-test` | pass — 15 cases, 15 "ok", including the three new G9 cases |
| The guard run against `2b44be918:src/tui/input.rs` | **2 violations**, as expected: the vim map binds `'h'`/`'l'` to `Left`/`Right` and the default map does not bind those keys at all |
| `cargo test --test tui_composer_claims` | **29 passed**, 0 failed (27 before this milestone, plus the two new tests) |
| `cargo test --test modifier_chords` | 20 passed, 0 failed |
| `cargo test --test tui_presentation_regressions` | 24 passed, 0 failed |
| `cargo test --test tui_project_picker` | 17 passed, 0 failed |
| `cargo test --test tui_composer_cursor` | 22 passed, 0 failed |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (1.89.0) | pass |
| `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` (1.99.0, the CI toolchain) | pass |
| `scripts/verify.sh quick` | pass |

**Not run, and why.** `scripts/verify.sh full` is not green on this machine for a reason that
predates this milestone and is documented in M002's closure record: its nextest sweeps fail on 59
tests that register a workspace at `/tmp`, which macOS makes a symlink. The failure set is
identical at the baseline commit `61cbe9222`, so it is the environment, not the tree. Hosted CI
is the environment where that suite is expected to pass.

## 5. Invariant review

| Invariant | Result | Note |
|---|---|---|
| I1 — one owner per chord per surface | pass | `h` and `l` have exactly one owner in the main buffer: the app's binding map. The composer does not claim them. The claim table is unchanged. |
| I2 — a claimed key never falls through | pass | The claim table and the guard are unchanged in this respect; G9 is an additional rule, not a relaxation. |
| I5 — the resolved mode is the gate | pass | Unchanged. `h`/`l` are app-level bindings, so they resolve through the same `InputMode` gate as every other app action. |
| I7 — one redraw cadence | pass | Nothing here touches rendering, timing or the cursor. The Insert-mode caret remains the terminal's own cursor. |
| A user binding still wins | pass | `build_bindings` applies `KeybindConfig` overrides after the built-in map. A user who bound `h` or `l` keeps their binding. |
| The editor's and the terminal panel's keymaps are unchanged | pass | Neither surface's files were modified. The editor and the terminal panel are consulted before the binding map, and both own their own key authority. |
| Insert mode still types bare letters | pass | AC-C5. |

## 6. Failure and recovery review

None. The change is a binding-table entry, a static check and a test. There is no runtime state,
no background task, no durable record, no restart path and no contention surface. The guard is a
lint-time check over two source files.

## 7. Migration and compatibility review

- **Migration.** None. Nothing is stored, and no schema or protocol changes.
- **Compatibility.** In the default map, `h` and `l` in the main buffer's Normal mode move the
  composer caret, where previously they did nothing. That is the entire user-visible change, and
  it is the change the app's own documentation already described.
- **The invariant this deviates from.** M002's plan §4 states that "a letter the composer does
  not claim must resolve to the same app action it resolves to today". In the default map `h`
  and `l` resolved to *no action*; they now resolve to the caret motions. The invariant protects
  an existing app action from being changed or taken, and nothing is lost here — but the
  deviation is real, it is recorded in the M002 plan itself, and it is recorded here.
- **The vim map.** Unchanged. Both letters were already bound there.

## 8. Security review

None. The composer remains a frontend-local string delivered to the daemon only on submit. The
change adds no authority, no path handling, no network surface and no state. It cannot change
what the daemon accepts, because it does not touch the submit path.

## 9. Documentation and operations

| Document | Change |
|---|---|
| `src/tui/input.rs` | The two bindings, with the comment that records why the asymmetry existed and what it cost. |
| `src/tui/composer_claims.rs` | The module doc's `h`/`l` sentence now names the check that keeps it true, and says what was wrong. |
| `architecture/tui.md` | The `h`/`l` sentence, plus the premise's failure mode and rule G9. |
| `docs/tui.md` | A new "Moving the caret in `[NOR]` mode" section: `h`/`l`, the arrows, `Home`/`End`, and an explicit statement that `w`/`b`/`e` are spoken for until their milestone lands. |
| `plans/subsystems/tui-composer-helix-parity-roadmap.md` | §4's baseline statement, with a correction note; M002's exit condition, with a correction note; §10's D3 row, closed with the decision that was actually made. |
| `plans/implementation/tui-composer-helix-parity/002-normal-mode-key-ownership-contract.md` | The two places that state the `h`/`l` premise, and §4's invariant, which the correction deviates from. |
| `plans/closure/tui-composer-helix-parity/002-status.md` | A factual correction at the head of the record, and the missing acceptance-criterion row in §2's matrix. The record is otherwise immutable. |
| `plans/subsystems/tui-composer-helix-parity-corrective-addendum.md` | New. |
| `plans/implementation/tui-composer-helix-parity-corrective/001-normal-mode-caret-motion-premise.md` | New. |
| `plans/registry.md` | The addendum's row, the parent row's status and correction note, and the dependency audit. |

**No ADR was written or changed.** ADR-0042's decision — the composer claims a closed letter
set, the fallthrough rule holds, `h`/`l` are not conflicts — is unchanged and remains correct.
What was wrong was a statement about the binding maps, and the fix makes the statement true
rather than changing the decision.

## 10. Unresolved findings

| Severity | Finding | Owner |
|---|---|---|
| low | The parent roadmap's §4 baseline paragraph still reads as a description of the baseline, with the correction in a callout beneath it. A reader who skips the callout reads the false premise. | Acceptable: rewriting the baseline paragraph in place would misstate what the baseline was, and the callout is the honest shape. |
| low | `h`/`l` are not in the help overlay's Normal-mode entries. The overlay lists the app's chords, and the composer's own verbs land with M004–M006. | M004–M006: the help entries for the composer's verbs belong with the verbs. |

No high or medium finding remains.

## 11. Roadmap disposition

- The parent roadmap's M001 and M002 remain **closed**. C001 does not re-open M002: it corrects
  M002's evidence and the premise behind one of its exclusions, and M002's decision stands.
- The parent roadmap's M003–M007 statuses are unchanged. M003 remains **eligible for fresh
  planning** and is not blocked by this milestone.
- The parent roadmap's §10 D3 row is closed with the decision M001 actually made.
- This addendum's C001 is **closed**. The addendum has no further milestones.

## 12. Registry updates

- Added the addendum's row to **Active subsystem roadmaps**, and the parent roadmap's row now
  records the correction and the stale D3 row's resolution.
- Added the C001 row to **Dependency-ready implementation plans** with this closure record.
- **Dependency audit.** C001 closed no blocker: no registered plan listed M002's `h`/`l` exit
  condition as a dependency, and M003's only hard dependency (none) is unaffected. The
  `Milestones eligible for fresh planning` row for M003 is unchanged and still accurate.
