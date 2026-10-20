# TUI Composer Helix Parity M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-composer-helix-parity/006-composer-undo-redo-and-insert-transitions.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md` (§7, M006)

Repository baseline reviewed: `3cad45929` (branch `feat/auto-20261019-8cc543fb`)

Implementation commits or pull requests:

- *uncommitted at the time of writing* — the working tree carries the four new verbs, the claim
  table's fourteenth row, the two model defects, the selection rule, the help rows, this record,
  and the plan. No hosted CI run is claimed.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **accepted** 2026-10-19. This
  milestone does not change its decision: it makes live the letters ADR-0042 part 4 had already
  given the composer (`u`, `U`, `a`, `o`), and it settles the roadmap's D4 and D6 in the
  roadmap's own rows rather than by editing the ADR.
- **ADR-0042 part 4's conflict table does not mention `a`.** That is a gap in an accepted ADR,
  which is recorded in the roadmap's D6 row and here. The ADR is **not** edited: an accepted ADR
  is immutable, and a gap in it is a finding for the next ADR that touches the claim set.

## 1. Executive finding

The composer's **undo, redo and insert transitions** are live, and the claim table is **closed**:
every letter the composer owns now has a behaviour, and the runner's match on the verbs is
exhaustive, so a verb added without an arm does not compile.

At the baseline the composer had a text model with an undo history (M003 built it), but nothing
reached it: `u`, `U`, `a` and `o` were claimed and **reserved** — consumed, and doing nothing.
M006 closes that gap:

- `u` and `U` undo and redo the composer's text, and they **report in both branches** — "Nothing
  to undo" against "Undid the last edit" — so a verb's silence is never ambiguous.
- `a` and `o` are the two insert transitions. `a` is a **movement** (the caret moves one
  character forward and the text does not change, so there is nothing to undo); `o` is an **edit**
  (the buffer gains a line, and `u` takes it back).
- The bare `a` in the **vim map** is gone. `FocusSidebar` already answered to `Ctrl+Alt+S` in
  **both** maps, so the bare `a` was a second spelling of an action that already had one; the
  letter joined the composer and no capability was lost with it. The claim table grew from
  thirteen rows to fourteen.
- Two **model defects** that M006 is the milestone to fix, because M006 is the milestone that
  makes undo reachable: the history's eviction removed the **oldest** entry, so an oversized
  paste was evicted the moment it was made and became unundoable; and `replace_range` left a
  held selection across a length-changing edit.
- One **shared rule**: an edit that changes the buffer's length drops the held selection. It
  lives in `replace_range` — the one place every edit passes through — rather than in each
  caller's judgement.

Two decisions were settled here and are recorded in the roadmap, not in the ADR: **D4** (the
undo scope is the composer's own text) and **D6** (`a` versus `FocusSidebar`).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| **AC-M1 — pressing `u` in Normal mode with the composer holding the keyboard undoes the newest composer edit, through the real key path** | `tests/tui_composer_undo.rs::u_undoes_the_newest_edit` | pass | The test goes through `App::on_key`, not the model, because the dispatch is what M006 changed. |
| **AC-M2 — `U` redoes it, and is reachable under both the `SHIFT` and the `NONE` encoding** | `tests/tui_composer_undo.rs::u_uppercase_redoes_the_newest_undone_edit`, `::the_redo_verb_answers_to_both_capital_encodings` | pass | Terminals disagree about whether a shifted key carries the SHIFT modifier, so the capital claim is `CapitalBoth`. |
| **AC-M3 — undo traverses deletions performed by a selection delete, and restores the deleted text** | `src/tui/composer_text.rs::tests::a_selection_deletion_is_undoable`; `tests/tui_composer_undo.rs::undo_and_redo_walk_through_several_edits` | pass | A selection delete is an ordinary edit in the history, not a special case. |
| **AC-M4 — the history is bounded in depth and in bytes, and the most recent edit is always undoable, including a paste larger than the byte bound** | `src/tui/composer_text.rs::tests::the_depth_bound_still_evicts_the_oldest_first`, `::an_oversized_edit_stays_undoable`, `::an_oversized_paste_is_still_undoable`; `tests/tui_composer_undo.rs::an_oversized_edit_is_still_undoable` | pass | **This is the defect M006 fixed.** The byte bound used to evict the oldest entry, and on an oversized paste the oldest entry *was* the paste — it was evicted the moment it was made, so the history was empty and the user's last action could not be undone at all. The bound now drops **older** entries and never the last one, so an edit over the bound stays undoable. The depth bound still evicts the oldest first. |
| **AC-M5 — a new edit after an undo discards the redo tail** | `src/tui/composer_text.rs::tests::an_eviction_does_not_strand_a_redo` (asserts `!history.can_redo()` after a push) | pass | The existing M003 test, unchanged. It covers the two together, because the eviction's safety argument depends on the push clearing the redo stack first. |
| **AC-M6 — undo cannot resurrect submitted text; the history is cleared on submit** | `tests/tui_composer_undo.rs::replacing_the_whole_buffer_resets_the_history`; `src/tui/composer_text.rs::tests::replacing_the_text_drops_the_history` | pass | A submit (`prompt.clear()`) and a staged projection (`prompt.set_text(include_text)`) both **replace** the buffer, so both drop the history. The test names the replacement path explicitly rather than asserting on the submit's side effects. |
| **AC-M7 — `a` enters insert mode with the caret after the character under it, clamped to the caret's line** | `tests/tui_composer_insert_transitions.rs::a_moves_the_caret_after_the_character_and_enters_insert_mode`, `::a_stays_on_its_own_line`, `::a_does_not_run_into_the_next_line`, `::a_at_the_end_of_a_line_leaves_the_caret_at_the_line_end`; `src/tui/composer_text.rs::tests::insert_after_moves_the_caret_one_character_forward`, `::insert_after_stays_on_its_own_line`, `::insert_after_on_the_last_character_moves_to_the_end_of_the_buffer` | pass | The caret's placement lives in the **model** (`ComposerText::insert_after`), not in the app. See §3.3 for the one measurement that corrected the implementation. |
| **AC-M8 — `o` opens a line below the caret's line and enters insert mode there; the new line is an ordinary, undoable edit** | `tests/tui_composer_insert_transitions.rs::o_opens_a_line_below_and_enters_insert_mode`, `::o_on_the_last_line_opens_a_line_below`, `::o_keeps_the_carets_line_text`, `::o_on_a_single_line_buffer_creates_the_second_line`, `::o_is_undoable`; `src/tui/composer_text.rs::tests::open_line_below_opens_a_line_below_the_carets_line`, `::open_line_below_keeps_the_carets_line_text`, `::open_line_below_is_undoable` | pass | The newline goes at the end of the caret's line's **content**, so the line's own terminator stays where it was and the new empty line is the one below it. |
| **AC-M9 — `i` still enters insert mode without moving the caret** | `tests/tui_composer_insert_transitions.rs::the_insert_transitions_are_normal_mode_verbs` (the `i` arm is covered by the table's own row); `src/tui/composer_claims.rs::tests::the_table_is_the_adrs_fixed_letter_set` | pass | `i` is unchanged: the claim's arm still only sets the mode. |
| **AC-M10 — after any of the three transitions the badge and the cursor agree, and the caret is where the transition put it** | `tests/tui_composer_insert_transitions.rs::a_moves_the_caret_after_the_character_and_enters_insert_mode`, `::o_opens_a_line_below_and_enters_insert_mode` (each asserts the mode and the caret together) | pass | The badge reads the same derived `composer_input_mode()` the transitions write, so they cannot disagree. |
| **AC-M11 — `a` is the composer's: it no longer reaches `FocusSidebar` in the vim map, and `FocusSidebar` is still reachable by `Ctrl+Alt+S` in both maps** | `tests/tui_composer_insert_transitions.rs::the_insert_after_verb_does_not_reach_an_app_level_action`; `scripts/check_tui_composer_key_authority.py` (G2, G4, G5) and its self-test cases `detects a claimed `a` still reaching FocusSidebar in the vim map`, `detects a FocusSidebar missing from one map`, `detects a FocusSidebar spelling that is a bare letter` | pass | The integration test binds `a` to `FocusSidebar` in the app's map and asserts the composer's transition ran instead. The guard proves the property statically, in both maps. |
| **AC-M12 — the help contains a row for every composer verb this milestone adds, and no row advertising bare `a` as a sidebar gesture** | `src/tui/input.rs::tests::the_help_does_not_advertise_bare_a_as_the_sidebars_gesture`, `::every_claimed_composer_letter_has_a_help_row` | pass | See §5.1 — the rows ended up **derived**, which is M007's design and a superset of this criterion. |
| **AC-M13 — an edit that changes the buffer's length drops the held selection; undo and redo do the same** | `src/tui/composer_text.rs::tests::an_edit_drops_the_held_selection`, `::an_undo_drops_the_held_selection`, `::a_redo_drops_the_held_selection`; `tests/tui_composer_undo.rs::undo_drops_the_held_selection` | pass | The rule lives in `ComposerText::replace_range`. `delete_selection`'s own `deactivate()` became redundant and was removed — the selection's range is read *before* the edit, which is the whole point. |
| **AC-M14 — the undo scope is recorded in the roadmap's D4 row, in `architecture/tui.md`, and in `docs/tui.md`** | `plans/subsystems/tui-composer-helix-parity-roadmap.md` (§10, D4); `architecture/tui.md` ("The undo scope is the composer's own text (roadmap D4)"); `docs/tui.md` ("Undoing, and the two insert transitions") | pass | The scope is the composer's **own text**: it is not a choice about which state to keep, it is what the buffer's history already is. |
| **AC-M15 — the guard and its `--self-test` pass, and the self-test rejects each of the three new violation classes** | `python3 scripts/check_tui_composer_key_authority.py`; `python3 scripts/check_tui_composer_key_authority.py --self-test` | pass | The three new classes: a claimed `a` still reaching `FocusSidebar` in the vim map; a `FocusSidebar` spelling that is a bare letter; a `FocusSidebar` missing from one map. |
| **AC-M16 — no new timer, poller, or animation cadence was introduced** | `git diff --stat` over `src/`; the only render change is `render_composer_hints`, which reads a derived row | pass | The hint row reads state; it never schedules anything. |

## 3. Production implementation evidence

### 3.1 The claim table — thirteen rows to fourteen

`src/tui/composer_claims.rs` gained one row and four status changes:

| Key | Verb | Before | After |
|---|---|---|---|
| `a` | insert after the caret | — | **Live** (new row) |
| `o` | open line below | Reserved | **Live** |
| `u` | undo | Reserved | **Live** |
| `U` | redo | Reserved | **Live** |

Every other row is unchanged. The table is now **all live**: `composer_claims.rs`'s own test
(`every_claim_is_live`) asserts that no claim is reserved, and `the_table_is_the_adrs_fixed_letter_set`
pins the key set to ADR-0042 part 4's closed set — `a b d e i j k o p u U v w x` — so a future
addition to the table that is not in the ADR's set fails a test.

The live-set assertion was **renamed** by this milestone, from
`the_live_claims_are_the_motions_the_selection_verbs_and_the_insert_transition` to
`every_claim_is_live`. The old name described a subset of the table ("the live claims are the
motions, the selection verbs and the insert transition") that was true when M004 wrote it and
widened by M005; this milestone made every claim live, so the name would have been a description
of the table's *history* rather than of its *state*. M004's and M005's closure records cite the
current name and record the rename chain.

### 3.2 The runner is exhaustive

`run_composer_claim`'s match on `ComposerVerb` used to end in a catch-all arm that logged a
warning when a live verb had no behaviour — the table/runner disagreement the claim table exists
to prevent. With every verb live, that arm became **unreachable**, and the compiler said so.

It is now gone. The match is exhaustive, so a verb added to `ComposerVerb` without an arm in
`run_composer_claim` **does not compile**. The warning said the disagreement was a programming
error; making it unbuildable is stronger than saying so loudly at runtime.

### 3.3 The two insert transitions live in the model

`ComposerText::insert_after` and `ComposerText::open_line_below` place the caret; the app only
switches the mode. `insert_after` moves the caret through **`move_caret`**, the
deliberate-movement path, because that is the path that extends a held character selection — a
re-anchored caret would throw the selection away.

**The measurement that corrected the implementation.** The first cut of `insert_after` used
`next_char_boundary(caret).unwrap_or(caret)`. That function returns `None` at the **end of the
buffer** — the editor's rule, because the position after the last character is not a character a
selection can cover. For an *insertion point* it is exactly the position wanted: `a` on the last
character should move the caret to the end of the buffer, so the next keystroke is typed after
that character. The implementation now uses `.unwrap_or(self.text.len())`, and the test
`insert_after_on_the_last_character_moves_to_the_end_of_the_buffer` pins it.

`o` carries no `ClaimCondition::MultiLineBuffer`: it *creates* the second line, so the condition
that gates `j` and `k` — which fall through to transcript scroll — does not apply to it.

### 3.4 The eviction fix, and the rule it protects

`UndoHistory::evict` used to remove the oldest entry until both bounds held. On an oversized
paste the oldest entry **was** the paste: it was evicted the moment it was made, the history was
left empty, and the user's last action could not be undone at all. The eviction now stops when
one entry remains, so the bound is enforced by dropping **older** entries and an edit over the
bound stays undoable. The history is then over its byte bound by exactly that one edit, which is
the price of "the last thing I did is always undoable".

### 3.5 The selection rule

`ComposerText::replace_range` now drops the held selection when the edit changes the buffer's
length. The selection's points are byte offsets into the text the edit replaces, so carrying them
across would make the selection cover different characters — or reach past the end of the
buffer. The rule lives in the one place every edit passes through, not in each caller's
judgement. `undo` and `redo` change the length too and drop it the same way.

`delete_selection` used to call `state.deactivate()` itself. That call became redundant and was
removed; the range is still read **before** the edit, which is the whole point of the deletion
acting on what was selected.

### 3.6 The re-spelling

The vim map's bare `a` → `FocusSidebar` is deleted. `FocusSidebar` answers to `Ctrl+Alt+S` in
**both** maps, so nothing is lost. The help's `a` → "Focus sidebar (agent tree)" row is deleted
with it: it taught a gesture that no longer reached the action. `RESPELLED_ACTIONS` in the guard
gained `"FocusSidebar": "a"`, which is what makes the guard check the property — the action is
bound in both maps, has a non-bare spelling in both, and does not answer to the bare `a`.

### 3.7 The verbs report in both branches

`u` and `U` toast on **both** outcomes: "Undid the last edit" / "Nothing to undo", "Redid the last
edit" / "Nothing to redo". This mirrors M005's paste rule. A silent no-op teaches the user that
the key is dead, and "there was nothing to undo" is a different fact from "nothing happened".

## 4. Verification

| Check | Command | Result |
|---|---|---|
| Formatting | `cargo fmt --all --check` | clean |
| Type check | `cargo check --workspace --all-targets --locked` | clean |
| Clippy (1.89, the repository minimum) | `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| Clippy (1.99, the CI toolchain) | `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` | clean |
| The composer guard | `python3 scripts/check_tui_composer_key_authority.py` | passed |
| The guard's self-test | `python3 scripts/check_tui_composer_key_authority.py --self-test` | passed — every violation class detected and rejected |

**The workspace sweep, measured against the baseline rather than asserted.** The sweep
(`cargo nextest run --workspace --locked --profile ci --no-fail-fast`) ran **14,993** tests:
**14,933 passed, 59 failed, 1 timed out**, 7 skipped. The baseline was then **measured**: the
commit `3cad45929` was extracted to a clean tree (`git archive`) and the same binaries were run
there, so every failure could be attributed by name.

**Three of the 59 were M007's doing, not M006's** — the hint row superseded a contract three
pre-existing tests encoded, and those three now express the M007 contract. They are recorded in
M007's closure record §5 and §6.4, and they pass in this tree.

**The other 56 failures and the 1 timeout reproduce at the baseline, by name.** They are this
machine's environment: 33 `codegg::lsp` tests (the LSP tool is built without a workspace root and
refuses), 6 identity live-audit tests (credentials and a shell), 4 `git_execution_origin_matrix`
tests (a git identity), 3 `tool_surface_minimization` tests (the tool registry's own contents), 2
`tool_program_search_palette` tests (the external `eggsearch` backend is not installed), 2
`session_family` controller-lease tests and 2 `core::daemon` tests (the macOS `/tmp` symlink
breaks the daemon's root-path checks), 2 more in the same symlink class, 1 `workspace_isolation`
test, 1 upstream `tool_advisor` receipt test, and 1 `agent::asset_refresh` **timeout** that
**passes in isolation**. M007's closure record §5 has the full table, the baseline's own 19
load-induced flakes, and the measurement behind each.

**So the tree's own failure count is 56 + 1, all of them confirmed at the baseline, and
`scripts/verify.sh full` is therefore not green on this machine.** Hosted CI is where the suite
is expected to pass. `bash scripts/verify.sh quick` **passes**.

## 5. Deviations and decisions

### 5.1 The help's composer section ended up **derived**, not hand-written

M006's plan asked for help rows for the verbs it adds. M007's plan asks for the composer section
to be **derived** from the claim table — "a hand-written list is a second copy of the table, and a
second copy is the drift the milestone exists to remove".

Because both milestones are delivered together, the section is derived, and the derivation is
`composer_verb_entries` (`src/tui/composer_claims.rs`), read by the help
(`composer_help_entries`, `src/tui/input.rs`), by the hint row
(`App::composer_hint_row`), and by the guard. M006's acceptance criterion is satisfied as a
**superset** of what it asked for: every live claim has a row, and no row advertises bare `a` as a
sidebar gesture. The hand-written rows M006 would have written are not in the tree, and the
derivation is recorded in M007's closure record as the thing that replaced them.

### 5.2 The I6 help audit needed an exemption for the composer's own keys

`src/tui/input.rs`'s `documented_keys_resolve_in_the_active_map` test audits every advertised key
against the app's binding maps. The composer's rows are resolved by the **composer**, not by the
app map, so the audit reported contradictions that do not exist — the app map still binds `p` to
`PreviousSubagentTab` and `w` to `OpenWorkspaceDashboard` in the vim map, and those bindings are
unreachable while the composer holds the keyboard.

The audit now **exempts** a claimed letter in Normal mode, and the exemption is not a hole:
`every_claimed_composer_letter_has_a_help_row` audits the composer's rows against the claim
table instead. That test is the other half of the bargain.

### 5.3 The undo scope (roadmap D4)

The scope is the composer's **own text**, and it is settled here. The composer has exactly one
buffer, so the scope is not a choice about which state to keep — it is what the buffer's history
already is. Two paths replace the whole buffer rather than editing it: a submit
(`prompt.clear()`) and a staged projection (`prompt.set_text(include_text)`). Both **drop the
history**, so an undo can never bring back text the user has already sent, and neither is itself
undoable. A *pending* slash command is ordinary buffer text and is undoable while it is pending;
what is not undoable is the replacement of the buffer around it.

### 5.4 The `a` re-spelling (roadmap D6)

`a` is the composer's. `FocusSidebar` already answered to `Ctrl+Alt+S` in both maps, so the bare
`a` was a second spelling of an action that already had one — the letter was **deleted**, not
moved, and no capability was lost. ADR-0042 part 4's conflict table omits `a`; that gap is
recorded in the roadmap's D6 row and in this record, and the ADR is not edited.

## 6. Known differences and open items

- **The word-geometry divergence between the composer, the editor and the terminal panel** is
  recorded, not fixed: the terminal panel's `e` and `b` are a row-scoped reader's motion over a
  PTY's screen, not a buffer motion. M007's sweep records it with the same verdict.
- **D7 (the line start/end motion) stays open.** It is not M006's to settle, and M007 records it
  as still open with the recommendation it already carries.
- **The hint row and the derived help section are M007's deliverable.** M006's closure record
  does not claim them; the closure record for M007 does.
