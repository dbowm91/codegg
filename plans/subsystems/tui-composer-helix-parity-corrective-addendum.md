# TUI Composer Helix Keybinding Parity — Post-Closure Corrective Addendum

Status: closed — C001 closed

Parent roadmap:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md`

Historical milestones preserved:

- M001 closed: `plans/closure/tui-composer-helix-parity/001-status.md`
- M002 closed: `plans/closure/tui-composer-helix-parity/002-status.md`

Long-term references:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-session-tui`
- `plans/003-planning-process.md`

Related ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19, with the
  `space`-chord amendment) — governs the composer's claim set. This addendum does **not** change
  ADR-0042's decision; it corrects a false statement about the app's binding maps that ADR-0042's
  decision rests on.

## 1. Corrective purpose

M002 shipped the composer's Normal-mode claim table and closed on an exit condition that was not
true in the app's default configuration.

M002's exit conditions include: *"`h` and `l` still move the composer caret, and `j`/`k` still
scroll the transcript while the composer buffer is a single line."* The `j`/`k` half held. The
`h`/`l` half held **only in the vim map**.

The composer does not claim `h` and `l`, and that is correct: ADR-0042's decision table records
them as *"No conflict. The existing action already moves the composer caret, which is Helix `h` and
`l`. The composer keeps it."* The premise is that the app already provides the Helix horizontal
motions. It provides them in `vim_bindings_internal()` and it did **not** provide them in
`default_bindings_internal()` — and `vim_mode` defaults to `false`
(`src/tui/app/mod.rs`, the config's `vim_mode` is `Option<bool>` read with `unwrap_or(false)`).

The consequence is a composer that shows a caret it cannot move. In Normal mode the composer
paints a solid block at the caret (M001). The only letters that could move it are `h` and `l`, and
in the default map both are unbound: `h` and `l` fell through to nothing, while `w`, `b` and `e`
are claimed by the composer and consumed as pending verbs under the fallthrough rule. The user
is left with a visible caret, a `q` that quits, a `g` that goes to the top, and no letter that
moves the caret.

This addendum corrects that, and adds the machine check that keeps the correction true.

## 2. Discovered corrective finding

### The composer's caret motions existed in one keymap only

| Configuration | `h` | `l` | Source |
|---|---|---|---|
| vim map | `InputAction::Left` | `InputAction::Right` | `src/tui/input.rs`, `vim_bindings_internal()` |
| default map | *unbound* | *unbound* | `src/tui/input.rs`, `default_bindings_internal()` |

Both facts are structural, not incidental: the default map's bare letters are `space` (help),
`/` (focus-prompt), `?` (help), `g` (go-to-top), `j` and `k` (transcript navigation) and `q`
(quit), and neither `h` nor `l` is among them.

### Why M002's verification did not catch it

M002's closure record carries a requirement-to-evidence matrix with eleven acceptance criteria
(AC1–AC11). The matrix is thorough about the claim table itself — the fallthrough rule, the
re-spelled chords in both maps, the capital-letter encodings, the conditional `j`/`k` pair, the
Insert-mode text rule, the panel and dialog seams. It is thorough **about the claim table**.

The false exit condition was not about the claim table. It was about a letter the table
deliberately does *not* claim, and the evidence for it was a statement about the binding maps
rather than a test over them. Three specific gaps:

1. **The acceptance matrix had no row for that exit condition.** AC7 covers the *adjacent*
   exit condition — `j`/`k` still scroll the transcript on a single-line composer — and it
   passes. The `h`/`l` exit condition had no acceptance-criterion row at all, so the matrix
   could not report it.
2. **The guard polices the table against the maps, and says nothing about unclaimed letters.**
   `scripts/check_tui_composer_key_authority.py` checks that a *claimed* letter cannot reach an
   app-level action (G2) and that a re-spelled action is bound in both maps (G4). `h` and `l`
   are not claimed and are not re-spelled, so every existing rule passed over them. A false
   premise expressed in prose *about* the maps is invisible to a guard that reads the *table*
   and the maps and compares them to each other.
3. **The map-level test was about re-spelled chords.** `the_re_spelled_chords_reach_their_
   actions_in_both_maps` proved that the three re-spelled capabilities are reachable in both
   maps. It says nothing about the capabilities the claim table *assumes* are already there.

### The survey that produced the premise

The parent roadmap's §4 records the baseline as: *"In the main buffer's Normal mode, `h` and `l`
already move the composer caret (through the app's `Left` / `Right` actions), so those two are not
conflicts."* The same paragraph is careful about the keymap difference one sentence later — *"`d`
is open-diff **in the vim map**"* — so the distinction was understood and simply not applied to
`h` and `l`. The premise was never checked against both maps; it was checked against the map
the author had in mind.

## 3. Work classification

### Invariants

- **C-I1 — A caret motion is not a keymap flavour.** If a key moves the composer caret in one
  built-in map, it moves the composer caret in the other, to the same action. The composer is a
  Helix-parity surface; a capability that appears and disappears with the keymap configuration
  is a capability the user does not have.
- **C-I2 — The claim table's premises are machine-checked.** A letter the table does not claim
  because the app already owns it must be bound in **every** map, and the check must fail when
  that stops being true.

### Capabilities

- **C-C1 — The composer caret is movable by letter in the default configuration.** `h` and `l`
  move it, in both maps, in the main buffer's Normal mode.

### Infrastructure

- **C-N1 — The composer's key-authority guard reads the binding maps' non-character key codes.**
  Rule G9 compares the two maps' caret motions by key identity, which requires the guard's
  parser to carry the key code instead of discarding it.

### Polish

- **C-P1 — The parent roadmap's decision table reflects the decisions that were made.** D3
  (the blinking cursor's source) is recorded as open and owned by M001, which closed. The
  decision was settled by invariant I7, and the table was never updated.

## 4. Non-goals

- **No composer verb.** This addendum adds no Helix verb, no motion, no selection, and no undo.
  `w`, `b`, `e`, `v`, `x`, `d`, `p`, `u`, `U` and `o` stay exactly as M002 left them: claimed,
  reserved, consumed. Motion is M004, selection and clipboard are M005, undo/redo and the insert
  transitions are M006.
- **No claim-table change.** The table keeps its 13 rows, its statuses and its encodings. The
  table's *content* was right; the *premise* behind two of its exclusions was not.
- **No ADR-0042 supersession.** ADR-0042's decision — the composer claims a closed letter set,
  the fallthrough rule holds, `h`/`l` are not conflicts — is unchanged and remains correct. What
  was wrong was a statement about the binding maps, and the fix makes the statement true rather
  than changing the decision.
- **No editor, terminal-panel, or keymap-flavour change.** The editor and the interactive
  terminal panel own their own key authority and are untouched. The vim map is untouched.
- **No new cadence, timer, or animation.** The Insert-mode caret's blink remains the terminal's
  own cursor, exactly as M001 recorded and as invariant I7 requires.

## 5. Current state

- `default_bindings_internal()` binds no bare `h` or `l`; `vim_bindings_internal()` binds both to
  `InputAction::Left` and `InputAction::Right`.
- `vim_mode` defaults to `false`, so the default map is the map a user gets without configuring
  anything.
- `InputAction::Left` and `InputAction::Right` are the composer's caret motions: they reach
  `TuiMsg::CursorLeft` / `TuiMsg::CursorRight`, which move `PromptWidget`'s caret. The arrow keys
  are bound to the same two actions in both maps, so binding the letters adds no new action and
  no new code path.
- `InputAction::Left` and `InputAction::Right` are not listed in `is_normal_only`, and do not
  need to be: a bare printable character is skipped by the Insert-mode bare-character rule, so
  `h` and `l` remain ordinary text while composing.
- The guard's `Binding` record carried the character but **discarded** the non-character key
  code, so it could not tell `KeyCode::Left` from `KeyCode::Home`.
- The parent roadmap's §10 decision table records D3 — the blinking cursor's source — as open,
  with the owner "Maintainers, in M001". M001 closed with the decision settled by invariant I7.

## 6. Target architecture

The correction is one binding in one map, plus the check that keeps it true:

- `h` and `l` are the composer caret's horizontal motions in **both** built-in maps, bound to the
  same two actions the arrow keys use.
- The composer's claim table is unchanged, and its exclusion of `h`/`l` is now backed by a
  fact that holds in every configuration.
- The guard reads each map's full key identity and fails when the two maps disagree about a
  caret motion.

## 7. Milestones

### C001 — Normal-mode caret motions in every map

**Class:** capability (C-C1), with an invariant (C-I1, C-I2) and infrastructure (C-N1).

**Objective.** Make `h` and `l` move the composer caret in the default map, and make the two
maps' caret motions a machine-checked invariant rather than an assumption written in prose.

**Dependencies.** None. M001 and M002 are closed; the fix touches one map and one guard.

**Deliverable boundary.** The two bindings, the guard's key-code-aware parser and rule G9, the
regression tests, and the documentation corrections. No composer verb, no claim-table change, no
other surface.

**Exit conditions.**

- `h` and `l` are bound to the caret's left and right motions in **both** built-in maps.
- A regression test presses `h` and `l` in the main buffer's Normal mode in both maps and
  asserts the caret moved.
- The guard's rule G9 fails when a caret motion exists in one map and not the other, and when
  the same key is bound to different actions in the two maps; the guard's `--self-test` proves
  both rejections.
- The guard fails on the pre-correction tree, which is the evidence that the rule is armed
  rather than merely present.
- `h` and `l` remain ordinary text in Insert mode in both maps.
- The parent roadmap's §4 baseline statement, its M002 exit condition, the M002 plan, the
  M002 closure record and the architecture doc state the corrected fact, and ADR-0042's decision
  is unchanged.
- The parent roadmap's D3 row records the decision that was actually made.

## 8. Cross-cutting requirements

- **Storage and migration.** None. The correction is a binding table and a guard.
- **Protocol and compatibility.** None. The change is confined to the frontend's Normal-mode
  key resolution.
- **Security and authorization.** None. The composer remains a frontend-local string; the
  correction changes no submit path and no authority boundary.
- **Concurrency, cancellation, and recovery.** None. No new state and no new task.
- **Observability and audit.** None. The change is a table entry and a static check.
- **Documentation and static guards.** `scripts/check_tui_composer_key_authority.py` gains rule
  G9 and its self-test cases. The parent roadmap, the M002 plan, the M002 closure record and
  `architecture/tui.md` are corrected. `docs/tui.md` gains the two keys if it documents the
  composer's Normal-mode keys.
- **Performance and resource use.** None. The guard reads two source files at lint time.

## 9. Verification strategy

- `python3 scripts/check_tui_composer_key_authority.py` — green on the corrected tree.
- `python3 scripts/check_tui_composer_key_authority.py --self-test` — every violation class
  detected, including the three new G9 cases.
- The guard run against the **pre-correction** `input.rs` — must fail with the two `h`/`l`
  findings, which is the evidence that G9 is armed.
- `cargo test --test tui_composer_claims` — the claim table's tests plus the new caret-motion
  tests.
- `cargo test --test modifier_chords` and `cargo test --test tui_presentation_regressions` —
  the two suites that read the built-in maps.
- `scripts/verify.sh quick` — the canonical gate.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` on the repository's minimum
  toolchain and on the CI toolchain.

## 10. Risks and decision points

- **R-C1 — The correction changes what an unclaimed letter resolves to.** M002's plan §4 states
  the invariant *"a letter the composer does not claim must resolve to the same app action it
  resolves to today, in the same mode, with the same preconditions."* In the default map `h`
  and `l` resolved to *no action*; they now resolve to the caret motions. The invariant protects
  against losing an existing app action, and nothing is lost here — but the deviation is real
  and is recorded rather than absorbed. The same reasoning is already recorded for M002's `i`
  deviation and for the default map's bare `/`.
- **R-C2 — A user who bound `h` or `l` to something else keeps their binding.** `build_bindings`
  applies the user's overrides after the built-in map, so an explicit user binding still wins.
  The change only fills a hole in the default map.
- **R-C3 — G9 is stricter than the defect.** G9 requires the two maps to agree on *every* caret
  motion, not only on `h` and `l`. That is the intent: a caret motion that exists in one
  configuration is the same defect class with a different letter.
- **R-C4 — The correction is invisible to a user who never leaves Insert mode.** It is a
  Normal-mode change, and Normal mode is reached with `Esc` (or by the surfaces that force it).

### Decision points

| Point | Status | Owner |
|---|---|---|
| C-D1 — Does the correction bind the letters in the default map, or does the composer claim `h`/`l` and own the motion itself? | **Decided in C001**: the default map binds them. ADR-0042's decision table already records `h`/`l` as "no conflict — the existing action already moves the composer caret", and claiming them would change ADR-0042's decision and pull M004's motion scope into a corrective. | Maintainers, in C001 |
| C-D2 — Is the Insert-mode caret's blink the terminal's own or app-driven? | **Settled by the parent roadmap's invariant I7**, which the M001 plan applied: the terminal's own cursor. The parent roadmap's decision table still records D3 as open; C001 closes that row. Not reopened here. | Maintainers, in C001 |

## 11. Completion definition

This addendum is complete when:

- `h` and `l` move the composer caret in the main buffer's Normal mode in both built-in maps,
  and a regression test proves it.
- The guard fails when a caret motion drifts between the maps, and its self-test proves the
  rejection.
- No document states the corrected premise as false, and no document states the true premise as
  false.
- ADR-0042's decision is unchanged and the parent roadmap's M001 and M002 remain closed.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | **closing** | `plans/implementation/tui-composer-helix-parity-corrective/001-normal-mode-caret-motion-premise.md` | `plans/closure/tui-composer-helix-parity-corrective/001-status.md` | none |

**Reading this table.** `closed` means a closure record was accepted. `closing` means the
implementation landed and the closure evidence is being gathered. `not started` means the
milestone has not been taken. The parent roadmap's M001–M007 statuses are unchanged by this
addendum, and the addendum adds no milestone to the parent workstream: C001 corrects M002's
evidence and M002's premise, and it does not re-open M002.
