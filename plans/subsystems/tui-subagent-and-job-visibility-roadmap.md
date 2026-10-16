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
- **Status:** **partially landed** at `5a109ab27`+. The curated name pool is built
  and enforced (46 entries). **This is the pool only** — no name is assigned to
  any subagent yet, and nothing renders one. The registry, the projection field,
  and the dead-`run_id`-join fix below all remain.
- **Deliverable boundary:**
  - *(landed)* `assets/subagent-names/*.toml` as the single authored source,
    compiled into `src/agent/subagent_names/generated.rs`. Names are validated at
    author time: unique, alphanumeric, letter-initial, bounded length. `sprite`
    is empty today and decodes to `None`; filling it in the TOML is the only
    change needed to give a name a sprite later.
  - *(landed)* Drift guard via `scripts/generate_subagent_names.py --check` in
    `verify.sh quick` and CI.
  - *(pending)* A name registry that draws from the compiled pool and, on
    collision with a currently-active subagent, takes the first free of
    `name`, `name-2`, `name-3`, … Releasing on completion/failure/cancel is
    what makes the name ephemeral and bounded.
  - `CoreEvent::SubagentStarted` and `CoreEvent::SubagentCompleted` /
    `SubagentFailed` carry the name (additive).
  - `AgentTreeNodeProjection` gains an additive `display_name` field, defaulted
    so old snapshots and replays still decode.
  - `SubagentRow` gains the same field, populated from the node.
  - **No storage change and no migration.** The per-turn tree is not persisted.
- **Exit conditions:**
  - Two concurrent subagents of the same agent type always display distinct
    names; a third distinct one too.
  - A finished subagent's name is released and can be re-drawn later.
  - Projection replay is byte-identical to live projection for the same event
    stream — the name travels **in the event**, never re-minted by the reducer,
    because a re-minted random value would make replay non-deterministic.
  - No row renders a name minted client-side (I1).
- **Note:** D-a (the dead `run_id` join) is resolved here, by emitting the real
  `run_id` alongside the name, or by removing the field. Leaving a provably
  always-`None` field in place is not an acceptable outcome.

### M002 — Unified tasks overlay

- **Class:** capability (C3, C4).
- **Objective:** one `Shift+T` overlay presents todos, then subagents, then jobs,
  separated, with empty groups omitted.
- **Dependencies:** M001 (names must appear here).
- **Deliverable boundary:**
  - `toggle_tasks_list()` modelled directly on `toggle_todo_list()`
    (`src/tui/app/mod.rs:13650`), with a `tasks_expanded` flag and a
    `tasks_strip_height()` participating in `session_layout` exactly as
    `todo_strip_height()` does today — I4 preserved.
  - Subagent and job groups filter to active/pending/queued.
  - Jobs are read from `snapshot.jobs`. No new request, no daemon change.
  - Group headers render as separators so a user can tell what each block is.
- **Exit conditions:**
  - With nothing running, the overlay shows todos only, with no empty subagent or
    job heading.
  - With a running subagent and a queued job, both appear with names/statuses.
  - Layout math is covered by a test asserting the strip height never eats the
    transcript below `Min(10)`.
- **Open question:** the existing `/tasks` slash command (`src/tui/commands/tasks.rs`)
  governs *schedules*, not this overlay. Terminology overlap must not mislead —
  the overlay's help text should not call scheduled runs "tasks" in the same
  breath as in-flight subagents. Resolve in M005's documentation pass.

### M003 — Subagent tabs in the main buffer

- **Class:** capability (C2).
- **Objective:** open a subagent as a tab in the main LLM buffer panel; cycle
  with `Shift+N`/`Shift+P`; close with `Shift+X`.
- **Dependencies:** M001. Interface dependency from M004.
- **Deliverable boundary:**
  - A tab set mirroring `ProjectTabs`' `add_tab` / `remove_tab` / `set_active` /
    `active` / close-fallback semantics, including a capacity bound and an
    explicit "subject left the snapshot" state for D-b.
  - `Shift+N`, `Shift+P`, `Shift+X` bound in **both** encodings (SHIFT and NONE)
    per `src/tui/input.rs:556-579`, in whichever of the three keymaps owns
    normal-mode navigation.
  - A keyboard path to open a subagent tab from the focused panel row and from
    the tasks overlay. Today there is none; the only path is a mouse click.
  - The active tab's view keeps its existing Esc-closes behaviour; tab identity
    and view state are kept separate so a closed tab cannot resurrect scroll.
- **Exit conditions:**
  - Cycle order is stable and wraps; closing the last tab falls back exactly as
    `close_fallback_tab` does for projects.
  - A tab whose subject leaves the snapshot renders a defined state and never a
    panic or a stale claim.
  - Every new binding appears in the help overlay and passes the chord-disjoint
    tests in `tests/modifier_chords.rs`.
- **Explicitly not here:** any ability to type into a tab.

### M004 — Sessions panel subagent and job sections

- **Class:** capability (C1, C4).
- **Objective:** the sessions panel lists active subagents (name + status) and
  jobs the session is waiting on, navigable with `j`/`k`/arrows, opening a
  subagent as a tab on Enter/click.
- **Dependencies:** M003 (interface), M001.
- **Deliverable boundary:**
  - New collapsible sections in `src/tui/components/sidebar.rs` alongside the
    existing `AgentRuns` tree. Adding a section touches the row enum, section
    rendering, focus-identity, and the two `SidebarActivation` match sites.
  - A new `SidebarActivation::InspectSubagent(..)` arm mapping to a new
    `TuiMsg::OpenSubagentTab(..)`, dispatched through `src/tui/app/input.rs`
    (I2). It must **not** call `process_msg` directly.
  - Jobs filtered to those the session is waiting on; a section with nothing to
    show collapses or hides rather than rendering an empty list.
- **Exit conditions:**
  - `j`/`k`/arrows move focus across todo, subagent, and job rows without
    collapsing sections.
  - Enter and click both open the same tab for the same subagent.
  - The panel never renders a row absent from the snapshot.

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
| M001 ephemeral display name | partially landed (pool + drift guard; registry/projection pending) | — | — | none |
| M002 unified tasks overlay | not started | — | — | M001 |
| M003 subagent buffer tabs | not started | — | — | M001 |
| M004 sessions panel sections | not started | — | — | M001, M003 |
| M005 guards, help, docs | not started | — | — | M002, M003, M004 |
| M900 send-to-subagent | not approved for planning | — | — | design decision |