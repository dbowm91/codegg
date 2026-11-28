# TUI Workspace Central Surface Corrective Addendum

Status: closed — M001–M005 are implemented, post-closure verification on 2026-11-27 found defects F1–F5, and corrective M006–M010 are implemented and closed (§13; closures `006`–`010`)

Repository baseline: `eefbfa0fa3b135fc1af3fb2ed9c1d94d1f8cf643` (branch `feat/auto-20261126-cae46b00`, worktree clean at plan time).

Owner audit date: 2026-11-27. Owner direction: `/workspace` is the central area to create, edit, and manage workspaces. Enter on a workspace reveals the workspace: all worktrees, team members, add-member, and working chat typing. Input stays on the bottom. The right-sided chat panel is forced even when absent. All edit/action surfaces for `/workspace` and `/worktree` are full-size in the main panel — never the centered small-box style in `notthis.png`. This addendum records the plan only; no production code is changed here.

Parent and adjacent lines (none is reopened by this addendum):

- `plans/subsystems/tui-workspace-and-panel-surface-corrective-addendum.md` — active; owns the whole-view shell, NOR/INS, tree, inline detail, lifecycle entry points, and the launch sheet. This addendum consumes that shell and corrects five named gaps. It does not edit the parent, its §14 record, or its closure files.
- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md` — closed; owns shared panel chrome and the placeholder contract. Consumed unchanged.
- `plans/subsystems/tui-panel-reachability-corrective-addendum.md` — closed; owns gesture-to-load. Consumed unchanged.
- `plans/subsystems/workspace-authority-and-session-navigation-corrective-addendum.md` — active; owns execution-context authority and session navigation. Consumed unchanged; no session-navigation rule is changed here.
- `plans/subsystems/project-picker-rework-roadmap.md` — closing; owns directory-tied project identity and the picker as global navigation. The picker stays for `Shift+F` / `Ctrl+\\` global navigation; this addendum only removes the workspace route as a caller of it.
- `plans/subsystems/team-collaboration-corrective-addendum.md` and its post-closure addendum — closed/closing; own the durable `team.v1` authority. Consumed unchanged; no membership semantic is changed here.

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

- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md` — accepted; consumed unchanged.
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — accepted; any `/worktree` dispatch change stays inside the daemon-owned catalog contract; the ownership guard is the proof.
- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md` — accepted; WorkOrders stay the durable task layer.
- `plans/adrs/ADR-0048-*` (containment) — consumed unchanged; no launch-authority change.

No new ADR is required. No identity, ownership, protocol, authorization, persistence, or compatibility contract changes. If implementation finds a chat-scope field or a per-dimension preference change is needed, it stops and writes an ADR rather than smuggling it in.

## 1. Purpose and ownership boundary

This addendum owns the `/workspace` route as the central workspace surface: one big main panel where the user creates, edits, and manages workspaces, reveals worktrees and team membership, adds members, launches sessions, and types chat. It owns what the route paints, what its keys do, and which surfaces it refuses to open.

It consumes but does not own: the daemon catalog, `SessionLaunch` (both placements), `ProjectGet`, `ManagedWorktreeList`, `GitRead(Branches)`, the `team.v1` membership operations, the chat channel/history flow, the panel host (`SidePanelId`, `SidePanelHost`), and the project-tab execution context.

It must not own: daemon authorization, session navigation, scheduler/WorkOrder semantics, provider/model policy, or desktop surfaces.

## 2. Work classification

### Invariants

- The workspace route never opens a centered small-box dialog for a workspace or worktree action. Lifecycle, launch, team, and worktree results render full-size in the main panel.
- A printable key the user types while intending to chat reaches exactly one visible input, and the visible input names its destination before the key is typed.
- Entering `/workspace` leaves the route with a chat panel docked Right, visible, and bound to the selected project. A closed Right edge reopens; the "Chat pane unavailable" fallback is unreachable from this route.
- Row-scoped actions use the selected row's project identity, never the active tab as a silent substitute. A team add for a row names that row's project.
- The TUI never mints a worktree path or branch name, never shells to `git`, and never invents a launch identity. One idempotency key per submit; a retry reuses it, a validation refusal starts a new one.

### Capabilities

- A user can type chat from `/workspace` through a visible bottom input whose target badge names the chat destination.
- A user entering `/workspace` always sees the right-sided chat panel, even when it was previously closed.
- A user pressing Enter on a workspace row sees that workspace in-panel: its worktrees, its member list, and an add-member form — all full-size.
- A user can add, edit, archive/restore, and destroy projects, manage membership, and run the new-session launch, all as main-panel modes with no `/workspace`-originated centered dialog.
- A user invoking `/worktree` lands in `/workspace` on the matching workspace/worktree instead of receiving a toast or a centered list dialog.

### Infrastructure

- A route-owned composer binding for the workspace route with explicit focus accounting (route composer vs panel vs filter), so key fall-through is derived state rather than a per-key exception.
- A row-scoped team fetch adopted for the view's selection (in addition to the existing active-tab adoption), reusing the existing `team.v1` request family.
- A main-panel mode state for lifecycle and launch that replaces the picker-phase and footer-only launch rendering on this route, reusing existing daemon operations.

### Polish

- Footer, title-row, and hint vocabulary for the new modes share the route's existing dialect; no new global key is introduced.
- Receipts reuse the isolated-HOME + isolated DATA_HOME `tui-test` recipe with recorded size, cwd, and waits.

## 3. Non-goals

- No new panel framework, panel kind, or host contract. `SidePanelId`, `SidePanelHost`, and the bespoke/hosted split stay.
- No project-scoped sandbox, approval, network, or runtime policy store. Membership defaults to write access; a read-only member is a role (D6 from the parent, unchanged).
- No second worktree-creation path. `SessionLaunch` with `NewManagedCheckout` / `ExistingManagedWorktree` is the only creation route.
- No model-assisted naming. Daemon deterministic minting (`codegg/worktree/<id>`) stays.
- No scheduler, WorkOrder, or task-model change. No new daemon request or field.
- No session-navigation change. Selecting a project still goes through the tab/execution-context path.
- No desktop or web frontend work.
- `HTTP /api/projects*` server routes are untouched. This is a TUI slash-command and route-surface change only.
- The one-time token plaintext display (`DeviceSecretDialog`) stays a dedicated modal and is never inlined into the scrollable panel. Secrecy invariant is unchanged.
- The shared Project Picker stays for global navigation (`Shift+F` / `Ctrl+\\`). Only the workspace route stops calling it.
- No rewrite of the parent addendum, its §14 record, or any closure file. The D11 reversal below is recorded here, not edited there.

## 4. Current state

At baseline `eefbfa0fa`, verified by source inspection plus a live isolated-HOME `tui-test` drive of the real `/workspace` route (remote-backed repo, 150x44):

- `/workspace` and `/worktree` are separate registrations (`crates/codegg-tui/src/command.rs`) with separate dispatches (`crates/codegg-tui/src/app/mod.rs`). Workspace dispatch opens the dashboard route (`crates/codegg-tui/src/commands/workspace_dashboard.rs`); worktree dispatch sends `TuiCommand::WorktreeList` (`crates/codegg-tui/src/commands/tasks.rs`, handled in `crates/codegg-tui/src/runtime/command_dispatch.rs`).
- The normal workspace fast path (`crates/codegg-tui/src/app/render.rs` route branch into `render_workspace_frame`) is a whole frame that explicitly paints no composer, no sidebar, no badge row; the route footer is the whole vocabulary. The route key handler claims every key while active (`crates/codegg-tui/src/commands/workspace_view.rs`).
- Entering the view selects Chat then immediately blurs it (`open_workspace_dashboard`), so a closed Right edge renders the "Chat pane unavailable — Shift+S to dock the Chat panel" fallback instead of chat.
- `Shift+H` ("back to composer") only blurs the panel; it does not leave the route and there is no route composer to land on. Printable keys therefore cannot reach a bottom input on this route, which is the reported "typing in chat is broken" mechanism. The chat panel itself is a viewer with pass-through composition (`crates/codegg-tui/src/panels/chat.rs`, `crates/codegg-tui/src/chat_browser.rs`, composer routing in `crates/codegg-tui/src/app/prompt_turn.rs` and `crates/codegg-tui/src/commands/chat.rs`).
- Enter on a project switches to its tab and leaves the route; Enter on a workspace/worktree toggles expansion (`workspace_view.rs` key router). `d` opens an inline project detail that replaces the tree in the main body; the launch sheet renders as a footer one-liner (`render.rs` workspace frame).
- `a`/`e`/`s`/`x` shell to the shared Project Picker via `begin_lifecycle_from` (`crates/codegg-tui/src/commands/project_picker.rs`), a centered ≤64x18 modal (`crates/codegg-tui/src/components/dialogs/project_picker.rs`, mounted in `crates/codegg-tui/src/app/modal.rs`). That centered box is the `notthis.png` surface.
- Workspace `t` invokes `start_show_team` with the row's project id, but results open the centered `InfoType::Team` dialog (`crates/codegg-tui/src/commands/team.rs`, geometry in `crates/codegg-tui/src/components/dialogs/info.rs`). `/team add` resolves the active tab's project, not the selected row.
- `/worktree` reads the active tab root, issues legacy `CoreRequest::WorktreeList`, and renders a toast for ≤5 results or a centered `InfoType::WorktreeList` dialog above five — never the workspace route.
- Live probe confirmed: tree, Space/l expand, `d` inline detail, footer vocabulary, right-side Chat viewer, and the picker/team dialog layering over the main panel.

## 5. Target architecture

One route-owned main panel with three regions: title row, main body, bottom input. The right portion of the main body is the docked Chat viewer; the left/remaining portion is the tree, or the in-panel reveal, or the in-panel lifecycle/launch mode. The bottom input is the route's composer: it names the chat destination when armed and falls back to the tree filter only through an explicit mode, never by key accident.

Key fall-through is structural: the route handler owns navigation keys and explicitly declines composition keys so they reach the focused panel host and then the route composer. No per-verb exception list.

Team state is row-scoped: the view fetches membership for the selected row's project through the existing `team.v1` family and adopts completions for the view's selection (alongside the existing active-tab adoption). Add-member carries the same row identity; denials render in-panel as denials.

Lifecycle and launch are modes of the main body, not dialogs: Register/Edit/State/Destroy phases and the launch placement/base/branch steps reuse the existing request functions and state machines, with rendering moved from the picker modal and the footer line into the main body. The shared picker keeps its global-navigation entry points; the workspace route simply stops calling `begin_lifecycle_from`.

`/worktree` becomes a navigator: it resolves an optional workspace/worktree selector, opens `/workspace`, expands and selects the target, and focuses it. It renders no toast-or-dialog of its own.

## 6. Dependency graph

```text
M001 route composer + key fall-through (chat typing)
  |
  +--> M002 force Chat docked Right on entry
          |
          +--> M003 Enter-reveal: worktrees + row-scoped members + add-member
                  |
                  +--> M004 lifecycle + launch as main-panel modes (no route dialogs)
                          |
                          +--> M005 /worktree navigates into /workspace
```

Hard edges only: M002 needs M001's composer and fall-through (forced chat without a typing path repeats the defect); M003 needs M002's bound panel (reveal adopts chat and team for a visible selection); M004 needs M003's row-scoped adoption shape (lifecycle/team completions share the selection guard); M005 needs M004's dialog-free route (a navigator must not land on a route that still opens boxes). No daemon, protocol, storage, or scheduler dependency between milestones.

## 7. Milestones

### M001 — The workspace route owns a composer and stops swallowing composition keys

Class: capability / invariant

Objective: typing chat from `/workspace` reaches a visible bottom input whose badge names the chat destination.

Dependencies: none (hard). Interface: existing chat compose seed, armed-composer delivery, and panel pass-through contracts stay.

Deliverable boundary: route-owned composer binding painted on `render_workspace_frame`; route key router declines composition keys so they reach the panel host then the route composer; `Shift+H` focuses the route composer; route exit (`Shift+X`/`Esc`) is the only path back to the session composer; footer and badge vocabulary updated in the same change.

User value: chat typing works from the central surface.

Exit conditions: printable keys typed with chat focus land in the visible route input exactly once; the badge names the destination before typing; no key is both a tree verb and a typed character in the same mode; leaving the route restores the session composer untouched.

Deferred work: forced chat docking (M002); reveal and lifecycle rendering (M003/M004).

### M002 — Entering /workspace forces Chat docked Right, visible, and bound

Class: capability

Objective: `/workspace` entry always presents the right-sided chat panel, even when the Right edge was previously closed.

Dependencies: hard on M001 (forced chat without the typing path is the current defect).

Deliverable boundary: entry docks Chat Right, makes the edge visible, and binds it to the selected project; the unavailable-pane fallback is unreachable from this route; selection changes rebind through the existing sync path; no new panel kind, no placement persistence change.

User value: the Chat viewer the reveal and chat flows assume is always present.

Exit conditions: entry from a closed-Right state paints Chat bound to the selection; switching selection rebinds; no toast or manager detour is required.

Deferred work: reveal content (M003); lifecycle/launch modes (M004).

### M003 — Enter on a workspace reveals worktrees, members, and add-member in-panel

Class: capability

Objective: Enter on a workspace row reveals that workspace in the main body: its worktrees (already fetched), its member list (row-scoped fetch), and an add-member form — all full-size, no centered dialog.

Dependencies: hard on M002 (reveal binds the chat and team adoption to a visible selection).

Deliverable boundary: Enter expands the row and opens the workspace section of the inline detail; worktrees render from the existing cache; membership fetches with the row's project id and adopts for the view's selection; add-member carries the same row identity and renders success/denial in-panel; `/team add` semantics outside the route are unchanged.

User value: the "reveal" the owner asked for: worktrees plus people plus joining, in one place.

Exit conditions: a workspace with worktrees shows them; a project with members shows them; add-member names the row's project and reports its result in-panel; no `InfoType::Team` opens from this route; unauthorized projects render a truthful denial, never another project's rows.

Deferred work: lifecycle/launch modes (M004); `/worktree` navigation (M005).

### M004 — Lifecycle and launch are main-panel modes; the route opens no small boxes

Class: capability

Objective: add, edit, archive/restore, destroy, team administration, and the new-session launch all render as full-size main-panel modes. No `/workspace`-originated Project Picker, Team info box, or Worktrees info box remains.

Dependencies: hard on M003 (lifecycle/team completions share the row-scoped selection guard).

Deliverable boundary: the picker phases used by `a`/`e`/`s`/`x` move into view-owned modes with full-width forms and explicit confirmations; the launch placement/base/branch steps move from the footer line into the main body; the Team dialog opener is removed from the route path; the shared picker keeps its global hotkeys but the route never calls `begin_lifecycle_from`; the one-time token secret modal stays exactly as is.

User value: every workspace action happens in the big panel the owner pointed at.

Exit conditions: each of add/edit/state/destroy/team/launch completes from the main panel with no centered box; destroy still names the session count; archive still arms-then-performs; token plaintext never enters the panel transcript; global `Shift+F` picker still works outside the route.

Deferred work: `/worktree` navigation (M005).

### M005 — /worktree navigates into /workspace instead of toast-or-dialog

Class: capability

Objective: `/worktree` resolves a workspace/worktree selector and lands the user in `/workspace` on that target. It renders no list of its own.

Dependencies: hard on M004 (the landing route must already be dialog-free).

Deliverable boundary: parse an optional workspace/worktree selector; open the route, expand the lineage, select and focus the target worktree (or workspace when no worktree is named); unknown selectors render a typed in-panel notice on the route, not a toast-or-dialog; the legacy toast/centered-dialog branches are removed from this path; catalog ownership guard stays green.

User value: one central surface; `/worktree` is a deep link, not a second list.

Exit conditions: naming a workspace selects it; naming a worktree selects and focuses it; an unknown name stays on the route with a typed notice; no `WorktreeList` toast or dialog appears from this command.

## 8. Cross-cutting requirements

### Storage and migration

No migration. No new durable field, no draft marker, no schema bump. Team chat scope grouping rides the existing lineage projection. If implementation finds a durable field is required, it stops and re-plans.

### Protocol and compatibility

No new request, no new field, no renumbered variant. Reuse `ProjectGet`, `ManagedWorktreeList`, `GitRead(Branches)`, `SessionLaunch` (both placements, one idempotency key per submit), the `team.v1` membership family, and the existing chat channel/history flow. Catalog changes (if any spelling moves) keep the daemon catalog and TUI set equal with the ownership guard as proof.

### Security and authorization

The view stays a projection. Every action goes through daemon authorization. Team operations stay owner-gated and revision-checked. Launch authorization stays the daemon's; denials render as denials. No secret enters the panel, filter, logs, or receipts. The token secret modal is the only centered surface retained, and it is never fed from panel state.

### Concurrency, cancellation, and recovery

Generation and reconnect guards are preserved; leaving the route cancels only frontend workspace requests and never daemon work. Late completions are dropped against the selection/request generation they no longer match. A dropped-connection retry reuses the submit's idempotency key; a validation refusal starts a new one.

### Observability and audit

No new event family. Toasts name the row and the refusal code; the footer names the mode and destination. Receipts record terminal size, cwd, commands, and waits.

### Performance and resource use

No N+1 fan-out. Workspaces load on expansion; worktrees load per workspace; membership loads on reveal. Bounded rows, bounded filter, bounded patch/launch rendering as today.

### Documentation and operations

Update `architecture/tui.md`, `docs/tui.md`, the help overlay, and command descriptions in the same change as the behavior. Guards named in each implementation plan stay green in the same change.

## 9. Verification strategy

Per milestone, before closure:

1. Focused tests: `cargo test -p codegg-tui --lib`, the relevant TUI family, and the daemon family the milestone touches. New tests fail on the baseline first where the defect is reproducible without a live daemon.
2. `tui-test` receipts on the implementation head with isolated HOME + isolated DATA_HOME, remote-backed repo, recorded cols/rows/cwd/commands/waits. Minimum set: open route with closed-Right edge; type chat and show the badge + delivery; Enter on a workspace and show worktrees + members + add-member; each lifecycle/launch mode with no centered box; `/worktree <selector>` landing on the target; unknown selector notice.
3. Static guards: command-catalog ownership, TUI transport authority, test assertion quality, test duplication, daemon cwd usage, no-hardcoded-models, plus the panel-surface guards each plan names.
4. `scripts/verify.sh scoped`, then `scripts/verify.sh quick`; `full` before any milestone that touches daemon/protocol (none is expected to).
5. Closure records under `plans/closure/tui-workspace-central-surface-corrective/` with receipt paths and exact commands. A capability with no receipt is conditionally closed at best.

## 10. Risks and decision points

| # | Decision / risk | Disposition | Stop condition |
|---|---|---|---|
| D1 | Parent D11 said the whole view paints no composer and the chat pane is a viewer | **Reversed for this line, recorded here (owner direction 2026-11-27):** the central surface owns a bottom input and the chat viewer composes through it. The parent record is not edited; this addendum is the authority for the new reading | If the frame cannot host the input without breaking the panel host's hit-testing or focus contract, stop and record the geometry limit rather than silently keeping the viewer-only frame |
| D2 | Key fall-through shape | Route owns navigation keys and declines composition keys to the panel host then the route composer; no per-verb exception list | If a printable key must be both a tree verb and typed text in the same mode, stop: the modes are wrong, not the key |
| D3 | Row vs tab identity for team add | Row-scoped inside the route; active-tab semantics outside the route are unchanged | If the daemon family cannot carry a row identity distinct from the tab scope, stop rather than falling back to the tab silently |
| D4 | Picker reuse vs duplication | Move the lifecycle rendering into the view; share the request functions, not the modal phases | If a lifecycle operation has no daemon request, the view says so and the milestone stops rather than synthesizing one |
| D5 | `/worktree` selector ambiguity | Optional workspace/worktree selector; unknown names are typed in-panel notices on the route | If the selector grammar collides with an existing catalog spelling, stop and re-plan the grammar rather than shadowing it |
| D6 | Receipt environment | Isolated HOME + isolated DATA_HOME, remote-backed repo (local-only repos have no repository identity — separate open decision, unchanged) | If receipts are captured against real HOME, they are not evidence |

## 11. Completion definition

The addendum is complete when all five milestones have closure records and:

- typing chat from `/workspace` lands in a visible bottom input whose badge names the destination, proven by a live receipt;
- entry with a closed Right edge still paints Chat bound to the selection, with the unavailable fallback unreachable;
- Enter on a workspace shows worktrees, members, and add-member full-size with no centered dialog;
- every lifecycle and launch action completes in the main panel with no route-originated small box, while the global picker and the token secret modal behave exactly as before;
- `/worktree` lands in `/workspace` on the named target with no toast-or-dialog of its own;
- every guard named in §8 passes and every capability has a `tui-test` receipt.

## 12. Milestone status

All five milestones landed and each has a closure record. Post-closure verification on 2026-11-27 found defects in four of the five, so they are recorded here as **implemented, not verified**. §13 holds the measurements, the findings F1–F5, and the corrective milestones that close them.

§11 is **not** edited, and M001–M005 do not satisfy it on their own: an entry whose Right edge is closed still paints "Chat unavailable" once the selection rebinds, and `/worktree`'s own runtime test is red. The corrective milestones are what make §11 true; no milestone is re-opened and no closure record is rewritten.

| Milestone | Status | Implementation plan | Closure record | Verification result (2026-11-27) |
|---|---|---|---|---|
| M001 | **implemented — finding F1** | `plans/implementation/tui-workspace-central-surface-corrective/001-route-composer-and-key-fallthrough.md` | `plans/closure/tui-workspace-central-surface-corrective/001-status.md` | **FAIL.** The capital arms run ahead of the route's own Panel/Composer branch, so `Shift+H`, `Shift+L` and `Shift+X` are taken from the focused composer and `Shift+X` never reaches the route. Corrected by **M006**. |
| M002 | **implemented — finding F2** | `plans/implementation/tui-workspace-central-surface-corrective/002-force-chat-docked-right.md` | `plans/closure/tui-workspace-central-surface-corrective/002-status.md` | **FAIL on the selection path.** The forced dock and the aggregate reveal hold; a rebind or a filter clamp binds the Chat without fetching, so it paints "Chat unavailable". Corrected by **M007**. |
| M003 | **implemented — findings F2, F3** | `plans/implementation/tui-workspace-central-surface-corrective/003-workspace-reveal-team-inline.md` | `plans/closure/tui-workspace-central-surface-corrective/003-status.md` | **PASS WITH FINDINGS.** `run_store_unavailable` on a route with no session, the selection hidden after reveal, and `t` a no-op under an open detail. Corrected by **M008** (scope) and **M009** (detail). |
| M004 | **implemented — finding F4** | `plans/implementation/tui-workspace-central-surface-corrective/004-lifecycle-and-launch-in-panel.md` | `plans/closure/tui-workspace-central-surface-corrective/004-status.md` | **FAIL.** Archive-first plus an active-only route list makes destroy unreachable for every listed row, and launch discards the request identity it was given. Corrected by **M009**. |
| M005 | **implemented — finding F5** | `plans/implementation/tui-workspace-central-surface-corrective/005-worktree-navigates-to-workspace.md` | `plans/closure/tui-workspace-central-surface-corrective/005-status.md` | **PASS WITH FINDINGS.** One new red test in the runtime family, three descriptions still advertising the old behaviour, and an unreachable TUI `WorktreeList` surface. Corrected by **M010**. |

## 13. Post-closure verification and corrective milestones

This section is the record of the verification that produced §12's results, and of the corrective line it registered. It adds no code, no protocol, and no daemon authority: it names defects that the five closures missed, says why the closures could not see them, and points at the five plans that close them.

### 13.1 What was measured, and why the filtered run could not see the defects

Verification ran **read-only** against `256b215ac` on `feat/auto-20261126-cae46b00` (tree clean; the plan baseline is `eefbfa0fa`). The unfiltered family was the instrument, because a filtered run is exactly what let the closures claim more than the code held:

| Run | Command | Result |
|---|---|---|
| HEAD `256b215ac` | `cargo test -p codegg-tui --test tui_render` | **1247 passed / 9 failed** (1256 total) |
| Baseline `eefbfa0fa` | the same command in a `git archive` export sharing one `CARGO_TARGET_DIR` | **1246 passed / 10 failed** (1256 total) |
| Closure 001's own claim | `cargo test -p codegg-tui --test tui_render -- workspace` | **19 passed / 1237 filtered out**, reproducing as recorded |

The arithmetic is the finding: of the baseline's ten failures, **two were fixed** and **eight are shared with the baseline**, so they are pre-existing and out of scope for this line; against those eight the head adds **one new failing test**, M005's own runtime-family constraint. Every corrective plan therefore requires the *unfiltered* command, names the same eight pre-existing failures, and forbids claiming a green family on a filtered run.

Closure 001's filtered command is the concrete reason F1 was invisible. `-- workspace` selects the route's own test members, which call the route handler directly; none of them presses a capital through the shared key-dispatch path, which is the only path a user has. A filter that selects the code under test while skipping the path a keystroke takes is a filter that can only confirm what the author already believed.

### 13.2 The live receipt

The findings below were observed in a live `tui-test` session driving the built binary, not inferred from source alone. The recipe, recorded so a corrective milestone reproduces the evidence instead of trusting this prose:

- An **isolated identity**: a fresh temp directory for `HOME` and for `CODEGG_DATA_HOME`, so the session cannot read, write, or be confused by the operator's real catalog, sessions, or credentials. No receipt from a real `HOME` is evidence (D6 in §10).
- A **remote-backed** repository, because a local-only repository has no repository identity — that is the unchanged open decision named in D6, not something this verification fixed.
- Keys driven through the terminal test API (text and style locators plus snapshots), with the pane captured at the moment of the claim rather than summarized afterwards.

No frame, log, or session artifact was stored in the repository by this verification, so for the findings below a later reader has the recipe and the source citations, not a stored screenshot; where a frame was not capturable, the finding rests on the source path plus a test, and that distinction is stated per finding. The receipt for the **corrective** line is a separate record with its own stored frames: §13.6 and `plans/closure/tui-workspace-central-surface-corrective/live-receipt-006-010.md`.

### 13.3 Findings

**F1 — the route's capital keys belong to the route, not to the focused input.** `Shift+H`, `Shift+L` and `Shift+X` are consumed by the route's capital arms before its Panel/Composer branch is reached, so a user typing a capital into the route's own composer gets a panel verb (or, for `Shift+X`, nothing, because the key is claimed before the branch that would have handled it). The governing rule is **ADR-0042**: in Insert, a printable key is text, and a claimed key must not fall into an unrelated verb. `plans/000-long-term-specification.md` §25 does **not** settle key ownership, and `architecture/tui.md` states both that `Shift+X` "leaves the route from any Workspace focus" and that composition keys fall through — the first sentence is the document error M006 corrects. The existing sweep for shifted letters in Insert sweeps **keymaps, not routes**, which is why a green sweep and a broken route coexisted. → **M006**.

**F2 — the Chat dock follows the entry, not the selection.** `sync_workspace_chat_panel` already binds *and* starts history when the chat is stale; the selection and filter paths only bind. Painting an unbound or unfetched chat yields "Chat unavailable", so the correct-looking path (enter the route, see Chat bound) and the defective path (move the selection, or clamp it by filtering) diverge without a visible cause. The M002 test seeds both chats and drives the message path only, so the bind-without-fetch path had no test to fail. `catch_unwind` around the render keeps a paint-time panic from taking the session down; it also turns one into a blank frame, which is a **watch item** for a later evidence-backed decision, not a change in this line (§13.5). → **M007**.

**F3 — a workspace row is a workspace only when a session happens to exist.** `scope_for` fills the command scope from the session alone, so a route with no session sends an empty project identity; the daemon refuses and the route paints `run_store_unavailable`. The team path already carries the row's project explicitly — the same shape the scope needs. Independently, detail and team are separate fields, so `t` under an open detail is a no-op, and the reveal leaves the selection hidden behind it. → scope half **M008**; detail half **M009**.

**F4 — the lifecycle is unreachable and the launch does not know its request.** `ProjectCatalog::delete_project` archives first, and the route lists active projects only, so the destroy action is unreachable for precisely the rows the route can show: the closure recorded destroy "reachable through the counted confirmation" while the confirmation could not be reached from any listed row. Separately, the launch carries a `request_id` that the dispatch site discards, so a duplicate or retried launch cannot be recognized as the same request. → **M009**.

**F5 — the cutover is described, not finished.** M005's own runtime-family test is red; three descriptions still say "List worktrees for the active workspace (/worktree)" in the two code catalogs and in `architecture/command.md`; and no sender of `TuiCommand::WorktreeList` remains — only the dispatch arm, which is therefore unreachable. The catalog-ownership guard compares description text **exactly**, so the two code sites must change in one edit or the guard fails. → **M010**.

### 13.4 Corrective milestones

Five milestones, no reconciliation milestone. The registry rows and this section are *registration*, not work, so nothing here needs a milestone of its own.

| Milestone | Plan | Findings closed | Files it edits | Handoff order | Status |
|---|---|---|---|---|---|
| **M006** route input owns the capitals | `plans/implementation/tui-workspace-central-surface-corrective/006-route-input-owns-the-capitals.md` | F1 | `commands/workspace_view.rs` (`handle_workspace_view_key` branch order), `tests/tui_render/tui_composer_claims.inc` | 1 | **closed** — `plans/closure/tui-workspace-central-surface-corrective/006-status.md` |
| **M007** the Chat dock follows the selection, and filter mode keeps its promises | `plans/implementation/tui-workspace-central-surface-corrective/007-chat-dock-follows-selection-and-filter-keys.md` | F2 | `commands/workspace_dashboard.rs` (`sync_workspace_chat_panel` reused, filter paths), `commands/workspace_view.rs` (`sync_selected_project`, `activate_selected_row`), `panels/chat.rs`, `app/render.rs` | 2 | **closed** — `plans/closure/tui-workspace-central-surface-corrective/007-status.md` |
| **M008** a workspace row is a workspace, with or without a session | `plans/implementation/tui-workspace-central-surface-corrective/008-workspace-scope-without-a-session.md` | F3 (scope) | `commands/team.rs` (`start_show_team` scope), `commands/workspace_view.rs` (`prepare_workspace_section`, selection guards) | 3 | **closed** — `plans/closure/tui-workspace-central-surface-corrective/008-status.md` |
| **M009** archive-first lifecycle and launch completions that know their request | `plans/implementation/tui-workspace-central-surface-corrective/009-archive-first-lifecycle-and-launch-identity.md` | F3 (detail), F4 | `commands/workspace_view.rs` (lifecycle refusal, request ids), `app/commands.rs` (`WorkspaceLaunchOutcome`), `runtime/command_dispatch.rs` (apply arms), `app/state/workspace_view.rs` (two id fields). **D withdrawn** | 4 | **closed** — `plans/closure/tui-workspace-central-surface-corrective/009-status.md` |
| **M010** finish the `/worktree` cutover and retire the unreachable list path | `plans/implementation/tui-workspace-central-surface-corrective/010-worktree-cutover-completion-and-legacy-retirement.md` | F5 | `runtime/command_dispatch.rs`, `commands/tasks.rs`, `app/types.rs`, `app/state/dialog.rs`, `components/component.rs`, `components/dialogs/info.rs`, `lib.rs`, `app/render.rs`, `command.rs` + `command_catalog.rs` (one edit), invariant fixtures, docs | 5 | **closed** — `plans/closure/tui-workspace-central-surface-corrective/010-status.md` |

**What actually changed, where the table above was wrong, and how each milestone ended.** All five are closed; the closure records are the evidence and this paragraph is the correction to §13.4's original column. That column named `app/state/workspace_view.rs` for M006–M009, but the behaviour lived in `commands/workspace_view.rs`, `commands/workspace_dashboard.rs` and `commands/team.rs`; the state file gained only M009's two request-id fields, which is why the column looked plausible and was wrong. M008 named a `command_route.rs` that no milestone touched.

Three results are worth carrying forward rather than burying in the closures. **M009's notice text departs from its plan**: the plan prescribed `press s to archive <name>, then x to destroy it`, but this route refuses its own `x` for **every** row and lists active projects only, so the second clause named a gesture that cannot succeed; the notice now names the picker's `d` on a `Tab`-revealed archived row, and the test asserts the notice does **not** say `then x`. **M009's D is withdrawn on evidence**: no cached DTO carries a repository identity and `core.rs` has no `ProjectContext` request, so the precondition needs a protocol change that §9 forbids. **M010's plan is wrong about its own red**: `/worktree <selector>` already navigated at `256b215ac` (`dispatch_worktree` and `begin_selector` both exist there), so the rewritten test passes before the change too — it pins the contract instead of reproducing a failure, and the closure says so rather than presenting a red/green proof it does not have.

One repo-wide gate moved as a side effect: the five milestones' explanatory comments put the comment corpus 16 lines over `docs/comment-budget.toml`'s recorded ceiling, so the manifest was re-banked (`check_comment_budget.py --bank`) in the same change — the guard's documented way to spend lines, and a visible 34-line manifest diff.

**The order is file-serialized, not semantic.** M006–M009 all edit `app/state/workspace_view.rs`, so they must not run concurrently; M010 edits a different file and has no dependency on any of them. No milestone requires another's *behaviour*, so a lone M010 handoff is safe, and so is M006 alone.

Each plan is baselined at `256b215ac`, states §13's eight pre-existing failures as out of scope, and requires the unfiltered `tui_render` command. Earlier plans in the same directory are **not** amended: §5 and §10 of M001–M005 stay as the original intent, and the corrective plan is where the defect and its guard are recorded, per `plans/003-planning-process.md` §7.

### 13.5 What the corrective line deliberately does not do

- **The eight shared `tui_render` failures are not fixed here.** They fail identically at `eefbfa0fa` and `256b215ac`; a milestone that repaired them would be a different corrective with its own baseline.
- `catch_unwind` **stays** in the render path. Removing it converts one malformed frame into a lost session, and no test yet proves the panic is unreachable. M007 records it as a watch item.
- **No loading watchdog.** No test proves a future that never completes, so a watchdog would be a guessed timeout rather than a measured one.
- **No archived-project toggle and no new key.** Route-level destroy is **withdrawn** from M009 pending an owner decision about whether the route lists archived projects; the archive-first rule is a catalog authority and is left intact.
- `CoreRequest::WorktreeList` **and** `GitOperation::WorktreeList` **stay**. M010 deletes only the unreachable TUI surface.
- **No daemon authority, protocol, or storage change** in any of the five. A change to the daemon's refusal behaviour or to §13.1's scope contract is a **stop condition**, not a local decision.
- **No sixth milestone and no re-opened closure.** Closure records `001`–`005` are left exactly as written; §12 carries the correction beside them, so the record shows both what was claimed and what was true.

### 13.6 The delivered build's live receipt

§13.2 recorded findings in a live session and stored no frame. The corrective milestones needed the
opposite: someone has to be able to press the keys and see what M006–M010 actually do. That receipt
now exists, with stored screenshots, at
`plans/closure/tui-workspace-central-surface-corrective/live-receipt-006-010.md` and
`plans/closure/tui-workspace-central-surface-corrective/receipts/`.

| Milestone | Frame | The terminal's own line |
|---|---|---|
| **M006** | `receipts/m006-route-surface.png` | `WORKSPACE  NOR · tree  j/k move · Enter reveal · i filter · Shift+H input · Shift+L chat · Shift+X exit`, with the footer naming `s archive/restore` and `x destroy` |
| **M006** | `receipts/m006-composer-focus.png` | after `Shift+H`: `NOR · composer`, badge `[INS][CHAT]`, placeholder `Message the project chat…`; `Shift+X` typed there yields `HelloX` and the route stays open |
| **M007** | `receipts/m007-filter.png` | `i` gives `INS · filter  type to filter · Enter reveal · Esc back` |
| **M009** | `receipts/m009-archive-first-notice.png` | `x` on an active row: `press s to archive feat-auto-20261126-cae46b00 first; destroy it from the project picker (Tab shows archived)` — the corrected notice, verbatim, with no `then x` |
| **M010** | `receipts/m010-selector-unknown.png` | `/worktree no-such-target`: `No workspace or worktree matched /worktree no-such-target` |
| **M010** | `receipts/m010-selector-resolved.png` | `/worktree feat-auto-20261126-cae46b00`: `Workspace: feat-auto-20261126-cae46b00 (77fe16a7-…)` over a `worktrees` section — the route, not a toast and not a dialog |

Four things the receipt records because they cost time, and because a later reader will otherwise
assume they are defects. **The startup `ERROR` dialog** (`run_store_unavailable`) is the session
view's run store refusing in an isolated data root — the same message already recorded at
`plans/closure/tui-workspace-and-panel-surface-corrective/m009-m010-receipt.md:22`, on a path no
milestone here touches; the route itself paints clean, which is M008's finding F3 fixed. **The
`INFO` panel after leaving the route** is the leave-path toast at `commands/workspace_dashboard.rs:156`,
not a dialog that outlived the route. **A route-composer draft surviving the exit** is the single
shared composer being restored, which M001's contract test pins. And **`tui-test run <binary>` does
not reproduce a frame here** — it stays a canonical-mode tty — while `tui-test open` plus
`submit "exec <binary>"` does; a submitted `/command` also needs a second Return while its completion
list is open. The recipe is in the receipt, not repeated here.

**A pre-existing red this line found by running the whole workspace.** Widening to the workspace
sweep (`cargo nextest run --workspace --locked --profile ci --no-fail-fast`) reported 17877 passed,
12 failed, 11 skipped. Eight are §13.5's recorded `tui_render` failures. The other four are
`codegg::core_daemon command_catalog_m007d::*` — `command_catalog_carries_the_reviewed_classification`
(`the catalog holds 161 commands`, measured 159), `command_catalog_publishes_the_action_of_every_frontend_effect`
(`42 forms are frontend effects`, measured 40), `command_catalog_publishes_the_daemons_serve_ability`,
and `command_execute_refuses_a_frontend_effect`. They are **pre-existing and unrelated**: the only
change to `crates/codegg-core/src/command_catalog.rs` in this line is one `description` string, and
the file's `command_id`/`kind` line sequence is byte-identical to `HEAD` at 159 commands — a
description cannot move a count or a serve-ability. Restoring that one file to `HEAD`'s content and
re-running the family reproduces the same four failures (8 passed / 4 failed, the tree restored
byte-identically), so they are properties of the catalog `HEAD` already ships;
`command_execute_refuses_a_frontend_effect` even reports
`unknown_command: no command named projects`, i.e. it drives a command the catalog no longer
defines. What is new is the *exposure*: every earlier
verification in this line ran the `tui_render` family and focused lib tests, and the scoped tier
cancels on the first failure, so nothing had ever executed the root `core_daemon` family. A red that
only a whole-workspace sweep can see is not a red `quick` will catch, and `verify.sh quick` runs no
tests at all.
