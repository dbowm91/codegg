# Desktop Frontend and IDE Foundation Milestone M007-B — Chat Region and Side-Panel Frame

Status: ready for handoff

Repository baseline: `c5d3f4dc4c35ae702a7bbaf14009526295d6ac8a` — the merged tree carrying
the desktop-parity work (`cd9c2fab4 feat(desktop): project the whole session snapshot and
render it`)

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`
- M007 decomposition table: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m007-decomposition`

Planning basis:

- Roadmap §3.1 amendment (`:133`) — the **docking system** moved from "not delivered by this
  roadmap" (`:120`) into scope **for M007-B**, as the side-panel frame and its placement model
  (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:143-144`).
- Roadmap §7 — the line's own "what does not exist, and is why the line has a gate
  milestone first" evidence (`:498-511`). This milestone is the first to consume it.

Long-term requirements:

- `plans/000-long-term-specification.md:97` §4.4 — frontends MUST render frontend-neutral
  snapshots and events, and MUST NOT become independent owners of provider credentials,
  project authorization, global scheduling, or durable session state.
- `plans/000-long-term-specification.md:426` — "Observation MUST render the same logical
  activity panel from frontend-neutral state." The canonical **panel-parity** requirement is
  *logical* panel identity, not pixel identity.
- `plans/000-long-term-specification.md:476` — frontends MUST use project-relative
  file/artifact operations.
- `plans/000-long-term-specification.md:536-538` — projections must support snapshots,
  monotonic events, resume and explicit resynchronization, and every payload must have an
  explicit size bound.
- `plans/002-long-term-roadmap.md:262` — Phase 5 exit criterion: "Two different frontend
  implementations can reconstruct the same session state."
- `plans/002-long-term-roadmap.md:243` — Phase 5 objective names "local TUI, remote TUI,
  observer mode, ACP, web, and future clients" as consumers of one canonical session view.
- `plans/002-long-term-roadmap.md:17` — cross-phase execution rule 3: "add typed protocol
  and storage representations before frontend-only state."

Applicable ADRs and architecture:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md` (accepted) — the
  desktop is a frontend over the shared client, not a second daemon and not a second
  authority.
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted) —
  frontend-owned presentation state is legitimate; durable state is not.
- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md` (accepted) —
  bound on the M007 line, not consumed by this milestone.
- `architecture/desktop.md` — the "Trust boundary" section (`:14-22`), the hosted
  qualification section (`:154-198`), and "Deferred surfaces" (`:289`).
- `architecture/tui.md` — the side-panel host model (`:3507-3600`) and the frame split
  (`:3644-3660`).
- `architecture/client.md` — the frontend-side client boundary and the session projection
  driver.

Primary class: capability

> This milestone is a **renderer presentation** milestone. It adds no daemon authority, no
> protocol variant, and no durable state, and it is not a claim of IDE parity. The
> keyboard surface it will eventually need is **M007-C**, and the `/` command surface is
> **M007-D** and is blocked on ADR-0052.

## 1. Objective

Deliver a **main chat region and a dockable side panel** in the desktop renderer, mirroring
the TUI's placement model, and render the projection fields the desktop already receives:
the 631-line single component at `apps/desktop/src/App.tsx:28` becomes named regions (chat
region, side-panel frame, composer region, status/context row), each in its own module
under `apps/desktop/src/`; the side panel gains the TUI's **placement** concept (`right` /
`left` / `bottom`), a panel registry, one slot per edge and a pointer **resize seam**; a
**tasks panel** renders the TUI's canonical task surface — **todos, live jobs and active
subagents** — together with the two projection identifiers the baseline still drops
(`runs` and `recentTurns`), from the `SessionPresentationView` the desktop already
receives, with **zero Rust work**; session and activity panels render from the same
existing projection; a real layout system replaces the single-column ad-hoc frame in
`style.css`; and the new regions and panels get renderer unit/component tests.

One bounded outcome: **a user can open the desktop app, read the conversation in a main
chat region, and dock a side panel — on the right, left, or along the bottom — that shows
the todos, live jobs, active subagents, runs and recent turns the daemon is already
reporting, and the renderer is no longer one component.**

## 2. Why this milestone is ready

Closed hard dependencies:

- M001…M006-E are closed, and ADR-0010, ADR-0011 and ADR-0012 are accepted
  (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:648`). The projection
  contract this milestone renders is the same one the TUI renders.
- The `SessionPresentationView` DTO and its Rust/TypeScript field parity are already
  pinned by `apps/desktop/scripts/check-bindings.mjs`, whose `SessionPresentationView`
  contract lists `runs`, `jobs`, `subagents`, `agent_tree`, `agent_runs`, `tool_programs`,
  `worktrees`, `run_groups`, `convergences`, `recent_turns` and `todos`
  (`apps/desktop/scripts/check-bindings.mjs:41`). That guard makes the fields a **frozen,
  already-checked contract**, not an invitation to add one.

Stable interface dependencies:

- `present_snapshot` (`apps/desktop/src-tauri/src/present.rs:488`) already derives
  `runs` (`:604-616`), `jobs` (`:617-629`), `subagents` (`:639-646`) and `recent_turns`
  (`:700-705`) with explicit bounds — `MAX_VISIBLE_RUNS = 10`, `MAX_VISIBLE_JOBS = 10`,
  `MAX_VISIBLE_SUBAGENTS = 10`, `MAX_RECENT_TURNS = 5` (`:54-63`).
- The desktop's live projection path already goes through `present_driver`
  (`apps/desktop/src-tauri/src/projection.rs:226`), which delegates to `present_snapshot`
  when a snapshot is installed (`present.rs:735-741`).

**The hard dependency on M007-A is not satisfied by this plan and is not waived by it.**
`plans/implementation/desktop-frontend-ide-foundation/011-desktop-build-and-hosted-gate.md`
installs the gate that compiles the desktop Rust workspace and runs the renderer's
typecheck and tests on every desktop-tree change. Until M007-A closes:
`apps/desktop/src-tauri` is excluded from the root Cargo workspace (`Cargo.toml:6`),
deliberately, per roadmap §9 (`:575-583`), so **no automated gate compiles or tests the
desktop tree at all** (`:505-507`); the hosted visible-window lane is manual-dispatch-only
and still red (`architecture/desktop.md:181-192`); and `scripts/verify.sh quick` runs
`scripts/check-desktop-boundary.sh` (a static source guard, `scripts/verify.sh:71-72`)
but **does not compile the desktop tree** and does not run `npm run typecheck`,
`npm test`, or `npm run bindings:check`.

Roadmap §10 states the rule this milestone inherits: "No closure condition may silently
become 'builds on the maintainer machine.'" (`:597`). **M007-B may be implemented before
M007-A closes, but it cannot be closed until M007-A has installed the gate and this
milestone's evidence has run inside it.**

No ADR is required for M007-B. It introduces no protocol, no storage, no authorization
surface, and no new ownership contract: it renders an existing projection and owns
presentation state that ADR-0011 already classifies as frontend-owned.

## 3. Current implementation evidence

At the baseline (the merged desktop-parity tree, in which `apps/desktop/src/panels/*.tsx`
now exists):

- **The renderer is one component.** `App()` is declared at `apps/desktop/src/App.tsx:28`
  and the file is 631 lines. It holds 26 `useState` hooks (`:29-78`) and 6 `useEffect`
  calls (`:217`, `:227`, `:261`, `:286`, `:320`, `:372`), and it renders four stacked
  `<section className="panel">` cards — Daemon (`:596`), Projects (`:599`), Route
  (`:600-605`), Session (`:611-621`) — followed by the baseline's panel modules
  (`:622-629`). There is still **no region, no frame and no region composition
  boundary**: the panels are composed inline in the same file, in document order, with no
  region container, no named slot and no per-edge area.
- **The panel modules now exist, and they are panel-level, not region-level.** The
  desktop-parity work added eight modules under `apps/desktop/src/panels/`:
  `TaskPanel.tsx`, `SubagentPanel.tsx`, `ActivityPanel.tsx`, `TranscriptPanel.tsx`,
  `TurnSummary.tsx`, `ToolDetail.tsx`, `GitPanel.tsx` and `SkillsPanel.tsx` (plus
  `format.ts`). `App.tsx:623-628` composes six of them and `App.tsx:608-609` composes the
  other two. Each renders one
  `<section className="surface">` over props; **none** owns a region container, a slot, a
  placement or a resize seam, and **none** of them is this milestone's to create.
- **The desktop's bridge invoke targets are 23.** `apps/desktop/src/bridge.ts` names 23
  distinct `desktop_*` targets (`:22-94`), and the host registers the same 23
  `#[tauri::command]` functions (`apps/desktop/src-tauri/src/lib.rs:1058-1078`). The three
  that arrived with the desktop-parity work are `desktop_todo_refresh`
  (`bridge.ts:53-54`), `desktop_git_read` (`:60-61`) and `desktop_skill_catalogue`
  (`:62-63`). M007-B adds **none** of them.
- **There is still no layout system.** `apps/desktop/src/style.css` is 145 lines — a themed
  design system with custom properties on `:root` (`:28-`), typography and component
  classes — but the **frame** is still one centred column:
  `main { max-width: 760px; margin: 8vh auto; padding: 0 24px; }` (`:55`), which the
  stylesheet's own header describes as "the desktop is a single-column shell" (`:24-25`).
  The only `display: grid` is the definition-list layout `dl { display: grid; … }` (`:73`);
  there is no frame grid, no `grid-template-areas`, and no named region.
- **The renderer has zero keyboard handlers.** There is no `onKeyDown`, `onKeyUp`,
  `onKeyPress` or `addEventListener` for any key event anywhere under `apps/desktop/src/`,
  and no `KeyboardEvent` type is named there. Twelve `onClick` bindings live in `App.tsx`
  and four more in the panel modules — 16 across `apps/desktop/src/`. **This milestone does
  not fix that** — the keyboard surface is M007-C, a recorded dependency rather than scope.
- **Two of the four identifiers are now rendered; two are still dead payload.**
  `SessionPresentationView` declares `runs: RunSummaryView[]`
  (`apps/desktop/src/bridge-types.ts:298`), `jobs: JobSummaryView[]` (`:299`),
  `subagents: SubagentSummaryView[]` (`:301`) and `recentTurns: TurnSummaryView[]`
  (`:310`). The baseline **renders** `jobs` and `subagents`: `App.tsx:628` passes
  `projection.jobs` to `TaskPanel`, and `App.tsx:626` passes `projection.subagents` and
  `projection.agentTree` to `SubagentPanel`. The identifiers `projection.runs` and
  `projection.recentTurns` appear **nowhere** under `apps/desktop/src/` — the presenter
  populates them and the renderer drops them: `present.rs:604-616` derives `runs` and
  `present.rs:700-705` derives `recent_turns`, and neither reaches a component. The
  renderer reads **21** distinct `projection.<field>` identifiers today; the two it still
  drops are `runs` and `recentTurns`. The Rust presenter already derives all four with
  explicit bounds (`present.rs:604-705`, `:54-63`), and finishing the pair the baseline
  left open **requires zero Rust work**.
- **The TUI's canonical task surface is `Ctrl+T`, and it carries todos, live jobs and
  active subagents.** The overlay's rows are built by `build_task_rows`
  (`src/tui/app/state/tasks.rs:173`), whose three sources are `todos`, `subagents` and
  `jobs` in a fixed order (`:170`, `:174-176`); the overlay is drawn by
  `render_tasks_strip` (`src/tui/app/render.rs:1394`, documented at `:1387-1393`), fed
  from `App::task_rows` (`src/tui/app/mod.rs:15374`). A desktop tasks panel that agrees
  with the TUI carries that same content set — not a different one.
- **The daemon already bounds what a renderer may show.** `MAX_PENDING_PERMISSIONS = 16`
  (`present.rs:98`) with the pending set derived from `turn.pending_permissions`
  (`:177-178`, `:546-569`); messages cap at 100 (`:50`, `:509-511`), tools at 20 (`:52`,
  `:528`). The renderer must render what arrives and must not invent its own truncation
  markers.
- **"No snapshot yet" and "empty session" are distinguishable.** `present_driver`
  publishes a skeleton with empty `runs`/`jobs`/`subagents`/`recent_turns`
  (`present.rs:742-774`) and a `state` of `connecting` / `subscribing` / `resyncing` /
  `disconnected` / `unavailable` (`present.rs:110-119`) when no snapshot is installed. A
  panel that renders "nothing to show" for both is lying about the second.
- **The TUI's model to mirror is already a model, not a widget.** `PanelRegions
  { main, left, right, bottom }` (`src/tui/layout.rs:37`), with `main` documented as "The
  transcript, composer and everything that is not a panel"; `TuiLayout::split`
  (`src/tui/layout.rs:242`) resolves columns first and gives the bottom row whatever
  remains; `PanelPlacement { Right, Left, Bottom }` with **`Right` the default**
  (`src/tui/panels/mod.rs:219-229`, `architecture/tui.md:3516-3518`); `SidePanelHost`
  (`src/tui/panels/mod.rs:1191`) holds **one slot per edge** (`{panel, visible, history}`,
  `src/tui/panels/mod.rs:1093`, `architecture/tui.md:3520-3524`) and `SidePanelHost::dock`
  **moves** rather than copies (`src/tui/panels/mod.rs:1336`,
  `architecture/tui.md:3536-3538`).
- **The TUI's resize rules are geometric, not keyboard-specific.**
  `resolve_resize` (`src/tui/panel_resize.rs:108`) answers "which panel's size changes,
  and in which direction", and only *shared* borders move, with growth relative to the
  other surface (`architecture/tui.md:3568-3581`). `column_panel_width`
  (`src/tui/layout.rs:292-298`) collapses the column rather than squeezing the transcript
  out of existence. The desktop must mirror those **rules**; the **chord** is M007-C.
- **The TUI's panel catalogue is 32 kinds, and it is not the desktop's target.**
  `SidePanelId::ALL` has 32 variants (`src/tui/panels/mod.rs:513-546`) spanning session,
  chat, terminal, tree, tasks, plan, goal, memory, logs, context, cost, models, agents,
  MCP, plugins, team, diff, review and files. M007-B mirrors the **model** — a registry,
  an id, a placement, a slot — and ships a **small** registry. Shipping 32 empty panels
  would be padding, not parity.
- **The renderer test harness already exists and mocks the bridge.**
  `apps/desktop/src/App.test.tsx` mocks `./bridge` (`:6`), drives it with
  `@testing-library/react` and `vitest` (`:1-2`), and asserts through `data-testid`
  attributes. The new panels extend that harness rather than inventing a second one.

## 4. Invariants that must not regress

1. The renderer renders **frontend-neutral snapshots and events** and owns no provider
   credentials, project authorization, global scheduling, or durable session state
   (`plans/000-long-term-specification.md:97`).
2. Authorization is enforced **in the daemon, not solely in frontends**
   (`plans/000-long-term-specification.md:602`). The renderer validates nothing and
   authorizes nothing; a panel is a view over what the daemon already decided.
3. The renderer holds **no** provider state, credential, scheduler authority, job
   admission, or writer permit. It cannot start, cancel, or reprioritize anything. The
   only mutations it may perform are the ones the existing bridge already exposes to the
   whole frontend (`promptSubmit`, `permissionRespond`, `questionRespond`,
   `controlRefresh` — `apps/desktop/src/bridge.ts:32-41`).
4. `scripts/check-desktop-boundary.sh` keeps its **current meaning**: the production
   capability file grants the renderer **zero** Tauri plugin permissions, the CSP stays
   local-only with no `unsafe-eval`, and no shell/filesystem/process/HTTP/clipboard/
   updater/global-shortcut or generic `core_request` bridge exists. M007-B must not
   weaken, bypass, or re-scope that guard.
5. The desktop Rust tree stays a **separate Cargo workspace** with its own MSRV
   (`apps/desktop/src-tauri/Cargo.toml:6`, `:8-9`; roadmap §9, `:575-583`). M007-B is a
   renderer milestone: it does not make `apps/desktop/src-tauri` a root workspace member
   and does not change the root's Rust 1.89 contract (`Cargo.toml:12`).
6. Node/TypeScript dependencies and lockfile stay confined to `apps/desktop`
   (roadmap §9, `:579`).
7. **Logical panel parity, not pixel parity** (`plans/000-long-term-specification.md:426`).
   The desktop's tasks panel shows the same logical activity — the same todos, live jobs,
   active subagents, runs and turns — as the TUI's, in the desktop's own layout. The two
   frontends are not required to look alike, and this milestone does not attempt to.
8. Presentation state the renderer owns (which panel is docked where, panel width, scroll
   position) is **frontend-owned** under ADR-0011 and is never transmitted as authority,
   never persisted daemon-side, and never sent to the daemon as a request argument.
9. Every payload the renderer displays is **already bounded by the daemon**. The renderer
   adds no unbounded buffer and no unbounded list.
10. No change to the TUI's behavior, no change to the shared `codegg-client` crate's
    semantics, and no change to `codegg-protocol`.

## 5. Scope

### In scope

- Splitting `App.tsx` into named regions — **chat region**, **side-panel frame**,
  **composer region**, **status/context row** — as separate modules under
  `apps/desktop/src/`. Adding TypeScript files under `apps/desktop/src/` is expected and
  encouraged.
- A side-panel frame with the TUI's placement concept (`right` / `left` / `bottom`),
  `right` as the default, a panel registry, one slot per edge, a dock operation that
  moves rather than copies, and a pointer **resize seam**.
- A **tasks panel** rendering the TUI's canonical task surface — **todos, live jobs and
  active subagents** — plus the two projection identifiers the baseline still drops
  (`runs` and `recentTurns`), from the projection the desktop already receives — **no
  protocol change**.
- A **session panel** and an **activity panel** rendered from the same existing
  projection.
- A real layout and styling system replacing the single-column frame in `style.css`.
- Renderer unit/component tests for the new regions, the registry, the panel host and the
  panels.
- Documentation updates named in §12.

**Why the tasks panel's content set is what it is.** The baseline's `TaskPanel`
(`apps/desktop/src/panels/TaskPanel.tsx:44`) already renders the todos and the live jobs,
and its own header states that it is "the TUI's `Ctrl+T` task surface in DOM form — 'todos,
live jobs, active subagents'" (`TaskPanel.tsx:6-10`), with the subagents half in
`SubagentPanel` (`SubagentPanel.tsx:22`). The TUI's canonical surface agrees: the
`Ctrl+T` overlay's rows come from exactly three sources — todos, subagents and jobs
(`src/tui/app/state/tasks.rs:173-176`, order fixed at `:170`). So the content set is
corrected to **that** set — todos, live jobs, active subagents — rather than the
`runs`/`jobs`/`subagents`/`recentTurns` set this plan originally named, which would have
invented a task surface different from the TUI's and would have duplicated work the
baseline has already done. On top of that corrected set, the panel additionally renders the
two identifiers the baseline still drops — `runs` and `recentTurns` — because this
milestone's own acceptance criterion (§13.5) requires the renderer to stop
receiving-and-discarding all four. `runs` and `recentTurns` are *not* part of the TUI's
`Ctrl+T` task surface; they are additional history the daemon already reports and the
baseline already carries, and this milestone renders them so the "received and discarded"
defect is closed rather than narrowed.

### Explicitly out of scope

- **Keyboard and chord handling** — that is **M007-C**
  (`plans/implementation/desktop-frontend-ide-foundation/013-desktop-keyboard-first-input.md`).
  M007-B ships pointer-only affordances; the zero-keyboard-handler fact is a recorded
  dependency, not a defect M007-B may fix.
- **The `/` command surface** — that is **M007-D**, blocked on ADR-0052's acceptance
  (roadmap `:520`, `:523-534`). The desktop's command surface must not be reimplemented
  in the renderer.
- **The editor and the file tree** — that is **M007-E**
  (`plans/implementation/desktop-frontend-ide-foundation/015-desktop-editor-surface.md`):
  no editor, no document-open affordance, no diagnostics, no completion.
- **Any new `CoreRequest` variant, any protocol change, any storage change, any
  migration.** If a panel appears to need new daemon data, name the existing variant it
  would use, or stop and report per §14.
- **Any change to the TUI.** M007 mirrors the TUI; it does not modify it (roadmap
  `:541-545`).
- **Any Tauri plugin, shell, filesystem, process, HTTP, clipboard, updater, or
  global-shortcut capability in the renderer.** `scripts/check-desktop-boundary.sh`
  enforces this; M007-B must not weaken it and must not add a permission to
  `apps/desktop/src-tauri/capabilities/main.json`.
- **Git UI, graphical terminal, Monaco, or any third-party editor engine** (roadmap
  `:541-545`, `:144-153`). The baseline's read-only project-facts `GitPanel`
  (`apps/desktop/src/panels/GitPanel.tsx`) and `SkillsPanel`
  (`apps/desktop/src/panels/SkillsPanel.tsx`) arrived with the desktop-parity work and are
  **not** M007-B's; M007-B adds no Git surface and does not replace them.
- **Persisting the panel layout.** The TUI persists placement as `ui.panel_placement`
  (`architecture/tui.md:3656-3658`); M007-B does not. Panel placement is renderer state
  for the lifetime of the renderer, and a renderer reload returns it to the default. A
  durable frontend-owned store is a separate decision (§14).
- **Collaborative editing, web/mobile frontends, remote desktop access.** Product
  non-goals.

## 6. Required production changes

### Frontend or renderer

- **Regions.** Four named regions, each in its own module:
  - **chat region** — the transcript: `projection.messages`,
    `projection.truncatedMessages`, `projection.tools`, and the artifact list. This is the
    surface the TUI calls the "main buffer".
  - **side-panel frame** — the docked panel, its header, and its resize seam.
  - **composer region** — the prompt textarea, the plan-mode checkbox, and the submit
    button, taken from the existing Route card (`App.tsx:604`).
  - **status/context row** — `projection.state`, the current turn, the cursor diagnostic,
    and the controller summary, taken from the existing Session card (`:611`, `:616`).
  `App.tsx` remains the composition root: it keeps the bridge wiring, the React state that
  owns **no** durable state, and the region composition. The Daemon and Projects cards
  (`App.tsx:596`, `:599`) stay as they are — connection and catalog surfaces, not session
  surfaces — but they leave the chat region. The baseline's panel modules
  (`App.tsx:623-628`) move **into** the side-panel frame and the chat region rather than
  being rewritten: they are panels, and this milestone gives them somewhere to be.
- **A panel registry.** A TypeScript module exporting a `PanelId` union, a `PANEL_REGISTRY`
  mapping id → `{ label, render }`, and the `PanelPlacement` type
  (`'right' | 'left' | 'bottom'`). The registry is **data, not a switch statement** — the
  TUI's registry is a list and the manager renders that list directly
  (`src/tui/panels/mod.rs:500-506`), and a second registry that can drift from the
  render path is the defect that rule exists to prevent.
- **A panel host.** One slot per edge; each slot is `{ panelId, visible }`. `dock(id,
  placement)` **moves** the panel and never loads one panel on two edges at once
  (mirroring `src/tui/panels/mod.rs:1191`, `:1093`, `:1336`). `right` is the default edge.
  The host owns no session state — a panel reads the projection it is given.
- **The tasks panel.** Renders the TUI's canonical task content — **todos, live jobs and
  active subagents** — plus the two identifiers the baseline still drops, **`runs` and
  `recentTurns`**. The baseline already renders the todos and the live jobs in
  `apps/desktop/src/panels/TaskPanel.tsx` and the subagents in
  `apps/desktop/src/panels/SubagentPanel.tsx`; **this milestone's tasks panel reuses those
  modules' rendering and adds the `runs` and `recentTurns` sections**, rather than
  reimplementing the todo and job rows. Each row shows the daemon's own status token and
  summary. The panel renders **exactly what arrives** and adds no truncation marker of its
  own — the daemon already bounded the lists (`present.rs:54-63`) and reported no
  truncation count for them.
- **The session panel and the activity panel.** Session renders the session's identity,
  current turn, and controller summary from the same projection; activity renders the
  projection's cursor diagnostic, resync reason, and the artifact handle list. Neither
  performs a mutation beyond the existing bridge calls.
- **A pointer resize seam.** A draggable inner border on the docked panel. The geometry
  mirrors the TUI's rules, not its chords: only **shared** borders move, growth is
  **relative** (growing the panel shrinks the main region), and a column **collapses**
  rather than squeezing the main region below the minimum (`src/tui/layout.rs:292-298`).
  A bottom-docked panel takes a **ratio** of the frame height, not a fixed row count
  (`src/tui/layout.rs:130-135`).
- **A layout system.** `style.css` becomes a real stylesheet: a CSS grid frame with named
  areas for the regions and the three panel edges, CSS custom properties for the panel
  width and the minimum main width, and a scroll container per region so the chat region
  and the side panel scroll **independently**. The current `max-width: 760px` single
  column is removed.

### Runtime (Tauri host)

**None.** M007-B changes nothing under `apps/desktop/src-tauri/`. The presenter already
derives every field the new panels render, the bridge already forwards the whole
`SessionPresentationView`, and `apps/desktop/scripts/check-bindings.mjs` already pins the
Rust/TypeScript field parity for `RunSummaryView`, `JobSummaryView`,
`SubagentSummaryView` and `TurnSummaryView`. If the implementation appears to need a
host-side change, that is a stop condition (§14), not a task.

### Protocol and DTOs

**This milestone adds no new protocol variant, no new DTO field and no new bridge
target.** The baseline is: the daemon's `CoreRequest` enum has **268** variants (242
struct + 26 unit) and `CoreResponse` has **176** (175 struct + 1 unit)
(`crates/codegg-protocol/src/core.rs:2330` and `:1202`; measured by
`python3 scripts/measure_overview_counts.py` and cross-checked against
`architecture/overview.md`'s "Verified counts" table, `:565-566`), and the desktop's bridge
names **23** `desktop_*` invoke targets (`apps/desktop/src/bridge.ts:22-94`). Three of those
targets — `desktop_todo_refresh` (`bridge.ts:53-54`), `desktop_git_read` (`:60-61`) and
`desktop_skill_catalogue` (`:62-63`) — arrived with the desktop-parity work and are
**recorded here as the baseline, not as this milestone's to change**. M007-B adds **zero**
to all three numbers: no `CoreRequest` variant, no `CoreResponse` variant, no `CoreEvent`
variant, no DTO field added, removed or renamed, and no bridge target.

If a panel this milestone adds appears to need new daemon data, **name the existing
variant it would use** (`apps/desktop/src/bridge.ts:22-94` lists every bridge call, and
`crates/codegg-protocol/src/core.rs:2330` lists every request variant) **or stop and
report** (§14).

### Storage and migrations

**None.** No migration, no schema version change, no new persisted state, no
`STORAGE_LAYOUT_VERSION` change. The panel layout is **not** persisted — see §5 and §14.

### Security and authorization

- The renderer gains **no** Tauri capability: `apps/desktop/src-tauri/capabilities/main.json`
  keeps its empty permission list and the CSP keeps local-only origins with no
  `unsafe-eval`.
- **This milestone's panel modules add no bridge call and fetch nothing.** They render the
  projection they are given; every value they display arrived on the existing projection
  channel. **Recorded baseline fact, not a violation:** the desktop's existing fetches
  live in `App.tsx`, not in the panel modules — `desktop_git_read` (`App.tsx:608`),
  `desktop_skill_catalogue` (`App.tsx:609`) and `desktop_todo_refresh` (`App.tsx:628`'s
  refresh callback) are called from the composition root, and the git, skills and todo
  **values** arrive on those bridge calls rather than on the projection channel. The
  presenter says so explicitly for todos: they "live behind a separate daemon read
  (`CoreRequest::TodoList`), so the pure presenter carries no list" and the host's refresh
  overlays it (`present.rs:706-710`). M007-B adds **no** new bridge call and **no** new
  fetch; the modules it adds fetch nothing.
- The renderer adds **no** credential, provider, scheduler, job-admission, or
  writer-permit surface. A panel that appears to need one is a stop condition (§14).
- The resize seam is a **pointer** interaction inside the renderer. It changes a CSS
  custom property and nothing else — no file, no process, no daemon state.
- Panel content is rendered as **text**, never as HTML. `dangerouslySetInnerHTML` and
  equivalent are prohibited in `apps/desktop/src/`: a projection field is untrusted input
  from a model's output, and the renderer must not interpret it.
- `scripts/check-desktop-boundary.sh` must pass **unchanged**, and its meaning must not
  change: it is the mechanical form of ADR-0010's renderer-compromise containment.

### Documentation and static guards

- One new focused static guard under `scripts/check_*.py` asserting that the desktop
  renderer's panel layer introduces **no** new `invoke` target beyond the existing bridge
  surface, and that no panel module holds a Tauri capability or a filesystem/process
  call. Follow the pattern of `scripts/check-desktop-boundary.sh`. If the guard can be
  expressed as an extension of an existing guard instead of a new file, prefer that; do
  not leave the invariant as a prose promise. A guard whose defects fail silently
  protects nothing — give it a `--self-test` that injects the defect and proves the
  guard rejects it, and wire it into `scripts/verify.sh quick`.
- `scripts/check-desktop-boundary.sh` remains applicable and must pass unchanged.

## 7. Ordered work packages

### Work package A — Surface the two projection fields the renderer still receives

Intent: take the single cheapest user-visible win in the M007 line before touching the
frame. Zero Rust work, zero protocol work.

**Delivered by the baseline — not re-done here.** The baseline already renders `jobs`
(`App.tsx:628` → `TaskPanel`) and `subagents` and `agentTree` (`App.tsx:626` →
`SubagentPanel`), and the baseline's `TaskPanel` already carries the TUI's canonical task
content — the todos and the live jobs (`TaskPanel.tsx:6-10`, `:42`). That half of this
work package is **done**.

Files:

- **NEW** `apps/desktop/src/panels/RunHistory.tsx` — renders the projection's `runs`
  (`bridge-types.ts:298`), each row showing the daemon's own status token and summary.
- **NEW** `apps/desktop/src/panels/RecentTurns.tsx` — renders the projection's
  `recentTurns` (`bridge-types.ts:310`), with the same status/summary rule.
- **MODIFIED** `apps/desktop/src/panels/TaskPanel.tsx` — compose the two new sections
  into the existing task surface (or render them in one module; the split above is the
  expected shape, not a second registry).
- **MODIFIED** `apps/desktop/src/App.tsx` — pass `projection.runs` and
  `projection.recentTurns` into the composition (`:622-629`).
- **NOT created by this milestone**: the eight panel modules under
  `apps/desktop/src/panels/` that the desktop-parity work already added.

Required changes:

- A distinct "no snapshot installed yet" state when `projection.state` is one of
  `connecting`, `subscribing`, `resyncing`, `disconnected`, `unavailable`
  (`present.rs:110-119`) — the driver skeleton publishes empty lists in exactly those states
  (`present.rs:742-774`), and "no snapshot yet" must not read as "this session has no
  jobs".
- The identifiers `projection.runs` and `projection.recentTurns` now appear in the
  renderer, alongside the already-rendered `projection.jobs` and `projection.subagents`.
  That is the observable end of this work package.

Acceptance evidence:

- A renderer test that feeds a projection carrying one run, one job, one subagent and two
  recent turns and asserts each is rendered with its status and summary.
- A renderer test that feeds a driver-skeleton projection (empty lists, `state:
  "connecting"`) and asserts the panel says so rather than rendering an empty list.
- `git diff` shows **no** change under `apps/desktop/src-tauri/`.

### Work package B — Region split

Intent: stop being one component, without changing what the user sees yet.

Files:

- **NEW** `apps/desktop/src/regions/ChatRegion.tsx`,
  `apps/desktop/src/regions/SidePanelFrame.tsx`,
  `apps/desktop/src/regions/ComposerRegion.tsx`,
  `apps/desktop/src/regions/StatusRow.tsx` — one module per region named in §6.
- **NEW** `apps/desktop/src/regions/index.ts` — the region registry, if the implementer
  wants one; the region set is small enough that a plain exported map is enough.
- **MODIFIED** `apps/desktop/src/App.tsx` — the composition root.
- **NOT created by this milestone**: `apps/desktop/src/panels/*.tsx` (the eight modules
  the desktop-parity work added) — they are moved **into** regions, not rewritten.

Required changes:

- Extract the four regions named in §6 into their own modules. Each region is a pure
  presentational component over props; none calls `bridge` directly and none holds
  durable state.
- `App.tsx` keeps the bridge wiring, the subscription lifecycle, and the region
  composition. It must no longer contain the transcript, permission/question, composer, or
  status markup inline.
- The existing `data-testid` values **must survive** the split unchanged — the current
  renderer test suite (`apps/desktop/src/App.test.tsx`) is the compatibility contract, and
  a renamed test id is a silent break of the E2E specs at `apps/desktop/e2e/specs/`, which
  select on those attributes.

Acceptance evidence:

- `App.tsx` is materially shorter and no longer contains the transcript markup.
- The **existing** renderer test suite passes **unmodified** — no test id changed, no
  assertion weakened.
- A new test asserting the chat region and the side-panel frame are distinct elements in
  the composed tree.

### Work package C — Panel registry, placement model, and resize seam

Intent: give the side panel the TUI's *shape* without importing the TUI's *catalogue*.

Files:

- **NEW** `apps/desktop/src/panels/registry.ts` — the `PanelId` union, the `PANEL_REGISTRY`
  map and the `PanelPlacement` type.
- **NEW** `apps/desktop/src/panels/host.ts` — the panel host, its per-edge slots and the
  `dock(id, placement)` operation.
- **NEW** `apps/desktop/src/panels/ResizeSeam.tsx` — the pointer resize seam.
- **MODIFIED** `apps/desktop/src/App.tsx` — the composition root holds the host's state.
- **NOT created by this milestone**: the eight panel modules under `apps/desktop/src/panels/`
  that the desktop-parity work already added — the registry **registers** them, it does
  not author them.

Required changes:

- A `PanelPlacement` type (`'right' | 'left' | 'bottom'`) with `'right'` as the default
  edge, mirroring `PanelPlacement { Right, Left, Bottom }` with `Right` default
  (`src/tui/panels/mod.rs:219-229`, `architecture/tui.md:3516-3518`).
- A panel registry as data: id → `{ label, render }`. The desktop's registry ships the
  panels the tree already has content for: **tasks** (`TaskPanel.tsx`), **subagents**
  (`SubagentPanel.tsx`), **activity** (`ActivityPanel.tsx`) and **session** (the session
  summary this milestone extracts into its own module). The baseline's `TranscriptPanel`,
  `TurnSummary`, `ToolDetail`, `GitPanel` and `SkillsPanel` are **not** side-panel
  entries at this milestone — `App.tsx` composes them inline and they carry their own
  content, so registering them would claim a placement the milestone does not give them.
  The registry does **not** ship the TUI's 32 panel kinds, and it does not ship empty
  placeholders for them — a panel that renders nothing is a claim the renderer cannot keep.
- A panel host with one slot per edge, a `dock(id, placement)` that **moves** rather than
  copies, and a per-slot visibility flag (mirroring `SidePanelHost`,
  `src/tui/panels/mod.rs:1191`, `:1093`, `:1336`).
- A pointer resize seam on the docked panel's shared border, honoring the TUI's rules:
  shared borders only, relative growth, a minimum main width, and a collapsed column
  rather than a squeezed main region (`src/tui/layout.rs:292-298`,
  `architecture/tui.md:3568-3581`).
- The panel's own state (scroll position, expanded sections) is keyed by panel id and
  travels with the id when the panel moves between edges — the TUI's split between panel
  state and slot state (`architecture/tui.md:3520-3524`).

Acceptance evidence:

- A test asserting the default edge is `right` and the default panel is the tasks panel.
- A test asserting `dock` moves a panel: after docking the tasks panel `left`, the
  `right` slot is **empty**, not a second copy of the tasks panel.
- A test asserting the resize seam clamps: dragging past the minimum main width leaves
  the main region at its minimum and the panel at its minimum, rather than producing a
  negative or zero-width main region.
- A test asserting the registry has no entry whose renderer returns nothing.

### Work package D — Layout and styling system

Intent: replace ad-hoc CSS with a frame that can actually host a dockable panel.

Files:

- **MODIFIED** `apps/desktop/src/style.css` — the only stylesheet. The frame rules, the
  named areas and the panel-geometry custom properties land here.
- **NOT created by this milestone**: no new stylesheet, no CSS module and no
  `grid-template-areas` exists in the tree today — `style.css:55` is still the whole
  frame.

Required changes:

- A CSS grid frame with named areas: the chat region, the composer region, the status
  row, and one area per panel edge. The `left`, `right` and `bottom` areas are empty when
  that edge is unoccupied and the grid template collapses them — the `None` case in
  `PanelRegions` (`src/tui/layout.rs:34-35`), where an unshown edge means "collapsed, or
  squeezed out of a frame too small to host it honestly".
- A scroll container per region, so the chat region and the side panel scroll
  **independently** and scrolling one does not move the other.
- CSS custom properties for the panel width, the minimum main width, and the bottom panel
  ratio, mirroring `LayoutConfig` (`src/tui/layout.rs:108-140`).
- The `max-width: 760px` single column (`style.css:55`) is removed; the desktop frame is a
  full-window application frame, not a document, and must render in a narrow window
  without the main region collapsing to zero.

Acceptance evidence:

- A test asserting the chat region and the side panel have **separate** scroll
  containers.
- A test asserting the frame's region elements are all present in the composed tree.
- A visual/manual check recorded in the closure record: the app at 1280×800 and at
  900×600.

### Work package E — Renderer test sweep

Intent: make the new surfaces regression-proof and prove the *old* path is unchanged.

Files:

- **NEW** `apps/desktop/src/regions/*.test.tsx` — one component test file per new region.
- **NEW** `apps/desktop/src/panels/registry.test.ts` and
  `apps/desktop/src/panels/host.test.ts` — the registry and the host.
- **NEW** `apps/desktop/src/panels/RunHistory.test.tsx` and
  `apps/desktop/src/panels/RecentTurns.test.tsx` — the two new panel sections from work
  package A.
- **NOT created by this milestone**: the eight panel modules' own `*.test.tsx` files
  already exist beside them (`apps/desktop/src/panels/*.test.tsx`) and are the baseline's
  to maintain; they must pass unmodified.

Required changes:

- Component tests for each new region and each new panel, following the existing
  `apps/desktop/src/App.test.tsx` conventions (mock `./bridge`, assert through
  `data-testid`, `vitest` + `@testing-library/react`).
- A test proving the **existing** render path is unchanged: the same projection still
  produces the same transcript, tool list, artifact list, permission list, question list
  and controller line as before the split.
- A test proving no projection field is silently dropped: given a projection with every
  field populated, the rendered tree contains a row derived from **each** of `messages`,
  `tools`, `runs`, `jobs`, `subagents`, `recentTurns`, `artifactHandles`,
  `pendingPermissions` and `pendingQuestions`.
- The new static guard from §6, with its `--self-test` proving it is not vacuous.

Acceptance evidence:

- `npm test` passes with the full suite, including the pre-existing tests unmodified.
- The guard's `--self-test` injects a violating construct and proves the guard rejects
  it, then the construct is reverted.

## 8. Failure, cancellation, restart, and contention semantics

- **Projection resync.** A resync replaces the **snapshot**, not the panel host's state.
  The docked panel, its edge, and its width survive a resync unchanged — panel layout is
  frontend presentation state under ADR-0011 and is not daemon-owned. The panel's
  *contents* are re-rendered from the new snapshot.
- **A panel outliving its subject.** If the session is closed, changes, or a resync
  removes the subject a row referred to, the panel renders an explicit empty state. It
  must not keep showing a row the daemon has stopped vouching for. The TUI models exactly
  this shape — a tab whose subject vanishes is "a state, not an error"
  (`architecture/tui.md:999-1004`) — and the desktop does the same.
- **"No snapshot yet" versus "empty session".** A driver skeleton (empty lists, state
  `connecting` / `subscribing` / `resyncing` / `disconnected` / `unavailable`,
  `present.rs:742-774`) renders the "no snapshot installed yet" state. A snapshot that
  genuinely contains no runs renders the per-section empty state. These are different
  facts and the renderer must not conflate them.
- **Renderer reload.** The renderer reloads to its defaults: the panel is docked `right`,
  the tasks panel is selected, widths are the defaults. Nothing is persisted, so nothing
  is lost that the daemon owns. The **daemon's** session, runs, jobs and turns are
  unaffected by a renderer reload — the renderer holds no authority and loses none.
- **Stale completion.** The renderer's existing generation-guard discipline (a stale
  event after a reconnect is ignored — `App.test.tsx:153`) applies unchanged to every
  region. A projection delivered for a superseded session must not overwrite the current
  one, and M007-B must not weaken that guard while splitting the component.
- **Disconnect.** The panels keep showing the last received snapshot and the status row
  shows the disconnected state. The panels do **not** clear themselves, because the last
  snapshot is still the daemon's last word. The status row carries the state, not the
  panels.
- **Contention on the resize seam.** One pointer drag at a time; a second drag while one
  is in flight is ignored rather than interleaved. The resize is a pure presentation
  write, and a drag interrupted mid-gesture leaves the width at its last committed value.
- **Renderer unload.** The existing teardown discipline (the projection-subscription effect
  at `App.tsx:320`, its cleanup at `:358`) is preserved exactly. Splitting the component
  must not introduce a second subscription or drop the existing one.

## 9. Compatibility and migration

- **No wire change.** No protocol negotiation, no version-skew behavior, no
  `PROTOCOL_VERSION` change. The daemon's 268 `CoreRequest` variants, the desktop's 23
  bridge targets and the `SessionPresentationView` DTO are untouched — this milestone
  adds none of them (§6 "Protocol and DTOs").
- **No storage change.** No migration, no `STORAGE_LAYOUT_VERSION` update, no new
  persisted state.
- **The existing render path is unchanged.** This is the compatibility requirement, not a
  convenience: a user on the current build must see the same conversation, tool list,
  artifacts, permission and question controls, and controller line after this milestone.
  **The change is additive in what is shown (four new sections of activity) and structural
  in how it is composed (regions, a frame).** The existing renderer test suite passing
  unmodified is the proof.
- **No configuration keys are added.** The panel layout is not persisted; restoring the
  last layout would need a decision this milestone does not make (§14).
- **Nothing is removed.** There is no legacy layout path to retire, and the Daemon and
  Projects cards stay.
- **The Rust/TypeScript bridge contract is unchanged.** `apps/desktop/scripts/check-bindings.mjs`
  must pass unmodified — it is the guard that would catch a silent field divergence.

## 10. Required tests

### Focused unit tests

- The panel registry: every registered id has a label and a renderer; every registered
  renderer returns something; the registry and the render path cannot drift.
- The panel host: default edge is `right`; `dock` moves rather than copies; a slot's
  visibility is per-slot, not a global flag (mirroring
  `architecture/tui.md:3526-3532`); re-docking a panel to the edge it is already on is a
  no-op rather than a self-push (`:3548-3550`).
- The resize geometry: growth is relative; only shared borders move; the main region
  clamps at its minimum; the bottom panel's height is a ratio of the frame height.
- Panel state travels with the panel id when the panel moves between edges.

### Integration tests

- The tasks panel renders **todos, live jobs and active subagents** plus the **runs** and
  **recent turns** the daemon reports, from a populated projection; the session and
  activity panels render from the same projection.
- A panel switch (tasks → subagents → activity → session) changes the docked content
  without changing the chat region's content.
- Docking a panel to a second edge moves it, leaving the first edge empty.
- The composed frame contains the chat region, the composer region, the status row and
  the side-panel frame, and the chat region and the side panel scroll independently.

### Restart and recovery tests

- A projection resync replaces the panel contents and leaves the panel layout unchanged.
- A renderer unmount before the projection resolves leaves no subscription and no stale
  update — the existing teardown behavior, asserted in the new composition.
- A daemon disconnect leaves the last snapshot visible and the status row reporting the
  disconnect.

### Contention and cancellation tests

- A second resize drag while one is in flight is ignored.
- A stale projection delivered after a reconnect does not overwrite the current one — the
  existing generation guard, asserted against the new regions.
- The resize seam cannot drive the main region to zero width or the panel to zero width.

### Security and negative tests

- **No panel module added or changed by this milestone performs an `invoke` call** —
  asserted by the new static guard, not only by a test. (The baseline's own fetches live
  in `App.tsx`; see §6 "Security and authorization".)
- No panel module contains a Tauri capability, a filesystem read, a process spawn, or a
  `fetch`/`XMLHttpRequest`.
- No panel renders projection text as HTML; a projection message containing markup
  renders as literal text.
- `scripts/check-desktop-boundary.sh` passes unchanged, and its capability and CSP
  assertions still hold.

### Migration and compatibility tests

- **None required** — M007-B introduces no migration. Assert instead that the **existing**
  renderer test suite passes unmodified, that `npm run bindings:check` passes unmodified,
  and that no file under `apps/desktop/src-tauri/` changed.

## 11. Required verification commands

```bash
# the desktop is a separate npm workspace
cd apps/desktop
npm run typecheck
npm test
npm run bindings:check
cd ../..

# the desktop Rust host is a separate Cargo workspace and is NOT touched by M007-B
cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml

# the renderer boundary guard and the new panel guard
bash scripts/check-desktop-boundary.sh
python3 scripts/check_<new-desktop-panel-guard>.py
python3 scripts/check_<new-desktop-panel-guard>.py --self-test

# root gates
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

**`scripts/verify.sh quick` does not compile the desktop tree.** It runs `cargo fmt
--check --all`, the two generated-asset checks, the guard scripts (including
`scripts/check-desktop-boundary.sh`, `scripts/verify.sh:71-72`) and a root workspace
`cargo check` — and `apps/desktop/src-tauri` is excluded from the root workspace
(`Cargo.toml:6`). The gate that compiles the desktop tree and runs the renderer's
typecheck and tests on every desktop change is **M007-A**
(`plans/implementation/desktop-frontend-ide-foundation/011-desktop-build-and-hosted-gate.md`).
Until that gate exists, the commands above are run by hand, and per roadmap §10 that
cannot be the closure condition (`:597`).

Do not claim a command that was not actually run. If a command fails, record it and its
classification in the closure record rather than dropping it.

## 12. Documentation updates

- `architecture/desktop.md` — record the new renderer structure: the four regions, the
  panel registry and host, the placement model, the pointer resize seam, the rule that
  panel layout is **not** persisted, and the explicit statement that the panel layer
  renders only what the daemon's projection already carries. Update the "Trust boundary"
  section to state that the panel layer adds no capability, and keep the
  hosted-qualification section (`:154-198`) accurate.
- `architecture/overview.md` — it carries **counted numbers**: the module map
  (`:740-770`), the `apps/desktop/` tree entry (`:761`), the "Integration tests (335
  files)" count (`:762`), the workspace-member exclusions list (`:275`), and the
  superseded-counts note (`:1023`). Re-derive any count this milestone changes and update
  the number rather than appending prose — a stale count in this file is a defect. The
  new renderer modules sit inside the `apps/desktop/` entry and do not change the
  workspace-member list.
- `architecture/tui.md` — **only if** an ownership description moves. M007-B changes no
  TUI behavior; the expected edit is a cross-reference recording that the desktop now
  mirrors the `PanelRegions` / `SidePanelHost` placement model. Do not restate the TUI's
  own rules here.
- `architecture/client.md` — **only if** the projection-consumer description needs it. The
  expected edit is one clause recording that a second frontend now renders the canonical
  session view.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — update **M007-B only**:
  its row in the M007 decomposition table (`:518`) and its row in the milestone-status
  table (`:637`). Do not touch M007-A, M007-C, M007-D or M007-E, and do not change any
  other section.
- `plans/registry.md` — register the M007-B plan, then update that registration to its
  closure on acceptance.
- Do **not** edit `plans/000-long-term-specification.md`,
  `plans/001-terminology-and-domain-model.md`, `plans/002-long-term-roadmap.md`, or any
  ADR. A milestone plan is not justification for changing long-term direction, and the
  canonical documents are the authority this milestone is measured against.

## 13. Acceptance criteria

1. A user can open the desktop app, see the conversation in a **main chat region** and a
   **side panel** side by side, and scroll each **independently**.
2. A user can dock the side panel **right**, **left**, or **bottom**, and the layout
   follows — the main region stays usable at every placement, and the panel is never
   rendered on two edges at once.
3. A user can **resize the side panel by dragging its border**, and the main region and
   the panel grow and shrink relative to one another, clamping at a minimum rather than
   collapsing.
4. The **tasks panel shows the todos, live jobs and active subagents the daemon reports**,
   together with the **runs** and **recent turns** the daemon reports, with their statuses,
   and says so distinctly when no snapshot is installed. Its content set agrees with the
   TUI's `Ctrl+T` overlay (`src/tui/app/state/tasks.rs:173-176`).
5. The renderer **no longer receives-and-discards** `runs`, `jobs`, `subagents` and
   `recentTurns`: each is rendered, and a test asserts a row derived from each. (`jobs` and
   `subagents` are already rendered by the baseline; `runs` and `recentTurns` are the two
   this milestone closes.)
6. The renderer is **no longer one component**: the transcript, the composer, the status
   row and the side-panel frame are separate modules, and `App.tsx` no longer contains
   the transcript markup inline.
7. The renderer has a **real layout system** — a grid frame with named regions, per-edge
   panel areas, custom properties for panel geometry, and independent scroll containers —
   replacing the 760-pixel single column (`style.css:55`).
8. **No protocol variant was added, removed, or renamed**, no `CoreRequest` variant was
   introduced, no DTO field changed, and no bridge target was added: the daemon's 268
   `CoreRequest` variants, the 176 `CoreResponse` variants and the desktop's 23 bridge
   targets are unchanged by this milestone. `apps/desktop/scripts/check-bindings.mjs`
   passes unmodified.
9. **No file under `apps/desktop/src-tauri/` changed**, no storage change landed, and no
   migration was written.
10. `scripts/check-desktop-boundary.sh` still passes, with its meaning unchanged: the
    renderer holds zero Tauri plugin permissions, the CSP stays local-only, and no
    shell/filesystem/process/HTTP/clipboard/updater/global-shortcut or generic
    `core_request` bridge exists.
11. The **existing** renderer test suite passes **unmodified** — the pre-existing
    transcript, tool, artifact, permission, question and controller surfaces behave
    exactly as before.
12. `npm run typecheck`, `npm test` and `npm run bindings:check` are green, and
    `cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml` succeeds.
13. `scripts/verify.sh quick`, `cargo fmt --all -- --check` and workspace Clippy with
    `-D warnings` are green.
14. Keyboard handling, the `/` command surface, the editor, the file tree, any **new** Git
    UI, a graphical terminal and any third-party editor engine are **provably absent**
    from the diff, and no durable panel-layout persistence was added. (The baseline's
    read-only `GitPanel` and `SkillsPanel` are the desktop-parity work's and are not
    touched.)
15. The panel layer holds **no** credential, provider, scheduler, job-admission or
    writer-permit surface, and **no panel module added or changed by this milestone
    performs an `invoke` call**.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- **A panel this milestone adds appears to need new daemon data** — a new `CoreRequest`
  variant, a new field on `SessionPresentationView`, or a new bridge target. This milestone
  adds **none** of the three (§6 "Protocol and DTOs"): the daemon's 268 request variants,
  its 176 response variants and the desktop's 23 bridge targets are the recorded baseline,
  and the three targets the desktop-parity work added (`desktop_todo_refresh`,
  `desktop_git_read`, `desktop_skill_catalogue`) are **not** this milestone's to change.
  **Name the existing variant the panel would use, or stop.**
- **The layout appears to need the renderer to hold durable state** — a persisted panel
  layout, a stored scrollback, a remembered selection that must survive a reload. That
  would introduce a durable frontend-owned store and a persistence surface M007-B has no
  storage or configuration change for. Panel placement stays renderer-local.
- **The work appears to require a Tauri capability the boundary forbids** — any
  filesystem, shell, process, HTTP, clipboard, updater, or global-shortcut permission, or
  any relaxation of the local-only CSP. `scripts/check-desktop-boundary.sh` is the
  authority, and weakening it is not an option.
- **A panel appears to need credentials, provider state, scheduler authority, job
  admission, or a writer permit.** Those are daemon-owned. A panel that needs them is
  asking for a second authority, which ADR-0010 forbids.
- **The work starts absorbing keyboard handling** — a keymap, chords, focus traversal, an
  insert/normal mode. That is **M007-C**. If the frame cannot be operated by pointer
  alone, that is an M007-C finding to report, not an M007-B task to take.
- **The work starts absorbing the `/` command surface.** That is **M007-D**, blocked on
  ADR-0052's acceptance, and it changes an ownership contract.
- **The milestone would expand into the editor, the file tree, LSP, diagnostics, or
  completion.** That is **M007-E**.
- **A panel's content cannot be rendered from the existing projection** — because the
  daemon does not report it. Stop and name what is missing; do not infer it, and do not
  compute it in the renderer from data the daemon did not send.
- **The registry would have to ship panels that render nothing** to reach the TUI's 32
  panel kinds. The desktop mirrors the TUI's *model*, not its catalogue; a placeholder
  panel is a claim the renderer cannot keep. Report the gap instead.
- **The existing renderer test suite cannot pass unmodified** after the split. A changed
  `data-testid` is a silent break of the built-app E2E specs at `apps/desktop/e2e/specs/`;
  if a test id genuinely must change, stop and report the E2E impact.
- **A static guard or the canonical invariant set cannot be satisfied as written**, or the
  milestone would expand into another subsystem roadmap.

## 15. Closure evidence required

The closure record at `plans/closure/desktop-frontend-ide-foundation/012-status.md` must
contain:

- implementation commits, and confirmation that **no protocol, storage, or migration
  change landed** — shown by an empty diff for `crates/codegg-protocol/`,
  `crates/codegg-core/`, and `apps/desktop/src-tauri/`;
- a **requirement-to-evidence matrix** mapping each §13 acceptance criterion to a named
  test, guard, or command outcome;
- the full §11 command list with **actual** outcomes, including any command that failed
  and its classification;
- **proof the projection fields are genuinely rendered rather than merely present in the
  DTO** — the renderer test asserting a row derived from each of `runs`, `jobs`,
  `subagents` and `recentTurns`, plus the diff showing those four identifiers now appear
  in `apps/desktop/src/`. `jobs` and `subagents` were already rendered by the baseline
  (`App.tsx:626`, `:628`); the milestone's own contribution is `runs` and `recentTurns`,
  and the evidence must show those two moving from **nowhere** under `apps/desktop/src/` to
  rendered;
- **proof the milestone's protocol scope held** — the re-measured daemon `CoreRequest` and
  `CoreResponse` variant counts (the baseline is 268 / 176) and the desktop's bridge
  target count (the baseline is 23), shown before and after and unchanged;
- **proof the boundary guard still passes** — the `scripts/check-desktop-boundary.sh`
  output, and confirmation that `apps/desktop/src-tauri/capabilities/main.json` and the
  CSP are byte-identical to the baseline;
- **proof the new guard is not vacuous** — its `--self-test` output, with the injected
  defect and the revert;
- **proof the existing render path is unchanged** — the pre-existing renderer test suite
  passing **unmodified**, and the list of `data-testid` values before and after the split
  being identical;
- the **M007-A gate** evidence: the command outputs from inside the gate M007-A installs,
  not a maintainer-machine transcript. If M007-A has not closed when M007-B closes,
  M007-B is **not closed** — it is implemented and awaiting the gate;
- the panel-registry inventory — every registered panel id, its placement, and the
  projection fields it renders — with the deliberate omissions from the TUI's 32 panel
  kinds named and justified;
- the recorded decisions that panel layout is **not** persisted (and the deferred
  persistence follow-up), and that the desktop registry mirrors the TUI's *model* and not
  its *catalogue*;
- the evidence that the panel layer performs no `invoke` call and holds no capability,
  with the baseline's own `App.tsx` fetches (`desktop_git_read`, `desktop_skill_catalogue`,
  `desktop_todo_refresh`) named as the baseline's and not the milestone's;
- documentation updates actually made, with the re-derived `architecture/overview.md`
  counts shown before and after;
- unresolved findings classified by severity, with any medium or high finding keeping
  M007-B open;
- a recommendation of **closed**, **conditionally closed**, **corrective pass required**, or
  **blocked**.

Per `plans/003-planning-process.md` §2.5, a commit message asserting closure is not closure
evidence. A build that compiles is not closure.

## 16. Handoff notes

- **Authority order** (`plans/003-planning-process.md:204-210`): the canonical long-term
  specification and terminology; then the accepted ADRs; then the subsystem roadmap; then
  this plan; then current repository evidence. Where repository evidence conflicts with
  this plan, preserve the canonical invariant, record the discrepancy, and make the
  smallest coherent adjustment. The required planning review is
  `plans/003-planning-process.md:266-279`.
- **Canonical vocabulary.** Use "client connection"
  (`plans/001-terminology-and-domain-model.md:143`), "snapshot" (`:672-676`), "agent
  activity projection" (`:393-395`), "session open" (`:277-281`) and "session attachment"
  (`:271-275`). **The canonical set has no domain noun for a UI panel** — the word does
  not occur in `plans/001-terminology-and-domain-model.md`. "Side panel", "chat region" and
  "tasks panel" are **UI terms only**; the canonical requirement they serve is logical
  panel parity (`plans/000-long-term-specification.md:426`).
- **Do not edit** `plans/000-long-term-specification.md`,
  `plans/001-terminology-and-domain-model.md`, `plans/002-long-term-roadmap.md`, any ADR,
  the M007 decomposition rows other than M007-B, the M007-A plan, or any closure record.
  If the roadmap's M007-B row looks wrong, report it — the parent owns the roadmap.
- **Do not edit** the TUI. M007 mirrors the TUI; it does not modify it (roadmap
  `:541-545`). `src/tui/layout.rs` and `src/tui/panels/mod.rs` are **read-only
  references** for this milestone.
- **Do not edit** `apps/desktop/src-tauri/`. M007-B has no host-side change. If one appears
  necessary, that is a stop condition (§14).
- **The desktop-parity work's panel modules are references, not this milestone's work.**
  `apps/desktop/src/panels/` already holds eight modules (`TaskPanel`, `SubagentPanel`,
  `ActivityPanel`, `TranscriptPanel`, `TurnSummary`, `ToolDetail`, `GitPanel`,
  `SkillsPanel`) plus `format.ts`. M007-B **registers** and **places** them; the only one
  it edits is `TaskPanel.tsx` (work package A composes the two new sections into it), and
  the only files it adds under that directory are `RunHistory.tsx` and `RecentTurns.tsx`.
- **Environment.** The root Cargo workspace is Rust 1.89 (`Cargo.toml:12`, edition 2021).
  `apps/desktop/src-tauri` is a **separate Cargo workspace** with `rust-version = "1.90"`
  (`apps/desktop/src-tauri/Cargo.toml:6`, `[workspace]` at `:8-9`), because the selected
  Tauri minor requires it (roadmap §4 item 7, `:191`). Ordinary CLI/TUI builds and tests
  must not require Node, npm, WebKitGTK, Tauri, or desktop build tooling (roadmap §9,
  `:581`).
- **Serial tests.** Repository tests mutate process-global environment; use the `ci`
  nextest profile (one process per test) for broad sweeps. New `#[tokio::test]`s default
  to `current_thread`.
- **Storage tests.** Use `isolated_pool()` where a pool is needed; migrations run inside
  it. M007-B needs no storage test — the migration line in §10 is "none required" for a
  reason, and that reason must not change.
- **No `--all-features`.** It pulls in real-LSP-server tests. Broad Rust sweeps use
  `scripts/verify.sh full`, which pins `--features server,plugins,lsp-test-support`.
- **Preserve unrelated user changes.** If the working tree is not clean, do not revert
  anything you did not author.
- **The desktop has no npm workspace root.** `apps/desktop/package.json` is its own package
  (`name: "codegg-desktop"`, `private: true`); the repository has no root `package.json`.
  Run the npm commands from `apps/desktop/`.
