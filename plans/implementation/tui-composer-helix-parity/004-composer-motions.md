# TUI Composer Helix Parity Milestone 004 — Composer Motions

Status: implemented — closed at `plans/closure/tui-composer-helix-parity/004-status.md`

Repository baseline: `3cad45929` (branch `feat/auto-20261019-8cc543fb`)

Source roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md#m004--composer-motions`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19) — the
  composer's claim set. This milestone implements verbs for keys the ADR already gave to the
  composer (`w`, `b`, `e`, `j`, `k`); it **claims no new key** and changes no decision.

Primary class: capability (roadmap C1, navigability).

## 1. Objective

Make the composer's caret navigable **by word** and **by line** in Normal mode, through the
composer's own key claims, using the geometry the editor already shares with it.

The five verbs are Helix's: `w` to the start of the next word, `b` to the start of the previous
word, `e` to the end of the word at the caret, `j` down a line, `k` up a line.

## 2. Why this milestone is ready

- **Both hard dependencies are closed.** M002 (the claim table the motions are reached through)
  closed at `plans/closure/tui-composer-helix-parity/002-status.md`; M003 (the geometry and the
  caret model) closed at `plans/closure/tui-composer-helix-parity/003-status.md`. The corrective
  C001 closed on top of them.
- **The keys already exist and are already owned by the composer.** `w`, `b`, `e`, `j` and `k` are
  five of the thirteen rows of `COMPOSER_CLAIMS`, each with its verb named and its status
  `Reserved`. This milestone flips five statuses and implements five verbs. It adds **no** claim
  and claims **no** new key.
- **The geometry is settled and shared.** M003 moved the word and line geometry into
  `src/tui/text_geometry.rs` behind a `TextSource` trait, and the composer already implements
  that trait. The word boundaries this milestone moves to are therefore *the same rule* the editor
  moves by, not a second rule that agrees today.
- **The targets are already computed.** `ComposerText` already computes the word targets
  (`word_forward`, `word_backward`, `word_end`) and the shared module already computes the
  vertical target (`move_vertical`). What is missing is the *motion*: the step that puts the caret
  on the target.
- **Independently shippable and invisible until pressed.** No rendering change, no new key, no
  protocol, no stored state. The only visible effect is a caret that moves.

## 3. Current implementation evidence

At the baseline:

- **The claim table has thirteen rows and exactly one live verb.** `COMPOSER_CLAIMS`
  (`src/tui/composer_claims.rs:267-374`) lists `b` (`:268`), `d` (`:276`), `e` (`:282`), `i`
  (`:289`), `j` (`:311`), `k` (`:318`), `o` (`:326`), `p` (`:332`), `u` (`:339`), `U` (`:346`),
  `v` (`:353`), `w` (`:360`) and `x` (`:367`). Twelve are `ClaimStatus::Reserved`; only `i` is
  `Live` (`:307`).
- **`j` and `k` are conditional.** Their condition is `ClaimCondition::MultiLineBuffer`
  (`:316`, `:323`), which ADR-0042 part 4 requires: on a single-line buffer they fall through to
  transcript scroll unchanged.
- **The dispatch gate already exists and already consumes reserved claims.**
  `src/tui/app/mod.rs:4024-4041` resolves the claim before the binding map is consulted, and
  `run_composer_claim` (`src/tui/app/mod.rs:12807-12841`) runs a live verb or consumes the key
  as a deliberate no-op. The fallthrough rule is therefore already true for all thirteen keys —
  what is missing is behaviour behind five of them.
- **`run_composer_claim` implements one verb.** Its `Live` arm handles `ComposerVerb::InsertBefore`
  (`:12811-12817`) and logs a warning for any other live verb (`:12824-12827`). Flipping a claim
  to `Live` without adding the behaviour would trip that warning, which is the guard the runner
  already carries.
- **The word targets exist on the model.** `ComposerText::word_forward`
  (`src/tui/composer_text.rs:366`), `word_backward` (`:371`) and `word_end` (`:376`) delegate to
  `src/tui/text_geometry.rs`. They return an **offset**; none of them moves the caret.
- **The vertical target exists in the shared module.** `text_geometry::move_vertical`
  (`src/tui/text_geometry.rs:262`) moves an offset by a number of lines, preserving the column
  where the target line allows and clamping to the last line. `ComposerText` has **no** vertical
  motion; `ComposerText` implements `TextSource` (`src/tui/composer_text.rs:516`), so the shared
  function is callable against the model directly.
- **The horizontal motions already work, and are not the composer's.** `h` and `l` are bound in
  both binding maps (C001) and reach `TuiMsg::CursorLeft` / `CursorRight`
  (`src/tui/app/input.rs:793-794`) → `PromptWidget::cursor_left` / `cursor_right`
  (`src/tui/components/prompt.rs:178-182`) → `ComposerText::move_left` / `move_right`
  (`src/tui/composer_text.rs:342-353`). ADR-0042 part 4 records them as **no conflict**: the
  composer keeps the existing actions and does not claim them. This milestone leaves them alone.
- **`Home` and `End` are app-level keys.** They are bound in both maps to `InputAction::Home` /
  `End` and move the caret to the **buffer** start and end (`src/tui/components/prompt.rs:186-192`
  → `ComposerText::move_home` / `move_end`, `src/tui/composer_text.rs:356-363`). They are not
  composer claims.
- **The caret's visibility is the render layer's job.** `src/tui/app/render.rs:2064-2066` calls
  `ensure_cursor_visible_with_width` before painting, so a motion that moves the caret past the
  visible area scrolls the composer without this milestone touching the scroll code.
- **The scroll offsets are not the motions' problem, but the multi-line buffer is real.** The
  composer accepts newlines (`PromptWidget::insert_newline`, `src/tui/components/prompt.rs:166`),
  so `j`/`k` have something to move between.

## 4. Invariants that must not regress

- **I1 — the claim set is fixed.** ADR-0042 part 2 and the roadmap's §3 fix the composer's claim
  set; this milestone adds no row, removes no row, and changes no key. The table still has
  thirteen rows.
- **I2 — a claimed key never falls through.** Unchanged. `w`, `b`, `e`, `j` and `k` are consumed
  by the composer in Normal mode whether or not the motion had anywhere to go.
- **I3 — the composer is a frontend-local string.** Unchanged. A motion touches the model's
  buffer and nothing else; no daemon call, no revision, no document.
- **I5 — the resolved mode is the gate.** Unchanged. The claim is live only when
  `App::composer_holds_keyboard()` and `App::composer_input_mode() == Normal`. The raw
  `UiState::input_mode` field is still never the gate.
- **I7 — one redraw cadence.** Unchanged. Nothing here touches rendering or timing; the caret
  moves and the next frame paints it.
- **The `j`/`k` conditional is preserved.** On a single-line buffer both keys still fall through
  to transcript scroll. This is ADR-0042 part 4's explicit requirement, and the existing tests
  assert it.
- **Every claimed letter is still typed as text in Insert mode.** The Insert-mode sweep over the
  claim set must keep passing unchanged.
- **No rendering change.** The widget draws what it drew. A motion moves the caret; the caret
  cell is computed from the caret by the existing code.
- **The editor is unchanged.** Its geometry functions keep their signatures and their results; the
  shared module is not modified.
- **The terminal panel is unchanged.** Its whitespace-delimited token rule is a different rule on
  purpose.

## 5. Scope

**In scope.**

- The composer's **moving** word motions: `w`, `b`, `e`.
- The composer's **vertical** motions: `j`, `k`.
- The five claim statuses flipped from `Reserved` to `Live`, and the runner's behaviour for each.
- The dispatch-reachability tests that prove each motion is reachable by pressing its key through
  the app's real key path.

**Out of scope.**

- **Any new claim or key.** `0`/`$` for line start/end are not in this milestone: the claim set
  is fixed (roadmap §3), and the two available routes — a new claim, or re-pointing the
  app-level `Home`/`End` from the buffer's start/end to the line's start/end — each change an
  ownership boundary or a key's existing behaviour, and M004's own exit condition forbids the
  latter. The gap is recorded as decision **D7** in the roadmap's §10 rather than decided here.
- **Selection verbs** (`v`, `x`, `d`) — M005.
- **Undo, redo and insert transitions** (`u`, `U`, `i`, `o`) — M006.
- **The paste verb** (`p`) — M005.
- **The copy gesture** (`space` then `y`) — M005. ADR-0042 part 7 gives that gesture to the help
  box, not to the composer.
- **Any rendering change**, including the roadmap's P1 hint row and the C7 help-surface work.
- **Any change to the editor's, the terminal panel's or the dialogs' keymaps.**
- **Multi-selection.** The shared `SelectionState` holds a single `Option<Selection<P>>`; the
  composer gets one selection, matching the terminal panel and the editor's current model.

## 6. Required production changes

- **`src/tui/composer_text.rs`.** Add the moving motions. The natural shape is one method per
  verb, each computing the target through the shared geometry and then moving the caret:
  `move_word_forward`, `move_word_backward`, `move_word_end`, `move_line_down`, `move_line_up`.
  Each must move the caret through `ComposerText::move_caret` — the path that calls the shared
  machine's `move_to` — and **not** through `set_caret`, because "moving the cursor extends a
  held character selection" is a rule of the shared machine and M005's selection depends on it.
  `move_line_down` and `move_line_up` call `text_geometry::move_vertical(self, caret, ±1)`.
- **`src/tui/components/prompt.rs`.** Add the widget's public surface for the five motions,
  named consistently with the existing `cursor_left` / `cursor_right` / `cursor_home` /
  `cursor_end`: `cursor_word_forward`, `cursor_word_backward`, `cursor_word_end`,
  `cursor_line_down`, `cursor_line_up`. The existing public API is preserved; no method is
  removed or renamed.
- **`src/tui/composer_claims.rs`.** Flip `w` (`:360`), `b` (`:268`), `e` (`:282`), `j` (`:311`)
  and `k` (`:318`) from `ClaimStatus::Reserved` to `ClaimStatus::Live`. No other field of any
  row changes — in particular the `j`/`k` condition stays `MultiLineBuffer` and every row's
  encodings stay as they are.
- **`src/tui/app/mod.rs`.** `run_composer_claim` (`:12807-12841`) gains the five verbs. The
  existing `Live` arm's warning (`:12824-12827`) is the check that a live verb has behaviour, so
  the five verbs must be added there, not elsewhere.
- **No other production file changes.** In particular `src/tui/input.rs`, the binding maps, the
  guard script, `src/tui/text_geometry.rs` and `src/tui/editor.rs` are untouched.

## 7. Ordered work packages

**WP1 — The model's moving motions.** Five methods on `ComposerText`. The targets come from the
shared geometry; the caret move goes through `move_caret`. A motion with nowhere to go computes
the current caret's own target and leaves the caret unchanged — `move_vertical` already returns
the same offset at the first and last lines, and `word_forward`/`word_backward` already return
the clamped position at the buffer's ends, so the "nowhere to go" case falls out of the shared
functions rather than needing a special case here.

**WP2 — The widget's surface.** Five thin delegations on `PromptWidget`. The widget owns
presentation, the model owns the buffer; a motion is a model call, not a widget computation.

**WP3 — The claim statuses.** Five `Reserved` → `Live` flips, with a comment at each that names
the verb it now runs. The claim table is the one place that says who owns a key, so the comment
belongs beside the row rather than in the runner.

**WP4 — The runner.** `run_composer_claim` implements the five verbs. Each verb is a call into
the widget's method; the runner owns no geometry.

**WP5 — Dispatch-reachability tests.** The roadmap requires each motion to be "verified by a
dispatch-reachability test rather than by calling a helper". The tests in
`tests/tui_composer_claims.rs` already have the machinery for this: `test_app()`, `press(app,
code, modifiers)` driving `App::on_key`, `normal_mode(app)` and `prompt_text(app)`
(`tests/tui_composer_claims.rs:41-64`). Each motion is tested by **pressing the key** and
asserting the caret's new position — not by calling `ComposerText::move_word_forward` and
asserting its return value.

**WP6 — Verify.** The model's tests, the claim tests, the cursor tests, the guard, and the
workspace gates.

## 8. Failure, cancellation, restart, contention semantics

The motions are frontend-local values with no I/O, no background task and no durable state, so
there is nothing to fail, cancel or contend over.

Three semantics are load-bearing and are stated here because they are the ones a later milestone
depends on:

- **A motion with nowhere to go leaves the caret unchanged and still consumes the key.** This is
  the fallthrough rule applied to a live verb: `w` at the end of the buffer is the composer's
  key, so it does not become some other app action.
- **A motion moves the caret through the selection-extending path.** `move_caret` calls the
  shared machine's `move_to`, which grows a held character selection and disarms line-mode
  growth. A motion that used `set_caret` instead would silently break M005's selection.
- **`j`/`k` are conditional on the buffer's line count.** The condition is evaluated at the
  dispatch gate (`src/tui/app/mod.rs:4028-4030`), against the composer's current line count, so
  a buffer that becomes multi-line *while* the composer is in Normal mode changes the keys'
  ownership without any other state changing.

## 9. Compatibility and migration

- **Migration.** None. Nothing is stored, and no schema or protocol changes.
- **Compatibility.** The composer renders and behaves as before except that five keys now move
  the caret in Normal mode — which is the point of the milestone. The widget's public API is
  preserved; every existing method keeps its semantics.
- **Insert mode is untouched.** Every claimed letter is still typed as text, and the Insert-mode
  sweep over the claim set must keep passing.
- **The app-level `h`/`l` path is untouched.** The composer keeps the existing actions for those
  keys, per ADR-0042 part 4.
- **The editor and the terminal panel are untouched.**

## 10. Required tests

- **`src/tui/composer_text.rs`**: `w` lands on the start of the next word; `b` on the start of
  the previous word; `e` on the end of the word at the caret; `j` and `k` move a line and keep the
  column where the target line allows; a motion at the buffer's first and last lines leaves the
  caret unchanged; a motion never lands inside a multi-byte character; a motion extends a held
  character selection (the `move_caret` path, not the `set_caret` path).
- **`tests/tui_composer_claims.rs`**:
  - the five claims are `Live` and the other eight are unchanged;
  - **dispatch reachability** — pressing `w`, `b`, `e`, `j` and `k` in Normal mode moves the
    caret to the expected place, through `App::on_key`;
  - the Insert-mode sweep over the claim set still passes;
  - `j`/`k` still fall through to transcript scroll on a single-line buffer, and are claimed on
    a multi-line buffer;
  - a claimed key still cannot reach an app-level action.
- **`tests/tui_composer_cursor.rs`**: the painted caret cell follows the caret after a motion,
  and the composer scrolls to keep a moved caret visible.
- **`scripts/check_tui_composer_key_authority.py`**: unchanged and passing, plus its
  `--self-test`. The guard polices ownership, not behaviour, so flipping five statuses must not
  change its verdict.
- The editor's suites and the terminal panel's suites, unchanged and passing.

## 11. Required verification commands

```bash
cargo test --lib tui::composer_text
cargo test --lib tui::text_geometry
cargo test --test tui_composer_claims --test tui_composer_cursor --test tui
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_composer_key_authority.py --self-test
scripts/verify.sh quick
```

## 12. Documentation updates

- `src/tui/composer_claims.rs` — the module doc's "Why almost every entry is reserved" section
  and the comments on the five flipped rows. After this milestone the table has six live verbs,
  not one, and the doc's account of why only `i` was live is no longer the whole truth.
- `src/tui/composer_text.rs` — the module docs for the new motion methods, and the note that a
  motion moves the caret through the selection-extending path.
- `architecture/tui.md` — the composer's Normal-mode motions.
- `docs/tui.md` — the user-facing section for the composer's Normal-mode keys, which already
  exists for the caret motions and gains the word and line motions.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md` — M004's status, the §10 decision
  table (D7), and the §12 status table.
- `plans/registry.md` — M004's row and the dependency audit.

## 13. Acceptance criteria

- **AC-M1** — pressing `w` in Normal mode with the composer holding the keyboard moves the caret
  to the start of the next word, through the app's real key path.
- **AC-M2** — pressing `b` moves the caret to the start of the previous word, and pressing `e`
  moves it to the end of the word at the caret, through the app's real key path.
- **AC-M3** — pressing `j` and `k` moves the caret a line up and down, keeping the column where
  the target line allows, and **only** when the composer buffer has more than one line; on a
  single-line buffer both keys still scroll the transcript.
- **AC-M4** — a motion with nowhere to go leaves the caret unchanged and consumes the key.
- **AC-M5** — the composer's word boundaries are the shared ones: the motion lands on exactly the
  offset `src/tui/text_geometry.rs` computes for the same text, and the shared policy function
  remains the only word-segmentation rule.
- **AC-M6** — the claim table still has thirteen rows, the five motion claims are `Live`, and the
  other eight are unchanged; no key was added and none was removed.
- **AC-M7** — every claimed letter is still typed as text in Insert mode, and the Insert-mode
  sweep passes.
- **AC-M8** — a motion moves the caret through the shared machine's selection-extending path, so
  a held character selection grows with the motion.
- **AC-M9** — the widget's public API is intact, the composer renders as before, and the editor
  and the terminal panel are unchanged.
- **AC-M10** — the workspace gates are green: `verify.sh quick`, clippy on the repository's
  minimum toolchain and on the CI toolchain, and the composer guard with its self-test.

## 14. Stop conditions

Report rather than improvise if:

- **A motion needs a key the composer does not own.** The claim set is fixed by ADR-0042 and the
  roadmap's §3, and adding a key is a decision this milestone does not own. Report it; do not
  widen the table.
- **A motion cannot be expressed through the shared geometry** without changing the geometry's
  results for the editor. The shared functions are the contract with the editor, and a change to
  them is a change to a surface this milestone does not own.
- **The `j`/`k` conditional cannot be preserved** — for instance if making `j` live would force
  the keys to be claimed on a single-line buffer. ADR-0042 part 4 requires the fall-through, and
  the existing tests assert it.
- **A motion needs a second caret or a second selection state.** The composer has one caret and
  one selection, held by the shared machine; a second copy is the drift class M003 was written to
  remove.
- **Any verification command fails for a reason this milestone did not cause**: report the failure
  and its evidence rather than widening the change.

## 15. Closure evidence required

- The focused test runs, with their counts.
- `scripts/verify.sh quick` output.
- The clippy gate on both the repository's minimum toolchain and the CI toolchain.
- The composer guard's output and its `--self-test` output.
- A requirement-to-evidence matrix mapping AC-M1…AC-M10 to the evidence above.
- The registry update and the dependency audit — in particular, whether closing M004 unblocks
  M005.

## 16. Handoff notes

- **The claim table is the whole surface.** A motion is reached by pressing its key; there is no
  second path to it, and the plan's tests press the key rather than call the method. A test that
  calls `ComposerText::move_word_forward` directly proves the arithmetic and nothing about
  reachability.
- **The primitives were built in M003 on purpose.** `ComposerText` computes the word targets and
  the shared module computes the vertical target; this milestone adds the step that moves the
  caret. If a target has to be computed differently here, the shared function is the thing to
  change — and changing it changes the editor's answers too, which is a decision to report.
- **`move_caret`, not `set_caret`.** The difference is invisible until M005 exists, and then it is
  the difference between a selection that grows and one that does not.
- **The claim table's doc comment is now partly history.** It explains why only `i` was live;
  after this milestone six verbs are live, and the doc has to say so.
- **Nothing here touches the daemon, storage, or the protocol.** The composer stays a
  frontend-local string.
