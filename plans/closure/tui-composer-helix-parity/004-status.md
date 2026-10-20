# TUI Composer Helix Parity M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-composer-helix-parity/004-composer-motions.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md` (§7, M004)

Repository baseline reviewed: `3cad45929` (branch `feat/auto-20261019-8cc543fb`)

Implementation commits or pull requests:

- *uncommitted at the time of writing* — the working tree carries the five motion verbs, the claim
  table's five new `Live` rows, the composer's motion methods, the widget's five accessors, this
  record, and the plan. No hosted CI run is claimed.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **accepted** 2026-10-19. This
  milestone does not change its decision: it claims **no new key**, and it makes live the five
  letters ADR-0042 part 4 had already given the composer.

## 1. Executive finding

The composer's **motions** are live, and they move the caret the way the shared geometry says.

At the baseline the composer's word motions existed as *offsets* — `ComposerText::word_forward`,
`word_backward` and `word_end` answered "where is the next word" — but nothing moved a caret with
them, and the letters `w`, `b`, `e`, `j` and `k` were claimed and reserved: consumed, and doing
nothing. M004 closes that gap:

- `ComposerText` gains five motion methods that move the caret through the shared machine's
  `move_caret`, so the caret is *moved* rather than *set*.
- `PromptWidget` exposes them as five accessors, in the shape the widget's other caret accessors
  already have.
- The claim table's five motion rows become `Live`, and `run_composer_claim` gains the five arms.

Two properties of the implementation are load-bearing, and both were decisions rather than
accidents:

- **The motion moves the caret through `move_caret`, not `set_caret`.** The difference is invisible
  until a selection exists, and then it is the whole story: `move_caret` moves the machine's
  cursor, so in character mode the selection's head follows it and a held selection **grows** with
  the motion. M005's selection verbs depend on this; a motion that set the caret directly would
  re-anchor the selection on every press.
- **The targets come from `text_geometry`, not from a second opinion.** The word motions call
  `word_forward` / `word_backward` / `word_end` and the vertical motion calls `move_vertical`, so
  the composer and the editor land on the same offset for the same text.

No key was added and none was removed. The claim table still has thirteen rows, and the workstream's
§3 fixes the composer's claim set — so M004 claims nothing new and makes five existing claims live.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| **AC-M1 — `w` in Normal mode moves the caret to the start of the next word, through the app's real key path** | `tests/tui_composer_motions.rs::w_moves_the_caret_to_the_start_of_the_next_word` | pass | The test presses the key into `App::on_key`, not into a helper: a test that called `ComposerText::move_word_forward` directly would pass even if the claim table and the runner had drifted apart, and the drift is what the table exists to prevent. |
| **AC-M2 — `b` moves the caret to the start of the previous word and `e` to the end of the word at the caret** | `::b_moves_the_caret_to_the_start_of_the_previous_word`, `::e_moves_the_caret_to_the_end_of_the_word` | pass | `e` lands on the **last character** of the word, which is what the shared `word_end` answers. |
| **AC-M3 — `j` and `k` move the caret a line up and down, keeping the column, and only on a multi-line buffer** | `::j_and_k_move_the_caret_between_lines`, `::the_vertical_motions_keep_the_column`, `::the_vertical_motions_do_not_apply_to_a_single_line_buffer`; `tests/tui_composer_claims.rs::k_still_scrolls_the_transcript_on_a_single_line_composer`, `::k_is_claimed_on_a_multi_line_composer`, `::j_follows_the_same_conditional_claim_as_k` | pass | The condition is evaluated at the claim gate, before the binding map. On a single-line buffer the letters fall through to `NavigateDown` / `NavigateUp` exactly as before. |
| **AC-M4 — a motion with nowhere to go leaves the caret unchanged and consumes the key** | `::the_word_motions_stop_at_the_buffers_edges`; `src/tui/composer_text.rs::tests::a_vertical_motion_clamps_to_a_shorter_target_line` | pass | `w` at the end of the buffer and `b` at the start leave the caret where it is, and the key is still consumed by the composer. |
| **AC-M5 — the motion lands on exactly the offset `text_geometry` computes** | `::the_word_motions_land_on_the_shared_geometrys_offsets`, `::the_vertical_motions_land_on_the_shared_geometrys_offsets`; `src/tui/composer_text.rs::tests::a_word_motion_moves_the_caret_to_the_geometrys_answer`, `::a_word_end_motion_moves_the_caret_to_the_geometrys_answer`, `::a_vertical_motion_moves_the_caret_to_the_geometrys_answer` | pass | The integration test walks **every** offset of a text; the unit test does the same at the model level. The shared policy function remains the only word-segmentation rule — the editor's private copy was deleted in M003 and nothing here reintroduced one. |
| **AC-M6 — the claim table still has thirteen rows, the five motion claims are `Live`, the other seven are unchanged** | `src/tui/composer_claims.rs::tests::every_claim_is_live` (the live-set assertion; it was named `the_live_claims_are_the_motions_and_the_insert_transition` at this milestone, was widened by M005, which added four more live verbs, and was renamed to `every_claim_is_live` when M006 closed the table) and `::the_table_is_closed_and_single_owner` | pass | M004 changes **five** `Reserved` rows to `Live` and adds no row. The seven rows still reserved at this milestone are `d`, `o`, `p`, `u`, `U`, `v` and `x`; M005 then made `d`, `p`, `v` and `x` live. The `j` / `k` conditions are untouched. |
| **AC-M7 — every claimed letter is still typed as text in Insert mode** | `tests/tui_composer_claims.rs::every_claim_table_letter_is_typed_as_text_in_insert_mode`; `tests/tui_composer_motions.rs::the_motions_are_normal_mode_verbs` | pass | The sweep walks the claim table, so it covers the five new live verbs automatically. |
| **AC-M8 — a motion moves the caret through the selection-extending path** | `src/tui/composer_text.rs::tests::a_motion_extends_a_held_character_selection`, `::a_motion_does_not_extend_a_held_line_selection` | pass | Character mode extends; line mode does not, on purpose — a motion must not turn "this line" into "every line I walked over". |
| **AC-M9 — the widget's public API is intact, the composer renders as before, the editor and the terminal panel are unchanged** | `tests/tui_render.rs` (111 passed); `src/tui/components/prompt.rs`'s new accessors are additions; `src/tui/editor.rs` and `src/tui/interactive_terminal.rs` are not touched | pass | The five new widget methods are **additions**. The composer draws exactly what it drew: the render path is unchanged, and `ensure_cursor_visible_with_width` already follows the caret, so the scroll follows the motion for free. |
| **AC-M10 — the workspace gates are green** | §4 | pass | `verify.sh quick` green; workspace clippy green on 1.89.0 (the repository's minimum) and on 1.99.0 (the CI toolchain); `cargo fmt --all --check` clean; the composer guard and its self-test green. |

## 3. Production implementation evidence

### 3.1 The model's motions — `src/tui/composer_text.rs`

Five methods: `move_word_forward`, `move_word_backward`, `move_word_end`, `move_line_down` and
`move_line_up`. Each computes its target with the shared geometry and then calls `move_caret`:

```rust
pub fn move_word_forward(&mut self) {
    let target = self.word_forward();
    self.move_caret(target);
}
```

`move_caret` is the method M003 already had, and it is the shared machine's `move_to` under another
name. The distinction from `set_caret` is the whole of AC-M8.

`move_line_down` and `move_line_up` call `text_geometry::move_vertical`, which keeps the column
where the target line allows and clamps to the target line's end — so `j` on the last line is a
no-op rather than an error, and `j` into a shorter line lands at that line's end.

### 3.2 The widget's accessors — `src/tui/components/prompt.rs`

Five methods in the shape the widget's other caret accessors already have: `cursor_word_forward`,
`cursor_word_backward`, `cursor_word_end`, `cursor_line_down` and `cursor_line_up`. They are
**additions**: `PromptWidget`'s public API is intact, so the app-side call sites did not move.

### 3.3 The claim table — `src/tui/composer_claims.rs`

Five `Reserved` rows became `Live`: `b` (word back), `e` (word end), `j` (line down), `k` (line
up) and `w` (word forward). No row was added and no condition was changed. The module's doc
comment, which said that no verb in the table was new behaviour, was updated to say which verbs
are live and why, and the table's own test was rewritten to assert the live set rather than the
single live verb it used to assert.

### 3.4 The runner — `src/tui/app/mod.rs`

`run_composer_claim`'s `Live` arm became a match on the verb, with one arm per live verb. The
`Reserved` arm is unchanged: the key is consumed and nothing happens, which is the fallthrough
rule. The catch-all arm still warns loudly when a live verb has no behaviour — that is a
table/runner disagreement, and a consumed key with no behaviour is a dead key.

## 4. Verification executed

All commands were run locally in the worktree. **No hosted CI run is claimed for this milestone.**

| Command | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo check --workspace --all-targets --locked` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (1.89.0) | pass |
| `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` (1.99.0, the CI toolchain) | pass |
| `python3 scripts/check_tui_composer_key_authority.py` | pass |
| `python3 scripts/check_tui_composer_key_authority.py --self-test` | pass — every violation class detected and rejected |
| `scripts/verify.sh quick` | pass |
| `cargo nextest run --workspace --locked --profile ci --no-fail-fast` | 14931 tests run: **14875 passed, 56 failed, 7 skipped** — see §4.2 |

### 4.1 The focused suites

| Suite | Result |
|---|---|
| `tests/tui_composer_motions.rs` (new) | 13 passed, 0 failed |
| `tests/tui_composer_claims.rs` | 29 passed, 0 failed |
| `tests/tui_render.rs` | 111 passed, 0 failed |
| `cargo test --lib tui::` | 2180 passed, 0 failed (4555 filtered out) |

### 4.2 The full sweep, against the baseline

The workspace sweep was run **twice**: once on this milestone's tree, and once on the baseline
commit with the milestone's work stashed, so the two failure sets could be compared rather than
assumed.

| Tree | Tests run | Passed | Failed | Skipped |
|---|---|---|---|---|
| This milestone | 14931 | 14875 | 56 | 7 |
| Baseline `3cad45929` | 14873 | 14816 | 56 | 7 |

**The failure sets are identical** — the same 56 tests, in the same places — and the milestone
adds **58 tests, all passing**. The baseline run also showed one timeout
(`agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`); that test passes
in isolation on this tree and is a flake in a full sweep, not a failure of this milestone.

The 56 failures are environmental and pre-date this milestone. They fall into five groups: tests
that register a workspace at `/tmp`, which macOS makes a symlink
(`core::daemon::tests::session_create_through_daemon`, `workspace_isolation`, and the
`session_family` controller-lease pair); the LSP suites, which need an installed language server;
the live identity-audit suites, which need credentials and a shell; the git execution-origin
matrix, which needs a git identity; and three tool-surface assertions about the tool registry's
own contents. **None of them touches the composer, the claim table, the help dialog or the key
path.**

`scripts/verify.sh full` is therefore not green on this machine for the same reason it was not
green at the baseline: its sweeps include the `/tmp` family above. Hosted CI is the environment
where that suite is expected to pass, and **no hosted CI run is claimed for this milestone.**

## 5. Invariant review

| Invariant | Result | Note |
|---|---|---|
| I1 — one owner per chord per surface | pass | The claim table still has thirteen rows, and the guard passes. M004 claimed no key. |
| I2 — a claimed key never falls through | pass | The five live verbs are consumed by the composer. A live verb is a *stronger* claim than a reserved one, and the fallthrough rule is unchanged for the rest. |
| I3 — the composer is a frontend-local string | pass | The motions touch the buffer and the caret only. No daemon state, no revision, no document. |
| I5 — the resolved mode is the gate | pass | The claim gate is unchanged: the resolved mode and the derived ownership, never the raw field. |
| I7 — one redraw cadence | pass | Nothing here adds a timer, a poller or a redraw. The scroll follows the caret through the render path that already existed. |
| The editor's behaviour is unchanged | pass | `src/tui/editor.rs` is not touched. The shared geometry is called, not changed. |
| The terminal panel is unchanged | pass | Its word rule is a different rule on purpose. |

## 6. Failure and recovery review

The motions are pure buffer arithmetic: no I/O, no background task, no durable state, nothing to
fail, cancel or contend over. A motion cannot panic — the shared geometry clamps its answers into
the buffer, and `move_caret` snaps the offset onto a character boundary, so a multi-byte
character's interior is not a reachable caret position.

The one subtlety worth recording: `text_geometry::word_end` answers with a **byte** offset, and
for a multi-byte word that offset can be the middle of a code point. The caret cannot sit there,
so `move_caret` snaps it back onto the boundary — which puts the caret *on* the word's last
character, the Helix answer. The editor keeps a byte offset and has no such constraint, so the
two surfaces can differ by that snap; the composer's own test asserts the boundary rather than
pretending the raw byte offset is reachable.

## 7. Migration and compatibility review

- **Migration.** None. Nothing is stored, and no schema or protocol changes.
- **Compatibility.** `PromptWidget`'s public API is intact — the five accessors are additions. The
  editor's public geometry API is untouched. The composer's rendering is unchanged.
- **The live set grew.** Five claims moved from `Reserved` to `Live`. A reserved claim and a live
  claim both consume the key; the difference is that a live claim also *does* something. Nothing
  that depended on a claim being reserved changes, because a reserved claim never did anything
  either.

## 8. Security review

None. The composer remains a frontend-local string delivered to the daemon only on submit. The
motions add no authority, no path handling, no network surface and no durable record, and they do
not touch the submit path.

## 9. Documentation and operations

| Document | Change |
|---|---|
| `src/tui/composer_text.rs` | The five motion methods, with the doc that says why the movement goes through `move_caret` and where the targets come from. |
| `src/tui/components/prompt.rs` | The five caret accessors, in the shape the widget's other accessors have. |
| `src/tui/composer_claims.rs` | Five `Live` rows; the module doc and the table doc updated to say which verbs are live. |
| `src/tui/app/mod.rs` | `run_composer_claim`'s five arms. |
| `architecture/tui.md` | The claim table's "Lands with" column, the paragraph about reserved verbs, and a new subsection on the motions and the shared machine. |
| `docs/tui.md` | The word motions and the vertical motions in the `[NOR]`-mode table, with the multi-line condition spelled out. |
| `plans/subsystems/tui-composer-helix-parity-roadmap.md` | M004's status; §12's status table. |
| `plans/implementation/tui-composer-helix-parity/004-composer-motions.md` | New. |
| `plans/registry.md` | M004's row, the parent row's status, and the dependency audit. |

**No ADR was written or changed.** ADR-0042's decision is untouched: M004 claims no key and adds
no verb to the table.

## 10. Unresolved findings

| Severity | Finding | Owner |
|---|---|---|
| low | **Line start and line end are not motions.** The composer has `Home` / `End` at the app level, and those go to the **buffer's** start and end rather than to the line's. Helix's `0` and `$` are not in the claim table, and adding them would change ADR-0042 part 2's shape — the claim set is a closed set of *letters*, and `0` and `$` are not letters. For a single-line composer the two are identical, so the gap is invisible in practice. | Deliberate. Recorded as decision **D7** in the roadmap; it is a claim-set change, and the claim set is the ADR's. |
| low | The `word_end` byte offset is snapped to a character boundary, so on a multi-byte word the composer's caret and the editor's byte offset can differ by that snap. | Deliberate. The caret cannot sit inside a code point, and the composer's test asserts the boundary rather than the raw byte. |
| low | Undo is per-edit, so undoing a typed word takes one keystroke at a time. Grouping consecutive typing into one step is a UX question with a UX answer. | M006: the undo verbs. |

No high or medium finding remains.

## 11. Roadmap disposition

- M004 is **closed**. M005 (selection and clipboard) had its hard dependencies, M004 and M003, both
  discharged by this milestone, and was **implemented and closed in the same commit** — see
  `plans/closure/tui-composer-helix-parity/005-status.md`.
- M006 is now **eligible for fresh planning**: its hard dependencies, M005 and M003, are both
  closed. M007 is unchanged: it waits on M006.
- The parent roadmap's open decision **D4** (the undo scope) stays open and stays owned by M006.
- The parent roadmap's **D7** (line start and line end) is recorded here as deliberate, with the
  reasoning above.
- The parent roadmap's **D6** (the `a` conflict with the vim map's `FocusSidebar`) is untouched by
  this milestone and remains owned by M006.

## 12. Registry updates

- Added M004's row to **Dependency-ready implementation plans** with this closure record.
- The parent roadmap's row now reads "M001 closed; M002 closed; M003 closed; M004 closed; M005
  closed" — M005 was implemented and closed in the same commit as M004, because M005's hard
  dependency on M004 is discharged by this milestone and the two were delivered together.
- M005's row moved out of **Blocked work** and into **Dependency-ready implementation plans** as
  `closed`, with its closure record.
- **Dependency audit.** Closing M004 unblocked **M005**: the roadmap's dependency graph lists M004
  as a hard dependency of M005 ("Selection verbs need the selection and undo model" and the
  motions the selection grows with). M005's other hard dependency, M003, was already closed, so
  M005 became dependency-ready and was implemented in this commit. Closing M005 in turn
  unblocked **M006**, whose hard dependencies are M005 and M003; M006 is now registered under
  **Milestones eligible for fresh planning**.
