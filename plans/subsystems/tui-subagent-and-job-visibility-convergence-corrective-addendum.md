# TUI Subagent and Job Visibility — Convergence Corrective Addendum

Status: active

Source roadmap: `plans/subsystems/tui-subagent-and-job-visibility-roadmap.md`
Long-term references:
- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`

Related ADRs:
- None required for C001–C006. **C007 requires a new ADR** — it changes how a
  frontend addresses a delegated run, which meets the threshold in
  `plans/adrs/README.md`.

Repository baseline: `2acce2138`

## 1. Purpose and ownership boundary

This addendum exists because the parent roadmap's M001–M005 are marked
**landed** while a user following the natural path — `/sessions`, then `Ctrl+T`
— sees none of it.

The parent roadmap is **not wrong that the code landed**. A full trace of every
link confirms the naming chain, the overlay render chain, the sidebar sections,
the tab set, and the activation paths are all present and wired in production
code. This addendum therefore does not re-implement those surfaces. It fixes
the four things that actually make them unusable:

1. **The combined block is behind a chord that types instead of opening.** The
   overlay lives on `Shift+T`, which is Normal-mode-only, and the app boots in
   Insert — so the first press composes a literal `T`. The consolidation
   (D-C6) moves the combined block to `Ctrl+T`, which **resolves** this rather
   than documenting it: a CONTROL chord needs no `is_normal_only` entry, so the
   gate disappears instead of being explained.
2. **`/sessions` shows none of it.** The subagents and jobs sections landed in a
   *side panel* whose title bar literally reads `SESSIONS`; `/sessions` opens a
   flat modal dialog with **zero** `subagent` matches across its 3,101-line
   command module. The roadmap called the side panel "the sessions panel", so
   its closure claim reads to a user as "`/sessions` has subagents". It does
   not — and per D-C4 it now will.
3. **`/agents` opens the panel picker.** Bare `/agents` answers "which panel
   should hold agents?" instead of showing agents (C003).
4. **The chord requests are settled.** `Ctrl+S` and `Ctrl+J` are withdrawn in
   favour of the single `Ctrl+T`; `Ctrl+J` was independently undeliverable, and
   `Shift+W` stays `/workspace`.

**Ownership boundary.** Same as the parent: TUI *presentation and navigation* of
already-projected state. This addendum reads the snapshot, adds two additive
presentation fields (descriptors), and changes no scheduler, job, storage, or
execution authority. `scripts/check_execution_ownership.py` and
`scripts/check_scheduler_bypass.py` must stay green and `docs/execution-ownership.toml`
must not change.

## 2. Work classification

### Invariants

- **I1 — Projection-derived rows.** Every subagent, job, and todo row rendered
  originates in a `SessionProjectionSnapshot` field. No client-side fabrication,
  no client-side name minting. Carried forward unchanged from the parent.
- **I2 — Panel input authority.** Panel keys resolve through
  `SidebarActivation` → `PanelKey::Consumed` / `PanelKey::Intent(TuiMsg)` →
  `src/tui/app/input.rs`. Enforced by `scripts/check_tui_transport_authority.py`.
- **I3 — Bare-Ctrl prohibition.** A bare `Ctrl+<letter>` is never bound. See §5.
- **I4 — Working strip row 0 reserved.** Unchanged.
- **I5 — Tab ordering is not an execution boundary.** A subagent tab cancels
  nothing.
- **I6 (new) — Every advertised chord resolves to the action it is advertised
  for, in the mode it is advertised under.** The parent roadmap's M005 exit
  condition was `documented_keys_resolve_in_the_active_map`, which asserts only
  that the key resolves to *something*. A stale label — the live
  `Ctrl+T` → "Toggle sidebar" row — passes it. This is the guard gap that let a
  wrong help row survive, and it is the gap C001 closes.

### Capabilities

- **K1 — A binding is reachable without prior knowledge.** Every chord required
  to use the subagent surfaces declares its mode requirement on the surface the
  user is already looking at — or, better, needs no mode at all.
- **K2 — "Sessions" names one thing, and it lists the work.** `/sessions` shows
  the session's active subagents and the jobs it is waiting on, in the same
  place a user already looks for sessions. Superseded as *disambiguation only*
  by K2′ below.
- **K2′ — One chord for the three kinds of pending work.** `Ctrl+T` opens a
  combined todos / subagents / jobs block, showing only the groups that have
  rows. This is the consolidation of the original three requests (`Ctrl+T`
  todos, `Ctrl+S` subagents, `Ctrl+J` jobs) into the single chord the codebase
  already uses for the combined block.
- **K3 — Agent descriptors are visible.** A subagent row can show what its agent
  *is*, not only what it was asked to do.
- **K4 — A subagent started outside an active turn is observable.** The reducer
  currently discards the node and records a diagnostic; whether that is correct
  is an open question, not an assumption.
- **K5 — `/agents` opens agents.** Bare `/agents` currently opens the panel
  *picker*, so a user asking to see agents is asked instead which panel should
  hold them.
- **K6 — Interact with a subagent.** Blocked on an ADR. See §7 (C007).

### Infrastructure

- **F1 (carried, landed)** — `SubagentNameRegistry` and the 46-entry curated
  pool. Verified complete: `scripts/generate_subagent_names.py --check` passes
  and `src/agent/worker.rs:994-1004` is the only publisher of
  `AppEvent::SubagentStarted` in the repository, and it draws the name.

### Polish

- **P1 (new)** — The orphaned `src/tui/components/help_overlay.rs` is either
  wired into the module tree or deleted. It is currently neither.

## 3. Non-goals

- **Re-implementing M001–M005.** They landed in `c0d6ff2d3`. This addendum
  re-verifies and exposes them; it does not rebuild them.
- **A fourth independent list.** The surfaces must continue to read **one**
  derived `SubagentRow` vector (`src/tui/app/render.rs:2539-2544`). Adding
  subagents to the `/sessions` dialog is therefore **in scope** (C2), but it is
  a fourth *rendering of that same vector*, not a fourth projection of it. The
  selection model must discriminate row type; the data must not be re-derived.
- **Durable subagent names.** D1 in the parent roadmap stands. No migration, no
  `STORAGE_LAYOUT_VERSION` churn.
- **A second scheduler, a job store, or any job-lifecycle change.**
- **Changing `ToolBroker` / `JobSubmissionService` / `JobScheduler` authority.**
- **Nesting subagent rows.** `AgentTreeNodeProjection.parent_task_id` is still
  always `None`.

## 4. Current state (repo evidence)

### What genuinely works, verified end to end

A full trace confirms the parent's closure claims. These are **not** the problem:

| Surface | Status | Evidence |
|---|---|---|
| Curated name pool | 46 entries, generator in sync | `assets/subagent-names/names.toml`; `generate_subagent_names.py --check` exits 0 |
| Name drawn at spawn | Yes, RAII lease released on every exit path | `src/agent/worker.rs:994-1004` |
| Name carried in event → projection → TUI | Yes, `#[serde(default)]` on both new fields | `crates/codegg-core/src/bus/events.rs:307`, `crates/codegg-protocol/src/projection/dto.rs:465,473`, `src/tui/app/render.rs:2623` |
| `Shift+T` overlay render chain | Complete: allowance → allocation → carve → paint | `src/tui/app/mod.rs:14634` → `src/tui/app/render.rs:1323,223,285,1351` |
| Sidebar `Subagents` / `Jobs` sections | Rendered from the same derived vector | `src/tui/components/sidebar.rs:1341-1347`; fed at `src/tui/app/render.rs:2545,2510` |
| `j`/`k` focus + `Enter`/`click` → tab | Wired in the side panel and the overlay | `src/tui/panels/session.rs:66-99`; `src/tui/app/mod.rs:14440` |
| Subagent tab set, cycle, close, detached state | Complete | `src/tui/app/state/subagent_tabs.rs` |

`worker.rs:999` is the **only** publisher of `AppEvent::SubagentStarted` in the
repository, and it populates `display_name`. There is no second, name-less
producer. The naming chain does not break.

### D-1 — Every new chord is Normal-mode-only, and the app boots in Insert

`InputMode` defaults to `Insert`:

```rust
// src/tui/input.rs:73-78
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Insert,
    Normal,
}
```

`ToggleTasksList`, `NextSubagentTab`, `PreviousSubagentTab`, and
`CloseSubagentTab` are all listed in `is_normal_only`
(`src/tui/input.rs:561-599`). In Insert mode a capital `T` is caught by
`is_builtin_text_key` and dispatched as `InputAction::Char('T')` — a literal
character into the composer. Reaching the overlay requires knowing to press
`Esc` first (`src/tui/app/mod.rs:9558-9560`), and the only on-screen cue is the
`[NOR]` badge at `src/tui/app/render.rs:1961-1963`.

This is the single most likely reason the features read as dead. Every one of
the four chords has the same shape, which is why the failure looks uniform
rather than per-feature.

### D-2 — Two surfaces are both "sessions", and only one has subagents

| Surface | Identity | Opened by | Title | Subagent rows |
|---|---|---|---|---|
| Session **switcher** dialog | `Dialog::Session` / `SidePanelId::Sessions` | `/sessions` | `" Sessions "` | **none** |
| Session **side panel** | `SidePanelId::Session` / `SidebarWidget` | `Shift+S` panel manager | `"SESSIONS"` | **yes** |

`SidePanelId::Session` has title `"SESSIONS"` and config key `"sessions"`
(`src/tui/panels/mod.rs:624,672`); `SidePanelId::Sessions` has title
`"SWITCHER"` and config key `"session_switcher"`
(`src/tui/panels/mod.rs:630,678`) and routes straight to the dialog
(`src/tui/commands/panels.rs:412-415`).

`SessionDialog` (`src/tui/components/dialogs/session.rs:79-97`) has no
subagent field, renders one flat `sorted.iter()` loop (`session.rs:631`), and
`j`/`k` move a flat session index. `src/tui/commands/sessions.rs` (3101 lines)
contains **zero** occurrences of `subagent`. The parent's M004 test
(`tests/tui_side_panels.rs:811`) passes because its helper `focus_session`
opens the *panel manager*, never `/sessions`.

So M004 is landed, tested, and true — and unreachable by the route a user would
try first.

### D-3 — The middle of the naming chain has zero test coverage

Every TUI test hand-seeds `SubagentRow { display_name: Some("Sally") }`
(`tests/tui_main_buffer.rs:437,671,1143`; `tests/tui_side_panels.rs:798`).
`sync_subagents_from_snapshot` — the one function that turns a projection node
into a TUI row, and therefore the entire risk surface of "does a name actually
appear" — has no test reference anywhere in the repository. A regression in
`CoreEvent → adapter → snapshot → row` would be invisible to CI, which is
precisely the class of failure being reported.

### D-4 — Three requested chords, and what became of them

| Chord | Requested for | Actual state | Disposition |
|---|---|---|---|
| `Ctrl+S` | subagent overlay | `StashPrompt` (`src/tui/input.rs:829-832`); **save document** on the Editor route (`src/tui/commands/editor.rs:2666-2668`); pinned by `tests/modifier_chords.rs:50` and `src/tui/input.rs:2844` | **withdrawn** (D-C1) — keeps `StashPrompt` |
| `Ctrl+J` | jobs overlay | byte `0x0A`; crossterm reports `KeyCode::Enter`; binding it fires **Send** (`src/tui/input.rs:780-786`) | **withdrawn and undeliverable** (D-C2) |
| `Shift+W` | close tab | `OpenWorkspaceDashboard` (`/workspace`); P1 aligned it deliberately | **unchanged** (D-C3) — close is `Shift+X` |

`Ctrl+S` and `Ctrl+J` are withdrawn because the user consolidated all three
groups under one key (D-C6), so neither needs its own chord. `Ctrl+J` was
*independently* undeliverable: it was already raised, decided, and recorded as
**D4** in the parent roadmap — "It cannot be bound without firing Send. Jobs
were folded into `Shift+T` instead, which matches the user's own consolidation.
Recorded so it is not re-litigated." The withdrawal removes the desire for it;
it does not make it possible.

Separately, `Ctrl+T` is **route-dependent and undocumented**: on
`Route::Editor` it toggles the file tree and never reaches the binding table
(`src/tui/commands/editor.rs:1849-1853`, intercepted at
`src/tui/app/mod.rs:3988-3994`). C001 makes `Ctrl+T` the primary key for "what
is this session waiting on", which makes that route difference load-bearing —
see **D-C8**.

### D-5 — Two help defects

- **Stale label.** `src/tui/input.rs:1589-1594` advertises `Ctrl+T` as
  "Toggle sidebar" in the **Normal** section. The sidebar is `Ctrl+B`; `Ctrl+T`
  is `ToggleTodoList`. The Insert section (`:1231-1236`) is correct, so the two
  halves of one dialog disagree.
- **Orphaned source.** `src/tui/components/help_overlay.rs` is not declared in
  `src/tui/components/mod.rs` and is compiled nowhere; its own tests never run.
  It is also the only place documenting the Editor-route `Ctrl+S`.

### D-6 — Descriptors exist in the pool and are unreachable

`SubagentName.profile` and `.usefulness` are authored in `names.toml`, compiled
into `generated.rs`, and retrievable via `registry.rs::pool_entry`. **No
producer puts a descriptor on the event**, so the TUI never receives one. The
pool file states this at `names.toml:14-16`: *"Not rendered by the TUI today."*
All 46 `sprite` values are empty strings.

Separately, agent-type descriptions already exist in `assets/agents/*.toml` and
surface in the agents management panel (`src/tui/panels/agents.rs:362`) — but
not in the subagent strip, where the description column carries the *task*
description instead.

### D-7 — A subagent started outside an active turn is discarded

`crates/codegg-protocol/src/projection/reducer.rs:754-762` drops the node when
`snapshot.active_turn` is `None`, recording `orphan_subagent_started`. This is
the one place in the pipeline that actually loses data. Whether a subagent can
legitimately outlive its turn is an **open question**, not an established fact,
and C004 exists to answer it with evidence rather than to assume it.

### D-8 — Governance: "landed" was never closed through the gate

The parent roadmap's status table records M001–M005 as **landed** with `—` in
both the *Implementation plan* and *Closure record* columns, and the roadmap's
own header still reads `Status: proposed` while `plans/registry.md` reads
**active; M001–M005 landed; roadmap complete**. Per `plans/README.md`, *"A
commit message saying 'closed' is not closure evidence"* — these milestones
were marked landed without the implementation plan or closure record the
process requires. That is why a genuine defect (D-1, D-5) sat unrecorded for
two days behind a green suite.

## 5. Decision points

- **D-C1 — `Ctrl+S` for subagents. WITHDRAWN — superseded by D-C6.** The user
  withdrew the request: the subagent group does not need its own chord, because
  the combined block already shows it. `Ctrl+S` therefore keeps `StashPrompt`,
  and keeps *save* on the Editor route. No rebinding.
- **D-C2 — `Ctrl+J` for jobs. WITHDRAWN — superseded by D-C6, and independently
  undeliverable.** Byte `0x0A` is reported by crossterm as `KeyCode::Enter`, so
  binding it would fire **Send** on every press. This remains parent decision
  **D4** and must not be re-opened by any future request. The withdrawal removes
  the desire for it; it does not make it possible.
- **D-C3 — `Shift+W` to close a tab. WITHDRAWN — no change.** It is
  `/workspace`, aligned deliberately under the parent's P1. `Shift+X` already
  closes a subagent tab.
- **D-C4 — Subagents in the `/sessions` dialog. REVERSED — now in scope (C002).**
  An earlier draft recommended disambiguation instead, on the grounds that a
  fourth list would duplicate the selection model. The user directed otherwise:
  subagents and jobs **must** appear in the default `/sessions` surface, which is
  where someone looks for them. The duplication concern is answered by
  construction rather than by refusing the work — the dialog reads the same
  derived vector and gains a row-type-aware selection model (C002's boundary).
- **D-C6 (new; replaces D-C1/D-C2) — One chord: `Ctrl+T`.** The combined
  todos/subagents/jobs block moves to `Ctrl+T`, reusing the grouping and
  empty-group-omission behavior `build_task_rows` already implements.
  **This resolves D-1 rather than mitigating it**, and the mechanism is worth
  stating because it is not obvious:

  `ToggleTasksList` is in `is_normal_only` **only** because `Shift+T` is spelled
  as a capital letter, and a capital `T` mid-word must stay text
  (`src/tui/input.rs:580-584`). `Ctrl+T` carries `CONTROL` and is not a
  composable capital, so once `Shift+T` is retired the action needs no
  `is_normal_only` entry at all — and the overlay becomes reachable from Insert
  mode, which is where the app boots. **The mode gate disappears because the
  chord becomes unambiguous, not because the gate was relaxed.**

  Consequences the implementer must decide explicitly, never by omission:
  - Retire the `Shift+T` binding in **both** the `SHIFT` and `NONE` encodings.
    Keeping it would force `ToggleTasksList` to stay `is_normal_only`,
    reintroducing the exact Insert-mode bug that entry exists to prevent.
  - `Ctrl+T` currently maps to `ToggleTodoList` in all three keymaps, and two
    tests assert that mapping (`src/tui/input.rs:2529`, `:2570`).
  - `todo_expanded` / `toggle_todo_list` govern the compact above-input todo
    strip. `todo_strip_height` already returns 1 whenever todos exist
    (`src/tui/app/mod.rs:14462-14474`), so the always-visible one-line summary
    survives untouched — only its *in-place expansion* is lost, and the full todo
    list stays reachable as the overlay's first group.
  - **D-C9 (new, recorded during C001) — opening the overlay from Insert takes
    Normal mode with it.** Retiring the mode gate makes the overlay reachable
    from the composer, but the overlay's own keys (`j`/`k`/`Enter`/`Esc`) are
    Normal-mode gestures *by contract*: claiming bare letters while composing
    would break the composer. Shipping only the rebind therefore produced an
    overlay that opened from Insert and then ignored every navigation key — a
    different instance of the "the feature is dead" symptom this consolidation
    exists to remove. `App::tasks_restore_mode` records the mode to hand back,
    so opening borrows Normal mode and closing restores Insert. The alternative
    — leaving navigation Normal-only and documenting "press Esc first" — was
    rejected as a trap that looks like a bug.
  - **D-C7 — `todo_expanded`: retire, or keep and drive from the tasks toggle?**
    *Recommend retiring it.* The compact line stays legible without expanding
    anything, which is the parent's stated reason for keeping it, and the
    overlay is the only place the full list is ever needed.
- **D-C8 (new) — `Ctrl+T` on the Editor route.** The Editor route claims
  `Ctrl+T` for the **file tree** and never reaches the binding table
  (`src/tui/commands/editor.rs:1849-1853`, intercepted at
  `src/tui/app/mod.rs:3988-3994`). That was tolerable when `Ctrl+T` was a
  convenience; it is not tolerable once it is the primary key for "what is this
  session waiting on", because the key would then mean two different things
  depending on route. *Recommend: keep the Editor's tree toggle on `Ctrl+T`*
  (a long-established editor convention) **and make the route difference
  explicit in help** — the treatment the parent's I6 guard demands. Moving the
  editor toggle instead would change a working surface and belongs to the editor
  roadmap, not here.
- **D-C5 — Interaction with a subagent (C007).** *Blocked on an ADR.* There is
  no send path at all: `CoreRequest::TurnSteer` has **zero** callers under
  `src/tui/`, and it is keyed on a *session turn*, not a subagent run.
  `InputTarget` has only `Build` and `Chat`. Addressing a subagent run directly
  is net-new daemon work requiring decisions on run addressing, permission
  scoping, and what "reply" means for a delegated run.

## 6. Dependency graph

```
        (none — starts from a clean base)
                  |
                  v
  C001  one chord: Ctrl+T consolidates todos +        D-C6, D-C7, D-C8
         subagents + jobs; retire Shift+T;                     |
         help correctness + the I6 guard  <--------------------+
                  |
      +-----------+-----------+----------------+
      |           |           |                |
      v           v           v                |
  C002 /sessions  C003      C004 chain    C006 node
  gains Subagents + Jobs    coverage        lifetime
  sections  [K2]          [invariant]     (needs C002, C004)
      |           |           |
      |           |           |
      +-----------+-----------+
                  |
                  v
           C005 agent descriptors
                  |
                  v
           C007 send-to-subagent
            [BLOCKED — ADR required]
```

- **C001 → C002, C003, C004**: hard. C001 settles the chord set and the
  overlay every later milestone references; the `/sessions` sections (C002) and
  the `/agents` box (C003) are both entry points a user reaches first, so they
  should not land against a chord set that is about to change.
- **C002 → C006**: soft. The dialog's sections and the node-lifetime question
  both concern "a row disappeared"; sequencing them together turns a repeat
  report into one investigation.
- **C004 → C005, C006**: hard. C005 extends the row the chain C004 makes
  observable, and C006 changes whether that row survives its own event.
- **C007**: no dependency from the near-term slice; blocked on its ADR.


## 7. Milestones

### C001 — One chord: `Ctrl+T` consolidates todos, subagents, and jobs

- **Class:** capability, delivering invariants I3 and I6.
- **Objective:** one key opens everything this session is waiting on, reachable
  from the mode the app boots in.
- **Dependencies:** none.
- **Deliverable boundary:**
  - `(CONTROL, Char('t'))` maps to `ToggleTasksList` in **all three keymaps**.
  - Retire the `Shift+T` binding in both its `SHIFT` and `NONE` encodings, and
    drop `ToggleTasksList` from `is_normal_only` — justified by D-C6, and
    **only** because no capital-letter spelling reaches the action any more.
  - The group-omission behavior is reused as-is; `build_task_rows` is already
    correct on this point and is not rewritten.
  - Resolve D-C7 for `todo_expanded`, recording the choice.
  - I6 guard: `documented_keys_resolve_in_the_active_map` compares the resolved
    action against the advertised label, not merely that the key resolves.
  - Fix the stale `Ctrl+T` Normal help row (`src/tui/input.rs:1592`), and
    document the Editor-route `Ctrl+T`/`Ctrl+S` overrides per D-C8.
  - Purge `src/tui/components/help_overlay.rs` — not declared in
    `src/tui/components/mod.rs`, compiled nowhere, tests never run.
- **Exit conditions:**
  - `Ctrl+T` opens the combined block from Insert mode with no prior `Esc`.
  - With nothing running, no empty subagent or job heading appears.
  - A capital `T` typed mid-word in Insert mode still composes text.
  - Every advertised chord resolves to the action it advertises, in its mode.
  - The I6 guard is proven non-vacuous by an injected mismatch.

### C002 — `/sessions` gains Subagents and Jobs sections

- **Class:** capability (K2), reversing D-C4.
- **Objective:** the default `/sessions` surface lists the session's subagents
  and the jobs it is waiting on, with status, navigable, and — for subagents —
  opening a buffer tab.
- **Dependencies:** C001.
- **Deliverable boundary:**
  - `SessionDialog` gains sectioned subagent and job rows, fed from the **same**
    derived `SubagentRow` vector the strip, sidebar, and overlay read
    (`src/tui/app/render.rs:2539-2544`). No fourth projection.
  - Its selection model becomes **row-type aware**. Today `selected: usize`
    indexes a flat session list (`src/tui/components/dialogs/session.rs:83`) and
    `j`/`k` move that index; that cannot address a subagent row. Introduce a row
    discriminator without breaking existing session navigation.
  - `j`/`k`/arrows walk sessions, then subagents, then jobs; Enter on a
    subagent row opens the same tab the sidebar and overlay open, via
    `TuiMsg::OpenSubagentTab { task_id }`.
  - Jobs are filtered with `state::tasks::is_live_state`, the one predicate the
    overlay uses. A job row is **read-only** and consumes Enter rather than
    fabricating a destination — there is no job transcript, and inventing one is
    the parent's recorded refusal.
  - Empty sections contribute **no header and no row**.
- **Exit conditions:**
  - The dialog lists exactly the rows the snapshot reported, with no fabrication.
  - Enter and the sidebar's Enter resolve the same tab for the same `task_id`.
  - `j`/`k` never leak to the composer as a turn.
  - A finished job does not rebind focus to a different job (id-keyed, not
    index-keyed — the parent's recorded defect).

### C003 — `/agents` opens agents

- **Class:** capability (K5).
- **Objective:** bare `/agents` shows agents instead of asking which panel
  should hold them.
- **Dependencies:** C001.
- **Current evidence:** bare `/agents` calls
  `open_panel_picker(self, "agents")` (`src/tui/app/mod.rs:5530-5541`). The
  comment there defends the choice — the old direct-jump "looked like nothing
  happened" — but the experience is a dead end: a user who asked to see agents
  is asked instead where to put them.
- **Deliverable boundary:** open an agents surface directly. Panel *placement*
  stays reachable through `/panels`; the two questions are different and should
  not share an entry point. If no real agents dialog exists, that is the work —
  do not route around it by relabelling the picker.
- **Stop condition:** if satisfying this requires building a new agents dialog,
  report scope before building rather than substituting the picker.

### C004 — Projection-to-row chain coverage

- **Class:** invariant.
- **Objective:** the middle of the naming chain — the part with zero coverage —
  is pinned, so a name that stops appearing fails CI instead of a person.
- **Dependencies:** C001.
- **Deliverable boundary:** an end-to-end test driving
  `CoreEvent::SubagentStarted { display_name, run_id, .. }` → projection adapter
  → `SessionProjectionSnapshot.agent_tree` → `sync_subagents_from_snapshot` →
  `SubagentRow.display_name.is_some()`. Plus the mirror paths — `sidebar.set_subagents`,
  `build_task_rows`, and (after C002) the `/sessions` sections — asserting every
  surface renders one name per `task_id`.
- **Exit conditions:** every surface is covered by a named test that fails if
  any link drops the field; a replay-determinism test reproduces identical names
  (the parent's M001 condition, still untested).

### C005 — Agent descriptors

- **Class:** capability.
- **Objective:** a subagent row shows what its agent *is* as well as what it was
  asked to do.
- **Dependencies:** C004.
- **Deliverable boundary:** thread `profile` / `usefulness` from the pool onto
  the event and into the projection as **additive, `#[serde(default)]`** fields,
  exactly as `display_name` and `run_id` were. **No new pool authoring** — the
  descriptors are already curated and compiled; this only makes them reachable.
  Keep `apps/desktop` bridge field parity
  (`apps/desktop/scripts/check-bindings.mjs`).

### C006 — Node lifetime when there is no active turn

- **Class:** capability.
- **Objective:** determine, with evidence, whether
  `ProjectionEvent::SubagentStarted` should be discarded when `active_turn` is
  `None`, and either fix it or record why it is correct.
- **Dependencies:** C002, C004.
- **Deliverable boundary:** establish from the job/subagent lifecycle whether a
  running subagent can exist with no active parent turn. If yes, the reducer's
  drop is a defect and the node must be retained (bounded, evicted on terminal
  state). If no, pin the invariant with a test so a future change cannot quietly
  invalidate it.
- **Stop condition:** if the answer requires durable subagent identity, stop and
  escalate — that is parent D5 and needs an ADR.

### C007 — Send-to-subagent *(blocked)*

- **Class:** capability. **Status: blocked on a new ADR.**
- **Objective:** let a user interact with an open subagent.
- **Blocker:** no send path exists. `TurnSteer` has zero TUI callers and is keyed
  on a session turn, not a subagent run; `InputTarget` has only `Build` and
  `Chat`.
- **Required ADR scope:** how a frontend addresses a subagent run; permission
  scoping for a delegated run; what "reply" means for a run that reports to a
  parent rather than a human; and whether replying re-enters the parent's turn
  or forks a new one.
- **Until then:** C001–C006 must not imply this capability exists. A subagent
  tab is read-only, and a closed tab does not stop its run (I5).


## 8. Cross-cutting requirements

**Storage.** None. D1 in the parent roadmap still holds and is what keeps this
addendum migration-free.

**Protocol.** Additive, `Option`-wrapped, `#[serde(default)]` fields only, so a
mixed-version daemon and frontend still connect. **C005 is the only milestone
that adds any, and it adds two.**

**Security.** No new authority. Viewing a subagent discloses projected state the
user can already see. Tabs cancel nothing. C007, when unblocked, is the first
milestone here that could change authorization, which is a second reason it
needs an ADR.

**Concurrency.** Name release must remain exactly-once including cancel and
panic paths. The RAII lease already guarantees this; C002 must not introduce a
second release path.

**Observability.** `orphan_subagent_started` is currently the only signal that
a node was dropped. C006 decides whether it stays a diagnostic or becomes a
user-visible state.

## 9. Verification strategy

- `scripts/verify.sh quick` after every milestone.
- `cargo test --test modifier_chords` for binding disjointness (already covers
  the `BARE_CTRL_TWINS` set that pins `Ctrl+S`).
- `cargo test --test tui_main_buffer` and `cargo test --test tui_side_panels`
  for the render and selection claims.
- `cargo test -p codegg-core` for the reducer and replay determinism.
- `scripts/check_tui_transport_authority.py`, `scripts/check_projection_*.py`,
  `scripts/check_websocket_bounds.py`, and
  `apps/desktop/scripts/check-bindings.mjs` where fields change.

## 10. Completion definition

This addendum is complete when a user can, without reading documentation: press
a single chord and land on a surface that lists their running subagents by name
and status; open one as a tab and cycle and close it; see todos, subagents, and
jobs together; and never encounter two surfaces that disagree about whether
subagents exist — with a test that fails if any of those regresses silently.

Interaction with a subagent (C007) is **not** part of this definition.

## 11. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 `Ctrl+T` consolidation + reachability | closed | `plans/implementation/tui-subagent-and-job-visibility-convergence/001-ctrl-t-consolidation-and-reachability.md` | `plans/closure/tui-subagent-and-job-visibility-convergence/001-status.md` | none |
| C002 `/sessions` Subagents + Jobs sections | not started | — | — | C001 (done) |
| C003 `/agents` opens agents | implemented, unclosed | — | — | C001 (done) |
| C004 projection-to-row coverage | implemented, unclosed | — | — | C001 (done) |
| C005 agent descriptors | not started | — | — | C004 (done) |
| C006 node lifetime | implemented, unclosed | — | — | C002, C004 (C004 done) |
| C007 send-to-subagent | blocked | — | — | `plans/adrs/ADR-0040-subagent-run-addressing-and-human-interaction-authority.md` (**proposed**, not accepted) |
