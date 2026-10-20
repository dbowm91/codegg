# TUI Composer Helix Parity M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-composer-helix-parity/005-composer-selection-and-clipboard.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md` (§7, M005)

Repository baseline reviewed: `3cad45929` (branch `feat/auto-20261019-8cc543fb`)

Implementation commits or pull requests:

- *uncommitted at the time of writing* — the working tree carries the four selection verbs, the
  shared selection range, the composer's selection rendering, the help box's route and the copy
  gesture, this record, and the plan. No hosted CI run is claimed.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **accepted** 2026-10-19. This
  milestone does not change its decision: it makes live the four letters ADR-0042 part 4 had
  already given the composer, and it implements part 7's copy gesture in the box the `space` chord
  opens.

## 1. Executive finding

The composer's **selection and clipboard verbs** are live, and the **copy gesture** belongs to the
help box the `space` chord opens — and to no other surface.

At the baseline the composer had a selection *state* (M003 built it) and a `delete_selection` that
used it, but no verb armed a selection, nothing rendered one, and the letters `v`, `x`, `d` and
`p` were claimed and reserved: consumed, and doing nothing. M005 closes that gap:

- `ComposerText` gains the two selection verbs, the shared **selection range**, and the selected
  text. The range is the editor's range, term for term, and it has **three** consumers: the delete
  verb removes it, the copy verb copies it, and the renderer paints it.
- `PromptWidget` paints the selection with the theme's selection colours, and exposes the range
  and the selected text.
- The claim table's four rows become `Live`, and `run_composer_claim` gains the four arms.
- The help box learns **how it was opened** — `HelpRoute` — and the `space`-opened box returns
  `TuiMsg::CopyComposerSelection` when `y` is pressed.

The decisive finding is the one the plan recorded and this milestone confirmed: **the mode cannot
decide whether `y` is a verb.** A `/help` box starts in Insert and `Esc` moves it to Normal, so a
mode-only rule would make `y` copy in a box that must not copy; a `space`-opened box can be
switched to Insert with `Enter`, where `y` must stay a filter character. The **route** travels
with the open request instead, and a call site cannot open the help box without saying which help
box it is.

No key was added to the composer's table and none was removed. The table still has thirteen rows.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| **AC-M1 — `v` arms a character selection, a movement extends it, a second `v` discards it; `x` takes a line at a time and stops at the last line** | `tests/tui_composer_selection.rs::v_arms_a_character_selection_at_the_caret`, `::a_movement_extends_the_held_selection`, `::a_second_v_discards_the_selection`, `::x_selects_the_line_and_grows_it_a_line_at_a_time`, `::x_stops_growing_at_the_last_line`; `src/tui/composer_text.rs::tests::a_character_selection_is_armed_at_the_caret_and_toggled_off`, `::a_line_selection_grows_a_line_at_a_time_and_stops_at_the_last_line`, `::a_line_selection_re_anchors_after_a_movement` | pass | `v` is a **mode** and `x` is an **action** — the shared machine's own distinction, and the reason the two keys are not interchangeable. A movement disarms the line growth, so the next `x` re-anchors on the caret's line. |
| **AC-M2 — the held selection is rendered with the theme's selection colours, and the unselected text renders exactly as before** | `src/tui/components/prompt.rs::tests::a_selected_range_is_painted_with_the_selection_colours`, `::a_selection_spanning_a_newline_is_painted_on_both_lines`, `::an_unselected_composer_renders_as_before`, `::the_placeholder_path_is_unaffected` | pass | The selection is painted as a **background** — `theme.selection` with `theme.selection_text` on it, the theme's derived token for text drawn on that fill. The unselected bytes are one span in the theme's foreground, exactly as before, so a composer with no selection is byte-identical to the composer before this milestone. |
| **AC-M3 — the selection has one byte range, and delete, copy and render agree** | `src/tui/composer_text.rs::tests::the_delete_and_the_copy_read_the_same_range`; `src/tui/components/prompt.rs::tests::the_widget_reports_the_selection_the_delete_verb_removes`; `::the_composer_and_the_editor_agree_on_words` (the shared geometry's own agreement test) | pass | `ComposerText::selection_range` is the editor's `selection_range`, term for term: a character selection is inclusive of the head character, a line selection spans whole lines including their terminators, and both endpoints are clamped into the buffer and onto character boundaries. `delete_selection` was rewritten to read the shared range rather than computing its own. |
| **AC-M4 — deleting with nothing selected changes nothing and still consumes the key** | `tests/tui_composer_selection.rs::d_with_nothing_selected_changes_nothing_and_is_consumed`; `src/tui/composer_text.rs::tests::deleting_with_nothing_selected_changes_nothing` | pass | The test binds `d` to an app-level action first and asserts the action did **not** fire: the letter is the composer's, so it must not leak. |
| **AC-M5 — the copy is reached by `space` then `y` in the help box that opened, and copies exactly the selected text** | `tests/tui_composer_selection.rs::space_then_y_copies_the_composers_selection`, `::the_copy_takes_exactly_the_selection`; `src/tui/components/dialogs/help.rs::tests::the_chord_route_copies_in_normal_mode` | pass | The reachability is proven end to end: the chord opens the box, the box returns the message, the app copies. The **range** is pinned by the report, which names the size of what was copied — a copy of the whole buffer would say "11 characters" and a copy of one character would say "1". The clipboard's own contents are **not** asserted in a test; see §4.1. |
| **AC-M6 — the `/help` command's box does not copy, including after `Esc`, and including in the chord's box after `Enter`** | `tests/tui_composer_selection.rs::the_help_commands_box_does_not_copy_even_after_esc`, `::the_chords_box_copies_after_the_same_esc`, `::y_is_a_filter_character_in_insert_mode_in_both_routes`; `src/tui/components/dialogs/help.rs::tests::the_command_route_never_copies_even_in_normal_mode`, `::a_command_route_box_that_esc_reaches_normal_still_does_not_copy`, `::a_chord_route_box_in_insert_mode_filters_instead_of_copying` | pass | The two integration tests are a **pair**: the same box is opened in Insert, `Esc` moves it into Normal, and the chord's box copies while the `/help` box does not. The mode is therefore the same in both, and only the route differs — which is the whole claim. |
| **AC-M7 — the copy reports its outcome in both cases** | `tests/tui_composer_selection.rs::the_copy_reports_when_there_is_nothing_to_copy`; `::the_copy_takes_exactly_the_selection` | pass | The success report names the **size** of the selection, not its text; see §3.5. |
| **AC-M8 — `p` inserts the clipboard's contents at the caret, and is a no-op that still consumes the key when the clipboard is empty** | `tests/tui_composer_selection.rs::p_is_the_composers_own_verb`, `::p_is_consumed_by_the_composer`; `src/tui/components/prompt.rs::tests::a_paste_inserts_the_text_at_the_caret` | pass | The **insertion** is pinned where it is deterministic, at the widget, where the pasted text is a parameter. The system clipboard is not asserted on; see §4.1. The verb always reports, in both branches. |
| **AC-M9 — the composer is the only surface with a paste verb; the editor and the terminal panel are unchanged** | `src/tui/editor.rs` and `src/tui/interactive_terminal.rs` are not touched; `tests/tui_render.rs` (111 passed); the editor's suites pass | pass | The editor's own verbs are unchanged, including its own yank and delete. |
| **AC-M10 — the claim table still has thirteen rows, the four verbs are `Live`, `y` is not claimed, and every claimed letter is still typed as text in Insert mode** | `src/tui/composer_claims.rs::tests::the_live_claims_are_the_motions_the_selection_verbs_and_the_insert_transition`, `::the_table_does_not_claim_space_or_y`; `tests/tui_composer_claims.rs::every_claim_table_letter_is_typed_as_text_in_insert_mode` | pass | `y` is **not** in the table: the copy gesture is the help box's, and one gesture has one owner. The composer does not claim it and the help box is not a composer surface. |
| **AC-M11 — the workspace gates are green** | §4 | pass | `verify.sh quick` green; workspace clippy green on 1.89.0 (the repository's minimum) and on 1.99.0 (the CI toolchain); `cargo fmt --all --check` clean; the composer guard and its self-test green. |

## 3. Production implementation evidence

### 3.1 One range — `src/tui/composer_text.rs`

`ComposerText::selection_range` is new, and it is the editor's `selection_range` term for term: the
same `clamp_offset` at the start, the same `next_char_boundary(last).unwrap_or(last)` at the end,
the same `line_start(final_line + 1)` for a line selection, and the same clamping of a stale line
index to the buffer's last line. `delete_selection` was rewritten to **read** the range instead of
computing its own, which is what makes AC-M3 true by construction rather than by coincidence.

`selected_text` reads the same range, so what is copied is what is deleted and what is painted.

### 3.2 The verbs — `src/tui/composer_text.rs` and `src/tui/app/mod.rs`

`toggle_select_chars` is the shared machine's `toggle_char`: `v` is a mode, and a second press
discards the selection rather than re-anchoring it. `select_line` is the editor's `SelectLine`:
the shared machine's `line_select_extends` says whether this press grows the selection, and the
composer's own `next_line_content_start` says which line that is — the two halves from where they
belong.

`run_composer_claim` gained four arms. `d` deletes the selection; with nothing selected it changes
nothing and the key is still consumed. `p` reads the clipboard and pastes at the caret.

### 3.3 The rendering — `src/tui/components/prompt.rs`

`PromptWidget::render` now splits each line into the unselected part, the selected part and the
rest, and paints the selected part with `theme.selection` as a background and `theme.selection_text`
as the foreground. The unselected bytes are one span in the theme's foreground, exactly as before.

The offsets are byte offsets into the whole buffer, so they are translated into each line's own
coordinates before they slice it. They are character boundaries — the model's range method
guarantees that — so a slice cannot split a code point.

### 3.4 The route — `src/tui/components/dialogs/help.rs` and `src/tui/app/types.rs`

`HelpRoute` is new: `Chord` (the composer's help) and `Command` (the app's help). The route
travels **in the open request**: `Dialog::Help(HelpRoute)`, so a call site cannot open the help box
without saying which help box it is. `HelpDialog` carries the route, and `with_route` sets it.

The `HelpRoute::Command` default is the one that **cannot** copy, so a box opened without a route —
the pre-rendered constructor, the docked help panel — is inert rather than armed.

The copy is a `TuiMsg`, not a dialog-side mutation. `open_dialog` pushes a *clone* of the dialog onto
the focus manager, and the clone is what receives the keys; a clone cannot reach the app's
composer. So the box returns `TuiMsg::CopyComposerSelection` and the app performs the copy — the
same pattern `q` already uses to close a box.

**`?` shares the chord route with `space`.** Both are bound to `InputAction::Help` in both maps, and
both are Normal-mode help chords in the composer, so both open the same box in the same mode. The
route distinguishes *which help box* this is, not *which key* opened it, and a second spelling of
the copy gesture is exactly what "one gesture, one owner" rules out.

### 3.5 The reports — `src/tui/app/mod.rs`

Both clipboard verbs report, and the copy's success report names the **size** of the selection
rather than its text. Toasts are retained for `/logs`, and a preview of the composer's contents in
a retained log is a copy of the user's text in a place they did not ask for it to be. The size is
also what pins the range in a test (§2, AC-M5).

The paste reports in both branches — "Pasted N characters" and "Nothing to paste — the clipboard is
empty" — because a silent no-op teaches the user that the key is dead. The pasted text is not
echoed back, for the same reason.

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
| `tests/tui_composer_selection.rs` (new) | 18 passed, 0 failed |
| `tests/tui_composer_motions.rs` | 13 passed, 0 failed |
| `tests/tui_composer_claims.rs` | 29 passed, 0 failed |
| `tests/tui_render.rs` | 111 passed, 0 failed |
| `cargo test --lib tui::` | 2180 passed, 0 failed (4555 filtered out) |

**The system clipboard is not asserted on, and that is deliberate.** The clipboard is **process-
global state**: a test that wrote to it would race every other test in the process — including
this file's own copy test, which writes to it through the app — and would clobber whatever the
user had copied. An early version of these tests did exactly that, and it failed intermittently
with the clipboard holding another test's text. The clipboard's *contents* are therefore not
asserted anywhere.

What **is** asserted is everything either side of the system boundary:

- the **range** the copy takes, by the size the report names — a copy of the whole buffer or of
  one character would report a different size;
- the **text the paste inserts**, at the widget, where the pasted text is a parameter;
- the verbs' **reachability and consumption**, through the app's real key path.

The one line between them — the app reading the system clipboard — is the same system boundary
every other clipboard call in the app shares (`copy_message`, the share dialog, the session
export), and none of them is asserted at that boundary either.

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

`scripts/verify.sh full` is therefore not green on this machine for the same reason it was
not green at the baseline: its sweeps include the `/tmp` family above. Hosted CI is the
environment where that suite is expected to pass, and **no hosted CI run is claimed for
this milestone.**

## 5. Invariant review

| Invariant | Result | Note |
|---|---|---|
| I1 — one owner per chord per surface | pass | The claim table still has thirteen rows and the guard passes. The copy gesture's owner is the help box, not the composer, and `y` is not in the table. |
| I2 — a claimed key never falls through | pass | The four live verbs are consumed by the composer. `d` with nothing selected changes nothing and is still consumed, and the test asserts that the app-level action bound to `d` did not fire. |
| I3 — the composer is a frontend-local string | pass | The selection is a frontend-local range over a frontend-local string. Nothing is stored, and the copy is an explicit user action on text the user can already see. |
| I5 — the resolved mode is the gate | pass | Unchanged. The route is a property of the **dialog**, not of the composer's mode, and it does not interact with the claim gate. |
| I7 — one redraw cadence | pass | The selection is painted in the render path that already exists. No timer, no poller, no second redraw. |
| The editor's behaviour is unchanged | pass | `src/tui/editor.rs` is not touched. The composer mirrors the editor's range rule; it does not change the editor's. |
| The terminal panel is unchanged | pass | Its selection and its own verbs are untouched. |

## 6. Failure and recovery review

The selection verbs are pure buffer arithmetic: no I/O, no background task, no durable state.

The clipboard is the one failure surface, and it is best-effort by design. A failed **write** is
reported as an error, not swallowed. An empty or unavailable clipboard makes the paste a no-op
that still consumes the key, and it is reported — a silent no-op teaches the user that the key is
dead. On a build without the default-on `arboard` feature, `read_from_clipboard` always returns
`None` and the paste verb is inert, which is exactly the "unavailable" branch.

Two subtleties are worth recording:

- **A selection's points are byte offsets, and an edit does not move them.** This is the editor's
  convention, and the composer's `selection_range` clamps its endpoints into the buffer and onto
  character boundaries, so a stale selection can neither panic nor slice mid-code-point. The
  composer and the editor therefore agree on the same stale selection.
- **The head character at the very end of a buffer is not inside a character selection.** The
  shared boundary rule is `next_char_boundary(head).unwrap_or(head)`, and `next_char_boundary` is
  `None` at the end of the buffer. The editor's rule is the same, term for term, so this is a
  shared convention rather than a composer defect — but it is visible, and it is recorded here
  rather than left for a reader to discover.

## 7. Migration and compatibility review

- **Migration.** None. Nothing is stored, and no schema or protocol changes.
- **Compatibility.** `PromptWidget`'s public API is intact — the new accessors are additions. The
  `Dialog` enum's `Help` variant changed from a unit variant to `Dialog::Help(HelpRoute)`; the
  type is crate-internal, and the two call sites now name their route explicitly.
- **The selection's range is a new concept in the composer**, but not a new convention: it is the
  editor's, and the two are now the same code.
- **The clipboard is a new outbound surface for the composer.** It is the system clipboard, it is
  reached only by an explicit user gesture, and it holds only text the user could already copy by
  hand.

## 8. Security review

The composer remains a frontend-local string delivered to the daemon only on submit. The selection
adds no authority, no path handling, no network surface and no durable record.

The **clipboard** is the one new egress, and it is bounded:

- It is reached only by an explicit gesture — `space` then `y` — and only in the help box the
  chord opened. The `/help` command's box does not copy, and the composer itself does not claim
  `y`.
- It writes the **selection**, nothing else: the range is the shared range, so it is exactly what
  the user highlighted and nothing more.
- The report names the **size** of what was copied, not the text, so the user's prompt does not
  enter the retained toast history.
- The `arboard` feature is default-on, and the code compiles and behaves without it: the paste is
  then inert and says so, and the copy reports its failure.

## 9. Documentation and operations

| Document | Change |
|---|---|
| `src/tui/composer_text.rs` | The selection range, the selected text, the two selection verbs, and the next-line helper. |
| `src/tui/components/prompt.rs` | The selection verbs' accessors, the range and text accessors, and the rendering of the selection. |
| `src/tui/composer_claims.rs` | Four `Live` rows; the module doc and the table doc updated to say which verbs are live. |
| `src/tui/app/mod.rs` | `run_composer_claim`'s four arms; `copy_composer_selection`; the `/help` command's route. |
| `src/tui/app/types.rs` | `Dialog::Help(HelpRoute)` and `TuiMsg::CopyComposerSelection`. |
| `src/tui/app/input.rs` | The chord's route and the copy message's handler. |
| `src/tui/app/modal.rs` | The help dialog is built with the request's route. |
| `src/tui/components/dialogs/help.rs` | `HelpRoute`, the route on the dialog, and the `y` arm. |
| `architecture/tui.md` | The claim table's "Lands with" column, and a new subsection on the selection range, the copy gesture and the route. |
| `docs/tui.md` | A new section on selecting and the clipboard in `[NOR]` mode, and the copy gesture. |
| `plans/subsystems/tui-composer-helix-parity-roadmap.md` | M005's status; §10's decision table; §12's status table. |
| `plans/implementation/tui-composer-helix-parity/005-composer-selection-and-clipboard.md` | New. |
| `plans/registry.md` | M005's row, the parent row's status, and the dependency audit. |

**No ADR was written or changed.** ADR-0042's decision is untouched: M005 claims no key, and
part 7's copy gesture is implemented in the box part 7 names.

## 10. Unresolved findings

| Severity | Finding | Owner |
|---|---|---|
| low | **The system clipboard's contents are not asserted in a test.** The clipboard is process-global state, and a test that wrote to it would race every other test in the process and clobber the user's own clipboard. The range the copy takes is pinned by the size the report names, and the text the paste inserts is pinned at the widget; the one line between them is the same boundary every other clipboard call in the app shares. | Deliberate. Recorded here rather than left as a silent gap. |
| low | **The head character at the very end of a buffer is not inside a character selection.** The shared boundary rule is `next_char_boundary(head).unwrap_or(head)`, and the editor's rule is the same, term for term. Fixing it would change the editor's selection at the end of a document, which is a change to the shared geometry and therefore to a surface this milestone does not own. | Deliberate for now. A shared-geometry change is a separate decision. |
| low | The copy reports the **size** of the selection, not the text. The plan said "a success message naming what was copied", and a size is a naming — but a user with two selections of the same length cannot tell them apart from the report. | Deliberate. The text is not echoed because toasts are retained for `/logs` and the composer can hold a secret. |
| low | The paste reports "Pasted N characters" even on success. The copy reports on success too, so the two verbs are symmetrical, but a paste is visible in the composer and the report is therefore redundant. | Deliberate. A verb that sometimes reports and sometimes does not is a verb whose silence is ambiguous. |

No high or medium finding remains.

## 11. Roadmap disposition

- M005 is **closed**. M006 (undo, redo and the insert transitions) becomes **eligible for fresh
  planning**: its hard dependencies, M005 and M003, are both closed.
- M007 is unchanged: it waits on M004, M005 and M006, and M004 and M005 are now closed.
- The parent roadmap's open decision **D4** (the undo scope) stays open and stays owned by M006.
- The parent roadmap's **D5** (the paste scope) is **discharged by this milestone** and is
  recorded in §10 of the plan: the editor has its own paste milestone in the editor roadmap, and
  the only open surface was the terminal panel, which M005 does not touch.
- The parent roadmap's **D6** (the `a` conflict with the vim map's `FocusSidebar`) is untouched by
  this milestone and remains owned by M006.

## 12. Registry updates

- Added M005's row to **Dependency-ready implementation plans** with this closure record.
- The parent roadmap's row now reads "M001 closed; M002 closed; M003 closed; M004 closed; M005
  closed; M006 eligible for fresh planning".
- The **Milestones eligible for fresh planning** row moved from M005 to **M006**, with both hard
  dependencies discharged.
- **Dependency audit.** Closing M005 unblocked **M006**: the roadmap's dependency graph lists M005
  as a hard dependency of M006 ("the undo verbs need the selection model, and `d` and `u` must
  agree about what an edit is") and M003 as its other. M003 was already closed, so M006 is now
  dependency-ready. M007 still waits on M006.
