# TUI Subagent Presentation and Name Allocation — Milestone P001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-subagent-presentation-and-name-allocation/001-tab-surface-ownership.md`

Source subsystem roadmap:

- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md` §7 (P001)

Repository baseline reviewed: `31a412844fbbf36055b29245075121035eefc385`

Implementation commits or pull requests:

- Working tree only — not committed at the time of writing. The change is
  confined to `src/tui/app/render.rs`, `tests/tui_main_buffer.rs`,
  `docs/tui.md`, `architecture/tui.md`, and `.opencode/skills/tui/SKILL.md`,
  plus the plan records named above.

## 1. Executive finding

The reported symptom — "the buffer does not appear to belong to the selected
subagent", with the main conversation showing through the subagent's own view
— was an **overlay**, not a wrong-data bug. `render()` painted the viewport and
then painted the subagent view into the **same rect**, so the transcript stayed
legible through the subagent's text and in the rect's last column; the change
review had the same defect one level up, taking the viewport and then having
it taken back. The tab strip painted **foreground only** over a row carved out
of the viewport chunk, which made it the one surface in the TUI with no
background at all.

The viewport rect now has exactly **one owner**, chosen by
`App::viewport_owner()` and rendered by exactly one branch: the route's own
view, the change review, or the open subagent tab. The change review outranks
the tab — it is a modal decision surface the user has to answer — and the tab
outranks the transcript, because the tab *is* the buffer the user asked to
read. The subagent view fills its rect with the theme background before it is
decorated and renders into the rect's **full** width; the tab strip's row
carries the theme background too, and its active entry now wears the same
`primary`-on-`selection` treatment the project tab strip gives its active
entry.

**Two of the plan's five required tests were vacuous as written and were
caught by running them against the unfixed code.** §4.2. Both now fail against
the baseline and pass against the fix.

**One premise of the plan was corrected against source and the correction is
recorded rather than absorbed silently:** a subagent tab **cannot** show a
transcript, because `AgentRunSummary` carries no conversation
(`crates/codegg-protocol/src/projection/dto.rs:544-550`) and `RunDetailView`
is a manifest (`crates/codegg-core/src/run_store.rs:967-977`). The tab is a run
inspector, and the `docs/tui.md` claims that promised a transcript are
corrected — six of them, not the five the plan enumerated (§3.1).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| AC1 — with a tab open and no review, the main transcript is not visible anywhere in the viewport rect | `an_open_subagent_tab_replaces_the_transcript_instead_of_ghosting_over_it` | pass | Baseline-failing: 4 of 6 transcript tokens were visible at `31a4128` (§4.2) |
| AC2 — with a change review open, the review is visible and is not overlaid by the subagent view | `an_open_change_review_is_not_covered_by_a_subagent_tab` | pass | Baseline-failing (§4.2) |
| AC3 — the subagent view renders into the full width of the viewport rect | `the_subagent_view_fills_the_viewport_to_its_last_column` | pass | Baseline-failing: the last column held 20 spaces (§4.2) |
| AC4 — the tab strip row carries an opaque background | `the_tab_strip_row_is_opaque` | pass | Baseline-failing: the row's cells were `Color::Reset` (§4.2) |
| AC5 — `docs/tui.md` makes no claim that a subagent tab shows a transcript | diff review of `docs/tui.md`; §3 | pass | A prose correction, stated as such — no test claims it |
| Owner selection is a decision, not an ordering accident | `viewport_owner_tests` (4 unit tests in `src/tui/app/render.rs`) | pass | New behaviour, so no baseline-failing claim is made |
| The strip's base style carries a background | `the_tab_strip_row_is_opaque` | pass | The plan asked for a unit test; it is covered at the render level, which is the stronger form |
| A panic inside the subagent view is still caught and latched | the `catch_unwind` wrapper is unchanged and still wraps the whole dispatch; `render_component_fallback` still paints the viewport | pass | By inspection of the diff — the wrapper's arms changed, its boundary did not |
| A tab whose run has left the projection still renders | no change to `render_subagent_view`'s liveness statement; the existing test `the_subagent_tab_shows_the_task_and_the_descriptor_together` still passes | pass | |
| No cancellation, restart, or contention behaviour changes | the milestone adds no state, no async work, and touches no execution path | pass | |

## 3. Production implementation evidence

All production changes are in `src/tui/app/render.rs`.

**The owner is a value.** A new `enum ViewportOwner { Route, ChangeReview,
SubagentTab }` and `App::viewport_owner()` decide which surface owns the
viewport rect for the frame. The review is checked first (it is modal), then
the open tab, then the route's own view. The decision is a value rather than
the order in which two calls happen, which is what made the old behaviour
possible.

**Exactly one branch renders into the rect.** `render()` computes the owner
once and dispatches to `render_viewport`, `render_change_review`, or
`render_subagent_view` — one call, not two. `render_viewport` no longer
special-cases the change review; that arm moved into its own
`render_change_review`, so the review's precedence is expressed by the owner
rather than by an early return buried in the route dispatch.

**The subagent view fills its rect.** `render_subagent_view` paints a
`Block` carrying `theme.background` over the whole rect before it decorates
it, and the paragraph is rendered into `area` rather than
`Rect { width: area.width - 1, ..area }`. The one-column inset is gone: the
last column of the viewport used to be left to whatever was behind it.

**The strip row is filled.** `render_subagent_tab_strip` paints the same
background block over the strip's rect before its spans. The active entry
gains `fg(theme.primary).bg(theme.selection)`, matching what
`render_tab_strip` gives the project tab strip's active entry (the same
`primary`-on-`selection` pairing, so the theme's contrast contract is not
invented here). Every other entry keeps its existing foreground styling.

**A rect may not name a surface that is not painted.** `self.scrollbar_area`
is cleared before the viewport dispatch. The scrollbar belongs to the
transcript; when the tab or the review owns the rect, no scrollbar is drawn,
and a hit-test rect that names an unpainted surface mis-resolves a click. The
session route sets the rect again on its own path. This is a small addition
beyond the plan's enumeration (§3.1).

**Documentation.** `docs/tui.md` §"Subagent tabs" now says the tab shows a
**run view** and **replaces** the main conversation in the buffer rather than
floating over it, and that the tab's own row is filled rather than
transparent. `architecture/tui.md` gains two paragraphs — one on the
one-owner rule and one on opaque backgrounds — under **Subagent buffer**, and
its one remaining "show me its transcript" wording became "show me its run
view". `.opencode/skills/tui/SKILL.md` gains the matching bullet, because a
module contract changed and the skill and the architecture doc must move
together.

The addendum's §4 is headed "Current state (repo evidence at `31a4128`)" and
stays as it is: it is a baseline record, and its line numbers are pinned to
that SHA. This closure record is what describes the state after the change.

### 3.1 Deviations from the plan, and why

1. **The plan's §3.4 enumerated five `docs/tui.md` transcript claims; there
   are six.** A sixth instance — "`Enter` keeps opening the child transcript"
   at `docs/tui.md:703` — was found during the pass and corrected. The plan's
   *enumeration* was incomplete; its *intent* — stop promising a capability
   the product does not have — is what was implemented. The six claims, at
   their post-change line numbers, are `docs/tui.md:273` (the `/sessions`
   table row), `:671` (the buffer-tabs table row), `:687` (the `Ctrl+T`
   overlay paragraph), `:703` (the sixth, found during the pass), `:739` (§
   "Subagent tabs"), and `:792` (the sessions-panel bullet list). The plan
   cited `:785` for the last of these; the §"Subagent tabs" rewrite added
   seven lines above it, so it now sits at `:792`.
2. **The plan's §10 named a background-opacity assertion for the last-column
   test; that form does not discriminate.** The transcript's own background
   block already painted the whole viewport rect, so "every cell of the
   viewport rect carries a background" was true of the broken code. The test
   now asserts that the subagent's **own text** reaches the final column,
   which is the property the one-column inset actually broke. The plan's
   *wording* — "assert that the subagent view's content occupies the final
   column" — is what the final test does; only the suggested formulation was
   replaced.
3. **The plan offered the active entry's selection background as optional**
   ("if the active entry is to read as selected, the same treatment
   `render_tab_strip` gives its entries"). It was implemented: the active
   entry now wears `fg(theme.primary).bg(theme.selection)`, exactly the
   pairing the project tab strip uses, so the theme's contrast contract is
   reused rather than reinvented.
4. **One addition beyond the plan's enumeration:** `self.scrollbar_area` is
   cleared before the viewport dispatch. The scrollbar belongs to the
   transcript; when the tab or the review owns the rect, no scrollbar is
   drawn, and a hit-test rect that names an unpainted surface mis-resolves a
   click. There is no observable behaviour change today — `ClickTarget::
   Scrollbar` is already unreachable, because `viewport_area` is tested first
   in the click hit-test and contains the scrollbar column (§10) — so this is
   a correctness-and-honesty fix, not a behaviour change.
5. **The owner-selection unit test lives in `src/tui/app/render.rs`**, in a
   new `viewport_owner_tests` module, rather than in the integration file.
   `App::viewport_owner` is private, so an integration test cannot reach it.
   The module is private to the crate, so the test is a unit test and adds no
   public API.

## 4. Verification executed

### 4.1 Commands run

```bash
# narrow tests
cargo test -p codegg --lib tui::app::render
cargo test -p codegg --test tui_main_buffer
cargo test -p codegg --test tui_render

# static guards — all four are also inside `scripts/verify.sh quick`, which
# is where they were actually executed; they were not run a second time
# standalone, and this record does not claim they were
python3 scripts/check_tui_transport_authority.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_tui_subagent_name_pool_isolation.py

# formatting and linting
cargo fmt
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the broader sweep
scripts/verify.sh quick
```

### 4.2 Baseline-failing evidence (non-vacuity)

The five new integration tests were run against the **unfixed**
`src/tui/app/render.rs` at baseline `31a4128`, with the tests themselves
present. The result was:

| Test | Baseline | Fixed |
|---|---|---|
| `an_open_subagent_tab_replaces_the_transcript_instead_of_ghosting_over_it` | **FAILED** — "the main conversation is showing through the subagent's own view: TRNS0004" | pass |
| `the_tab_strip_row_is_opaque` | **FAILED** — "the tab strip row is transparent at column 1" (`left: Reset, right: Reset`) | pass |
| `the_subagent_view_fills_the_viewport_to_its_last_column` | **FAILED** — "the subagent's own text never reaches the last column of the viewport: \"                    \"" | pass |
| `an_open_change_review_is_not_covered_by_a_subagent_tab` | **FAILED** — "the review must be what is on screen" | pass |
| `with_no_tab_open_the_transcript_still_renders` | pass (negative control — it must pass on both sides) | pass |

**Two of these tests were vacuous in their first form and were only caught
because they were run against the unfixed code.** Both are recorded here
because the fix to a test is part of the evidence, not a footnote to it:

1. **The ghosting test.** The first version put one 15-character marker
   (`ZEBRAFISH-MARKER`) in the transcript and asserted the whole marker was
   absent. At baseline the subagent view's first line — `" Subagent · Sally
   (explore)"`, 25 columns — covered exactly that marker, so the test **passed
   against the unfixed code**. It now puts six short tokens across the
   transcript's line and asserts that **every** token is absent, which asks
   the right question: is *any part* of the transcript showing through.
2. **The last-column test.** The first version asserted that every cell of the
   viewport rect carried a background. At baseline the transcript's own
   background block already painted the whole rect, so the test **passed
   against the unfixed code** and would have proved nothing. It now asserts
   that the subagent's **own text** reaches the final column, which is the
   property the one-column inset broke.

The plan's §10 named the background-opacity formulation for the last-column
test; the final form asserts on text instead, because that is the form that
discriminates.

### 4.3 Results

- `cargo fmt --check` — **pass** (no diff).
- `cargo test -p codegg --lib tui::app::render` — **8 passed, 0 failed**
  (4 new `viewport_owner_tests`, 4 pre-existing in the same module).
- `cargo test -p codegg --test tui_main_buffer` — **63 passed, 0 failed**
  (58 pre-existing, 5 new).
- `cargo test -p codegg --test tui_render` — **111 passed, 0 failed**.
- `cargo clippy --workspace --all-targets -- -D warnings` — **pass**, no
  warnings.
- `scripts/verify.sh quick` — **pass** ("Quick verification passed."), which
  includes `cargo fmt --check`, the agent schema check, the core/client/desktop
  boundary checks, the sandbox, execution-ownership, TUI-authority,
  HTTP-route-disposition, audit-coverage, queue-delivery-callsites,
  queue-write-transaction-shape, scheduler-bypass, config-merge-coverage,
  no-hardcoded-models, provider wire/catalog/resilience, OpenAI-endpoint, and
  Eggwork-target-routing guards, and `cargo check --workspace --all-targets
  --locked`.
- The four guards the plan named individually —
  `check_tui_transport_authority.py`, `check_execution_ownership.py`,
  `check_scheduler_bypass.py`, `check_tui_subagent_name_pool_isolation.py` —
  are all inside `verify.sh quick` and all pass with it. They were not run a
  second time standalone, and this record does not claim they were.

### 4.4 Not run

- `scripts/verify.sh full` was **not** run. It adds clippy over the workspace
  and the whole test suite, both of which were run directly here
  (`cargo clippy --workspace --all-targets -- -D warnings` and the three
  targeted test binaries), so nothing in the full sweep is unaccounted for
  **except** the workspace-wide test run across every crate's unit and
  integration tests.
- No hosted CI run is claimed. Nothing in this record was executed on CI.

### 4.5 Wider sweep

The workspace-wide test run in `verify.sh full` was not executed. The
workspace **check** (`cargo check --workspace --all-targets --locked`) is
inside `verify.sh quick` and passed.

## 5. Invariant review

| Invariant | Evidence |
|---|---|
| **I1 — Projection-derived rows** | Untouched. No row, name, or descriptor is minted or resolved in the TUI; `scripts/check_tui_subagent_name_pool_isolation.py` passes (§4.3) |
| **I2 — A surface owns the pixels it covers** | The owner is chosen by `App::viewport_owner()` and exactly one branch renders into the rect; `an_open_subagent_tab_replaces_the_transcript_instead_of_ghosting_over_it` and `an_open_change_review_is_not_covered_by_a_subagent_tab` pin both directions, and `viewport_owner_tests` pins the decision itself |
| **I3 — Every TUI surface paints an opaque background** | The subagent view paints `theme.background` over its rect before decorating it; the strip row paints it over its own rect; `the_tab_strip_row_is_opaque` and `the_subagent_view_fills_the_viewport_to_its_last_column` pin both |
| **I4 — Panel input authority** | Untouched. `scripts/check_tui_transport_authority.py` passes (§4.3) |
| **I5 — A focusable row has an action** | Not P001's scope; delivered by P003. Untouched here |
| **I6 — Tab ordering is not an execution boundary** | Untouched. No cancel, no job, no execution path is touched by this milestone |
| **I7 — The name is drawn once, at spawn** | Untouched. P001 does not read a name from anywhere |
| **K3 — The tab is the subagent's own surface** | Delivered by this milestone: the tab *replaces* the transcript rather than overlaying it |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency** — not applicable; a render is not a
  delivery.
- **Cancellation races** — not applicable; the milestone cancels nothing
  (I6 preserved).
- **Daemon or node restart** — not applicable; no state is added.
- **Partial persistence failure** — not applicable; nothing is persisted.
- **Stale generation or lease** — not applicable.
- **Contention and resource release** — not applicable; no resource is taken.
- **Malformed or unauthorized input** — not applicable; no new input path.
- **Bounded event and artifact behavior** — the render path is unchanged in
  shape. The change review and the subagent view are each rendered **once**
  per frame, where before the subagent view was rendered *in addition to* the
  viewport, and the review *in addition to* the viewport as well. The work per
  frame is therefore lower, not higher.
- **Render panic** — the `catch_unwind` wrapper still covers the whole
  viewport dispatch, still latches into `last_render_error`, still records
  the component panic diagnostic, and still paints
  `render_component_fallback` over the viewport (which itself carries
  `theme.background`, so the fallback is opaque too).

## 7. Migration and compatibility review

None. No schema, migration, protocol, configuration, storage, or projection
change. `STORAGE_LAYOUT_VERSION` is untouched. Old snapshots render exactly
as they did, except that they are no longer visible **through** an open
subagent tab — which is the defect this milestone closes. Rollback is a
revert of `src/tui/app/render.rs`; there is no data to roll back.

## 8. Security review

- No authorization, permission, or capability change. The change review's
  accept/reject verdicts are untouched.
- No secret handling, redaction, or path validation is affected.
- The milestone **reduces** what is displayed while a subagent tab is open:
  the main conversation is no longer painted underneath it, so fewer bytes of
  the primary agent's transcript are on screen while reading another agent's
  run. The subagent's own view already showed the same fields it shows now.
- No denial-of-service bound is changed.

## 9. Documentation and operations

Updated:

- `docs/tui.md` — six transcript claims corrected (§3.1) and §"Subagent tabs"
  rewritten to describe the run inspector, the replacement semantics, and the
  opaque row.
- `architecture/tui.md` — the one-owner rule and the opaque-background rule
  under **Subagent buffer**; one "show me its transcript" wording corrected.
- `.opencode/skills/tui/SKILL.md` — the matching operational bullet.
- `plans/implementation/tui-subagent-presentation-and-name-allocation/001-tab-surface-ownership.md`
  — status line and outcome.
- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md`
  — §12 milestone status.
- `plans/registry.md` — the addendum row and the P001 row.

Static guards: none added. The defect is a rendering-ordering defect and the
tests pin it at the render level, which is where the defect lives. The guards
named by the plan — `check_tui_transport_authority.py`,
`check_execution_ownership.py`, `check_scheduler_bypass.py`,
`check_tui_subagent_name_pool_isolation.py` — are unaffected and pass (§4.3).

Operator diagnostics: unchanged. `last_render_error` still latches a viewport
render panic, and the frame still falls back to the "Messages render error"
block.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The plan's §10 named a background-opacity assertion for the last-column test; that form does not discriminate (the transcript's own background already painted the rect) | Would have produced a test that passes against the defect | Resolved in this milestone — the final form asserts on the subagent's own text (§4.2) |
| low | The plan's §3.4 enumerated five `docs/tui.md` transcript claims; a sixth exists at `docs/tui.md:703` | The plan's enumeration was incomplete, not its intent | Resolved in this milestone — the sixth is corrected (§3.1) |
| low | `ClickTarget::Scrollbar` is unreachable today: `viewport_area` is tested first in the click hit-test and contains the scrollbar column | Pre-existing, not introduced here; a scrollbar click resolves as a viewport click | Not this milestone's scope. Worth a separate look — the scrollbar is not independently clickable, so the arm is dead weight |
| low | The unit test for owner selection lives in `src/tui/app/render.rs` rather than the integration file | `viewport_owner` is a private method, so an integration test cannot reach it | Noted; no action |

No critical, high, or medium findings remain open for this milestone.

## 11. Roadmap disposition

**Milestone closed and the next dependency may proceed.**

P001 is closed. In this addendum's graph, P002 (ready) and P004 (ready) were
never blocked on P001 — P002 had only a soft dependency on it, to avoid two
edits to `render_subagent_tab_strip` in one tree — so both remain open and
unblocked now. **P003 remains hard-blocked on P002** and is unaffected by this
closure.

The addendum as a whole is **not** closed: it closes when P001–P004 each have
a closure record under `plans/closure/tui-subagent-presentation-and-name-allocation/`.

## 12. Registry updates

- `plans/registry.md`, **Active subsystem roadmaps**: the "TUI subagent
  presentation and name allocation (corrective)" row moves from *P001–P004
  planned* to *P001 closed; P002/P004 ready; P003 blocked on P002*, and its
  notes record this closure.
- `plans/registry.md`, **Dependency-ready implementation plans**: the P001 row
  moves from **ready** to **closed** and gains the closure path
  `plans/closure/tui-subagent-presentation-and-name-allocation/001-status.md`.
- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md`
  §12: the P001 row moves from `ready` to `closed` and its closure column
  gains the path.
- `plans/implementation/tui-subagent-presentation-and-name-allocation/001-tab-surface-ownership.md`:
  status becomes `closed` with the closure path and an outcome paragraph.
