# TUI Subagent and Job Visibility Roadmap

Status: proposed

Long-term references:
- `plans/000-long-term-specification.md` (canonical end-state specification)
- `plans/001-terminology-and-domain-model.md` (canonical identity model)
- `plans/002-long-term-roadmap.md` (dependency-ordered capability roadmap)

Related ADRs:
- None required. The ephemeral-naming decision below is deliberately scoped so it
  does **not** change a canonical identity or ownership boundary, and therefore
  does not meet the ADR threshold in `plans/adrs/README.md`. If M001 later
  graduates to a durable name, that change **does** require an ADR.

Repository baseline: `5a109ab27`

## 1. Purpose and ownership boundary

Make delegated work observable and navigable from the terminal: a user must be
able to see which subagents and jobs exist right now, identify each one by a
memorable human-readable name, open any of them as a tab in the main LLM buffer
panel, and watch that work without losing their place.

**Ownership boundary.** This roadmap owns TUI *presentation and navigation* of
existing projected state. It does not own the projection schema, the scheduler,
the job lifecycle, or the agent loop.

- It **reads** the projection snapshot (`agent_tree`, `agent_runs`, `jobs`).
- It **adds one additive presentation field** to the projection node for the
  display name (M001) and nothing else.
- It **never** synthesises a subagent, a job, or a status. A row that is not in
  the snapshot does not exist in the UI.

This preserves the invariant already stated in `src/tui/app/state/subagents.rs:10-18`:
the working strip is projection-derived, never synthesised.

## 2. Work classification

### Invariants

- **I1 — Projection-derived rows.** Every subagent, job, and todo row rendered by
  this work originates in a `SessionProjectionSnapshot` field. No client-side
  fabrication of rows, statuses, or identity.
- **I2 — Panel input authority.** Panel keys resolve through
  `SidebarActivation` → `PanelKey::Consumed` / `PanelKey::Intent(TuiMsg)` →
  `src/tui/app/input.rs`. No panel calls `process_msg` directly. This is the
  existing seam recorded in `src/tui/panels/mod.rs` and the module contract that
  `scripts/check_tui_transport_authority.py` enforces.
- **I3 — Bare-Ctrl prohibition.** A bare `Ctrl+<letter>` is never bound. The
  terminal is in legacy xterm encoding because the TUI never pushes
  keyboard-enhancement flags; the free letters are terminal control characters.
  See the contract comment at `src/tui/input.rs:684-690` and the invariants list
  at `src/tui/input.rs:857-859`, which explicitly records `Ctrl+J` as
  never-used.
- **I4 — Working strip row 0 reserved.** Row 0 of the working strip remains
  unconditionally reserved as the main buffer's only status surface. Strip
  height continues to be subtracted from the transcript via `session_layout`
  (`Min(10)` against `prompt_height`). This work adds overlay groups; it does not
  move or resize row 0.
- **I5 — Tab ordering is not an execution boundary.** A subagent tab is a view
  onto projected state. It grants no authority, starts no work, and cancels
  nothing. Only the existing daemon cancel path cancels.

### Capabilities

- **C1 — Active subagent overview.** The sessions panel lists active subagents
  with their display name and status.
- **C2 — Subagent navigation.** A subagent opens as a tab in the main LLM buffer
  panel; tabs cycle and close.
- **C3 — Unified tasks overlay.** One overlay presents todos, subagents, and jobs
  together, with separators, hiding empty groups.
- **C4 — Active job visibility.** Jobs the session is waiting on are visible in
  the sessions panel and in the tasks overlay.

### Infrastructure

- **F1 — Ephemeral subagent display name.** A name registry that mints a unique,
  active-scoped display name per running subagent and carries it in the
  projection.

### Polish

- **P1 — Shift+W alignment** so the chord and `/workspace` reach the same handler.
- **P2 — Help overlay and documentation** for every new binding.

## 3. Non-goals

Explicitly **out of scope** for this roadmap:

- **Sending input to a subagent.** M005 is the milestone that would own it, and
  it is not approved for planning yet. No milestone in the near-term slice may
  imply that a user can type to a subagent.
- **Durable, persisted subagent names.** Names are ephemeral by decision (§10,
  Decision D1). No storage migration is in scope anywhere in this roadmap.
- **A second scheduler, a job store, or any change to job lifecycle or
  admission control.**
- **Changing `ToolBroker` / `JobSubmissionService` / `JobScheduler` execution
  authority.** This roadmap spawns no work; `scripts/check_execution_ownership.py`
  and `scripts/check_scheduler_bypass.py` must stay green and
  `docs/execution-ownership.toml` unchanged.
- **Nesting subagent rows.** `AgentTreeNodeProjection.parent_task_id` is
  currently always `None`; the per-turn tree carries no hierarchy. Parent/child
  grouping is a future concern, not a goal of the milestones below.
- **Refactoring `SubagentState` out of the TUI, or generalising it into a generic
  "entity tab" abstraction.** The tab set stays subagent-specific.
- **A separate jobs overlay.** The user explicitly consolidated jobs into the
  single tasks overlay rather than a second chord.

## 4. Current state (repo evidence)

### What already exists and should be reused

**Subagent state is already modelled.** `src/tui/app/state/subagents.rs` defines:

```rust
pub struct SubagentRow {
    pub task_id: u64,
    pub run_id: Option<String>,
    pub agent: String,
    pub description: String,
    pub status: AgentTreeStatus,
}
pub struct SubagentView { pub row: SubagentRow, pub scroll: u16 }
pub struct SubagentState { pub rows: Vec<SubagentRow>, pub open: Option<SubagentView>, pub hits: Vec<SubagentHit> }
```

Rows are rebuilt from the projection snapshot each frame
(`src/tui/app/render.rs:2070` `subagent_rows_from_snapshot`, applied at
`src/tui/app/render.rs:2051`). `running_count()` already drives a header badge.
The working strip already reserves row 0 and renders up to
`MAX_WORKING_STRIP_SUBAGENTS = 6` subagent rows.

**A subagent already opens a read-only buffer — by mouse only.** `open` is set
from a strip mouse click (`src/tui/app/mod.rs:3836`). No keyboard path opens it.
The open view consumes exactly one key (Esc) and exposes no scroll binding.

**Tabs already exist, for projects.** `ProjectTabs`
(`src/tui/app/state/project_tabs.rs:257`) implements `add_tab`, `remove_tab`,
``set_active`, `active`, `close_fallback_tab`, a capacity bound, and
`render_tab_strip` draws a tab bar in the header
(`src/tui/components/component.rs:94-130`). This is the precedent to mirror; it
is not a generic tab framework today and this roadmap does not generalise it.

**The todo overlay is the template.** `toggle_todo_list()`
(`src/tui/app/mod.rs:13650`) drives `todo_expanded`, and
`session_layout` (`src/tui/app/mod.rs:560-620`) subtracts `todo_strip_height()`
above the input. `render_todo_strip` is the rendering entry point, and
`set_todos` mirrors todo rows onto the sidebar widget — the existing precedent
for pushing a state list into a panel.

**Jobs are already shipped and completely unread.** `SessionProjectionSnapshot.jobs`
is a `VecDeque<JobProjection>` populated by the projection reducer and capped by
`MAX_PROJECTION_JOBS = 32`. It has **zero readers** anywhere in `src/tui/**`.
`JobProjection` already carries `job_id`, `workspace_id`, `kind`, `state`,
`summary`, `session_id`, `turn_id`, `active_attempt_id`, `error_class`, and
`updated_at`. `JobState` covers `Pending`, `Running`, and terminal states.
`CoreRequest::JobList { query: { session_id, limit } }` already exists
daemon-side and returns `JobSummaryDto`. **The jobs work therefore needs no
protocol, daemon, or storage change at all** — it is a TUI reader for data that
already ships.

**The sessions panel already has a drill-in authority seam.**
`SidebarActivation` returns `ToggleSection | ToggleAgentNode(_) |
InspectRun(run_id) | None`, which becomes `PanelKey::Consumed` or
`PanelKey::Intent(TuiMsg)`. `InspectRun` already maps to
`TuiMsg::OpenRunDetail { run_id }` and opens the `RunDetailDialog`. There is no
`OpenSubagentDetail` message today.

### Three defects this work trips over

**D-a — The `run_id` join is dead, not merely unused.** `src/tui/app/render.rs:2094`
populates `SubagentRow.run_id` by matching the node's `task_id` against
`AgentRunSummary.task_id`. The two types can never be equal:

- `AgentTreeNodeProjection.task_id` is a `u64`, minted as a polynomial hash of
  the job id string and then decimal-stringified
  (`src/scheduler/executors.rs:710-715`).
- `AgentRunSummary.task_id` is a UUID string
  (`crates/codegg-core/src/projection_replay/agent_runs.rs:65`).

So `SubagentRow.run_id` is always `None`, and the field is a silent lie. Any
plan that routes a subagent tab to durable run detail must fix this first, and
`scripts/check_project_catalog_invariants.py` plus the `AGENTS.md` note that a
frontend-local id is "never reused after the projection snapshot evicts the node"
both matter here: a `u64` task id is not a durable handle.

**D-b — The per-turn tree cannot outlive its turn.**
`ProjectionEvent::SubagentStarted` is dropped when there is no active turn
(`crates/codegg-protocol/src/projection/reducer.rs:754-762`), the tree is
capped at 16 nodes, and it is cleared at turn end. This is precisely why the
name decision (§10 D1) is scoped to ephemeral names, and why a subagent tab must
treat "my subject left the snapshot" as a first-class state rather than an
error.

**D-c — There is no send path, at all.** `CoreRequest::TurnSteer` exists
end-to-end daemon-side but has **zero** callers in `src/tui/**`; `InputTarget`
has only `Build` and `Chat` (`src/tui/panels/mod.rs:949`). "Interact with a
subagent" has no foundation and is not a small addition.

### Keybinding reality (verified, not assumed)

| Chord | Status today | Verdict for this roadmap |
|---|---|---|
| `Shift+N` | unbound in all three keymaps | **free** — next tab |
| `Shift+P` | unbound in all three keymaps | **free** — previous tab |
| `Shift+X` | unbound in all three keymaps | **free** — close tab |
| `Shift+W` | `OpenWorkspaceDashboard`, vim map (`src/tui/input.rs:1071-1074`); pinned by a test at `src/tui/commands/workspace_dashboard.rs:1211` | align to the `/workspace` handler (P1) |
| `Shift+T` | unbound in all three keymaps | **free** — tasks overlay |
| `Ctrl+S` | `StashPrompt` (default map, `src/tui/input.rs:734-737`), pinned by `BARE_CTRL_TWINS` in `tests/modifier_chords.rs:50` | **not used** — consolidated away |
| `Ctrl+J` | unbound, but **undeliverable** | **not used** — see I3 |

`Ctrl+J` is byte `0x0A` in legacy xterm encoding, which crossterm reports as
`KeyCode::Enter`. Binding it would fire **Send**. The `{CONTROL, ALT}` chord
family is the documented remedy (`src/tui/input.rs:684-690`), already used for
`Ctrl+Alt+N/B` (project tabs), `Ctrl+Alt+S` (sidebar focus), and
`Ctrl+Alt+P/F/Y`.

A `Shift+<letter>` binding must be inserted **twice** — once under
`KeyModifiers::SHIFT` and once under `KeyModifiers::NONE` — because terminals
disagree on whether the shifted letter carries the flag
(`src/tui/input.rs:556`, `:569`, `:579`, and the normalisation at `:1949-1950`).
M003 must follow the existing helper rather than inserting a single entry.

## 5. Target architecture

A **single new tab set over the main LLM buffer panel**, plus **one new
composited overlay**, plus **two new panel sections**. Nothing here changes how
the daemon schedules work.

```
projection snapshot
  ├─ agent_tree  ──► working strip (existing)  ──► SubagentState.rows
  │                     │                          (M001 adds display name)
  │                     └─► sessions panel "Subagents" section  [C1]
  │                     └─► sessions panel row ──Enter/click──► subagent tab  [C2]
  │                                                          │
  │                                    ┌─────────────────────┴──────────────┐
  │                                    │ buffer tab set (M003)              │
  │                                    │  tab 0: main conversation          │
  │                                    │  tab N: subagent (name + status)   │
  │                                    │  Shift+N / Shift+P / Shift+X       │
  │                                    └────────────────────────────────────┘
  ├─ jobs ──────► sessions panel "Jobs" section (waiting-on filter)       [C4]
  │            └─► tasks overlay, active/pending/queued only              [C3, C4]
  └─ todos ────► tasks overlay, top group                                [C3]
```

The tab set is a sibling of `ProjectTabs`, not a replacement. Project tabs live
in the header; subagent tabs live in the main buffer region. They are
independent axes and M003 must not couple them.

The tasks overlay is a composition of three already-bounded sources, not a new
list model: it renders groups in fixed order (todos, then subagents, then jobs),
separated by a labelled rule, and **omits a group entirely when it has no
rows**. Subagent and job groups filter to active/pending/queued; the todo group
keeps its current complete list.

## 6. Dependency graph

```
        (none — starts from a clean base)
                  |
                  v
        M001  ephemeral display name  (F1)
                  |
        +---------+---------+-------------------+
        |                   |                   |
        v                   v                   v
  M002 unified        M003 subagent        M004 sessions
  tasks overlay      buffer tabs           panel sections
  [C3, C4]            [C2]                  [C1, C4]
        |                   |                   |
        +---------+---------+-------------------+
                  |
                  v
        M005  cross-cutting guards, docs, help   (P1, P2)
                  |
                  v
        M900  send-to-subagent (NOT approved for planning)
```

- **M001 → M002, M003, M004**: hard. All three surfaces must show the *same*
  name for the same subagent; minting it once upstream is what guarantees that.
- **M002, M003, M004 → M005**: hard. Guards and help entries can only be written
  once the binding set is final.
- **M003 → M004**: interface. The panel's Enter/click must target a tab
  identifier that M003 defines, so the section rows and the tab set agree on
  identity.
- **M002 ⇄ M003**: soft. Both render from `SubagentState.rows` / snapshot but can
  land independently.
- **M900**: no dependencies from the near-term slice. It is blocked on a design
  decision, not on code.

## 7. Milestones

### M001 — Ephemeral subagent display name

- **Class:** infrastructure (F1). Not yet user-visible capability.
- **Objective:** every running subagent has a stable-for-its-lifetime,
  unique-among-active, human-readable display name carried in the projection.
- **Dependencies:** none.
- **Status:** **landed (M001 scope)** on `feat/auto-20261016-845b613f`. The curated
  pool, the active-scoped registry, the event-carried `display_name`/`run_id`, the
  projection fields, and the dead-`run_id`-join fix are all in. Still open for a
  later milestone: nothing *renders* a name yet — the sessions panel (M004) and the
  tasks overlay (M002) are what make it user-visible.
- **Deliverable boundary:**
  - *(landed)* `assets/subagent-names/*.toml` as the single authored source,
    compiled into `src/agent/subagent_names/generated.rs`. Names are validated at
    author time: unique, alphanumeric, letter-initial, bounded length. `sprite`
    is empty today and decodes to `None`; filling it in the TOML is the only
    change needed to give a name a sprite later.
  - *(landed)* Drift guard via `scripts/generate_subagent_names.py --check` in
    `verify.sh quick` and CI.
  - *(landed)* `SubagentNameRegistry` (`src/agent/subagent_names/registry.rs`)
    draws from the compiled pool and, on collision with a currently-active
    subagent, takes the first free of `name`, `name-2`, `name-3`, …. The lease
    is an RAII `SubagentNameLease` bound to the spawn function's stack frame, so
    completion, every early failure return, cancellation, and panic all release
    through `Drop`. No call site has to remember, which is what stops the pool
    draining under a path that forgets.
  - *(landed)* `CoreEvent::SubagentStarted` and `AppEvent::SubagentStarted` carry
    `display_name` and `run_id` (additive, `Option`).
  - *(landed)* `AgentTreeNodeProjection` gains `display_name` and `run_id`, both
    `#[serde(default)]` so old snapshots decode.
  - *(landed)* `SubagentRow` gains `display_name` plus `label()`, which falls
    back to the agent type when a node predates display names.
  - **No storage change and no migration.** The per-turn tree is not persisted.
- **Exit conditions:**
  - Two concurrent subagents of the same agent type always display distinct
    names; a third distinct one too. *(covered by
    `concurrent_subagents_never_share_a_name`, `draw_survives_a_pool_where_the_chosen_base_is_occupied`,
    `a_fully_occupied_pool_walks_the_suffix_ladder`)*
  - A finished subagent's name is released and can be re-drawn later. *(covered by
    `lease_releases_on_drop`, `lease_releases_even_when_the_body_unwinds`,
    `release_returns_the_name_to_the_pool`)*
  - Projection replay is byte-identical to live projection for the same event
    stream — the name travels **in the event**, never re-minted by the reducer,
    because a re-minted random value would make replay non-deterministic. The
    replay path (`projection_replay/publication.rs`) copies the recorded name
    verbatim; it does not draw one.
  - No row renders a name minted client-side (I1). The TUI reads `node.display_name`
    and never calls the registry.
- **Resolved here — the dead `run_id` join (D5):** fixed by the *narrow* route, so
  no ADR was needed. Rather than re-keying the per-turn tree onto durable run ids,
  `run_id` is now carried **in the event** from `SubAgentRequest::run_id` (already
  available at the publish site) and copied onto the node. The old code joined
  `node.task_id.to_string()` (a `u64` in decimal) against `AgentRunSummary::task_id`
  (a UUID string); those can never be equal, so `SubagentRow::run_id` was
  permanently `None`. The join is deleted rather than left in place.
- **Note:** a future producer that has no registry publishes `display_name: None`,
  and `label()` degrades to the agent type. That is the intended fallback, not a
  hole in the contract.

### M002 — Unified tasks overlay

- **Class:** capability (C3, C4).
- **Objective:** one `Shift+T` overlay presents todos, then subagents, then jobs,
  separated, with empty groups omitted.
- **Dependencies:** M001 (names must appear here).
- **Status:** **landed (M002 scope)** on `feat/auto-20261016-845b613f`. Three
  defects surfaced during implementation and are fixed; they are recorded below
  because each one is a way this overlay can look broken rather than absent.
- **Deliverable boundary:**
  - *(landed)* `tasks_expanded` + `toggle_tasks_list()`, independent of
    `todo_expanded`. `Ctrl+T` owns the always-visible compact progress line;
    `Shift+T` owns the on-demand block. Collapsing them would take away the
    one-line summary legible without expanding anything.
  - *(landed)* `tasks_strip_height()` participates in the prompt allocation
    exactly as `todo_strip_height()` does — I4 preserved (the input box keeps
    the height it asked for; the viewport absorbs the difference).
  - *(landed)* `src/tui/app/state/tasks.rs` owns the row model. Subagents come
    from `SubagentState.rows`, jobs from `snapshot.jobs`. No new request, no
    daemon change.
  - *(landed)* Bound under both SHIFT and NONE encodings in the **shared**
    binding function (`insert_panel_direction_bindings`), so every map gets it.
    An earlier attempt put it in the vim map only, where the default map never
    saw it — a test caught this, not review.
  - *(landed)* `is_normal_only` includes it, or Insert mode would treat a
    capital `T` mid-word as "open the overlay" instead of composing it.
- **Three defects the implementation surfaced:**
  1. **Per-group caps are not a total bound.** Three groups each under their own
     cap still outgrow a short terminal, and an overlay that outgrows its carve
     is skipped *entirely* — so `Shift+T` reads as a dead key on exactly the
     terminals where a status overlay would be most wanted. Fixed by
     `MAX_TASKS_TOTAL_ROWS`, a total budget *including headers*.
  2. **First-come-first-served starves the last group.** Filling groups in
     display order until the budget runs out let a long todo list consume
     everything and cost the jobs group rows it had room for. Fixed by
     fair-share allocation with redistribution: every group is granted an equal
     share, and what a small group does not need goes back to those that want
     it.
  3. **The reservation is clamped against real available height**
     (`tasks_overlay_allowance`), honouring header, footer, the transcript's
     `Min(10)` floor, the prompt box at its own maximum, and the working/hint/
     todo strips. A too-short pane shows no overlay rather than a wrong one.
- **Exit conditions:**
  - With nothing running, the overlay shows todos only, with no empty subagent or
    job heading. *(covered: `nothing_running_renders_todos_only_with_no_empty_headings`)*
  - With a running subagent and a queued job, both appear with names/statuses.
    *(covered: `a_running_subagent_and_a_queued_job_both_appear`,
    `the_overlay_appears_when_the_pane_has_room_for_it`)*
  - Layout math is covered by a test asserting the strip height never eats the
    transcript below `Min(10)`. *(covered:
    `a_saturated_overlay_never_squeezes_the_transcript_below_its_floor`, which
    asserts the floor across 100x30, 100x50 and 120x24 rather than one size)*
  - Headers survive truncation, so a partial list never has unattributed rows.
    *(covered: `bounding_keeps_every_header`,
    `bounding_respects_the_global_budget_however_many_groups_are_full`)*
- **Resolved terminology:** the existing `/tasks` slash command
  (`src/tui/commands/tasks.rs`) governs *schedules*. No group heading says
  "tasks" — they read `Todo list`, `Subagents`, `Jobs`, and the key is labelled
  "Toggle tasks (todos, subagents, jobs)" so the overlap with `/tasks` is
  explicit rather than accidental. Remaining doc surface is M005's job.

### M003 — Subagent tabs in the main buffer

- **Class:** capability (C2). **Status: landed.**
- **Objective:** open a subagent as a tab in the main LLM buffer panel; cycle
  with `Shift+N`/`Shift+P`; close with `Shift+X`.
- **Dependencies:** M001. Interface dependency from M004.
- **Delivered:**
  - `src/tui/app/state/subagent_tabs.rs` — a tab set mirroring
    `ProjectTabs`' `add_tab` / `remove_tab` / `set_active` / `active` /
    close-fallback semantics, plus the capacity bound
    (`MAX_OPEN_SUBAGENT_TABS = 16`), the attached/detached distinction for D-b,
    cycle-with-wrap, and the one-row bar's width arithmetic (`fit_bar_tabs`).
    `SubagentState` now holds it in place of the old single-slot
    `Option<SubagentView>`.
  - `Shift+N`, `Shift+P`, `Shift+X` bound in **both** encodings (SHIFT and NONE)
    via the shared `insert_panel_direction_bindings`, so every keymap receives
    them, and all three listed in `is_normal_only`.
  - A keyboard path into a tab from the tasks overlay: `j`/`k` move a cursor,
    `↵` opens the selected subagent, `Esc` closes the overlay. Normal mode only
    — the overlay is opened from Normal, but the user switches back to Insert
    to keep typing, and claiming `j`/`k`/`Enter` there would break the composer.
  - `TuiMsg::OpenSubagentTab { task_id }`, dispatched through
    `src/tui/app/input.rs` (I2), as the interface M004 wires the panel to.
  - A one-row tab bar under the header, carved off the top of the viewport and
    drawn from the same clamped spare-rows budget as the tasks overlay.
- **Exit conditions, each mapped to a named test:**
  - Cycle order is stable and wraps; closing the last tab falls back. *(covered:
    `cycling_forward_from_the_main_buffer_lands_on_the_first_tab`,
    `cycling_backward_from_the_main_buffer_lands_on_the_last_tab`,
    `cycling_with_a_single_tab_alternates_between_it_and_the_main_buffer`,
    `closing_the_active_tab_falls_back_to_its_neighbour`,
    `closing_the_last_tab_falls_back_to_the_main_conversation`,
    `shift_n_and_shift_p_cycle_the_tab_set_and_wrap_through_the_conversation`)*
  - A tab whose subject leaves the snapshot renders a defined state, never a
    panic or a stale claim. *(covered:
    `a_subject_that_leaves_the_snapshot_detaches_instead_of_claiming_to_run`,
    `a_still_running_row_in_a_detached_tab_reads_as_detached_on_the_bar`,
    `a_tab_whose_subject_left_the_snapshot_says_so_instead_of_claiming_to_run`,
    `a_detached_tab_returns_to_live_when_its_subject_comes_back`)*
  - Every new binding is in the help overlay and chord-disjoint. *(covered:
    `documented_keys_resolve_in_the_active_map`,
    `subagent_tab_letters_resolve_under_both_encodings`,
    `subagent_tab_letters_are_ordinary_text_in_insert_mode`,
    `subagent_tab_letters_are_distinct_from_the_project_tab_chords`,
    `no_subagent_tab_binding_is_reachable_as_a_control_shift_chord`)*
- **Layout is bounded like every other strip.** The bar is one row whenever it
  renders — never two, and never a row that pushes the transcript below its
  floor. Asserted across 100x30, 100x50, 120x24 and 80x20 rather than one size.
  *(covered: `the_tab_bar_costs_exactly_one_row_whenever_it_renders`,
  `the_tab_bar_never_pushes_the_transcript_below_its_floor`,
  `no_tab_bar_reserves_no_rows`)*

#### Decisions M003 settled

- **Tab identity is the projection's `task_id`, not a fresh UUID.** A subagent
  already has a stable snapshot-derived identity; minting a second one would
  mean carrying a mapping between the two for no benefit, and it would break
  the "every surface addresses the same subject" property that lets M004's
  panel rows and M003's overlay agree. It is *not* durable, matching D1.
- **The main conversation is `active_tab_id == None`,** not a sentinel tab.
  This is the close-fallback analogue of `ProjectTabs::close_fallback_tab`:
  projects must always have a tab to render so removing the last one mints an
  empty compat tab, whereas the main buffer already has a surface, so the
  fallback is simply to stop selecting a subagent. Closing the last tab needs
  no special case.
- **Capacity refuses rather than evicts.** At 16 tabs an open is declined with
  a toast. Silently closing a tab the user is reading is a worse failure than
  declining the open.
- **Separate Shift letters rather than reusing `Ctrl+Alt+N`/`Ctrl+Alt+B`.**
  Project tabs (header) and subagent tabs (main buffer) are independent axes;
  reusing the project chords would have collapsed two navigations onto one
  gesture and made existing muscle memory fire a subagent switch instead.
- **The cycle models the main conversation as position 0, not position −1.**
  Treating it as −1 and taking a remainder sends "previous" from the
  transcript to the *second* tab instead of the last. This is the wrap that a
  naive implementation gets wrong, and it is asserted directly.

#### Four defects this milestone tripped over

- **The single `Option<SubagentView>` slot destroyed the previous view.** A
  second click replaced the first tab outright, silently discarding its scroll
  position — the same shape as the `PermissionPending` dialog bug in
  `AGENTS.md`, where one slot serving two callers loses state the other holder
  still owns. Replacing it with an addressed tab set makes the collision
  impossible by construction rather than by a guard.
- **The bar was charged to the transcript twice.** It was added to
  `prompt_height` *and* carved off the viewport chunk. That sum is what reserves
  room for strips carved off the *prompt* chunk — the bar never touches the
  prompt chunk, so the second charge bought nothing and cost one row. The
  visible symptom was a bar that appeared to cost two rows, which is exactly
  the kind of thing a "one row" assertion catches and an eyeball does not.
- **The bar pushed the transcript under its floor on a short pane.** `Min(10)`
  in `session_layout` is a *soft* floor — ratatui yields it when the pane is
  oversubscribed, and the working strip is allowed to oversubscribe it. So the
  bar has to yield, not the transcript. It now draws from the same clamped
  spare-rows budget `tasks_overlay_allowance` established, and takes it ahead
  of the tasks overlay because a persistent bar missing is worse than an
  on-demand overlay one row shorter. On a pane with no room the bar is dropped
  entirely: tabs stay reachable by `Shift+N`, whereas a truncated transcript
  is the loss the user cannot see coming.
- **`viewport_area` was published before the bar was carved out of it.** The
  scrollbar hit-test and scroll math read that field; publishing it early would
  have handed them a row belonging to the bar. It is now published *after* the
  carve, so it names the rect the transcript actually occupies.

#### One more: the overlay had no addressable identity

`TaskRow::Subagent` carried a display label and an agent type, and neither is
an identity: a display name is optional and falls back to the agent type, which
is never unique across concurrent runs. Opening a tab from the overlay
therefore required adding `task_id` to the row — it could not have been matched
back from what was drawn.

#### Boundary note: the sessions-panel path belongs to M004

M003's deliverable list asked for a keyboard path "from the focused panel row
and from the tasks overlay". The overlay path is delivered here. The panel path
is M004's, and the reason is concrete rather than convenience: `Enter` on the
sessions panel's *existing* agent-tree rows already resolves to
`SidebarActivation::InspectRun` and opens the run-detail dialog. Repurposing it
would break a working surface to add a second one. M004 adds dedicated
collapsible subagent sections whose `Enter` opens a tab instead, which is what
the roadmap already asked for there. M003's contribution to that path is the
addressed message (`TuiMsg::OpenSubagentTab { task_id }`) and the tab set both
sides resolve against.

- **Explicitly not here:** any ability to type into a tab. `Shift+X` closes a
  tab; it cancels nothing, and a closed tab does not stop its run.

### M004 — Sessions panel subagent and job sections

- **Class:** capability (C1, C4).
- **Objective:** the sessions panel lists active subagents (name + status) and
  jobs the session is waiting on, navigable with `j`/`k`/arrows, opening a
  subagent as a tab on Enter/click.
- **Dependencies:** M003 (interface), M001.
- **Status:** **landed** on `feat/auto-20261016-845b613f`.
- **Delivered:**
  - `SidebarSection::{Subagents, Jobs}` and `SidebarSubagent`/`SidebarJob` in
    `src/tui/components/sidebar.rs`, with their own collapse state. Both sit
    between `Todos` and `MCP Servers` so the three "what is this session
    waiting on" surfaces read as one block, matching the `Shift+T` overlay's
    todos → subagents → jobs order.
  - `SidebarActivation::InspectSubagent(String)` carrying the projection's
    `task_id`. The panel maps it to `PanelKey::Intent(TuiMsg::OpenSubagentTab
    { task_id })`; the click path maps it to `process_msg` directly, which is
    the pre-existing `InspectRun` shape for that site and not the same thing
    as the panel calling into the app.
  - `SubagentFocusTarget::Subagent(task_id)` and `Job(job_id)`. Jobs key on
    `job_id` rather than a list index (see the decision below).
  - The panel's subagent rows are derived from the *same* `SubagentRow`
    vector the main buffer's strip and the overlay read — one derived value,
    three renderings — so all three surfaces show one name per subagent.
  - Jobs are filtered with `state::tasks::is_live_state`, the same predicate
    the overlay uses, so "waiting on" is one rule rather than two.
  - Bounded at `MAX_SIDEBAR_SUBAGENT_ROWS = 16` /
    `MAX_SIDEBAR_JOB_ROWS = 12`, with the header count carrying the true total
    and a `… N more` line marking the truncation.
- **Exit conditions, each mapped to a named test:**
  - `j`/`k`/arrows move focus across todo, subagent, and job rows without
    collapsing sections → `focus_walks_todo_then_subagent_then_job_rows_without_collapsing`
    (unit) and `a_job_row_is_read_only_and_never_reaches_the_composer`
    (integration).
  - Enter and click both open the same tab for the same subagent →
    `click_and_enter_resolve_the_same_subagent_activation` (unit),
    `enter_on_a_session_panel_subagent_row_opens_its_buffer_tab`, and
    `a_clicked_subagent_row_opens_the_same_tab_as_enter` (integration).
  - The panel never renders a row absent from the snapshot →
    `empty_sections_contribute_no_header_and_no_rows` and
    `rendered_rows_are_exactly_the_rows_the_snapshot_reported` (unit).

#### Decisions M004 settled

- **The agent tree still opens the run-detail dialog.** The roadmap's own
  boundary note anticipated this and it is enforced by
  `the_new_sections_do_not_take_over_the_agent_tree_activation`: adding a
  second, adjacent subagent surface made reusing `Enter` on the tree rows
  tempting, and refusing it is what keeps a working surface working. "What
  happened to this run" and "show me its transcript" stay separate questions
  on separate rows.
- **Job rows are keyed on `job_id`, not an index.** Todo and file-change rows
  use indices because the projection appends to them. Jobs are *upserted* and
  then filtered to live states, so the row before the focused one reaching a
  terminal state shifts every later index down by one; an index-keyed target
  would silently move focus onto a different job and then activate it. See
  `a_finished_job_does_not_rebind_focus_to_a_different_job`.
- **The subagent section lists every row the tree reports, not only running
  ones.** The `Shift+T` overlay filters to live because it is a transient
  status block; the panel is a persistent index, and it is the surface a user
  returns to. Filtering would make a finished subagent vanish the moment it
  finished, and would leave an open tab whose subject the panel no longer
  lists — the exact "panel outliving the tree" shape D3 asks to watch for.
  Status is drawn per row instead. When the tree does drop the node, the row
  goes and the tab shows its M003 detached terminal state: the two surfaces
  agree on the same subject because they read the same rows.
- **A job row activates to nothing.** There is no job transcript and no job
  dialog, so `Enter` consumes the key rather than fabricating a destination —
  and `a_job_row_is_read_only_and_never_reaches_the_composer` asserts the key
  never leaks to the composer as a turn.
- **A missing snapshot does not clear the subagent section.** `set_subagents`
  is only called with rows an arriving snapshot reported, matching the rule
  `SubagentState::set_rows` already follows: a projection that has not spoken
  is not evidence that no subagents exist.

#### Two defects this milestone tripped over

- **`line_targets` and `content_lines` are maintained separately, and nothing
  checked they agreed.** `element_at` maps a terminal `y` straight into the
  hover map, so a section that renders a line without registering a target
  shifts every row below it onto the wrong subject — clicking one subagent
  opens another, silently. The new `… N more` truncation line was exactly the
  kind of line that gets rendered and forgotten.
  `hover_targets_stay_aligned_with_rendered_lines` now asserts the two
  functions agree, both with and without truncation. (This is a latent risk
  the new sections inherited from the old ones, not one they created; the
  invariant test is what makes it safe to add a fourth section later.)
- **A panel row alone cannot open a tab.** The first version of the
  integration test seeded only `SidebarWidget.subagents` and the open was
  refused, because `SubagentState::open_task` resolves the row behind the
  `task_id` and returns `false` when there is none. That is correct — a tab
  with nothing to render is not a tab — and it is why the panel and the
  buffer are fed from one derived vector in `App::render_sidebar` rather than
  projected twice. The test now seeds both, with a helper that says why.

### M005 — Cross-cutting guards, help, and documentation

- **Class:** polish (P1, P2).
- **Objective:** make the new surface verifiable and documented.
- **Dependencies:** M002, M003, M004.
- **Deliverable boundary:**
  - P1: confirm the `Shift+W` path and the `/workspace` command reach the same
    handler (`open_workspace_dashboard`). Note that `/workspaces` does **not**
    exist as a command — it appears only in the domain-classification list at
    `src/tui/command.rs:402`. If they already agree, this is a no-op recorded as
    verified rather than a change.
  - P2: `docs/tui.md` binding table, help-overlay entries, and the
    `architecture/tui.md` module map.
  - Run `scripts/verify.sh quick` and the change-triggered guards this touches.
- **Exit conditions:** help overlay and `docs/tui.md` list every new binding;
  quick verification green.

### M900 — Send-to-subagent (not approved for planning)

- **Class:** capability. **Status: not started, blocked on design.**
- There is no send path: `CoreRequest::TurnSteer` has zero TUI callers and is
  keyed on a session turn rather than a subagent run, and `InputTarget` has only
  `Build` and `Chat`. Addressing a subagent run directly is net-new daemon work
  that needs its own decisions on run addressing, permission scoping, and what
  "reply" means for a delegated run before any UI is justified.
- M001–M005 must not imply this capability exists.

## 8. Cross-cutting requirements

**Storage.** None. The ephemeral-name decision (D1) is what keeps this roadmap
free of migrations, `STORAGE_LAYOUT_VERSION`, and the four coordinated edits a
new migration requires. If D1 is ever revisited toward durability, this changes
fundamentally and an ADR is required first.

**Protocol.** One additive field on `AgentTreeNodeProjection` and additive
fields on the subagent `CoreEvent` variants, all `#[serde(default)]`-compatible
so a mixed-version daemon and frontend still connect. Jobs need no protocol
change. Any protocol field addition must keep `apps/desktop` bridge field parity
(`apps/desktop/scripts/check-bindings.mjs:26`) and pass
`scripts/check_projection_*.py` plus `scripts/check_websocket_bounds.py`.

**Security.** No new authority. Viewing a subagent discloses only projected
state the user already can see. Tabs cancel nothing (I5). No new prompt input
reaches a subagent in M001–M005.

**Concurrency.** Subagent completion must release the display name exactly once,
including on the cancel and panic paths, or names leak and stop being unique.
A leaked name is a silent correctness failure, so this needs a test, not a code
review.

**Observability.** A name collision producing `name-2` is expected behaviour, not
an error; it must not be logged as a warning or counted as a fault.

## 9. Verification strategy

- `scripts/verify.sh quick` after every milestone.
- `cargo test -p codegg-core` for the projection reducer and replay.
- `cargo test --test modifier_chords` for binding disjointness.
- `scripts/check_projection_*.py`, `scripts/check_websocket_bounds.py`,
  `scripts/check_tui_transport_authority.py` for the projection, wire, and
  transport-authority contracts.
- Targeted TUI render tests for layout math: the tasks strip must never reduce
  the transcript below `Min(10)`, and row 0 stays reserved (I4).
- A replay-determinism test for M001: replaying a recorded event stream must
  reproduce identical names.

## 10. Risks and decision points

- **D1 — Names are ephemeral and active-scoped.** *Decided.* A name is held only
  while its subagent is running, uniqueness is against other active subagents,
  and collisions increment (`name-2`). Consequence: a name cannot be relied on
  after the subagent finishes, and a subagent tab that outlives its subject has a
  name with no subject. This is accepted: it buys no migration, no
  `STORAGE_LAYOUT_VERSION` churn, and no durability contract. If a user later
  needs to recognise a subagent across a restart, this decision must be reopened
  with an ADR.
- **D2 — Base name is drawn from a curated random pool.** *Decided as a pool,*
  with numeric disambiguation. Alternative reading — deriving the base from the
  agent type (`explore`, `explore-2`) — is still defensible and arguably more
  informative, since the agent type is otherwise shown beside it. Both satisfy
  the stated requirement. Flagged rather than blocking; settle during M001.
- **D3 — The per-turn tree bounds tab lifetime.** *Accepted.* A tab whose subject
  leaves the snapshot needs a defined terminal state (D-b). Revisit if M004 shows
  the sessions panel routinely outliving the tree it reads.
- **D4 — Ctrl+J was requested and is not deliverable.** *Decided.* It cannot be
  bound without firing Send (I3). Jobs were folded into `Shift+T` instead, which
  matches the user's own consolidation. Recorded so it is not re-litigated.
- **D5 — The dead `run_id` join (D-a) may be more than a cleanup.** If fixing it
  properly requires re-keying the per-turn tree onto durable run ids, that is a
  projection-identity change and may warrant its own ADR. First attempt is the
  narrow one: emit the real `run_id` on the event.
- **D6 — Scope creep into interaction.** The strongest pull in this area is
  "and then let me reply to it." M900 exists to hold that pull.

## 11. Completion definition

This roadmap is complete when a user in a terminal can: start a turn that spawns
subagents and jobs; see each running one by name and status in the sessions
panel; open one as a tab and cycle and close tabs; see todos, subagents, and jobs
together in one overlay with empty groups hidden; and do all of it with no
schema migration, no change to execution authority, and no dead-end keybinding.
Interaction with a subagent is **not** part of this definition.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 ephemeral display name | landed (pool, registry, event-carried name, dead join fixed) | — | — | none |
| M002 unified tasks overlay | landed (Shift+T; global row budget + height clamp) | — | — | none |
| M003 subagent buffer tabs | landed (tab set, Shift+N/P/X, detached terminal state, overlay keyboard path) | — | — | none |
| M004 sessions panel sections | landed (subagent + job sections, `InspectSubagent`, id-keyed job focus) | — | — | none |
| M005 guards, help, docs | not started | — | — | M002, M003, M004 |
| M900 send-to-subagent | not approved for planning | — | — | design decision |