# TUI Composer Helix Parity Milestone 002 — Normal-mode Key Ownership Contract

Status: implemented — closed at `plans/closure/tui-composer-helix-parity/002-status.md`

**One scoping deviation, recorded in the closure (§1.1).** §5 and §16 put "no insert
transition" out of scope and said "do not add verbs here". The `i` claim's verb is **live**
in this milestone, because `i` is the app's **only** route back to Insert mode: it is the
`None` arm of the key chain, not a binding. A reserved `i` would consume the key, do
nothing, and leave the user in Normal mode with no way to type. The verb that runs is the
app's **existing** transition, moved into the table so the table is the one place that says
who owns the key; M006 still owns the insert transitions *beyond* `i`.

**One reading recorded in the closure (§3.3.1).** The default map's bare `/` → `FocusPrompt`
**stays**. §4's invariant — "a letter the composer does not claim must resolve to the same
app action it resolves to today" — binds it, `/` is not a claimed letter, and it is the
gesture that enters command mode (and therefore the only way back into the panel manager
from a focused panel). Removing it broke
`tests/tui_side_panels.rs::the_slash_command_survives_inside_a_focused_panel`; the binding is
restored, and the guard's rule for a re-spelled action was corrected with it.

Repository baseline: `61cbe9222` (branch `feat/auto-20261019-8cc543fb`)

Source roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md#m002--normal-mode-key-ownership-contract`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19, with the
  `space`-chord amendment that keeps help on `space` and puts the copy verb in the help box that
  chord opens)

Primary class: invariant (one owner per chord, no fallthrough from a claimed key), with
infrastructure (the composer key-ownership guard)

## 1. Objective

Establish the composer's closed Normal-mode claim table, the rule that a claimed key never falls
through to an unrelated app action, and the re-spellings for the app-level verbs that conflict
with it — then ship the static guard that polices the table, before any Helix verb is added to it.

This milestone adds no text-manipulation behaviour. It establishes *who owns which key*, which is
the decision every later verb milestone depends on and the decision that is cheapest to get right
before the table exists and cheapest to fix after it does.

## 2. Why this milestone is ready

**It is ready.** Both of its former blockers are discharged:

- **ADR-0042 was accepted on 2026-10-19**, with one amendment: `space` keeps opening the help
  menu instead of becoming the Helix prefix, and the copy verb becomes the second step of that
  chord — `y` in the help box that `space` opens. The claim table, the fallthrough rule, and the
  re-spelling requirement are that record's decision, and a proposed ADR confers no implementation
  authorization, so the acceptance was the outstanding action.
- **M001 closed** (`plans/closure/tui-composer-helix-parity/001-status.md`). The claim table needs
  the keyboard-ownership derivation to know when a claim is live, and the guard must be able to
  distinguish "the composer is not holding the keyboard" from "the composer leaked the key". M001
  produced that derivation: `App::composer_holds_keyboard`, with `App::panel_owns_keyboard`
  extracted out of `App::on_key` so the key chain and the caret rules read one derivation.

With those discharged, this milestone has no remaining hard dependency and
no unresolved design question: the conflict table is enumerated in the ADR, the rule is stated, and
the exact chords are the only implementation choice left, and it is a product choice the ADR
assigns to this milestone.

**Interface dependencies are stable.**

- The global binding maps (`src/tui/input.rs`) are the established mechanism for every app-level
  chord, and both the default and the vim map are in place.
- `is_normal_only` (`src/tui/input.rs`) is the established mechanism that keeps a bare letter from
  being swallowed as text in Insert mode.
- `default_help_entries` (`src/tui/input.rs`) is the established single source of truth for the
  help surface, and the audit `documented_keys_resolve_in_the_active_map` already enforces that
  every advertised key resolves in an active map.
- The editor's key-authority guard (`scripts/check_tui_editor_key_authority.py`) is the
  established precedent for what a TUI key guard looks like, and
  `scripts/check_tui_subagent_name_pool_isolation.py` is the established precedent for a TUI guard
  that carries a `--self-test`.

## 3. Current implementation evidence

- The composer's Normal mode is the app's global Normal mode, and it currently has no composer
  verbs of any kind.
- `InputMode` is `Insert` or `Normal` and lives as a single global field on `UiState`. In Insert,
  bare characters are text; in Normal, they resolve to app-level actions.
- **The letters already spoken for in the main buffer's Normal mode:**

  | Letter | Existing owner | Location |
  |---|---|---|
  | `h` | `InputAction::Left` → `TuiMsg::CursorLeft` → `PromptWidget::cursor_left()` | `src/tui/input.rs`, `src/tui/app/input.rs` |
  | `l` | `InputAction::Right` → `TuiMsg::CursorRight` → `PromptWidget::cursor_right()` | `src/tui/input.rs`, `src/tui/app/input.rs` |
  | `j` | `InputAction::NavigateDown` (transcript scroll) | `src/tui/input.rs` |
  | `k` | `InputAction::NavigateUp` (transcript scroll) | `src/tui/input.rs` |
  | `x` | `InputAction::ToggleAgentRun` (stop/resume the turn) | `src/tui/input.rs`; also listed in `is_normal_only` |
  | `d` | `InputAction::OpenDiff` | `src/tui/input.rs` (vim map) |
  | `space` | `InputAction::Help` | `src/tui/input.rs` (both maps) — **not a conflict**: help keeps `space`; the composer does not claim it |
  | `g` | `InputAction::GoToTop` | `src/tui/input.rs` (both maps) |
  | `i` | `InputAction::FocusPrompt` | `src/tui/input.rs` (vim map) |

  `h` and `l` are **not** conflicts: the existing actions already move the composer caret, which
  is exactly Helix `h` and `l`.
- **The letters that are free:** `e`, `b`, `v`, `o`, `u`, `U`, `p`, `w`, and the rest of the
  alphabet.
- The app's key dispatch consults the binding map first, then the default map, and otherwise falls
  through to a `None` arm that handles a small number of unclaimed keys. A composer claim table
  must be consulted *before* that fallthrough, or the app-level binding wins.
- The interactive terminal panel already faces this exact problem and solves it by claiming `d`
  unconditionally, so that an unclaimed `d` cannot silently become `OpenDiff`. Its dispatch
  comment records the failure mode verbatim.
- `is_normal_only` is the list of actions that exist only in Normal mode. Any new action whose
  only spelling is a bare letter must be listed there, or Insert mode will swallow it as text.
- `is_builtin_text_key` compares against **both** built-in maps, so a letter claimed in one map
  but not the other is still treated as text in Insert mode.
- **Capital letters have two encodings.** `insert_panel_direction_bindings` in `src/tui/input.rs`
  binds `H`, `J`, `K`, `L`, `S`, and `F` under `SHIFT` **and** under `NONE`, because some
  terminals report a shifted key without the `SHIFT` modifier. A composer claim on an uppercase
  letter must follow the same convention.
- **Bare capital letters once swallowed typed text.** A capital letter bound under the unshifted
  encoding was consumed in Insert mode, so typing it opened a panel instead of inserting a
  character. The fix was made at the key, in the Insert-mode text-key guard, rather than by
  removing the binding, and `builtin_shift_letters_are_composed_text_in_insert_mode` sweeps both
  keymaps to assert the property and its non-vacuity. A composer claim table must not reintroduce
  the defect.
- The help audit requires every advertised key to resolve in `DEFAULT_BINDINGS` or `VIM_BINDINGS`
  under the matching mode, and requires the action's label to appear in the help prose. A
  composer verb that lives only in a bespoke table will fail this audit unless the table is
  reachable from a map.
- `scripts/check_tui_editor_key_authority.py` targets the editor's command module only. A
  composer claim table is invisible to it. None of the three TUI editor guards currently carry a
  `--self-test`; `scripts/check_tui_subagent_name_pool_isolation.py` does, and is the precedent
  for adding one.

## 4. Invariants that must not regress

- **I1 — one owner per chord per surface.** Every key in the main-buffer surface has exactly one
  owner. This is the property the guard exists to enforce.
- **I2 — a claimed key never falls through.** A claimed letter is consumed by the composer even
  when the composer's verb is currently a no-op. It must not silently become an unrelated app
  action.
- **I5 — the resolved mode is the gate.** Claims are live only when the *resolved* mode is Normal
  and the composer holds the keyboard. The raw `UiState::input_mode` field is never the gate.
- **No app-level capability is removed.** The re-spellings move turn stop/resume, open-diff, and
  focus-prompt to new chords. The capabilities themselves remain, and remain
  reachable. Help does **not** move: `space` keeps opening it.
- **The editor's and the terminal panel's keymaps are unchanged.** Nothing in this milestone may
  alter a binding that those surfaces rely on, except where a re-spelling is required in the main
  buffer and the change is recorded in the ADR.
- **Insert mode still types bare letters.** A letter in the composer's claim table must still be
  typed as text in Insert mode. The existing guarantee — a bare character in Insert mode is typed
  rather than interpreted as an app action — must hold for every letter in the table.
- **Unclaimed letters behave exactly as before.** A letter the composer does not claim must
  resolve to the same app action it resolves to today, in the same mode, with the same
  preconditions.
- **No authority change.** Nothing here alters turn admission, tool authority, or what the daemon
  accepts.
- **The single-cadence rule.** No timer, poll interval, or animation is introduced.
- **The composer's text model is untouched.** This milestone adds no selection and no undo.

## 5. Scope

### In scope

- The composer's claim table: a single, closed, enumerable mapping from claimed letters to
  composer verbs.
- The fallthrough rule: a claimed letter is consumed by the composer even when its verb is a
  no-op.
- The app-level chords that must be re-spelled, and their new spellings, in both binding maps.
- The `is_normal_only` and `is_builtin_text_key` updates that keep Insert mode typing bare
  letters.
- The help surface entries for the re-spelled chords and for the composer's claim set.
- A composer key-ownership guard under `scripts/`, wired into `scripts/verify.sh quick` and CI,
  with a `--self-test` that injects violations and proves rejection.
- Tests for the ownership rule, the fallthrough rule, and the re-spellings.

### Out of scope

- Any text-manipulation behaviour: no motion, no selection, no delete, no yank, no paste, no
  undo, no redo, no insert transition. Those are M004, M005, and M006.
- Any change to the composer's text, selection, or undo model. That is M003.
- Any change to the composer's cursor rules. That is M001.
- Any change to the editor's or the terminal panel's keymaps.
- Any change to the transcript's navigation behaviour beyond the `j`/`k` rule in the ADR.
- Making the composer's claim table user-configurable. Explicitly deferred.
- Multi-selection, text objects, match mode, and goto mode. Explicitly out of scope for the whole
  workstream.
- **The copy verb's implementation.** ADR-0042 part 7 places it in the help box the `space` chord
  opens, and it acts on a composer selection that does not exist until M005. This milestone only
  guarantees that the composer claims neither `space` nor `y`, leaving the copy gesture's landing
  place unchanged.
- Generalising the editor's key-authority guard to cover sibling surfaces. This milestone adds a
  composer-scoped guard.

## 6. Required production changes

**Core.** No domain types change.

**Storage.** None.

**Protocol.** None.

**Runtime.** The app's key-dispatch chain gains a composer claim step, consulted before the global
binding map, active only when the composer holds the keyboard and the resolved mode is Normal.

**Frontend.**

- `src/tui/input.rs`: the re-spelled app-level chords in both binding maps; the
  `is_normal_only` and `is_builtin_text_key` updates; the help entries for the re-spelled chords.
- A composer claim table and its dispatch step, placed where the composer can reach it without
  reaching into the app's private state. The terminal panel's dispatch and the editor's
  `BufferKey` consumed/unhandled pattern are the two precedents; the composer's table is its own
  surface with its own boundary.
- `src/tui/app/mod.rs`: the dispatch step in the key chain, using the M001 ownership derivation.

**Security.** None. No text is copied, transmitted, or logged by this milestone.

**Docs.** See section 12.

## 7. Ordered work packages

### WP1 — Conform to the accepted ADR-0042

*Intent.* Make the implementation match the decision as it was actually accepted.

*Required changes.*

- ADR-0042 was **accepted on 2026-10-19**, with the `space`-chord amendment. This work package is
  therefore a conformance check, not an acceptance request: the ADR's status is `accepted`, and the
  amendment is part of the accepted decision.
- The ADR's conflict table is the work list for WP3. If the maintainers resolve a conflict
  differently later, the ADR is amended by a superseding ADR **before** the implementation moves —
  an accepted ADR is immutable, and a change afterwards is a superseding ADR, not an edit.
- The amendment is load-bearing for two specific items, and both are recorded in the ADR: help is
  **not** re-spelled, and the composer's claim set contains neither `space` nor `y`.

*Acceptance evidence.*

- The ADR's status is `accepted`, with a date, and the amendment is part of the accepted text.
- The conflict table still enumerates every letter that this milestone must resolve, and every
  row has a stated resolution.
- No work item in this milestone re-spells help.

### WP2 — Write the composer claim table

*Intent.* One closed table mapping claimed letters to composer verbs.

*Required changes.*

- Define the table as a single enumerable structure, not as a set of scattered `match` arms. The
  guard must be able to read it, and a human must be able to read it.
- The table must be closed: every letter in it has exactly one entry, and no letter appears twice.
- Each entry names the composer verb it maps to. Entries whose verb is not yet implemented are
  explicitly marked as reserved, so the guard can distinguish "claimed and not yet live" from
  "not claimed" — and the fallthrough rule still applies to a reserved entry.
- **A claimed uppercase letter accepts both encodings.** The app binds some capital letters
  twice — `Char('H')` with `SHIFT` and `Char('H')` with `NONE` — because some terminals report a
  shifted key without the `SHIFT` modifier; `insert_panel_direction_bindings` in
  `src/tui/input.rs` is the established convention. A claim on an uppercase letter, notably `U`
  for redo, must match both, or the verb works on one terminal and not another. The
  Insert-mode text guarantee is unaffected, because a built-in binding on a text key is skipped
  in Insert mode rather than consumed.
- The table's scope is the main buffer's Normal mode only. It does not apply to the side-panel
  composers, the chat box, the dialogs, or the project picker, and it does not apply in Insert
  mode.
- The claim table's letters, together with the reserved ones, are the set the guard polices.

*Acceptance evidence.*

- A test enumerating the table and asserting no letter appears twice and no entry is empty.
- A test asserting the table's scope: a claimed letter in a side panel's composer, or in Insert
  mode, does not invoke the composer claim path.
- A test asserting that a claimed uppercase letter is reachable in both the shifted and the
  unshifted encoding.

### WP3 — Re-spell the conflicting app-level chords

*Intent.* Free the letters the composer needs, without removing any capability.

*Required changes.*

- Resolve every row of the ADR's conflict table. For each conflicting letter, the app-level action
  gains a new chord that is **not a bare letter**, and the new chord is added to **both** the
  default map and the vim map, so the action is reachable regardless of which map is active.
- The new chords must not be letters in the composer's claim set, or the composer will take them
  back. This is a hard constraint, and the guard must enforce it.
- `space` is **not** re-spelled and **not** claimed. ADR-0042's amendment keeps help on `space` in
  both maps, and puts the copy verb in the help box that chord opens, where M005 wires it. The
  composer's claim table therefore contains neither `space` nor `y`.
- `x` is the highest-risk entry among the letters that do move: it stops and resumes a running
  turn, which is a frequent action during a long turn.
- `d` is open-diff, `i` is focus-prompt. Both move.
- `j`/`k` follow the ADR's rule: the composer claims them only when the composer buffer has more
  than one line; on a single-line buffer they fall through to transcript scroll unchanged. This
  conditional claim is the one place where a claim is conditional, and it must be stated
  explicitly in the table and enforced by the guard, because an implicit conditional is how a
  leak reappears.
- `g` is not in the initial claim set, so `GoToTop` keeps `g`. If a later milestone claims `g`,
  `GoToTop` is re-spelled then, and that milestone owns the change.
- `h` and `l` are not re-spelled: the existing actions already move the composer caret.

*Acceptance evidence.*

- A test per re-spelled action: the new chord reaches the action in both maps and both modes.
- A test per re-spelled action: the old bare letter no longer reaches it from the main buffer in
  Normal mode.
- A test that the new chords are not in the composer's claim set.
- A test that `j`/`k` still scroll the transcript while the composer buffer is a single line, and
  are claimed by the composer when it is multi-line.

### WP4 — Dispatch the claim table

*Intent.* One dispatch step, in the right place in the chain.

*Required changes.*

- The claim step is consulted **before** the global binding map, in the main-buffer surface, when
  the composer holds the keyboard and the resolved mode is Normal.
- The step uses the M001 ownership derivation, not the raw mode field.
- A claimed letter is consumed by the composer even when its verb is not yet implemented. This is
  the fallthrough rule, and it is the whole point of the milestone: a reserved claim must not
  leak to the app action.
- A letter the composer does not claim resolves to the app's binding map exactly as it does today.
- The dispatch step must not alter the Insert-mode path.

*Acceptance evidence.*

- A dispatch-reachability test per claimed letter: pressing the key through the app's real key
  path reaches the composer claim, not the app binding map.
- A negative test per re-spelled action: pressing the old letter in the main buffer in Normal
  mode does not reach the app action.
- A test that a reserved claim with no implementation yet is still consumed, and that the app
  action it used to reach is not triggered.

### WP5 — Keep Insert mode typing bare letters

*Intent.* A letter in the claim table must still be typed as text in Insert mode.

*Required changes.*

- Update `is_normal_only` for any new action whose only spelling is a bare letter, and update
  `is_builtin_text_key`'s inputs so that a letter claimed in one map but not the other is still
  treated as text in Insert mode.
- Verify the whole claim set against the existing Insert-mode text guarantee.
- Model the sweep on `builtin_shift_letters_are_composed_text_in_insert_mode` in
  `src/tui/input.rs`, which sweeps both keymaps and asserts it is not vacuous. That test exists
  because a bare capital letter bound under the unshifted encoding used to be **consumed** in
  Insert mode — typing it opened a panel instead of inserting a character — and the fix was made
  at the key, in the Insert-mode text-key guard, rather than by removing the binding. A composer
  claim table must not reintroduce that defect, and the sweep must assert non-vacuity rather than
  passing on an empty claim set.

*Acceptance evidence.*

- A test per claim-table letter: in Insert mode, the letter is typed into the composer and does
  not trigger any app action.
- The existing single-letter regression test in the main-buffer suite continues to pass.
- A sweep over the whole claim set in both keymaps, asserting non-vacuity.

### WP6 — Help surface

*Intent.* The re-spelled chords and the composer's claim set are discoverable.

*Required changes.*

- Update the help entries for turn stop/resume, open-diff, and focus-prompt so that they
  name the new chords. Help's own entry is **not** changed: `space` keeps opening the help menu.
- Add help entries for the composer's claim set. The help audit requires every advertised key to
  resolve in an active map under the matching mode, and the claim table's letters are consumed by
  the composer rather than by a map. This milestone must therefore make the claim table
  reachable from a map, or the help entries must resolve to a map-backed action, so that the
  audit passes. Which of the two is chosen is a product decision, and the chosen approach must
  keep the claim table the single source of truth for what the composer claims.
- The entries must state the mode they apply to.

*Acceptance evidence.*

- The help audit passes.
- The modifier-chord audit passes.
- A test that each re-spelled action's help entry names its new chord and does not name the old
  one.

### WP7 — The composer key-ownership guard

*Intent.* Make the table's invariants mechanically enforced, not review-enforced.

*Required changes.*

- Add `scripts/check_tui_composer_key_authority.py`.
- The guard must fail when:
  - a letter in the claim set has no owner;
  - a letter is claimed by two owners;
  - a claimed letter can reach an app-level action (the fallthrough violation);
  - a re-spelled app-level chord is missing from either binding map;
  - a re-spelled app-level chord is itself a letter in the claim set;
  - a claimed uppercase letter is not covered in both the shifted and the unshifted encoding;
  - a claimed letter is not listed in `is_normal_only` where the Insert-mode text guarantee
    requires it.
- The guard must carry a `--self-test` that injects each violation class and proves the guard
  rejects it. This follows the `scripts/check_tui_subagent_name_pool_isolation.py` precedent; the
  three editor TUI guards predate that convention and do not carry one.
- Wire the guard into `scripts/verify.sh quick` and into CI, in the same manner as the existing
  TUI guards.
- The guard must be scoped to the composer. It must not police the editor's or the terminal
  panel's keys, and it must not modify the editor's existing guards.

*Acceptance evidence.*

- The guard passes on the tree as delivered.
- The guard's `--self-test` passes, and injecting each violation class makes the guard fail with a
  message naming the violation.
- `scripts/verify.sh quick` runs the guard and passes.

## 8. Failure, cancellation, restart, contention semantics

- **Failure.** A claim whose verb is not implemented must not panic and must not fall through.
  The reserved-claim state is the normal condition of a claim table that grows across milestones,
  not an error.
- **Cancellation.** Not applicable. No cancellable operation is introduced.
- **Restart.** Not applicable. The claim table is code, not state.
- **Contention.** The composer claim step runs in the app's single key-dispatch chain, which is
  already ordered by surface priority. The claim step must be placed so that a higher-priority
  surface — an open dialog, the terminal box, a focused panel — is consulted first. If a
  dialog is open, the composer must not claim anything.
- **Priority ordering.** The existing chain resolves dialogs, then panels, then the editor, then
  the composer. The claim step is inside the composer branch, so it inherits that ordering. This
  must be asserted by a test: with a dialog open, a claimed letter reaches the dialog, not the
  composer.

## 9. Compatibility and migration

- **No storage migration.**
- **No protocol change.**
- **No configuration change.** The claim table is not user-configurable in this milestone.
- **User-visible key change — deliberate and recorded.** The app-level verbs turn
  stop/resume, open-diff, and focus-prompt move to new, non-letter chords in the main buffer.
  The capabilities are not removed. Help does **not** move: `space` keeps opening the help menu.
  The change is recorded in ADR-0042 so that it is a decision
  rather than a surprise, and the help surface and `docs/tui.md` must state the new spellings in
  the same change that removes the old ones.
- **No change to the editor's or the terminal panel's keymaps**, and no change to their behaviour.
- **The claim table itself is not yet user-visible behaviour**, because the letters it claims have
  no composer verbs yet. The re-spellings *are* user-visible in this milestone. The closure
  record must say which is which, and must not present the reserved claims as a shipped
  capability.

## 10. Required tests

**Focused.**

- The claim table: no duplicate letters, no empty entries, correct scope (main buffer, Normal
  mode only).
- The fallthrough rule: a reserved claim is consumed and does not reach the app action.
- The fallthrough rule for `d` specifically: in the main buffer in Normal mode, `d` does not open
  the diff.
- The re-spellings: each new chord reaches its action in both maps; each old bare letter does
  not.
- `j`/`k` conditional claim: transcript scroll on a single-line composer buffer; composer claim
  on a multi-line buffer.
- Insert-mode text: every claim-table letter is typed as text in Insert mode.
- Key encoding: a claimed uppercase letter is reachable in both the shifted and the unshifted
  encoding.
- Priority: with a dialog open, a claimed letter reaches the dialog.

**Integration.**

- Dispatch-reachability: each claimed letter, pressed through the app's real key path, reaches
  the composer claim. A verb that is only reachable by calling a helper directly does not count.

**Static guard.**

- The guard passes on the delivered tree.
- The guard's `--self-test` passes.
- Each injected violation class is rejected.

**Regression.**

- The existing main-buffer Insert-mode single-letter test passes.
- The editor's and the terminal panel's key behaviour is unchanged; their existing guard
  scripts pass.

**Not required here.** Motion, selection, clipboard, and undo behaviour are M004–M006.

## 11. Required verification commands

```bash
# Guard, both modes
python3 scripts/check_tui_composer_key_authority.py --self-test
python3 scripts/check_tui_composer_key_authority.py

# Existing TUI guards must stay green
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_transport_authority.py

# Focused tests while iterating
cargo test -p codegg --lib tui::input
cargo test --test tui_main_buffer
cargo test --test modifier_chords

# Canonical gate
scripts/verify.sh quick

# Full gate before closure
scripts/verify.sh full
```

Do not run `--all-features` sweeps: they drag in the real-server tests. `cargo fmt` is part of
`quick`.

## 12. Documentation updates

- `docs/tui.md`: the re-spelled app-level chords — turn stop/resume, open-diff, focus-prompt
  — with their new spellings, in the section that describes where input goes. Help's chord is
  unchanged and is **not** restated as a change. The composer's
  Helix verbs are **not** documented yet; they arrive with M004–M006 and documenting them ahead of
  their implementation would be a false promise in the user guide.
- `architecture/tui.md`: the composer's claim-table rule, the fallthrough rule, the scope of the
  table, and the relationship to the editor's and the terminal panel's separate tables. Record
  that the three surfaces share selection semantics and geometry but not key ownership.
- `.opencode/skills/tui/SKILL.md`: the composer's key-ownership boundary and the guard.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md`: the M002 row of the milestone status
  table, and the status of the decision points the ADR settled.

## 13. Acceptance criteria

Externally observable, from a user's position at the terminal:

1. With the composer focused in Normal mode, every letter in the composer's claim set is consumed
   by the composer and triggers no app-level action.
2. In particular, in the main buffer in Normal mode: `d` does not open a diff, `x` does not stop
   or resume the turn, and `i` does not re-focus the composer. `space` still opens help.
3. Every re-spelled capability — turn stop/resume, open-diff, focus-prompt — is still
   reachable, in both the default and the vim keymap, by a chord that is not a bare letter.
4. With the composer focused in Insert mode, every letter in the claim set is typed as text and
   triggers no app action.
5. With a side panel, the terminal box, the editor, or a dialog holding the keyboard, no
   composer claim is live, and a claimed letter behaves exactly as it did before this milestone.
6. With a dialog open, a claimed letter reaches the dialog.
7. With the composer buffer empty, `j` and `k` still scroll the transcript.
8. The composer's text, caret, cursor, and selection behaviour are unchanged by this milestone.
9. The editor's and the terminal panel's key behaviour is unchanged.
10. A claimed uppercase letter is reachable in both the shifted and the unshifted encoding, so a
    terminal that reports a shifted key without the `SHIFT` modifier still reaches the verb.
11. Typing any capital letter in Insert mode inserts the character and opens nothing, in both
    keymaps.

## 14. Stop conditions

Report rather than improvise if any of the following holds.

- ADR-0042 is not accepted, or is accepted with a conflict table that this milestone cannot
  implement. The milestone is blocked until the record settles.
- A re-spelling would require a chord that is itself a letter in the claim set, and no non-letter
  chord is acceptable to the maintainers. That is a decision about the claim set's shape, not an
  implementation detail.
- The re-spellings would remove an app-level capability rather than move it. The ADR requires the
  capability to survive; if it cannot, the ADR needs amendment before implementation.
- The fallthrough rule cannot be implemented without changing the behaviour of a letter the
  composer does not claim. That would be a change to unclaimed behaviour, which is out of scope.
- The claim step cannot be placed inside the composer branch of the key chain without changing
  the priority of another surface.
- The help audit cannot be satisfied without making the claim table's single-source-of-truth
  property false — for example, by duplicating the table into a binding map. If that is the only
  way through, report it: the duplication would create a second source of truth for key
  ownership, which is the defect class the guard exists to prevent.
- The editor's guards would need modification to accommodate the composer's table. They must not
  be modified; report the conflict instead.

## 15. Closure evidence required

- A closure record at `plans/closure/tui-composer-helix-parity/002-status.md`, following the
  closure template, with a requirement-to-evidence matrix covering I1, I2, I5, and the re-spelling
  requirement.
- The accepted ADR-0042, referenced by the closure record.
- The guard's output, both plain and with `--self-test`, with the injected-violation evidence
  showing each violation class rejected.
- Test evidence for every acceptance criterion in section 13, with the exact commands from
  section 11 and their results, labelled truthfully as local or CI.
- The output of `scripts/verify.sh quick` and `scripts/verify.sh full` at the closure commit.
- An explicit statement of which part of this milestone is user-visible (the re-spellings) and
  which is infrastructure with no user-visible effect yet (the reserved claim set). The closure
  must not present a reserved claim as a shipped capability.
- Registry updates, including the M002 status change and an audit of blocked work for any plan
  whose blocker this closure resolves.

## 16. Handoff notes

- **Start here.** Read ADR-0042's conflict table before designing anything. It enumerates every
  letter this milestone must resolve and states the resolution for each.
- **The precedent to follow.** `src/tui/commands/interactive_terminal.rs` solves the same problem
  on a non-editor surface: it claims `d` unconditionally so that an unclaimed `d` cannot silently
  become `OpenDiff`, and its dispatch comment records the failure mode. The composer needs the
  same rule.
- **The other precedent.** `src/tui/commands/editor.rs` uses a `BufferKey` shape whose unhandled
  arm means "this surface does not want the key". The composer's claim table must have the same
  property: a letter the composer does not claim reaches the app's binding map unchanged.
- **Do not add verbs here.** The claim table's entries for M004–M006 are reserved, and a
  reserved entry is consumed but does nothing. That is intentional: it is what makes the
  fallthrough rule hold while the table grows.
- **Both maps.** Every re-spelled chord goes into the default map and the vim map. The help audit
  resolves advertised keys across both, and a chord that exists in only one map is a chord the
  user cannot find.
- **The guard is part of this milestone, not a follow-up.** The claim table is the moment where a
  leak is cheapest to prevent and most expensive to discover later.
- **`is_normal_only` is not optional.** A bare letter that is not listed there is typed as text
  in Insert mode, which is exactly what we want for the composer — but the *app-level* actions
  that are Normal-only must be listed, or they become unreachable.
