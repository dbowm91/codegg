# TUI Subagent and Job Visibility Convergence — Milestone 003 — Closure Status

Status: **closed**

Source implementation plan:

- `plans/implementation/tui-subagent-and-job-visibility-convergence/003-agent-descriptors.md`

Source subsystem roadmap:

- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`

Repository baseline: `e8a6b5ca2` (the committed C001/C003/C004/C006 tree), plus the
**uncommitted C002 working tree**.

Implementation: **working tree, uncommitted** at the time of this record. No
commit hash is claimed because none exists; this record describes the diff, not
a commit.

Scope note: the working tree also contains C002's uncommitted implementation and
this session's planning changes. Several files touched by C005
(`src/tui/components/sidebar.rs`, `src/tui/components/dialogs/session.rs`,
`src/tui/app/mod.rs`) therefore carry C002's diff as well — including C002's
`Shift+T` → `Ctrl+T` comment corrections, which are **not** C005 work.

## 1. Executive finding

C005 is implemented and verified locally. A subagent row now shows what its
agent **is** as well as what it was asked to do: the curated pool's `profile`
and `usefulness` travel from the pool entry, through the spawn event and the
projection, onto every surface that already renders a subagent row.

The milestone went as planned. All five work packages landed. **Neither stop
condition from §14 of the plan was hit** — no TUI-side pool lookup was
introduced, the bound was brought under a named and asserted budget, and the
desktop bridge was deliberately *not* widened. The plan's three recorded traps
(the desktop parity requirement being satisfiable by doing nothing, the two
generator-emitted lies, and the measurability of the bound) were each handled as
recorded rather than discovered late.

## 2. Requirement-to-evidence matrix

| Requirement (plan §13) | Evidence | Result |
|---|---|---|
| 1. A running subagent's row shows its pool profile on **every** surface, `/sessions` included | `summary_line_shows_the_descriptor_when_one_arrived`; `a_subagent_row_shows_its_descriptor_when_the_column_has_room`; `a_subagent_row_carries_the_projected_descriptor`; `the_dialog_shows_the_pool_descriptor`; `the_subagent_tab_shows_the_task_and_the_descriptor_together` | pass — five surfaces, five tests |
| 2. A pre-milestone node renders with no empty column and no error | `an_unnamed_node_still_renders_a_row`; `summary_line_falls_back_to_the_description_without_a_descriptor`; `a_blank_descriptor_is_treated_as_absent`; `the_dialog_falls_back_to_the_task_without_a_descriptor`; `the_subagent_tab_omits_the_agent_section_without_a_descriptor` | pass |
| 3. Old daemon + new frontend, and new daemon + old frontend, both connect | `a_node_without_descriptors_decodes_with_them_absent`; `subagent_started_without_a_name_is_still_a_node` | pass — both fields are `#[serde(default)]` |
| 4. No surface displays a descriptor that did not arrive in the projection | `check_tui_subagent_name_pool_isolation.py` + `--self-test`; `descriptor_is_the_projected_profile_or_nothing`; `a_name_outside_the_pool_yields_no_descriptor` | pass — see §4 |
| 5. A 16-subagent session with maximal descriptors stays inside the asserted budget | `a_full_turn_of_maximal_descriptors_fits_the_budget`; `a_turn_of_the_largest_curated_descriptors_fits_the_snapshot_budget`; `no_curated_descriptor_exceeds_the_projection_cap` | pass — see §5 |
| Descriptors survive the adapter verbatim | `subagent_started_carries_name_and_run_id_into_the_node` (extended, not duplicated) | pass |
| Replay determinism holds with descriptors present | `replaying_a_subagent_script_is_deterministic` (fixture now carries them); `replay_preserves_the_recorded_run_id`; `replay_is_deterministic_across_repeated_application` | pass |
| The publisher's descriptor belongs to the name that was leased | `a_lease_yields_the_entry_behind_its_own_name`; `a_suffixed_lease_resolves_to_the_same_entry_as_its_base` | pass |
| I1 — rows originate in the projection | `a_named_subagent_reaches_the_rendered_row` (extended); one row derivation unchanged | pass |
| I2 — transport authority | `check_tui_transport_authority.py` | pass — C005 binds no key |
| I5 — rendering a descriptor cancels nothing | No code path added; review of the diff | pass |
| Descriptor provenance — the reducer never re-draws | Adapter/replay copy verbatim; reducer untouched | pass |
| Normalise truncates at the named cap | `subagent_descriptors_truncate_at_their_own_cap`; `a_descriptor_within_the_cap_is_left_alone` | pass |
| No execution surface added | `git diff --stat -- docs/execution-ownership.toml` empty; `schema.rs` untouched | pass |

## 3. WP evidence

### WP-1 — Publisher

`SubagentNameLease::entry()` resolves the pool entry behind **its own leased
name**, bound to `self.name` rather than to a caller-supplied string. That
binding is the point: a second lookup keyed on anything else is the attribution
defect C001 already recorded once. `worker.rs` takes the descriptor off the
lease and puts both fields on `AppEvent::SubagentStarted`. A non-pool name
yields `None`, carried honestly rather than substituted — asserted by
`a_name_outside_the_pool_yields_no_descriptor`, which also pins that
`Sally-Bob` does *not* resolve to `Sally`.

### WP-2 — Wire shape

Two additive `Option<String>` fields on `AppEvent::SubagentStarted` and
`CoreEvent::SubagentStarted`, mirrored in the `AppEvent → CoreEvent` mapping,
copied verbatim by the projection adapter and by the replay publication path,
and `#[serde(default)]` on `AgentTreeNodeProjection` with `normalise()`
truncation at the new cap.

The adapter's existing test was **extended** rather than duplicated: there is
one adapter seam, and a parallel suite for the second pair of fields would be a
second thing to keep in step rather than a second defect to catch.

### WP-3 — Row

`SubagentRow` gains `profile` and `usefulness`; `sync_subagents_from_snapshot`
copies both in the same field-by-field pass. `a_named_subagent_reaches_the_rendered_row`
was extended with descriptor assertions for the same reason.

Two helpers were added to make the fallback rule expressible once:
`descriptor()` (the projected profile, or `None`, treating blank as absent the
way `label()` already does for a blank name) and `topic()` (descriptor, else the
task description).

### WP-4 — Render

Five surfaces, each with a named test:

| Surface | Change |
|---|---|
| Working strip (`summary_line`) | `topic()` in the activity slot |
| Sidebar `Subagents` | `profile` appended, width-budgeted |
| `Ctrl+T` overlay (`TaskRow::Subagent`) | `profile` preferred over `description` |
| `/sessions` dialog (`SubagentListRow`) | `profile` preferred over `description` |
| Subagent tab | new **Agent** section beside the existing **Task** section |

### WP-5 — Comments, guards, docs

The two "Not rendered by the TUI today" sentences were corrected **in
`scripts/generate_subagent_names.py`**, not in `generated.rs` — they are emitted
by the generator, so editing the output would have left the next regeneration to
reintroduce them and `--check` to pass on two identical lies again. The equivalent
prose in `names.toml:12-18` was corrected in step. Regenerated; `--check` green.

`architecture/tui.md` gains the provenance rule and the one-row/five-surfaces
trade; `docs/tui.md` describes what a row now shows and corrects "Three surfaces"
to five.

## 4. The TUI resolves no names, and that is enforced

This is the milestone's own I1 boundary, so it is enforced rather than asserted
in prose. `scripts/check_tui_subagent_name_pool_isolation.py` fails if any module
under `src/tui/` names the pool at all (`pool_entry`, `SUBAGENT_NAMES`,
`subagent_names`, `SubagentName`, `SubagentNameLease`), ignoring comments so that
a doc comment *explaining* the invariant is not a violation.

**Non-vacuity is proven two ways**, because a guard that cannot be shown to fail
is not evidence:

1. `--self-test` — 5 planted violations all detected; 4 allowed cases (including
   two lines that name `pool_entry` inside comments) not flagged. Wired into
   `verify.sh quick` **and** CI so it cannot rot silently.
2. A real injection into `src/tui/components/sidebar.rs` — a `use` statement at
   line 1 and a bare `pool_entry(&row.name)` call inside the subagent loop. The
   guard reported **both** with file:line and exited 1; after restoring the file
   it passed again.

## 5. The bound is measured, named, and asserted

Re-measured at this baseline, in UTF-8 **bytes** (the plan's figures were correct;
a naive character count under-read them because the pool uses typographic quotes):

| Quantity | Bytes |
|---|---|
| longest `profile` (Greg) | 223 |
| longest `usefulness` (Susan) | 107 |
| largest single node | 291 |
| whole pool, both fields | 10,973 |
| worst-case tree growth (16 × 291) | **4,656** |

`MAX_PROJECTION_DESCRIPTOR_BYTES = 256` — above the 223-byte measured maximum so
no curated descriptor is silently truncated, with authoring headroom.
`MAX_PROJECTION_SUBAGENT_DESCRIPTOR_BYTES = 16 × 256 × 2 = 8,192`, **derived**
from the caps rather than measured, so the ceiling is enforced rather than
observed. Reusing `MAX_PROJECTION_STRING_BYTES` (4,096) was rejected: a node
already carries up to 4 KB of `description`, so that bound would leave the
per-descriptor budget unenforced and a full tree would carry 128 KB of prose.

Two assertions guard it, one on each side of the crate boundary: a `const _: () =
assert!` that the derived budget still equals what the caps imply (a runtime
`assert_eq!` between two `const usize`s folds to `assert!(true)`, which clippy
correctly rejects), and a measured 16-node aggregation in
`a_full_turn_of_maximal_descriptors_fits_the_budget`. The real pool is asserted
in the root crate, where the pool actually lives.

## 6. Two decisions the plan asked to be recorded

### 6.1 The desktop bridge was deliberately not widened

`SubagentSummaryView` is `['task_id','agent','description','status','result_summary']`.
It has **no `display_name` and no `run_id`** — C001's two additive fields never
reached it — and descriptors follow that precedent.

`check-bindings.mjs` was read to confirm the plan's §3.2 finding: it iterates
`expected` and asserts each **listed** field exists on both sides
(`check-bindings.mjs:35-41`). It cannot detect a Rust struct gaining a field it
never mentions. The guard passes today and would have passed identically had the
descriptors never been added — so citing it as evidence that desktop parity is
maintained would be a **vacuous claim**, and it is not made here. Routing
descriptors to desktop while leaving the name behind would present a row with a
personality and no name; that is a third milestone with its own field-list
update, and it is listed in §8 as a stop condition.

### 6.2 One-line surfaces show the descriptor *or* the task; the tab shows both

The addendum's objective is "what its agent *is* **as well as** what it was asked
to do", and a single terminal line cannot legibly carry two pieces of prose —
whichever is ellipsized second reads as noise. So the four one-line surfaces
render `topic()` (descriptor when present, the task otherwise, with the task as
the exact pre-milestone fallback), and the **subagent tab** carries `Task` and
`Agent` side by side, wrapped and untruncated.

That is what makes the trade lossless, which is why the tab gained a section
rather than staying a name and a status. It is a narrowing of the literal wording
of plan §5 item 3 ("every surface that already renders a subagent row"), made
because the alternative — two ellipsized prose fields per row — is strictly
worse to read. Recorded here rather than buried.

The sidebar additionally drops the descriptor below `MIN_SIDEBAR_DESCRIPTOR_CHARS`
(12). The row is one line in a column shared with the sections below it, so a
three-character stub reads as a rendering fault; below the floor the row is
byte-identical to what it was before, which
`a_subagent_row_drops_the_descriptor_when_the_column_is_too_narrow` asserts by
comparing rendered text against the descriptor-free widget.

## 7. Verification — local runs only, no CI

Every command below was run locally on this machine. **No hosted CI run was
performed for this milestone**, and nothing here should be read as a CI receipt.

| Command | Result |
|---|---|
| `cargo fmt --check` | pass |
| `scripts/verify.sh quick` | pass (includes the new guard and its self-test) |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `python3 scripts/generate_subagent_names.py --check` | pass (46 names) |
| `python3 scripts/check_projection_transport_isolation.py` | pass |
| `python3 scripts/check_projection_transport_lifecycle.py` | pass |
| `bash scripts/check_projection_disclosure.sh` | pass |
| `python3 scripts/check_websocket_bounds.py` | pass |
| `python3 scripts/check_tui_transport_authority.py` | pass |
| `python3 scripts/check_tui_subagent_name_pool_isolation.py` | pass |
| `python3 scripts/check_tui_subagent_name_pool_isolation.py --self-test` | pass (5 detected, 4 not flagged) |
| `node apps/desktop/scripts/check-bindings.mjs` (run from `apps/desktop`) | pass — see §6.1 for what this does and does not prove |
| `cargo test -p codegg-protocol --lib projection` | 93 passed, 0 failed |
| `cargo test --lib subagent` | 97 passed, 0 failed |
| `cargo test --test tui_sessions_pending_sections` | 11 passed, 0 failed (was 9; +2) |
| `cargo test --test tui_side_panels` | 31 passed, 0 failed |
| `cargo test --test tui_main_buffer` | 48 passed, 0 failed (was 46; +2) |
| `cargo test --lib` | 6597 passed, **8 failed** — attributed in §8 |
| `git diff --stat -- docs/execution-ownership.toml` | empty, as required |
| `crates/codegg-core/src/session/schema.rs` | untouched; `STORAGE_LAYOUT_VERSION` unchanged |

`check-bindings.mjs` is **not** referenced by `verify.sh` or CI — it is
change-triggered only, and resolves its paths relative to `apps/desktop`, so it
must be run from there.

### 8. The eight `cargo test --lib` failures are not this milestone's

The full-parallel run reported 8 failures. All eight were re-run **in isolation,
single-threaded**, and the attribution splits cleanly:

| Test | Full parallel run | In isolation | Attribution |
|---|---|---|---|
| `core::daemon::tests::session_create_through_daemon` | fail | **fail** | pre-existing: `InvalidRootPath("/tmp")` at `src/core/daemon.rs:4610` |
| `core::daemon::tests::turn_submit_rejects_when_active` | fail | **fail** | pre-existing: same registration panic |
| `tui::async_cmd_tests::socket_daemon_mode_never_takes_the_remote_input_path` | fail | **fail** | pre-existing: environment-dependent daemon-socket binding |
| `core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace` | fail | **pass** | load-dependent; same `/tmp` registration path as the two above |
| `tui::commands::interactive_terminal::tests::a_hidden_terminal_surface_does_not_arm_the_loop` | fail | **pass** | load-dependent PTY fixture |
| `…::a_live_terminal_surface_repaints_with_no_further_user_input` | fail | **pass** | load-dependent PTY fixture |
| `…::panel_rendered_buffer_shows_live_pty_output` | fail | **pass** | load-dependent PTY fixture |
| `…::reporting_the_mode_never_moves_the_keyboard` | fail | **pass** | load-dependent PTY fixture |

The three stable failures are **the same three** the C002 closure record recorded,
with the same root causes, and they live in `src/core/daemon.rs` and
`src/tui/mod.rs` — **neither file is touched by the C005 diff**. This milestone
does not add a stashed-baseline comparison of its own; the identical three were
already attributed against a stashed baseline in the C002 record, and re-running
the expensive attribution was not repeated for a milestone that touches neither
file. That is a weaker form of evidence than C002's and is labelled as such.

The other five pass in isolation and fail only under full-suite parallel load,
with a *different* membership each run — the same signature C002 recorded. This
is recorded as load-dependent flakiness present on both trees, **not** as
"verified unrelated".

## 9. What was not verified

- **No runtime observation.** The TUI was never launched. Every claim about what
  the user sees rests on static reading plus buffer-level tests. The
  `the_dialog_shows_the_pool_descriptor` and
  `the_subagent_tab_shows_the_task_and_the_descriptor_together` assertions prove
  the text reaches the rendered buffer, not that a human read it comfortably.
  In particular, the §6.2 trade — descriptor instead of task on four one-line
  surfaces — has not been looked at by anyone.
- **No CI receipt.** See the label at the head of this section.
- **The desktop bridge was not exercised at runtime.** No desktop app was built
  or launched; the guard is a static field-list comparison (§6.1).
- **C004 remains unclosed.** C005's hard dependency is satisfied in substance,
  not in process. If C004's closure review finds a defect in the additive-field
  chain, C005 must be re-reviewed against the corrected chain before it lands.
  That caveat carries forward unchanged from the plan.

## 10. Reconciled records

- `plans/registry.md` — the subsystem row's status cell and its dependency-ready
  row now read **closed** for C005 with this record linked; a **Recently closed**
  row was added. The row also contained a **self-contradiction** fixed here: its
  head read "C005 ready … C007 unblocked, ADR-0040 accepted" while its trailing
  sentences still read "C005 is not started … ADR-0040 is drafted (`proposed`)".
- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`
  — the C005 section reads **implemented and closed** with this record linked.
- `plans/implementation/tui-subagent-and-job-visibility-convergence/003-agent-descriptors.md`
  — status moved from *ready for handoff* to **implemented**, pointing here.

`architecture/bus.md:54` and `architecture/protocol.md:321-322` were checked as
the plan directed rather than assumed: both count event **variants**
(`SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` =
4), not fields, so neither count changes. No edit was made and none is needed.

## 11. Milestones beyond C005

C003, C004, and C006 remain **implemented and unclosed** in the same working
tree — unchanged by this milestone. C007 is **eligible for fresh planning** and
**not started**: ADR-0040's acceptance authorized the design and did not build
the feature. `TurnSteer` still has zero TUI callers, `InputTarget` still has only
`Build` and `Chat`, no key is bound, a subagent tab is still **read-only**, and
closing one still cancels nothing (I5). C005 makes a subagent's name and
character visible; it does not make the subagent addressable.