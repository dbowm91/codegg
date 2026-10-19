# TUI Subagent and Job Visibility Convergence — Milestone 002 — Closure Status

Status: **closed**

Source implementation plan:

- `plans/implementation/tui-subagent-and-job-visibility-convergence/002-sessions-dialog-pending-sections.md`

Source subsystem roadmap:

- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`

Repository baseline: `2acce2138`, plus the committed C001/C003/C004/C006 tree
at `e8a6b5ca2`.

Implementation: **working tree, uncommitted** at the time of this record. No
commit hash is claimed because none exists; this record describes the diff, not
a commit.

## 1. Executive finding

C002 is implemented and verified locally. `/sessions` — the surface a user
opens first to find their sessions — now lists the active subagents and the
live jobs of the current session beneath its session list, navigable with the
same `j`/`k`, with `Enter` on a subagent opening the same transcript tab the
sidebar and the `Ctrl+T` overlay open for the same `task_id`.

The milestone went as planned. All four work packages landed, and the single
stop condition from §14 of the plan — *a second derivation of subagent rows* —
was avoided rather than hit: the sections are fed by `App::task_rows()`, the
one existing derivation, and no new projection or liveness rule was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Sections appear with rows and are absent without them | `sections_appear_when_rows_exist_and_vanish_when_they_do_not`, `a_section_with_no_rows_contributes_no_heading`, `a_session_with_no_live_work_shows_neither_section` | pass | An empty section contributes **no heading**, not an empty one. |
| `j`/`k` walk sessions → subagents → jobs, never landing on a header | `navigation_walks_sessions_then_subagents_then_jobs_and_skips_headings` | pass | Walks down 7 rows and back up again, asserting no `Header` is ever the resting place. |
| `Enter` on a subagent resolves the same tab as the sidebar's `Enter` | `enter_on_a_subagent_row_opens_the_shared_tab_message` (unit), `enter_on_a_subagent_row_opens_the_same_tab_the_sidebar_opens` (integration) | pass | The integration test runs **both** routes on two apps and compares the resulting `tab_id`; it does not check one and assume the other. |
| `Enter` on a job row changes nothing and closes nothing | `enter_on_a_job_row_changes_nothing_and_closes_nothing` (unit), `enter_on_a_job_row_changes_nothing` (integration) | pass | Asserts no tab opened **and** the dialog is still on the focus stack. |
| A row disappearing does not move the cursor onto a different entity | `a_vanished_row_does_not_move_the_cursor_onto_another_entity`, `focus_falls_back_to_a_selectable_row_when_the_focused_row_is_gone` | pass | §8 of the plan, by identity rather than clamped index. |
| A stale `selected` index cannot panic or strand the cursor | `a_stale_selection_index_past_the_end_does_not_panic_or_strand_the_cursor` | pass | `selected` is a `pub` field assigned directly by the click handlers; `select_up` walks backwards from it, which is the shape that indexes out of bounds. |
| The mounted focus-manager instance carries the rows | `both_instances_receive_the_sections`, `the_painted_dialog_lists_the_live_subagents_and_jobs` | pass | The historical empty-dialog bug, restated for the new sections. |
| Both sections are actually painted, not merely held | `the_dialog_renders_both_sections`, `the_rendered_dialog_shows_both_sections` | pass | The defect class here is data that arrives correctly and is never drawn. |
| Live refresh while the dialog is mounted (WP-4) | `a_subagent_that_starts_while_the_dialog_is_open_appears`, `the_refresh_is_a_no_op_while_the_dialog_is_closed` | pass | Driven through a real render pass, which is the production path. |
| Jobs are id-keyed, never index-keyed | `JobListRow.job_id`; `a_finished_subagent_and_a_finished_job_are_both_left_out` | pass | Required adding `job_id` to `TaskRow::Job` — see §5. |
| I1 — rows originate in the projection | `App::session_dialog_pending_rows` derives from `App::task_rows()` | pass | No name is minted and no status is inferred; see §3. |
| I2 — panel input authority | `scripts/check_tui_transport_authority.py` | pass | Run by `scripts/verify.sh quick`. |
| I5 — opening a subagent row cancels nothing | `TuiMsg::OpenSubagentTab` → `App::open_subagent_tab` | pass | The same message and the same handler every other surface uses; a view change, not a turn action. |
| No send-to-subagent capability implied | Code review of the diff | pass | Nothing in this milestone addresses a subagent as a message target. |
| Bounded suite | `scripts/verify.sh quick`, `cargo clippy --workspace --all-targets -- -D warnings` | pass | Local runs only; see §6. |

## 3. One derivation, not a second

The plan's §14 stop condition was: *"Feeding the dialog requires a **second**
derivation of subagent rows."* It was avoided.

`App::session_dialog_pending_rows` consumes `App::task_rows()` — the same call
the `Ctrl+T` overlay and the working strip's row-0 counter already make — and
projects the result into the dialog's owned row types. The "is this subagent
live?" and "is this job live?" rules stay in exactly one place
(`build_task_rows`, filtering on `SubagentRow::is_running` and
`is_live_state`). A second call to `is_running` in the dialog would have created
a rule that could drift from the overlay's.

Two deliberate differences from the overlay, both recorded in the method's doc
comment:

- The overlay's `bounded()` cap is **not** applied. That cap exists because the
  overlay is carved out of the prompt allocation and can starve its own block;
  a scrolling modal has no such budget, and truncating to twelve rows there
  would hide live work for no reason.
- Headers and todos are dropped. The dialog builds its own section headings and
  has no todo section; every other row is carried across whole, keeping the
  identity (`task_id` / `job_id`) it was created with.

## 4. WP evidence

### WP-1 — Row model and navigation

`SessionDialogRow` is `Session(usize)` / `Subagent(SubagentListRow)` /
`Job(JobListRow)` / `Header(TaskGroup)`. Sessions **lead** the flat vector, so
a session's row index is also its index into `sorted_sessions()` — which is
what keeps `selected_session()` and every bulk-mode gesture working unchanged.

`selected_session()` was still rewritten to match on the row type rather than
to rely on that coincidence. It was also made **type-safe against the new
rows**: before, `sorted_sessions().get(self.selected)` would have resolved a
subagent row to the session sitting above it, and `toggle_selection` would have
ticked a bulk-archive checkbox against a subagent that bulk-archive cannot act
on. Both are now covered by tests.

`select_up`/`select_down` walk the vector and skip headings. `select_index`
(the click path) snaps forward off a heading rather than parking the cursor on
a line `Enter` does nothing with.

### WP-2 — Rendering

The render loop walks `rows()` uniformly, so `self.selected` indexes one
vector rather than two. With no pending rows the row space *is* the session
list, which is asserted by `a_sessions_only_dialog_renders_exactly_as_before`
as the regression guard for the rest of the `/sessions` suite. In bulk mode the
`[✓]` checkbox replaces the `▸` cursor marker — a checkbox column and a cursor
in the same cell is a column that means two things.

### WP-3 — `Enter` dispatch

One method, `activate_selected`, with three honest answers: session →
`SelectSession`; subagent → `OpenSubagentTab`; job → nothing. Dispatching the
shared message rather than opening a tab from the dialog is what makes "the
same tab every surface opens" true by construction rather than by three call
sites agreeing.

The `#[cfg(any())]`-disabled legacy `Send` arm was updated in step with the live
route so the retained source history does not contradict it. It is dead code
and is not compiled; that is stated rather than claimed as coverage.

### WP-4 — Live refresh

`App::refresh_sessions_dialog_pending_rows` is called from
`sync_subagent_rows_from_projection`, which runs on the render pass. It is a
no-op unless `ui_state.dialog == Dialog::Session`, so an ordinary turn does no
work for a surface nobody is looking at, and a snapshot that has not arrived
leaves the sections untouched — matching the strip's existing rule.

### WP-5 — Docs

`docs/tui.md` `/sessions` row now describes the sections and their keys.
`architecture/tui.md` gains the dual-instance rule extended to the C002
sections on both open and refresh, and its component tree notes the new
sections.

## 5. `job_id` was added to `TaskRow::Job`

The plan required jobs to be "keyed by `job_id`, never by list index". They
were not: `JobLike` dropped `job_id` when it was built from the projection, so
`TaskRow::Job` had no identity to key on at all. `JobProjection.job_id` has
always existed — it was simply not carried.

`job_id` is now on `JobLike` and `TaskRow::Job`, populated from the projection
rather than derived. This is additive and touches no wire format: the overlay
renders the same text and the desktop bridge is unaffected.

## 6. Verification — local runs only, no CI

Every command below was run locally on this machine. **No hosted CI run was
performed for this milestone**, and nothing here should be read as a CI receipt.

| Command | Result |
|---|---|
| `cargo fmt` | pass |
| `scripts/verify.sh quick` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `python3 scripts/check_tui_transport_authority.py` | pass |
| `python3 scripts/check_execution_ownership.py` | pass |
| `python3 scripts/check_scheduler_bypass.py` | pass |
| `cargo test --lib pending_section_tests` | 14 passed |
| `cargo test --test tui_sessions_pending_sections` | 9 passed |
| `cargo test --lib` | 6585 passed, 4 failed — all four attributed below |
| `cargo test --lib` (stashed baseline, same command) | 6572 passed, 4 failed — the same three, plus a *different* PTY test |
| `cargo test --test tui_side_panels` | pass |
| `cargo test --test tui_main_buffer` | pass |
| `docs/execution-ownership.toml` | unchanged (`git diff --stat` empty) |

### 7. The four `--lib` failures are not this milestone's

Attribution was done by running the **identical** `cargo test --lib` on the
stashed baseline (`git stash push -- src/ tests/ crates/ architecture/ docs/``,
then restored) and comparing. Three failures are identical on both trees. The
fourth is a different test in the same module on each tree, which is the
signature of load-dependent flakiness rather than a regression.

| Test | Baseline | This tree | Attribution |
|---|---|---|---|
| `core::daemon::tests::session_create_through_daemon` | fails | fails | pre-existing: `InvalidRootPath("/tmp")` at workspace registration |
| `core::daemon::tests::turn_submit_rejects_when_active` | fails | fails | pre-existing: the same registration panic |
| `tui::async_cmd_tests::socket_daemon_mode_never_takes_the_remote_input_path` | fails | fails | pre-existing: environment-dependent daemon-socket binding |
| *one `interactive_terminal` PTY test* | `a_hidden_terminal_surface_does_not_arm_the_loop` | `panel_rendered_buffer_shows_live_pty_output` | flaky under full-suite load; the failing test is **not** the same one on the two trees |

The PTY row deserves the honest phrasing rather than a tidy one: this tree's
failing test is **not** the baseline's failing test. Both are in
`tui::commands::interactive_terminal`, both are PTY-fixture assertions, and
this tree's one **passes in isolation in 0.24 s (run twice)**. A change that
broke a PTY mount deterministically would break the same test on both trees; it
does not. This is recorded as load-dependent flakiness present on both trees,
not as "verified unrelated".

## 8. What was not verified

- **No runtime observation.** The TUI was never launched. Every claim about
  what the user sees rests on static reading plus tests — including the
  `the_dialog_renders_both_sections` buffer assertion, which proves the text
  reaches the rendered buffer, not that a human read it comfortably.
- **No CI receipt.** See the label at the head of this section.
- **The dead legacy `Send` arm was not compiled.** It is kept consistent by
  hand; there is no compiler check behind it.

## 9. Two stale references fixed in passing

Both were pre-existing defects C001 left behind, found while working in these
files, and both are user-visible:

- `src/tui/app/render.rs` drew the overlay's own footer hint as
  `" Shift+T close "`. C001 retired `Shift+T`; the on-screen text still told
  the user to press a key that does nothing. This is the same I6 class the
  C001 guard was built for — the guard reads the help table, and these two
  strings were never in it.
- `architecture/tui.md` still described `src/tui/components/help_overlay.rs`
  as an existing-but-dead module. C001 deleted the file. The doc now says it
  was removed and where its one unique asset went.

## 10. Milestones beyond C002

### Factual correction — 2026-10-18

The paragraph above is accurate **as of this record's closure** and is left
unedited, per `plans/closure/README.md` (immutable except factual corrections).
Three of its statements stopped being true later the same day, and are corrected
here rather than by rewriting history:

- **C005 is no longer "not started".** It has an implementation plan,
  `plans/implementation/tui-subagent-and-job-visibility-convergence/003-agent-descriptors.md`,
  registered **ready for handoff**. Its hard dependency is C004, which remains
  *implemented and unclosed* — the dependency is satisfied in substance (the
  additive-field contract is stable and pinned by four named tests), not in
  process.
- **C007 is no longer blocked.** ADR-0041 was **accepted 2026-10-18** (Option A:
  route an addressed reply through the controlling parent turn's existing
  controller predicate). C007 is therefore *eligible for fresh planning* and is
  recorded as such. It remains **not implemented**: no plan exists, `TurnSteer`
  still has zero TUI callers, `InputTarget` still has only `Build` and `Chat`,
  and a subagent tab is still read-only.
- **C003/C004/C006 remain implemented and unclosed** — unchanged by this
  correction.

Registry rows for C001 and C002 were, until this correction, missing entirely
from the registry's *Recently closed* section while the dependency-ready table
still labelled C002 `ready`. Both rows are now present and C002 reads `closed`.

This record covers **C002 only**. C003, C004, and C006 are implemented in the
same tree and still have **no closure record**; the registry lists them as
`proposed`, which is the honest state until each has its own record. **C005 is
not started** and has no implementation plan. C007 remains blocked — ADR-0041
is drafted at `plans/adrs/ADR-0041-subagent-run-addressing-and-human-interaction-authority.md`
(status `proposed`, not accepted), and no capability implying interaction with a
subagent was added here.
