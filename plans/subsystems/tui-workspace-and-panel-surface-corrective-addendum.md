# TUI Workspace and Panel Surface Corrective Addendum

Status: active — **first implementation pass landed 2026-11-26** (see §14). D1,
D2, D3, D6, D9 and D12 are settled; M008, M012 and the per-milestone implementation
records and `tui-test` receipts are outstanding.

Repository baseline: `f2ae6d507` (`origin/feat/auto-20261109-f8abad0d-artifact-safe`,
verified identical to the worktree HEAD on 2026-11-26; 0 ahead, 0 behind).

Owner audit date: 2026-11-26. Owner decisions recorded the same day: D1, D2, D3,
D6 and D9 are settled (§10), and the worktree/branch session-launch scope was added
in the same pass (§5.9, M011–M012).

Parent and adjacent lines (none is reopened by this addendum):

- `plans/subsystems/tui-panel-reachability-corrective-addendum.md` — closed; owns
  whether the gesture that selects a panel loads it. This addendum owns what the
  panel shows once it is selected, and what the terminal does with the keyboard.
- `plans/subsystems/tui-live-surface-fill-corrective-addendum.md` — conditionally
  closed; M001 made the terminal ready for input on load. **The owner now reverses
  that acceptance**: the terminal panel must start in NOR, and `i` is the deliberate
  step into INS. The closed record is not edited; the reversal is recorded here.
- `plans/subsystems/project-picker-rework-roadmap.md` — closing; owns directory-tied
  project identity, registration, rename/repath and confirmed destroy. This addendum
  consumes those operations from the Workspace view instead of duplicating them.
- `plans/subsystems/project-work-orders-task-view-roadmap.md` and its
  `-ux-corrective-addendum.md` — closed; own the durable WorkOrder model and the
  Task view. This addendum refines the Task view's presentation only.
- `plans/subsystems/workspace-authority-and-session-navigation-corrective-addendum.md`
  — active; owns execution-context authority and session navigation. Its non-goal
  "split-panel working directories and the `Shift+H/J/K/L` chord" is **not**
  reopened: this addendum uses the existing chord for panel selection, not for
  split working directories.
- `plans/subsystems/tui-project-sessions-roadmap.md` and
  `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md`
  — closed; own project tabs, the catalog cache and the view-switch transaction.
- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md` — closed;
  owns the shared panel chrome and the placeholder contract.

Long-term references:

- `plans/000-long-term-specification.md#9-project-repository-workspace-and-worktree-model`
- `plans/000-long-term-specification.md#10-project-catalog-and-discovery`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants` (2, 3, 7, 8, 24)
- `plans/001-terminology-and-domain-model.md#5-project-and-source-control-terms`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Related ADRs:

- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md`
  — accepted; one root maps to one project; the client owns the working directory.
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — daemon-owned command catalog;
  the ownership guard is the enforcement. Any command removal is a catalog change,
  not a TUI-only edit.
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md` — the
  runtime preference row and its revision are the compare-and-set authority.
- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md` — WorkOrders
  are the durable task layer; the Task view is a projection.

No new ADR is required for the layout, keymap, session-lifecycle or command-surface
changes. Two decisions may require one, and each is a stop condition in §10: making
the preference compare-and-set per-dimension rather than row-wide, and adding a
project-scoped runtime-policy store.

## 1. Purpose and corrective trigger

The owner exercised the shipped TUI and reported a coherent set of defects that the
closed panel, diff, model, session and Workspace lines did not catch. They share one
theme: **a surface exists, but selecting it does not produce a usable surface**.

The audit found these concrete conditions at the baseline:

1. The Terminal side panel takes the PTY's keyboard on load, so it opens in INS. The
   owner wants it to open in NOR.
2. Walking the side panels with `Shift+J` / `Shift+K` lands on panels that still
   render as bordered boxes — an "Idle" placeholder, a "Loading…" placeholder, or a
   modal box opened over the column. The walk is therefore not trustworthy.
3. The Diff panel refuses a fileless selection ("Diff needs a file"), even though the
   daemon already answers a bare `/diff` with the whole working tree against `HEAD`.
   The aggregate exists on the wire and is not wired to the panel; there is no
   per-file navigation.
4. Selecting a model in `/models` while a thinking level is pending produces
   `preference_conflict` ("expected 180, current 183"). The model selection and the
   thinking selection are two preference writes, and the second one carries a stale
   revision.
5. Starting the TUI creates a durable "Untitled" session before the first prompt, and
   `/new` does the same. Repeated launches leave a long list of empty sessions. The
   session list is project-scoped, not workspace-scoped.
6. `/projects` and `/workspaces` duplicate surfaces the owner wants gone.
   `/workspaces` is a toast-only no-op. `/workspace` is a bordered dashboard that
   keeps the left sidebar, the bottom composer and its model/agent badges, and uses
   `Esc` to leave.
7. The project Task view has detail (`d`), cancel (`x`), resume and reorder already
   wired to daemon requests, but no live filter and no NOR/INS mode of its own, and
   it is not wired to the corresponding chat panel: opening a task does not put the
   conversation where the user is looking.
8. The Workspace view is a flat project list. It shows no worktree tree and offers no
   new-session flow that selects a worktree and a branch, although the daemon already
   carries `CoreRequest::SessionLaunch` with `NewManagedCheckout { base_ref,
   target_branch }` and `ExistingManagedWorktree` (§4.10).

This addendum is the smallest coherent corrective pass. It is not a redesign of the
panel host, the project catalog, the scheduler, the WorkOrder model, or the provider
stack. It corrects the surfaces those systems already authorize.

## 2. Work classification

### Invariants

- A panel selected by the walk never ends on a placeholder box as its final content.
  It fills, it states why it cannot act inline, or it is not in the walk.
- A panel-selection gesture never opens a modal over the column it is filling. A modal
  is a deliberate command's result; the walk is a reading gesture.
- Selecting the Terminal panel creates or attaches its shell and renders the live
  PTY immediately, exactly as an INS selection did, but leaves the keyboard in NOR.
  INS is entered only by an explicit `i`. Reading the panel and walking `Shift+J/K`
  therefore never types into a process.
- A durable session row is created only when there is user content to store (the first
  prompt) or when an explicit creation surface says so (template, import, fork, task,
  work order). A launch in a new or known directory creates a project, not a session.
- Every session carries the canonical `(project_id, workspace_id)` binding; the
  default session list is scoped to the active workspace, and cross-workspace rows
  are an explicit toggle, never the default.
- The owner requires that worktrees under one workspace share a chat. D12 records
  the two readings — a shared session list grouped by workspace lineage, or one
  conversation whose chat scope is the source workspace — and M012 is blocked until
  one is chosen. No reading is implemented implicitly.
- A new session that needs a checkout is launched through the existing
  `CoreRequest::SessionLaunch`; the frontend never mints a worktree path or branch
  name, never calls `git`, and reuses one idempotency key per user submit.
- Automatic worktree and branch names are deterministic daemon-side minting (a
  branch defaults to `codegg/worktree/<id>` when `target_branch` is omitted). No
  model call participates in naming.
- Project permissions are team membership. There is no project-scoped sandbox,
  approval or network policy store; membership defaults to write access because a
  coding harness exists to change code (D6).
- There is one command catalog (ADR-0052). Removing `/projects` or `/workspaces`
  removes the descriptor, the TUI action row, the dispatch arm, the tests and the help
  text together; the guard is the proof.
- The Workspace view is presentation and navigation only. It never becomes execution
  authority, never resolves a path, and never grants a project capability. Selecting
  a project still goes through the canonical project-tab/execution-context path.
- `Shift+X` exits the Workspace view only while that route is active **and** in NOR.
  In INS it is the capital letter `X` typed into the filter. Everywhere outside the
  route it remains the Normal-mode center-tab close action.
- One model selection is one preference write, or an ordered pair whose second write
  uses the revision the first one returned. A frontend never sends a cached revision
  it did not receive from the daemon after the preceding write.
- The Task view remains a projection of durable WorkOrders; no second scheduler,
  retry owner, or task model is introduced.

### Capabilities

- A user can walk every side panel and see its content, its loading state, or an
  inline reason without a modal interrupting the walk.
- A user can open the Terminal panel and read it in NOR, then press `i` to type.
- A user can select a model and a thinking level in one action and have both persist.
- A user can open the Diff panel with no argument and see the repository-wide change
  set, with per-file navigation; `/diff <path>` still shows one file.
- A user can launch CodeGG in a new repository and find it as a project; no empty
  session is created until the first prompt.
- A user can open `/workspace` and get a whole view: a project/workspace/worktree
  tree and a right-hand chat pane, no left panel, no bottom composer, no model/agent
  badge row.
- A user can filter the project list in INS (including Backspace), switch to NOR, move
  with `h/j/k/l`, open a detail with `Enter` or `l`, and return with `h`.
- A user can add, edit, archive and destroy projects, and manage project team
  membership (write by default), from the Workspace view.
- A user can see each workspace as a branch of top-level worktrees, and start a new
  session under an existing worktree or a new one.
- A user starting a new checkout can pick a base branch from the repository's
  branches, then accept the deterministic auto-named branch or type one.
- A user can reach a Workspace picker from `/panels` and switch to it with
  `Shift+J`/`Shift+K`, then switch projects without leaving the session.
- A user can filter and inspect the project Task view, open a task into the
  corresponding chat panel, and see truthful empty, loading, denied and error states.

### Infrastructure

- A panel-fill contract that distinguishes "adopted now", "loading in this column",
  "cannot act, reason visible", and "bespoke surface", with no modal in the walk path.
- A route-owned Workspace frame (whole view) with its own two-region split and its own
  NOR/INS mode, independent of the composer's mode.
- A bounded Workspace detail projection (metadata, workspaces, worktrees, sessions,
  tasks, team membership) using existing requests, with generation and reconnect
  guards. It carries no effective-policy section (D6).
- A session-draft state per tab, plus a workspace-scoped session-list filter.
- An additive aggregate-diff payload (file list plus per-file patch) produced by the
  existing daemon git read.
- A worktree projection built from `ManagedWorktreeList` and the existing managed
  worktree records, plus a new-session sheet driven by `GitRead(Branches)` and
  `SessionLaunch`.
- A single-cas model-and-reasoning preference write, or a sequenced write with the
  returned revision.

### Polish

- One vocabulary for panel entry: `Shift+H/J/K/L` and their arrow twins mean the same
  thing in every surface, and the panel footer names the gesture.
- One vocabulary for modes: `NOR`/`INS` in the Workspace view and the Terminal panel,
  matching the composer and the editor.
- One vocabulary for empty sessions: "no session yet" is a draft state, not a row.
- Help, `docs/tui.md` and `architecture/tui.md` agree with the surfaces.

## 3. Non-goals

- **No new panel framework.** `SidePanelId`, `SidePanelHost` and the hosted/bespoke
  split stay. A new panel is a variant plus an implementation.
- **No project-scoped runtime-policy store.** "Manage permissions" in this plan means
  project team membership (the durable `team.v1` authority) only. There is no
  per-project sandbox, approval or network policy: membership defaults to write
  access, and a member is made read-only by role, not by a project policy row.
  Per-project runtime policy remains a separate ADR-scale change.
- **No second worktree-creation path.** `SessionLaunch` with `NewManagedCheckout` or
  `ExistingManagedWorktree` is the only creation route. The TUI does not shell out to
  `git worktree add`, and it does not add a second launch request.
- **No model-assisted naming.** Worktree paths and auto branch names are the daemon's
  deterministic minting; no LLM namer is added.
- **No second task input surface.** The Task view keeps the ordinary composer and
  targets the corresponding chat panel.
- **No WorkOrder, scheduler, or task-model change.** The Task view is refined as a
  projection. No new action is added unless a daemon request already exists.
- **No provider/model-catalog change.** The reasoning capability record is consumed as
  it is; no model is added or removed.
- **No desktop or web frontend work.** The desktop panel layer is out of scope; its
  own guards remain unchanged.
- **No deletion of historical sessions without an explicit confirmation.** Existing
  empty rows are hidden by default and may be cleaned by a deliberate command with a
  count; silent deletion is prohibited.
- **No reopening of closed milestones.** Terminal NOR is a recorded reversal, not a
  retroactive edit to the closed TUI live-surface record.
- **No editor redesign.** The Workspace view may borrow the editor's whole-view
  discipline, not its buffer machinery.
- **No new keybinding that makes a capital letter un-typeable in Insert mode.** All new
  navigation is route-scoped or Normal-only.

## 4. Current state and audit evidence

### 4.1 A live Terminal re-enters INS

- `crates/codegg-tui/src/commands/panels.rs`: `select_panel` calls
  `ready_terminal_panel` for a visible Terminal panel; the selection is a load.
- `crates/codegg-tui/src/commands/interactive_terminal.rs`:
  `apply_terminal_attach_finished` calls `interactive_terminals.focus(&handle)` when
  `panel_host.active() == Terminal` and `focus_manager.is_empty()`, explicitly "so a
  shell the user asked for starts ready for its first key rather than waiting for a
  second `i`".
- `crates/codegg-tui/src/commands/panels.rs`: `step_panel` calls `select_panel` and
  then `focus_panel` for Chat/Terminal (`panels.rs:883-885`). `focus_panel` focuses the
  panel and parks the composer at Normal; it does **not** take the PTY. The INS comes
  from the attach completion below, gated on the panel being active and no dialog
  holding focus. Tests pin the behavior
  (`the_box_opens_in_insert_while_the_docked_panel_still_needs_i`).
- The live baseline narrows the condition: a first selection with **no** live shell
  renders "press i for a shell" in NOR, and `i` creates the shell and enters INS.
  Selecting the panel **again** while the shell is live — the owner's report — calls
  `focus` on attach and re-enters INS.
- The owner's requirement is not "do not start the shell". It is: selecting Terminal
  must produce the same live surface an INS selection produced — shell created,
  output rendered — while the keyboard stays NOR, so `Shift+J/K` can leave the panel.
  `ready_terminal_panel`'s eager create/attach stays; only the PTY `focus` call
  changes. The panel itself stays focused, so `i` and the panel's own navigation
  reach it. `i` is the deliberate step into the process.

### 4.2 The panel walk lands on boxes

- `crates/codegg-tui/src/panels/hosted.rs` defines the placeholder body as two
  lines — `Nothing loaded.` / `Open it from the panel manager (⇧S).` — plus
  `Loading {label}…` and `Cannot(reason)`. It is a bordered column body by
  construction.
- `crates/codegg-tui/src/commands/panels.rs`: `materialize_hosted_panel` maps
  `PanelFill::Filled` to "open the dialog, close it, adopt the widget"; `Loading` and
  `Cannot` leave the placeholder visible; `StillLoading` returns `false` so the
  caller does not select the panel and the modal box owns the keyboard.
- The asynchronous fillers are the affected set: `Research`, `Doctor`, `Plugins`,
  `Team`, `Control`, `TaskView`, `Goal`, `Memory`, `ShellShow`, `Security`. (`Diff`
  is not asynchronous today: its arm refuses synchronously — §4.3.) The synchronous
  boxes (`Tree`, `Sessions`, `Logs`, `Context`,
  `Cost`, `Usage`, `Stats`, `Models`, `Mcp`, `Keybinds`, `Themes`, `Review`,
  `Files`) already adopt, but the walk still opens a modal for one frame and can
  leave the column as a box if the modal is closed by another path.
- `Shift+H/J/K/L` is already bound (`crates/codegg-tui/src/input.rs`,
  `insert_panel_direction_bindings`; `is_normal_only` for the direction actions) and
  routed through `move_directionally` / `step_panel`. The chord is not missing; the
  destination is. `Right`/`Shift+L` enters the side column, `Left`/`Shift+H`
  returns to the buffer, `Up`/`Shift+K` and `Down`/`Shift+J` cycle the panel type.
- `Shift+X` is `InputAction::CloseCenterTab` in every Normal-mode surface. The
  Workspace view has no route-scoped exit; `Esc` is the only exit today.

### 4.3 Diff panel refuses the fileless case

- `crates/codegg-tui/src/commands/panels.rs`: the `P::Diff` arm returns
  `Cannot("no file selected — /diff <path>")` and a toast. It does not call
  `start_diff`.
- `crates/codegg-tui/src/commands/diff.rs`: `start_diff` already treats an empty
  path as the bare branch. `apply_diff_finished` adopts the result into a waiting
  panel; a clean tree becomes an explicit empty state.
- `src/core/daemon_git.rs`: the bare branch runs one `git diff` and returns
  `old_content: ""`, `new_content: <whole patch>`, `title: "Repository Diff"`,
  `path: None`. The per-file branch reads `HEAD:<path>` and the working-tree file.
- `crates/codegg-tui/src/components/dialogs/diff.rs` is a single `DiffViewer` over
  two strings. It has no file index and cannot navigate between files.
- The aggregate is therefore **already produced** and only needs to be rendered. A
  per-file list requires an additive payload or a bounded parse of the unified patch.

### 4.4 Model selection conflicts with its own thinking write

- `crates/codegg-tui/src/app/input.rs`, `TuiMsg::SelectModel`: the handler calls
  `apply_selected_reasoning_level(reasoning)`, then `persist_model_selection(&model)`
  (the local `user_preferences` write), then `persist_durable_model_selection(model)`
  (the daemon `ModelSelect`). The first and third writes touch the same durable row.
- `apply_selected_reasoning_level` -> `start_level_update` ->
  `start_preference_update` captures `expected_revision =
  ui_state.reasoning_preference_revision` and spawns the write.
- `persist_durable_model_selection` -> `CoreRequest::ModelSelect` ->
  `src/core/daemon_turns.rs` -> `RuntimePreferenceStore::set_model_preference(...,
  None)`, which **always** increments `runtime_preferences.revision`
  (`crates/codegg-core/src/approval.rs`, `set_model_preference`).
- The reasoning write then compares the old expected revision with the new current
  revision in `src/core/daemon_ops.rs` and returns `preference_conflict`; the TUI
  renders it through `refusal_message` as "the daemon's selection changed underneath
  this request".
- The revision is **never refreshed on the success path**: `apply_model_select_persisted`
  (`app/mod.rs:10513`) refreshes only when the model write fails. The expected value
  therefore stays at the last reasoning read (180 in the report) while each model
  selection increments the row (181, 182, 183). The screenshot's "expected 180,
  current 183" is consistent with repeated attempts, not a unique race.
- The store already has the shape M004 needs: `set_policy` writes approval, sandbox
  and network under **one** revision bump (`crates/codegg-core/src/approval.rs:923`;
  test `set_policy_writes_both_dimensions_in_one_revision`). `set_model_preference`
  preserves the stored reasoning selection but still bumps the row revision, which is
  exactly why the stale reasoning write fails.
- The same row-wide revision is touched by `SessionSelectionUpdate` in
  `src/core/daemon_sessions.rs` and by `TaskModelPreferenceSet`; any frontend that
  writes one dimension invalidates a cached revision held for another.

### 4.5 Empty sessions and project-scoped listing

- There are **three** eager creators. `crates/codegg-cli/src/main.rs` builds
  `InitialSessionRequest::New` in the daemon branch (`main.rs:4704`) and issues
  `CoreRequest::SessionCreate` through `App::load_initial_session_via_core`. The
  in-process branch (`if let Some(ss) = &session_store`) handles only fork/attach/
  continue, so a `--standalone` launch reaches `app.ensure_launch_session()`
  (`main.rs:4723-4725`), which calls `start_new_session`
  (`app/project_session.rs:150-155`). `start_new_session` is also `Ctrl+N` and `/new`
  (`commands/sessions.rs:2164`), and it is eager **by design**: its doc says eager
  creation is what makes `/models`, `/sessions` and the tree work from the first
  keystroke (`sessions.rs:2157-2163`).
- The audited baseline ran `--standalone`, so the `Untitled` row it read back came
  from `ensure_launch_session`, not from `InitialSessionRequest::New`. M005 must
  change both seams, or the in-process launch keeps creating the row.
- `crates/codegg-core/src/session/store.rs` stores a default title `Untitled` and
  writes the row before any message exists. The schema already has `project_id NOT
  NULL`, nullable `workspace_id`, and canonical `session_project_binding`; the
  problem is not the schema, it is eager creation.
- `crates/codegg-tui/src/app/prompt_turn.rs` and `crates/codegg-tui/src/commands/prompt.rs`
  already implement the lazy path: if no session is bound, the first submit creates
  one with the captured `(project_id, workspace_id)` and submits the captured prompt
  exactly once. Startup bypasses that path.
- The draft state is not unprecedented: with no session bound,
  `persist_durable_model_selection` records the choice on the tab and returns
  (`app/mod.rs:10500-10505`), and the first session bind applies it
  (`app/mod.rs:9698-9701`). M005 must give `/models`, `/sessions` and the tree the same
  draft-state answer rather than letting them go dark — the regression
  `start_new_session`'s comment documents.
- `CoreRequest::SessionList` is project-scoped. `/sessions` sends `project_id` and
  optionally fetches message counts; it does not filter by `workspace_id`, so a
  project with several checkouts mixes their sessions. The `Session` DTO carries
  `workspace_id` and `binding`, so a client-side filter is possible; a daemon filter
  is preferable for the 100-row bound.

### 4.6 Command duplication and the Workspace view

- `crates/codegg-tui/src/command.rs` registers `/projects` (alias `/project`) and
  `/workspaces`; `crates/codegg-core/src/command_catalog.rs` carries the matching
  descriptors. The ownership guard
  (`scripts/check_command_catalog_ownership.py`) requires the two sets to agree.
- `/projects` opens the project picker. `/workspaces` dispatches
  `dispatch_workspaces`, whose whole body is a toast: "Workspace management - use
  /sessions to switch workspaces". It is a no-op by construction.
- `/workspace` is a **frontend-effect** catalog row. It opens `Route::Workspace` and
  a `WorkspaceDashboardState`; `render_workspace_view` paints a bordered block in the
  viewport while the ordinary frame still paints the left sidebar, the composer, the
  composer hint row and the model/agent badges.
- The Workspace key router (`handle_workspace_view_key`) supports `Up`/`Down`,
  `j`/`k`, `g`/`G`, `Home`/`End`, `PgUp`/`PgDn`, `Space` (expand),
  `Enter` (with an empty prompt: descend to Task view), `Ctrl+R` (refresh), and
  `Esc` (leave). It does not implement `h`/`l`, a detail state, an INS filter, or a
  `Shift+X` exit. The filter is accepted only in Normal mode in `App::on_key`, and
  Backspace is handled by the legacy modal arm, not the non-modal view.
- `SidePanelId::ALL` has no Workspace/Projects variant. The panel manager cannot list
  it; `Shift+J/K` cannot reach it.

### 4.7 Project registration, task view and permissions

- `App::bootstrap_launch_project_context` (`crates/codegg-tui/src/app/mod.rs`) calls
  `ensure_bound_project_context` (`commands/project_picker.rs`), which registers the
  launch directory through `WorkspaceRegister` + `ProjectRegister` and is idempotent.
  `crates/codegg-cli/src/main.rs` calls it at startup when the app has a core client
  and no active project. The capability the owner asks for **already exists**; what is
  missing is an end-to-end receipt that a fresh repository becomes exactly one
  project, and a check that no other startup path bypasses the bootstrap.
- Project add/edit/destroy/archive already exist in the picker:
  `ProjectRegister`, `ProjectUpdate`, `ProjectArchive`, `ProjectRestore`,
  `ProjectDelete` with a confirmed session count. `ProjectEdit` and registration
  phases live in `app/state/project_picker.rs`.
- Project team membership already exists through `team.v1` and `commands/team.rs`
  (list/add/role/revoke, owner-gated). Effective runtime policy exists globally
  through `/policy`, `/approval`, `/sandbox`, `/network` and the `runtime_preferences`
  CAS, but it is not a project permission in this plan (D6). The gap is presentation:
  team membership is not reachable from the Workspace view or a project detail.
- `crates/codegg-tui/src/components/dialogs/task_view.rs` is **not** a read-only
  viewer. It handles `j/k`, `g/G`, `Home/End`, `PgUp/PgDn`, `Enter` (open), `d`
  (occurrence detail), `x` (cancel), `K` (reorder) and `r` (refresh)
  (`task_view.rs:216-233`), all wired through `TuiMsg` to daemon requests
  (`app/input.rs:930-949`, `commands/work_orders.rs:2971-2999`); `TaskViewResume`
  exists too. What it lacks is a live filter, a NOR/INS mode of its own
  (`docks_as_viewer()` is true), `h/l` navigation, a persistent detail pane, a
  confirmation before `x`, and — the owner's point — a wiring to the corresponding
  chat panel. `commands/work_orders.rs` already owns the list/detail requests a
  refinement consumes.

### 4.8 Why the closed verification did not catch these

- Panel tests asserted the host's placeholder copy and the manager's selection path;
  they did not walk every panel and assert that the destination paints content rather
  than a placeholder. The panel-reachability record even lists "walking past Terminal
  spawns a shell" as an accepted cost, not as a defect.
- The Terminal INS-on-load test **pinned the behavior the owner now rejects**; it was
  written as a capability, so no test could fail when the capability was wrong.
- Diff tests asserted the refusal as the intended empty-path behavior ("A diff is a
  view of one file"), not the daemon's aggregate branch.
- The models test asserted that the dialog emitted `SelectModel` with a reasoning
  level; it did not drive the two daemon writes or their revisions. The daemon's CAS
  tests covered each setter in isolation, not a model selection that also changes the
  thinking dimension.
- Session tests created sessions directly and asserted the startup "New" path; none
  asserted that a launch with no prompt leaves **no** row.
- Workspace tests asserted the route, the non-modal focus and generation guards; they
  did not assert the frame's regions, the composer's absence, or the complete keymap.
- Command tests asserted that `/projects` opens the picker; none asserted that
  `/workspaces` does anything, because it does not.

### 4.9 What this audit verified live, and what it did not

A live baseline was captured on the audited SHA. `cargo build -p codegg-cli --bin
codegg` produced `target/debug/codegg`; `tui-test` ran it with `--standalone`, an
isolated `HOME` + XDG tree, and a temporary git repository, so no user catalog was
touched.

Reproduced live:

- the default launch created exactly one `Untitled` session with **zero messages**,
  bound to the temporary project/workspace (read back from the isolated
  `sessions.db`);
- the Terminal panel with no session rendered an **ERROR box**: "No active session;
  open a project session before creating a terminal";
- selecting Terminal with a live shell and cycling away/back re-entered **INS**
  ("TERM · INS typing to the shell"), while `i` was the action that created the
  first shell;
- the walk to Help rendered a nested bordered Help box inside the side column;
  whether that is intended chrome or the reported box is part of M002's audit.

Not reproduced live:

- the `/models` conflict: the isolated profile had no provider connection or model
  catalog. The owner's screenshot and the two-write source trace are the evidence;
- the `/workspace` layout and keymap: the view was not opened in this baseline;
  source evidence and the owner report stand;
- the aggregate diff: not exercised live; source evidence shows the daemon's bare
  branch and the panel's refusal.

These are baseline receipts, not closure evidence. Every milestone still needs a
`tui-test` receipt on its implementation head; a milestone that cannot produce one is
not closed.

### 4.10 Worktree and branch session launch already exists on the wire

- `CoreRequest::SessionLaunch` (`crates/codegg-protocol/src/core.rs:2985-3002`)
  carries `project_id`, `intent` (`New`/`Fork`), an optional parent session, a
  `SessionLaunchPlacementDto`, and an idempotency key. The placement is either
  `NewManagedCheckout { base_ref, target_branch }` or
  `ExistingManagedWorktree { worktree_id }` (`core.rs:2611-2630`).
- `CoreResponse::SessionLaunch` (`core.rs:1435-1450`) returns the concrete
  `launch_id`, `session_id`, `workspace_id`, `worktree_id`, `branch`, `checkout_path`,
  `stage` and `converged` — the caller does not have to list sessions to find what it
  created.
- The daemon handles it (`src/core/daemon_sessions.rs:682`), constructs a durable
  `SessionLaunchService` whenever a pool exists (`src/core/daemon_construct.rs:448`),
  and gates a `NewManagedCheckout` through an authorization decision
  (`src/core/daemon.rs:1018-1030`). The launch record is durable and staged
  (`accepted` → `checkout_reserved` → `checkout_ready` → `session_created` → `bound`
  → `completed`, or `needs_attention`; `session_launch` table, `schema.rs:3810`).
- Checkout creation reuses `WorktreeService::reserve` with the caller's `base_ref` and
  `target_branch` (`src/core/session_launch.rs:548-577`). `CreateWorktreeRequest`
  documents both: `base_ref` is a ref, not a revision expression, and when
  `target_branch` is omitted the service mints `codegg/worktree/<id>`
  (`crates/codegg-core/src/worktree_service.rs:164-176`). That is the deterministic,
  model-free naming the owner asked for.
- The tree inputs also exist: `ManagedWorktreeList { workspace_id, repository_id,
  run_id, include_removed }` (`core.rs:3258`) returns `ManagedWorktreeDto` with
  `path`, `branch`, `state`, `health` and `lease_generation`; `GitRead` with
  `GitReadOperationDto::Branches` returns the local branch list
  (`crates/codegg-protocol/src/git.rs`).
- **The TUI never uses any of it.** `crates/codegg-tui` has no `SessionLaunch` call
  site; the Workspace view lists projects only. The capability is therefore present
  and unexposed, exactly like project auto-registration in §4.7. M011 is a
  presentation milestone, not a protocol milestone.
- **The one real conflict is the chat scope.** `src/core/session_launch.rs:1-9` states
  the rule it implements: "the checkout's own path, registered and bound to the
  project. A session in a checkout runs in the *checkout's* workspace, not in the
  project's." That makes every worktree a separate chat scope, which contradicts the
  owner's "all worktrees under the same workspace share a chat". D12 records the two
  readings and the stop condition; M012 is blocked on it.

## 5. Target behavior

### 5.1 Panel walk

Selecting any panel with `Shift+J`/`Shift+K` (or the arrow twins) produces one of:

- the panel's content, already adopted into the column;
- an inline progress state that says what is loading and ends in the same column;
- an inline reason it cannot act, in one sentence, with no instruction to leave the
  column and no modal.

`Shift+L`/`Shift+→` focuses the side column; `Shift+H`/`Shift+←` returns to the
main buffer. `Shift+X` remains close-center-tab outside the Workspace view.

### 5.2 Terminal

Selecting the Terminal panel creates or attaches its shell and paints the live PTY
immediately — the same surface an INS selection produced — but the keyboard stays in
NOR. `i` focuses the PTY and enters INS; `Esc` returns to NOR without losing the
shell. `Shift+J/K` from Terminal therefore moves to the next panel instead of typing
into the process. The panel's footer says `i type` in NOR and `INS typing to the
shell` in INS. The shell is still created by the same `ready_terminal_panel` seam and
the same daemon-authorized PTY route.

### 5.3 Diff

Selecting the Diff panel issues the bare `/diff` read and renders:

- a file index of the working-tree change set;
- the selected file's patch (or a unified patch view for a file the daemon cannot
  split);
- explicit empty ("working tree matches HEAD") and failed states;
- `/diff <path>` unchanged: one file, one patch.

The panel is bounded: at most 200 files, 1 MiB of patch text, 256 KiB per file, with a
truncation notice. The daemon remains the only reader; the TUI never spawns `git`.

### 5.4 Models

Selecting a model with a thinking level is one durable action:

- the daemon writes the model and the reasoning selection under one compare-and-set;
- the response carries the resulting `RuntimePreference` (including its revision);
- the TUI updates its cached model, reasoning mode and revision from that response;
- no `preference_conflict` is produced by the TUI's own two writes.

If the product later needs per-dimension CAS, that is a separate ADR; this plan does
not change the row-wide revision semantics.

### 5.5 Sessions

- Launching CodeGG in a new repository registers the project and leaves the tab in a
  **draft** state: no durable session row, no "Untitled" title, no empty transcript
  entry in `/sessions`.
- The first prompt creates the session with the captured `(project_id, workspace_id)`
  and submits the prompt once, through the existing lazy path.
- `/new` and `Ctrl+N` return to a draft; neither creates a row.
- In the draft state `/models`, `/sessions` and the tree must answer truthfully rather
  than going dark: a model choice is recorded on the tab and applied when the first
  session binds (the existing handoff), `/sessions` shows "no session yet", and the
  tree shows the workspace's project-scoped files. This is the regression that eager
  creation was introduced to prevent, and the draft state has to replace it.
- Explicit creation surfaces (template, import, fork, task/work-order materialization)
  still create rows because they carry content or an explicit intent.
- `/sessions` defaults to the active workspace. A project-wide toggle shows the other
  workspaces. Empty rows are hidden by default; a bounded, confirmed cleanup command
  can remove historical empties, naming the count first.
- Each row shows its workspace (name or canonical root) and message count so the list
  cannot silently mix checkouts.
- Under the reading D12 chooses, worktrees created from a workspace share its chat:
  either their sessions appear in the workspace's chat list (grouping), or the
  session's chat scope is the source workspace while the turn runs in the checkout
  (single conversation). Switching worktree must not fork the conversation.

### 5.6 Workspace view

`/workspace` is a **whole view**:

- one project window (the left/flex region) and one chat pane (the right region);
- no left sidebar, no bottom panel, no composer, no model/agent badge row, no
  composer hint row, no todo/tasks strips;
- a one-row title carrying the view name, `NOR`/`INS`, and the `Shift+X` exit hint;
- the chat pane renders the selected workspace's transcript as a **viewer**; the
  ordinary composer is not painted in this route and is restored intact on exit
  (D11). `Shift+L` focuses the pane, `Shift+H` returns to the tree;
- `Shift+X` in NOR exits to the route the user came from (the previous tab/session);
  in INS it types `X` into the filter. `Esc` no longer exits: it moves INS → NOR and,
  in NOR, clears the filter before anything else.

Modes:

- `NOR`: `j`/`k` move between rows, `l`/`Enter` expands a project or workspace (or
  opens a worktree's detail), `h` collapses or returns to the parent, `g`/`G`
  top/bottom, `n` opens the new-session sheet for the selected workspace or worktree,
  `i` enters INS, `Shift+X` exits, `Shift+L` focuses the chat pane.
- `INS`: printable text filters the tree, Backspace removes the last character
  (Unicode-safe), `Esc` returns to NOR, `Shift+X` types `X`. The filter is the view's
  own; it does not touch the composer's mode and is cleared when the view is left.
- The tree is hierarchical: project → workspace → managed worktree. A project's
  workspaces and a workspace's worktrees are its children; the chat pane follows the
  selected workspace.
- The detail replaces the tree in the same window. It shows project identity,
  workspaces, worktrees (path, branch, state, lease), sessions, tasks and team
  membership, with the same `h` back motion. No detail is a modal. There is no
  effective-policy section: permissions are membership only (D6).

Project actions (reusing existing daemon operations):

- `a` add/register a local project (reuse `ensure_bound_project_context` and the
  registration draft);
- `e` edit display name and root (`ProjectUpdate`, expected `updated_at`);
- `x` archive / restore, lowercase with a confirmation (D1 reserves `Shift+X` for the
  exit);
- `d` destroy, with the existing count-naming confirmation (`ProjectDelete`);
- `p` permissions: team membership list/add/role/revoke. Membership defaults to write
  access; a read-only member is a role, not a project policy row (D6).
- `n` new session for the selected workspace/worktree (§5.9);
- `Enter` on a project opens or focuses its tab and returns to the session route.

### 5.7 Workspace picker panel

A new bespoke `SidePanelId::Workspace` (config key `workspace`, label `Workspace`,
title `WORKSPACE`) renders the project catalog with the same NOR/INS filter contract
and a compact activity badge. `Enter` opens/focuses the selected project's tab and
returns the keyboard to the session; `Shift+J/K` cycles to it like any other panel;
`/panels` lists it. It never opens a modal and never requires `Route::Workspace`.

### 5.8 Task view

The Task view becomes a navigable projection:

- sections for running, waiting, future and recent WorkOrders;
- NOR/INS filter over title and state;
- `h/j/k/l` navigation and a persistent detail pane for the selected row;
- the existing actions stay: open into the corresponding chat panel, cancel with a
  confirmation, resume a paused order, reorder a lane;
- explicit loading, empty, denied, truncated and failed states.

The existing input panel is kept as is: the view does not mount a second composer, and
opening a task targets the chat panel that already owns the conversation (D9). No new
WorkOrder operation is invented. If an action has no request, the view says so and the
milestone stops rather than synthesizing one.

### 5.9 Worktrees and the new-session flow

The Workspace view shows each project's workspaces as a tree, and each workspace's
managed worktrees as its children. It is built from `ManagedWorktreeList` and the
existing catalog; it does not add a second catalog or an N+1 request.

`n` on a workspace or worktree opens a new-session sheet:

- **Existing worktree** — the sheet lists the workspace's managed worktrees
  (`ManagedWorktreeList`, `state == ready` only) and launches with
  `SessionLaunch { placement: ExistingManagedWorktree { worktree_id } }`.
- **New checkout** — the sheet offers the repository's local branches
  (`GitRead` with `GitReadOperationDto::Branches`) as `base_ref`, and a branch choice:
  the deterministic auto-named branch (`target_branch: None` → the daemon mints
  `codegg/worktree/<id>`) or a user-typed name (`target_branch: Some(name)`).
- The sheet never mints a path or a name itself and never calls `git`. A name
  collision is the daemon's typed refusal, not a client-side guess.
- One idempotency key per submit. A retry after a dropped connection reuses it; a
  refused validation error starts a new key. A `converged` response opens the
  original session rather than creating a second one.
- The launch response opens the new session automatically and switches the Workspace
  view to it. `stage` is rendered while the launch is in flight; `needs_attention` is
  an explicit error, not a spinner that never ends.
- Worktrees under one workspace share that workspace's chat, under the reading D12
  chooses.

## 6. Dependency graph

```text
M001 project-registration receipt (soft edge to M005 for its zero-row exit)
M002 panel fill + terminal NOR (independent)
M003 aggregate diff (independent; protocol additive)
M004 atomic model+reasoning write (independent; protocol additive)
M005 lazy sessions + workspace-scoped list (independent of M002-M004; touches startup)
M006 command-surface unification (independent; catalog change)
        |
        v
M007 Workspace whole-view shell, modes, navigation, exit
        |                       \
        v                       v
M008 Workspace project actions  M009 Workspace picker panel
        and permissions               |
        \_____________________________/
                      |
        +-------------+-------------+
        |                           |
        v                           v
M011 worktree tree + launch      M010 Task-view refinement
        |
        v
M012 shared chat scope across worktrees (blocked on D12)
```

Hard edges only: M011 needs M007's frame, tree state and key router; M010 needs M007
only for the shared NOR/INS/detail vocabulary; M012 needs M011 and D12. M008 and M009
need M007. M001-M006 are deliberately disjoint so they can land in parallel or in any
order, except that M001's "zero session rows" exit is a soft edge to M005: its receipt
can be captured now, but the exit cannot pass until M005 lands.

## 7. Milestones

### M001 — A new repository becomes exactly one project, with no session

Class: **capability** (verification and, if needed, a narrow fix).

Objective: prove that launching in a directory the catalog has never seen registers one
project and one workspace, binds the launch tab, and creates no durable session until
the first prompt. Cover local, socket and restored-tab paths.

Scope:

- Drive `App::bootstrap_launch_project_context` through `crates/codegg-cli/src/main.rs`
  and assert the catalog has exactly one new project for the root; a second launch
  resolves the same IDs.
- Assert that **neither** launch seam issues `SessionCreate`:
  `InitialSessionRequest::New` on the daemon branch and `ensure_launch_session` on the
  in-process branch (M005 owns the change). M001 records the current failure as a
  baseline and re-runs after M005; this exit is a soft edge to M005.
- If a startup path bypasses the bootstrap (remote/attach/restore), fix that path to
  call the same idempotent seam; do not add a second registration implementation.

Deliverable: an integration test family using a real daemon and a temp directory, plus
a `tui-test` receipt of a first launch in a fresh repo.

Exit conditions: one project row; one workspace bound; zero session rows before the
first prompt; a second launch reuses the same IDs; the test fails on a tree where the
bootstrap is removed.

### M002 — The panel walk fills the column and the terminal opens NOR

Class: **capability / invariant**.

Objective: every `SidePanelId` reachable by `Shift+J/K` shows content, an inline
loading state, or an inline reason; the Terminal panel creates and renders its shell
immediately but opens NOR, so the next `Shift+J/K` moves panels instead of typing into
the process.

Scope:

- Replace the walk's modal path with an inline-load contract. `open_panel_box`
  returns a typed outcome that names the panel it fills; `materialize_hosted_panel`
  selects the panel for `Loading` and `Cannot` (already true) and **never** returns
  `false` for `StillLoading`; the completion handler adopts into the waiting panel.
- Generalize the Diff panel's "panel waiting" adoption pattern to Doctor, Research,
  Plugins, Team, Control, TaskView, Goal and the other asynchronous fillers.
- Resolve the synchronous boxes by adopting the component directly (the existing
  `Filled` path) and by closing the modal before focus returns to the column.
- Remove the "Nothing loaded. Open it from the panel manager" body from any panel the
  walk can select; use a one-line state that describes the panel, not a box with a
  call to action.
- Terminal: keep `ready_terminal_panel`'s eager create/attach and the live render;
  do not call `interactive_terminals.focus` on attach. The panel stays selected and
  focused, but the PTY does not take the keyboard; `i` focuses it. Update
  `step_panel`'s comment, the footer, and the tests that pinned INS-on-load.

Deliverable: a table-driven test over `SidePanelId::ALL` asserting, for every panel,
that selection produces either `has_content() == true`, a non-box inline loading
state, or an inline reason; and a Terminal test asserting the shell is attached and
rendering while `composer_input_mode() == Normal`, `Insert` only after `i`, and
`Shift+J` after selection moves the panel rather than reaching the PTY.

Exit conditions: no panel in the walk opens a modal; no panel in the walk renders the
idle placeholder as final content; Terminal NOR receipt on a real PTY; the existing
panel-reachability tests still pass.

### M003 — The Diff panel shows the repository aggregate

Class: **capability**.

Objective: selecting Diff with no argument shows the working-tree change set and lets
the user move between files; `/diff <path>` remains single-file.

Scope:

- Daemon: extend `GitDiffResultDto` additively with `files: Vec<GitDiffFileDto>`
  (`path`, `old_content`, `new_content`, optional `binary`/`truncated`). The bare
  branch parses the one `git diff` output into file sections; no N+1 `git diff`.
  Bound the file count, total bytes and per-file bytes; report truncation.
- TUI: `P::Diff` starts `start_diff("")` instead of returning `Cannot`. The waiting
  panel adopts a `DiffBrowser` (file index + selected patch) or a unified patch view
  when the daemon cannot split; `j/k` file, `Enter`/`l` patch focus, `s` side-by-side.
- Preserve the clean-tree empty state and the typed refusal rendering.

Deliverable: protocol round-trip tests, daemon split tests (multi-file, binary,
rename, truncated), TUI browser tests, and a `tui-test` receipt opening Diff from the
walk and from `/diff`.

Exit conditions: no file argument is needed; at least one real multi-file repo renders
two files; `/diff src/lib.rs` still renders one; old peers that ignore `files` still
decode the response.

### M004 — One model selection is one preference write

Class: **invariant / capability**.

Objective: selecting a model and a thinking level in `/models` persists both without a
self-inflicted `preference_conflict`.

Scope:

- Extend the existing model-selection request additively with the reasoning selection
  and expected revision (the decided shape, D3). Do not add a second preference
  request and do not keep a parallel reasoning write in the TUI.
- Daemon: one `RuntimePreferenceStore` method that writes model identity and
  reasoning selection under one revision bump, following `set_policy`'s existing
  multi-dimension shape (`approval.rs:923`); validate both before writing; return the
  resulting `RuntimePreference` (revision included) in the response. The write must
  reach both call sites that currently pass `None` (`daemon_turns.rs:1799`,
  `daemon_sessions.rs:200`).
- TUI: apply the returned preference; remove the separate reasoning write from the
  `SelectModel` path; refresh `reasoning_preference_revision` on every model-write
  completion, including `SessionSelectionUpdate` and `TaskModelPreferenceSet`.
- Regression: seed revision 180, select a model with `high`, assert success, final
  revision 181 (one bump), model and reasoning both set, and no error toast.
- `set_policy` already proves the store can write multiple dimensions in one
  revision, so a storage-level stop is not expected. Stop only if the protocol shape
  cannot carry the reasoning selection additively; per-dimension CAS would be an ADR
  (see §10).

Deliverable: store tests, daemon handler tests, TUI handler tests, and a `tui-test`
receipt selecting a model with a level.

Exit conditions: the baseline reproduction fails with 180/current 181+; the corrected
tree passes with a single bump; a concurrent writer still gets a typed conflict.

### M005 — Sessions are born on the first prompt and are workspace-scoped

Class: **capability / invariant**.

Objective: no empty durable session on launch, `Ctrl+N` or `/new`; the default list
is the active workspace.

Scope:

- Add a per-tab draft state (no session id) and route **all three** eager creators
  through it: `InitialSessionRequest::New` (daemon branch), `ensure_launch_session`
  (in-process branch), and `start_new_session` (`Ctrl+N`/`/new`). The first submit
  uses the existing `prompt.rs` lazy create path with the captured project/workspace.
- Give the session-scoped surfaces a draft answer: `/models` records on the tab and
  applies on bind, `/sessions` says "no session yet", the tree shows the workspace's
  files. Removing eager creation must not turn them dark.
- Keep explicit creation for templates, imports, forks, tasks and work orders.
- Add an optional `workspace_id` filter to `SessionList` (additive, defaulted) or
  filter client-side within the existing 100-row bound; the default is the active
  workspace, with a project-wide toggle.
- Hide `message_count == 0` rows by default; add a confirmed cleanup command for
  historical empties that names the count and never deletes the active draft.
- Show workspace and message count in `/sessions` rows.

Deliverable: startup tests (zero rows before prompt, one row after), `/new` tests,
list-scoping tests across two workspaces, and a `tui-test` receipt of a fresh launch
and a second launch in another repo.

Exit conditions: no "Untitled" row from an untouched launch; the first prompt creates
exactly one row in the correct project/workspace; the list does not mix checkouts by
default; cleanup is explicit and counted.

### M006 — One command surface: remove `/projects` and `/workspaces`

Class: **capability / invariant**.

Objective: the surviving entry point is `/workspace` (plus the existing picker
hotkeys `Shift+F` / `Ctrl+\`); the duplicate commands are gone from the daemon
catalog and the TUI.

Scope:

- Remove the `projects` descriptor (with its `project` alias) and the
  `workspaces` descriptor from `crates/codegg-core/src/command_catalog.rs`.
- Remove `BuiltinSlashAction::{Projects, Workspaces}`, their TUI rows, dispatch arms,
  tests, help entries and docs.
- Keep the project picker reachable from `Shift+F` / `Ctrl+\`; update the help text
  and `/help` to name the picker, not a removed command.
- Run `scripts/check_command_catalog_ownership.py` (and its `--self-test`) as the
  gate; the daemon catalog and TUI set must be equal after the removal.

Deliverable: catalog and TUI diff, updated tests, guard receipt, and a `tui-test`
receipt showing `/workspace` and the picker still work.

Exit conditions: no `/projects` or `/workspaces` spelling in the catalog or TUI;
the picker still opens from both hotkeys; the guard passes.

### M007 — The Workspace view is a whole view with NOR/INS and `Shift+X`

Class: **capability**.

Objective: `/workspace` paints only the project window and the right chat pane, with
its own modes, `h/j/k/l` navigation, inline detail, and a route-scoped `Shift+X` exit.
M007 delivers the shell over the existing flat project list; M011 upgrades that list to
the worktree tree and adds the launch sheet.

Scope:

- Give the route its own full-frame renderer (or a route-owned frame plan) that does
  not paint the left sidebar, bottom panel, composer, composer hints, todo/tasks
  strips, or the model/agent badge row. A one-row title carries the mode.
- Add a dedicated `WorkspaceMode` and filter to `WorkspaceDashboardState` (or a new
  view state); do not reuse the composer's mode without restoring it.
- Key router: `h/j/k/l`, `g/G`, `Enter`/`l`, `h` back from detail, `i`,
  `Esc` (INS -> NOR; NOR -> clear filter/back), `Shift+L` chat, `Shift+H` list.
- Intercept `Shift+X` in the route handler before the global binding, **in NOR only**;
  in INS it is the character `X`. Call the existing `leave_workspace_view`, which
  returns through route history. The dispatch order already supports this:
  `handle_workspace_view_key` runs at `app/mod.rs:4251`, before the binding map at
  `mod.rs:4333`.
- Preserve generation/reconnect guards and the no-daemon-work-cancelled rule.

Deliverable: render tests asserting the frame regions and the absence of the composer/
sidebar; key tests in both modes; a `tui-test` receipt of open -> filter -> select ->
detail -> back -> `Shift+X` -> prior session.

Exit conditions: no bottom or left panel, no composer, no badge row; Backspace works in
INS; `Shift+X` exits only in this view and only in NOR, while INS types `X`; the prior
tab/session is restored without a reload.

### M008 — Project lifecycle and permissions from the Workspace view

Class: **capability**.

Objective: from the list or detail, add, edit, archive, destroy a project and manage
its team membership, reusing existing daemon operations.

Scope:

- Add registration, edit (`ProjectUpdate` with `expected_updated_at`), archive/
  restore, and confirmed destroy paths to the view; reuse the picker's phases or
  extract the shared command functions.
- Detail: project identity, workspaces, worktrees, sessions, tasks and team
  membership. Team actions call the existing `team.v1` operations with their
  revisions; owner-only denials render as denials.
- Membership defaults to write access. A read-only member is a role on the membership,
  not a project-scoped policy row. Do not add project-scoped sandbox, approval or
  network policy (D6).
- Keep `Shift+X` as the view exit; archive/destroy use lowercase keys with
  confirmation.

Deliverable: command tests for each lifecycle action, permission-denied tests, and a
`tui-test` receipt of add -> edit -> archive -> restore (on a temp project) and a team
list.

Exit conditions: all actions work from the view; destroy names the session count;
denials are truthful; no second project model is introduced.

### M009 — The Workspace picker side panel

Class: **capability**.

Objective: a bespoke `SidePanelId::Workspace` is listed by `/panels`, reachable
with `Shift+J/K`, and switches projects quickly.

Scope:

- Add the variant to `SidePanelId::ALL`, `label`, `title`, `config_key`,
  `class`, `parent`/bespoke predicate as required; give it a bespoke implementation
  (not a hosted placeholder).
- Render the catalog list with filter/NOR/INS, activity badge, and `Enter` to open
  or focus the project tab. Refresh from the existing catalog cache; do not add a
  second catalog or an N+1 request.
- Ensure the walk selects and focuses it like Chat/Terminal (or explicitly documents
  why it stays unfocused until `Shift+L`).
- Do not add a persisted binding that collides with `Ctrl+digit` slots.

Deliverable: registry tests (every panel names config/title/class), panel-manager
tests, and a `tui-test` receipt switching between `~/projects/codegg` and a second
temp project.

Exit conditions: the panel appears in `/panels`; `Shift+J/K` reaches it; selecting a
project switches tabs without leaving the session; no placeholder box.

### M010 — Task-view refinement

Class: **capability / polish**.

Objective: the project Task view is filterable, navigable and wired to the chat panel
within its existing projection.

Scope:

- Add a NOR/INS filter, `h/j/k/l` navigation, and a persistent detail pane to the Task
  view. Keep the existing `j/k`, `Enter`, `d`, `x`, `K` and `r` actions.
- Keep the existing input panel as is: no second composer, no new input surface.
  Opening a task targets the corresponding chat panel (D9).
- Add a confirmation before `x` cancels; running rows keep delegating to session
  controls.
- Share the detail/key vocabulary with the Workspace detail where practical.
- Render loading, empty, truncated, denied and failed states explicitly.

Deliverable: task-view state tests, render tests, and a `tui-test` receipt on a
project with at least one running and one future WorkOrder.

Exit conditions: filter and detail work; no fabricated action; no scheduler change;
the existing input panel is untouched.

### M011 — The Workspace tree and the worktree/branch session launch

Class: **capability**.

Objective: the Workspace view shows each workspace's managed worktrees as a tree, and
`n` launches a session into an existing worktree or a new checkout with a chosen base
branch and branch name, using the existing `SessionLaunch` request.

Scope:

- Grow `WorkspaceDashboardState` into a tree projection: project → workspace →
  managed worktree. Build it from the existing catalog plus
  `ManagedWorktreeList { workspace_id }` on expansion; cache per workspace and do not
  issue an N+1 read at startup.
- `n` opens a launch sheet for the selected workspace or worktree. Existing worktree:
  list `state == ready` worktrees and send `SessionLaunch { project_id, intent: New,
  placement: ExistingManagedWorktree { worktree_id }, idempotency_key }`. New
  checkout: read `GitRead { request: GitReadRequestDto { project_id, operation:
  Branches, max_rows } }`, offer a `base_ref`, and send the same request with
  `placement: NewManagedCheckout { base_ref, target_branch }`.
- Branch choice: `target_branch: None` uses the daemon's deterministic
  `codegg/worktree/<id>` minting; a user-typed name is sent as `target_branch` and a
  collision renders the daemon's typed refusal. The sheet never mints a name or path.
- One idempotency key per submit. Reuse it on a transport retry; start a new one after
  a validation refusal. A `converged` response opens the original session.
- Render the launch `stage` while in flight; `needs_attention` is an error with the
  stage and attention code, not an endless spinner. On success, open the returned
  `session_id` and switch the view to that workspace.
- The `NewManagedCheckout` authorization decision is the daemon's; render a denial as
  a denial. The TUI never calls `git` and adds no new request.

Deliverable: tree projection tests, launch-request tests (existing, new, auto branch,
named branch, collision, converged), idempotency tests, and a `tui-test` receipt with a
workspace holding two worktrees and one new checkout.

Exit conditions: the tree shows managed worktrees under their workspace; `n` launches
into both placement kinds; an omitted branch lands on the daemon-minted name; a named
branch is used verbatim; a collision is a typed error; a retry does not create a
second checkout; no `git` process is spawned by the TUI.

### M012 — Shared chat scope across a workspace's worktrees

Class: **invariant / capability** — **blocked on D12**.

Objective: satisfy "all worktrees under the same workspace share a chat" under the
reading the owner chooses.

Scope:

- **Grouping reading (recommended if the owner means a shared chat list):** keep each
  checkout's own execution workspace, and group sessions by workspace lineage so a
  child worktree's sessions appear under the source workspace's chat list. The
  lineage already exists in `managed_worktree.workspace_id` and
  `workspace_project_binding.worktree_id`; this is a query/grouping change, not a
  schema change.
- **Single-conversation reading:** bind the session's chat scope to the source
  workspace while the turn's execution workspace is the checkout. That changes the
  rule `src/core/session_launch.rs:1-9` states, needs an ADR, and likely needs a
  schema field for the chat scope. Stop and write the ADR rather than smuggling it
  into this milestone.
- Neither reading rewrites existing session rows. Legacy sessions keep their recorded
  workspace; the grouping is a projection.

Deliverable: the D12 decision record in this plan, plus either the lineage-grouping
tests and a `tui-test` receipt (two worktrees, one shared chat list) or the ADR.

Exit conditions: the chosen reading is recorded; the workspace chat list shows the
child worktrees' sessions; no historical row is rewritten; the execution workspace of
each turn is still explicit.

## 8. Cross-cutting requirements

### Authority and security

- The Workspace view is a projection. Every action still goes through the daemon's
  authorization, the project catalog, the team service, or the scheduler.
- A launch that creates a checkout is a daemon-authorized effect
  (`src/core/daemon.rs:1018-1030`). The TUI sends the placement and renders the
  denial; it never creates the checkout, mints the branch, or touches the repository.
- Worktree listing is a read. The tree never resolves a worktree path from process
  cwd; every request carries the workspace or project identity the daemon authorized.
- No route resolves a working directory from process cwd. The view uses the tab's
  explicit `ExecutionContext`; selecting a project opens its tab before any
  project-scoped request.
- Team membership operations remain owner-gated and revision-checked; no client-side
  authorization.
- No secret, token, or credential enters the view, its filter, its logs, or its
  receipts.

### Protocol and compatibility

- Aggregate diff and model preference changes are additive and defaulted; an older
  peer keeps decoding. Do not renumber or remove existing fields.
- `SessionList`'s workspace filter is optional; an older daemon ignores it and the
  client filters within the returned bound, saying so if truncation makes the filter
  incomplete.
- Removing commands is a catalog change, not a wire break: the daemon catalog and the
  TUI rows move together.
- `SessionLaunch` is consumed as it exists; M011 adds no request and no field. The
  response's `launch_id`/`session_id`/`workspace_id`/`worktree_id`/`branch` are the
  only identities the TUI adopts — it never predicts a minted id or branch.
- One idempotency key per launch submit; a transport retry reuses it, a validation
  refusal starts a new one. A `converged` response is a success, not a duplicate.

### Storage

- No migration is expected for M001–M011. Lazy session creation uses the existing
  schema; the workspace binding and `managed_worktree.workspace_id` already exist. If a
  draft marker must be durable, stop and re-plan: a durable draft is a schema
  decision, not a UI flag.
- The **single-conversation** reading of D12 does need a schema decision (a chat-scope
  field distinct from the turn's execution workspace). Stop and write the ADR rather
  than adding it informally.
- Historical empty rows are data. Hide them by default; delete only through a counted,
  confirmed command.

### Keymap

- New keys are route-scoped (`Workspace`) or Normal-only; no global capital-letter
  binding may make ordinary Insert typing impossible. `n`, `x`, `a`, `e`, `d` and `p`
  exist only inside the route.
- `Shift+X` keeps its global meaning outside the Workspace route. Inside it, the exit
  is NOR-only and INS types the capital letter (D1).
- The panel walk's `Shift+H/J/K/L` and arrow twins keep one dispatcher
  (`move_directionally`) and one panel registry.

### Documentation and guards

- Update `architecture/tui.md`, `docs/tui.md`, the help overlay, and the command
  catalog descriptions in the same change as the behavior.
- Guards: `check_command_catalog_ownership.py`, `check_tui_transport_authority.py`
  (Workspace actions), `check_test_assertion_quality.py` and
  `check_test_duplication.py` (new tests), `check_daemon_cwd_usage.py` (no cwd),
  `check_no_hardcoded_models.py` (no model list), and the panel-surface tests.
- A new writer surface is not expected: the launch's checkout creation is the
  daemon's existing `WorktreeService` effect, not a new TUI launcher. If a new launcher
  or process surface appears, `check_writer_disposition.py` must be updated in the
  same change.
- `architecture/worktree.md` and `architecture/session-storage.md` gain the launch and
  worktree-tree description; `architecture/tui.md` gains the tree/mode contract.

## 9. Verification strategy

Per milestone, before closure:

1. Focused tests: `cargo test -p codegg-tui --lib`, the relevant `codegg-tui`
   family, `cargo test -p codegg-core --lib`, and the daemon family the change touches.
2. `tui-test` receipts for the user-visible capability, on the implementation head,
   with the terminal size, cwd, command and waits recorded. At minimum:
   - first launch in a fresh temp repo; no empty session; one project;
   - `Shift+J/K` across every panel; no placeholder box; Terminal paints its shell and
     stays NOR, then `i` INS;
   - `/models` select model + level; no error toast;
   - Diff panel with no argument; two files; `/diff <path>` one file;
   - `/workspace` filter with Backspace, `h/j/k/l`, detail, back, `Shift+X` (NOR) and
     `X` typed in INS;
   - `/panels` -> Workspace picker -> switch project;
   - `/sessions` scoped to workspace A then B; no empty rows;
   - Workspace tree with two worktrees under one workspace; `n` launches into each;
   - a new checkout with a base branch and the auto-named branch, then a named branch,
     then a deliberate collision;
   - a dropped-connection retry with the same idempotency key converges on one
     checkout;
   - after D12, the workspace chat list shows the child worktrees' sessions.
3. Static guards: the command catalog guard, transport authority, test quality, test
   duplication, daemon cwd, and the panel-surface guards named in §8.
4. `scripts/verify.sh scoped` then `scripts/verify.sh quick`; `full` before a
   milestone that changes the daemon/protocol.
5. Closure records under `plans/closure/tui-workspace-and-panel-surface-corrective/`
   with the receipt paths and the exact commands run. A capability with no receipt is
   conditionally closed at best.

## 10. Risks and decision points

| # | Decision / risk | Disposition | Stop condition |
|---|---|---|---|
| D1 | `Shift+X` is the Workspace exit and the Normal-mode center-tab close | **Decided 2026-11-26:** route-scoped and NOR-only; INS types `X`; outside the route the global close is unchanged | None — the router already runs before the binding map (`app/mod.rs:4251` before `:4333`). Re-check if that ordering changes |
| D2 | Terminal NOR reverses closed TUI live-surface M001 | **Decided:** keep the eager create/attach and the live render; drop the PTY focus so the keyboard stays NOR and `Shift+J/K` leaves the panel. The panel itself stays focused. Record the reversal in M002; do not edit the closure | If the live surface cannot render unfocused, show it with an inline "press i to type" line rather than restoring INS-on-load |
| D3 | One preference write vs two sequenced writes | **Decided:** extend `ModelSelect` additively with the reasoning selection and expected revision; one CAS over both dimensions; return the preference; cover both daemon call sites (`set_policy` is the precedent) | Only if the protocol cannot carry the reasoning selection additively; per-dimension CAS needs an ADR |
| D4 | Aggregate diff payload vs new request | Additive `files` field on the existing response | If the wire bound cannot carry a bounded file list, stop and design a paged request |
| D5 | Whole-view Workspace vs the shared `TuiLayout` | Route-owned full frame; session route keeps the existing layout | If mouse hit-testing or the panel host cannot be isolated, keep the route-owned frame but record the hit-test limitation |
| D6 | "Manage permissions" scope | **Decided:** team membership only; write access by default; a read-only member is a role, never a project policy row | If per-project approval/sandbox/network policy is required, stop and write an ADR + plan |
| D7 | Historical empty sessions | Hide by default; counted, confirmed cleanup | Never delete silently; if the cleanup cannot name the count, do not ship it |
| D8 | Command catalog removal | One change across core catalog, TUI enum, rows, tests, docs | Guard failure means the sets disagree; do not merge a partial removal |
| D9 | Task-view action set | **Decided:** keep the existing input panel and the `d`/`x`/`K`/`r` actions; wire opening a task to the corresponding chat panel; add a confirmation before cancel | If opening a task cannot target the chat panel, stop and record why rather than adding a second composer |
| D10 | Existing empty rows and workspace binding drift | Do not rewrite history; report the count; bind only new sessions | If a legacy row has no workspace, show it under "unbound" rather than guessing one |
| D11 | Chat-pane composition in the Workspace view | The whole view paints no composer; the chat pane is a viewer; the ordinary composer is restored on exit. This is the reading of "no model input bar" | If composing inside the view is required, add a view-owned input and revisit the layout; do not silently keep the ordinary composer painted |
| D12 | "All worktrees under the same workspace share a chat" vs the launch service's per-checkout workspace | **Grouping reading (recommended):** keep the checkout's execution workspace and group sessions by workspace lineage under the source workspace. **Single-conversation reading:** bind chat scope to the source workspace and the turn to the checkout — ADR + schema | Do not implement either reading until the owner picks one; M012 is blocked |
| D13 | Deterministic auto-naming | The daemon already mints `codegg/worktree/<id>` when `target_branch` is omitted; use it. The TUI shows "automatic" and displays the returned name | If a human-readable auto name is required, it is a daemon naming policy, not a TUI string |

## 11. Completion definition

The addendum is complete when all twelve milestones have closure records and:

- every panel reachable by the walk shows content or an inline state, and the Terminal
  paints its live shell while opening NOR;
- the Diff panel shows the repository aggregate with per-file navigation;
- model + thinking selection persists in one action with no self-conflict;
- a fresh launch and `Ctrl+N`/`/new` create no empty session, the session-scoped
  surfaces answer in the draft state, and `/sessions` is workspace-scoped by default;
- `/projects` and `/workspaces` are gone and the command-catalog guard passes;
- `/workspace` is the whole view described in §5.6, including NOR-only `Shift+X`,
  NOR/INS with `X` typeable, `h/j/k/l` tree navigation, inline detail,
  add/edit/archive/destroy and team membership (write by default);
- the Workspace tree shows managed worktrees, and `n` launches a session into an
  existing worktree or a new checkout with deterministic auto-naming;
- the D12 reading is implemented and the workspace chat list shows its worktrees'
  sessions;
- the Workspace picker panel is listed in `/panels` and reachable by `Shift+J/K`;
- the Task view is filterable and navigable, keeps its input panel, and opens tasks
  into the chat panel;
- every capability has a `tui-test` receipt, and every guard above passes.

## 12. Milestone status table

| Milestone | Scope | Class | Status |
|---|---|---|---|
| M001 | New repository becomes one project; no empty session | capability | partial — launch is a draft (no row); the integration test and receipt are outstanding |
| M002 | Panel walk fills the column; Terminal NOR | capability / invariant | partial — Terminal NOR landed and is tested; the inline-load contract for every asynchronous filler is not |
| M003 | Aggregate Diff panel | capability | landed — additive `files` + splitter + panel browser; `tui-test` receipt outstanding |
| M004 | One model + reasoning preference write | invariant / capability | landed — one CAS, returned preference, store regression test; receipt outstanding |
| M005 | Lazy sessions and workspace-scoped listing | capability / invariant | landed — draft sessions for launch, `Ctrl+N` and `/new`; the list defaults to the active workspace with a project toggle, hides message-less rows unless asked, and a counted `C` cleanup spares the active session |
| M006 | Remove `/projects` and `/workspaces` | capability / invariant | landed — both catalogs, rows, dispatch and docs; `check_command_catalog_ownership.py` passes |
| M007 | Workspace whole view, NOR/INS, `Shift+X` | capability | landed — route-owned frame, modes, filter, `h/j/k/l`, inline detail, NOR-only exit; receipt outstanding |
| M008 | Workspace project lifecycle and permissions | capability | landed — `a`/`e`/`s`/`x`/`t` drive the picker's own phases through one shared entry point, the detail carries task counts and the membership policy line, team answers are adopted for the view's selection |
| M009 | Workspace picker side panel | capability | landed — `SidePanelId::Workspace`, real adopted component, catalog rows |
| M010 | Task-view refinement and chat-panel wiring | capability / polish | landed — filter, modes, section jumps, detail line, confirm-before-cancel; chat-panel wiring is the existing open path |
| M011 | Workspace worktree tree and session launch | capability | landed — `ProjectGet`/`ManagedWorktreeList` tree, `GitRead(Branches)` base, `SessionLaunch` with both placements, one idempotency key; receipt outstanding |
| M012 | Shared chat scope across worktrees | capability / invariant | landed (lineage reading) — a session in a managed checkout groups under the workspace that launched it; a projection, no row rewritten |

## 13. Handoff note

This is a **draft corrective plan**, not a handoff-ready implementation plan. Per
`plans/003-planning-process.md`, each milestone above needs its own bounded
`plans/implementation/tui-workspace-and-panel-surface-corrective/NNN-*.md` before an
agent executes it, with the repository baseline, exact work packages, tests, commands,
stop conditions and closure evidence.

The owner settled D1, D2, D3, D6 and D9 on 2026-11-26. Remaining before handoff:

- **D12** — the shared chat scope is the one architectural decision. M012 cannot start
  until it is answered; the grouping reading needs no migration, the
  single-conversation reading needs an ADR and a schema decision.
- **D11** — the chat-pane composition reading. It only changes M007's layout, but it
  must be confirmed before M007 is implemented.
- **D13** — the auto-name policy. The daemon's `codegg/worktree/<id>` satisfies
  "deterministic, no LLM"; a human-readable scheme would be a daemon change.

M008 and M011 depend on M007; M011's launch surface is the largest new UI in the plan
and should be implemented only after `SessionLaunch` has a focused integration test.

## 14. Implementation record (2026-11-26, first pass)

The owner asked for the plan to be implemented on branch `feat/auto-20261126-cae46b00`
from baseline `f2ae6d507`. This section records what landed, so the status table
above cannot be read as a claim that the addendum is closed.

**Landed with tests:**

- **M004** — `CoreRequest::ModelSelect` carries `reasoning` and
  `expected_revision` additively; `RuntimePreferenceStore::set_model_and_reasoning_preference`
  writes both dimensions under one revision bump; `CoreResponse::ModelPreference`
  returns the row; the TUI sends one request and adopts the returned revision.
  Regression test `model_and_reasoning_are_written_in_one_revision_bump`.
- **M003** — `GitDiffFileDto` and `GitDiffResultDto::files/truncated`; one
  `git diff` split into bounded per-file sections (200 files, 256 KiB/file, 1 MiB
  total); the Diff panel starts the bare read instead of refusing and adopts a
  file-index browser. Splitter tests cover multi-file, binary and empty answers.
- **M006** — `/projects` (alias `/project`) and `/workspaces` removed from the
  daemon catalog, the TUI registry, the dispatch arms, the help/docs and the tests;
  `check_command_catalog_ownership.py` passes.
- **M007** — `Route::Workspace` renders a route-owned whole view (title, tree,
  chat viewer, footer) with no sidebar/composer/badges; its own `WorkspaceMode`
  and filter; `h/j/k/l`, `g/G`, `i`, inline detail; `Shift+X` exits in NOR only.
- **M009** — `SidePanelId::Workspace` (config `workspace`) with a real adopted
  catalog component; `/panels` and `Shift+J/K` reach it.
- **M010** — Task view filter and NOR/INS, `h/l` section jumps, a persistent
  selected-row detail line, and `x` arming on first press.
- **M011** — project -> workspace -> worktree tree from `ProjectGet` and
  `ManagedWorktreeList`; the launch sheet reads `GitRead(Branches)` and sends
  `SessionLaunch` with `ExistingManagedWorktree` or `NewManagedCheckout`; one
  idempotency key per submit, replaced after a refusal; the view opens the returned
  session.
- **M002 (Terminal half)** — selecting the panel still creates/attaches and paints
  the live shell, but the PTY keyboard is not taken; `i` is the way into INS. The
  test that pinned INS-on-load was inverted.

**Not implemented in this pass:** M008 (project lifecycle and team membership from
the view), M012 (lineage-grouped chat — D12 is answered, the query is not written),
the workspace-scoped `/sessions` list and counted empty-row cleanup in M005, and the
inline-load contract for the asynchronous hosted panels in M002.

**Evidence captured:** `cargo test -p codegg-tui --lib` (3,314 passed),
the focused `codegg-core` and daemon splitter tests, the named guards
(`check_command_catalog_ownership`, `check_frontend_action_set`,
`check_tui_transport_authority`, `check_test_assertion_quality`,
`check_test_duplication`, `check_daemon_cwd_usage`, `check_no_hardcoded_models`,
`check_writer_disposition`, `check_test_self_reexec_paths`,
`check_test_common_binding`), `cargo fmt`, and `scripts/verify.sh quick`.

**Second pass (M005, M008, M012):**

- **M005 (listing half)** — `/sessions` carries its own scope (`SessionScope`,
  active workspace by default, `w` toggles to the project), hides message-less
  rows unless `e` reveals them, and gains a counted `C` cleanup that arms on the
  first press and deletes exactly the counted ids on the second. Three rules keep
  it honest: the active session is never hidden, an unanswered count request is
  never read as "everything is empty" (both the filter and the cleanup switch off),
  and hiding rows is a view setting rather than a deletion.
- **M008** — `a`/`e`/`s`/`x`/`t` act on the selected row.
  `commands::project_picker::begin_lifecycle_from` is the single place that
  resolves the picker phase, the pinned project and the request id, and the
  picker's own catalog keys now call it too, so the two surfaces cannot disagree.
  Archive arms then performs (any other key disarms); destroy opens the picker's
  counted confirmation. The detail carries the dashboard's task counts (showing
  `withheld` when the daemon returned none) and the D6 membership line; a team
  listing is adopted when its project is the active tab **or** the view's
  selection, so a project with no tab still shows its team.
- **M012 (lineage reading)** — a session in a managed checkout resolves its
  lineage through `managed_worktree.workspace_id` and groups under the workspace
  that launched it. The grouping is a filter over the existing bounded list: no
  row is rewritten, no schema field, no protocol change. Unattributable rows
  (no recorded workspace) are kept rather than dropped.

**Tests added:** six view-level lifecycle tests (edit pins the selection, archive
arms then performs, another key disarms, destroy opens the counted confirmation,
the no-daemon team path names its denial), three session-list tests (lineage
grouping across two workspaces, the empty-row filter with the active-session
exemption, and the cleanup count with its unanswered-counts refusal), and a
deterministic fix for the `a_refused_clear_reports_itself` fixture, which raced
the terminal create against its own attach.

**Outstanding evidence:** no `tui-test` receipt has been captured on this head, no
`plans/implementation/tui-workspace-and-panel-surface-corrective/NNN-*` work package
was written before execution (the owner asked for the implementation directly), and
`scripts/verify.sh full` has not been run.
