# TUI Workspace Central Surface Corrective Milestone 008 — A workspace row is a workspace, with or without a session

Status: ready for handoff

Repository baseline: `256b215ac` on `feat/auto-20261126-cae46b00` (worktree clean; verification performed 2026-11-27)

Source roadmap:

- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#13-post-closure-verification-and-corrective-milestones`
- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#m003--enter-on-a-workspace-reveals-worktrees-members-and-add-member-in-panel`

Source closure record this milestone corrects:

- `plans/closure/tui-workspace-central-surface-corrective/003-status.md` — records reveal and add-member closed; it also claims no workspace-row reveal frame was capturable, which the post-closure verification disproved (expanding the auto-registered project produced one, and the frame exposed the failure below).

Long-term requirements:

- `plans/000-long-term-specification.md#9-project-repository-workspace-and-worktree-model`
- `plans/000-long-term-specification.md#10-project-catalog-and-discovery`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui` — the Workspace view is a navigation surface; it does not require an open session to describe a project.

Applicable ADRs:

- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md` — a project is identified by its directory; the route's row already carries the identity a command needs.
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — the command catalog and its authority are unchanged.

Primary class: **capability** (membership must be listable from the route) with an **invariant** half (a view must not move a selection the user cannot see).

## 1. Objective

Revealing a workspace from `/workspace` lists and mutates that workspace's team membership **without an attached session**, the in-panel sections replace one another visibly, and navigation keys cannot move a selection that is not on screen.

## 2. Why this milestone is ready

- The route can already reveal a workspace row and add a member in-panel (live: `a` → `INS · add member` → `local-owner … (member added)`); only the list request is refused.
- The refusal is typed and daemon-side: `the command's workspace could not be resolved, so its run cannot be recorded` (`src/core/daemon_commands.rs:1267-1291`), reached because `scope_for` fills the scope from the attached session only (`crates/codegg-tui/src/command_route.rs:175-184`) and there is no session.
- The scope type already has the field: `CommandCatalogScopeDto { project_id: String, workspace_id: String, session_id: Option<String> }`. `start_show_team` already overrides `project_id` with the row's project (`crates/codegg-tui/src/commands/team.rs:302-345`); it never sets `workspace_id`.
- Sessionless commands are an established shape: `ProjectList { include_archived: false }` (`crates/codegg-tui/src/commands/project_catalog.rs:77-85`) and `ProjectGet { project_id }` (`crates/codegg-tui/src/commands/project_picker.rs:613-619`) are sent directly with the identity the surface holds.

## 3. Current implementation evidence

### 3.1 Why the list fails and the add succeeds

The team catalog command is routed through the command scope and needs a workspace to locate the run store; `start_show_team` supplies the project but leaves the workspace empty. `TeamMembershipAdd` carries explicit `project_id`, principal and role (`team.rs:621-665`), so it never consults the scope and works. That asymmetry is the defect, not a daemon rule.

### 3.2 The in-panel sections are not mutually exclusive

`view.detail` and `view.team_project_id` are independent fields (`crates/codegg-tui/src/app/state/workspace_view.rs:152-170`). `open_detail` sets detail without clearing team state (`crates/codegg-tui/src/commands/workspace_view.rs:325-336`); `open_inline_team` and `reveal_workspace` set team state without clearing detail (`:656-702`). Because detail "replaces the tree", `t` while a detail is open loads membership *behind* it and looks like a no-op — reproduced live.

### 3.3 Loading can outlive its request

`team_loading` is set when a load starts and cleared on every completion the client produces (`team.rs:302-329`, `:367-441`, apply `:475-511`). There is no TUI-side terminal state for a request that never completes and none for leaving the route while one is in flight.

### 3.4 Navigation still drives a hidden selection

`j`/`k` remain bound to `move_selection` (`workspace_view.rs:1103-1104`) even while detail or team replaces the tree, so the cursor moves where nothing is painted.

## 4. Invariants that must not regress

- Reveal stays row-scoped: the section describes the selected row's project/workspace, never "the project that happens to hold the active tab".
- Add-member keeps carrying its explicit principal and role, and a refusal stays a refusal.
- No session is fabricated, and no request names a project or workspace the surface did not hold.
- The route's own keys keep their meaning; no new key and no new claim table row.
- **The eight `tui_render` failures present at the baseline are pre-existing and out of scope** (same list as milestone 006 §4).

## 5. Scope

### In scope

- Put the row's workspace identity into the scope the team request is sent with.
- Make the in-panel sections mutually exclusive and make `j`/`k` stop moving an invisible selection.
- Give `team_loading` a terminal state on route leave and on a client failure.
- Tests that assert the request's identity with no session.

### Explicitly out of scope

- Changing the daemon's "needs a session" classification, the run-store root rule, or any authority check. If the daemon refuses even a fully named scope, this milestone stops.
- A loading watchdog or timeout. Add one only if a test demonstrates a future that never completes.
- Membership semantics (roles, revocation), the token secret modal, and the HTTP project routes.
- M007's chat binding, M009's lifecycle, M010's `/worktree`.
- The pre-existing eight failures.

## 6. Required production changes

### Frontend

1. `start_show_team`: set `workspace_id` on the scope from the revealed workspace (or from the selected row's `workspace_id`) alongside `project_id`. An unknown workspace stays empty so the daemon's typed refusal is preserved rather than guessed.
2. Section exclusivity: opening detail clears the team/reveal section; opening team or a reveal clears detail. Prefer one helper that both paths call so a third caller cannot reintroduce the hole.
3. Navigation: while a section is open, `j`/`k` either scroll that section or do nothing to the tree — they must not move a selection that is not painted. The section's own footer already advertises its keys (`h back · n new · e edit · s state · x destroy · t team`).
4. `team_loading`: clear it in `leave_workspace_view`, at the start of a new load, and on a client/transport failure.

### Documentation and static guards

`architecture/tui.md:3282-3342` gains the identity rule (a sessionless reveal names the row's project *and* workspace) and the exclusivity rule. No guard constrains these paths; `check_tui_transport_authority.py` and `check_frontend_action_set.py` must stay green.

## 7. Ordered work packages

### Work package A — Name the workspace

Intent: the list request is answerable without a session.

Required changes: the scope field; no new command, no direct request.

Acceptance evidence: a fake client records the team request carrying the row's project **and** workspace ids with `session_id: None`.

### Work package B — One visible section at a time

Intent: `t` while a detail is open shows membership; `j`/`k` cannot move an unseen cursor.

Required changes: the exclusivity helper and the navigation guard.

Acceptance evidence: `t` replaces an open detail; `j` with a section open leaves `view.selected` unchanged.

### Work package C — Loading terminates

Intent: the section cannot stay loading forever.

Required changes: the three clears.

Acceptance evidence: a failed load and a route leave both leave `team_loading == false`.

## 8. Failure, cancellation, restart, and contention semantics

A refused list paints its typed error and clears loading. Leaving the route mid-flight clears loading and drops the completion through the existing stale guard. Two rapid reveals must not interleave: the second load replaces the first's generation and only its answer applies (the existing stale check). No restart semantics are involved — nothing is persisted.

## 9. Compatibility and migration

No storage, protocol, config or DTO change: the scope field already exists and an absent workspace remains the empty string, which the daemon already refuses with a typed reason.

## 10. Required tests

### Focused unit tests

Using `test_app_with_tabs`'s fake `CoreClient` (`crates/codegg-tui/tests/tui_render/workspace_postclosure_m002_task_cancellation.inc:83-109`; `App::new_for_testing` leaves `session: None`, `crates/codegg-tui/src/app/mod.rs:1644-1648`):

1. `the_team_request_names_the_selected_rows_workspace` — scope assertion with no session.
2. `t_replaces_an_open_detail` — the painted section is membership.
3. `j_does_not_move_a_hidden_selection` — `view.selected` unchanged while a section is open.
4. `a_failed_team_load_clears_loading`.
5. `leaving_the_route_clears_loading`.

Each captures the request and asserts the identity it carries — not only that a request was made.

## 11. Required verification commands

```bash
cargo test -p codegg-tui --lib workspace_dashboard
cargo test -p codegg-tui --lib team
cargo test -p codegg-tui --test tui_render          # unfiltered; eight known reds only
scripts/verify.sh quick
scripts/verify.sh scoped 256b215ac
```

Live receipt (addendum §13.2): with no session and no provider configured, reveal the auto-registered project's workspace and capture the membership list. This is exactly the frame that failed with `run_store_unavailable`; it must now paint the member list.

## 12. Documentation updates

`architecture/tui.md` (the identity and exclusivity rules). `docs/tui.md` only if it states how membership is fetched.

## 13. Acceptance criteria

- A reveal with no session lists that workspace's members, and add-member still works.
- `t` while a detail is open shows the membership section.
- `j`/`k` with a section open do not move the tree selection.
- No surface can remain permanently loading after a failure or a route exit.
- The unfiltered family run adds no failure, and the live receipt shows the membership list.

## 14. Stop conditions

- Stop and report if the team catalogue command is classified as needing a session even when both ids are supplied: that is a daemon authority change and needs its own plan.
- Stop if the row's workspace id cannot be derived without a session.
- Do not add a timeout or watchdog under this plan; if a never-completing future is observed, report it with the reproduction.

## 15. Closure evidence required

`plans/closure/tui-workspace-central-surface-corrective/008-status.md` with: the unfiltered family run, the five tests and their results, the membership-list receipt path, and the recorded request scope.

## 16. Handoff notes

- `start_show_team` is the only place that must learn the workspace; do not add a second request path for the same list.
- The section fields are two `Option`s, not a mode. Keep it that way and enforce exclusivity at the two openers, not in the renderer.
- This plan and M007 both edit `workspace_view.rs`; run M007 first and rebase this plan's line anchors if they moved.
