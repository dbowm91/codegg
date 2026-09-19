# Team Collaboration Post-Closure Corrective M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/team-collaboration-post-closure-corrective/002-workspace-task-cancellation-ownership.md`

Source subsystem roadmap:

- `plans/subsystems/team-collaboration-post-closure-corrective-addendum.md#m002--workspace-owned-task-cancellation`

Repository baseline reviewed: `06d92a17`

Implementation commits or pull requests:

- `06d92a17` — feat(tui): workspace-owned task cancellation (team-collaboration post-closure M002)

## 1. Executive finding

M002 is complete. Leaving the non-modal `Route::Workspace` view no
longer aborts unrelated generic `TuiTaskKind::Command` tasks: both
close paths (`leave_workspace_view` and the legacy
`Dialog::WorkspaceDashboard` teardown) now cancel only the new
dedicated `TuiTaskKind::Workspace` kind, which is spawned solely by the
Workspace dashboard refresh and inline-expansion fetches. Stale
generation/request/reconnect fencing is untouched and remains the
correctness boundary; per-project chat drafts/cache survive the close;
no daemon turn/job cancellation was introduced. `M003` is now
dependency-ready (M001 closed, M002 closed) and moves to `ready` in
this same commit.

## 2. Requirement-to-evidence matrix

| Plan requirement (§6) | Evidence | Result | Notes |
|---|---|---|---|
| A. `leave_workspace_view` must not call `cancel_kind(Command)`; cancel a dedicated Workspace kind (preferred) | `src/tui/commands/workspace_dashboard.rs::leave_workspace_view` calls `cancel_kind(TuiTaskKind::Workspace)`; legacy `Dialog::WorkspaceDashboard` arm in `src/tui/app/mod.rs` likewise | pass | Both close paths fixed; no other Workspace-close path exists |
| A. Only genuinely Workspace-owned spawns use the new kind; no mechanical relabel of chat/team/WorkOrder ops | Only `start_dashboard_refresh` (`workspace_dashboard_refresh`) and `toggle_dashboard_expand` (`workspace_dashboard_expand`) spawn `TuiTaskKind::Workspace`; all chat/team/control/provider/session/WorkOrder spawns remain `Command` | pass | Verified by diff: 2 spawn sites changed, 0 outside `workspace_dashboard.rs` |
| A. No tab/session cancellation proxy | No `cancel_for_tab`/`cancel_for_session` added to any Workspace path | pass | Pinned by `scoped_and_shutdown_cancellation_still_work` |
| B. Generation/request/reconnect fencing preserved; racing completion observes dropped state and does nothing | `apply_dashboard_loaded`/`apply_dashboard_expanded` guards untouched; close still bumps generation, cancels the `AsyncUiRequestState`, clears route/panel binding | pass | Pinned by M005 suite + new stale-drop test |
| C. Chat stays independent: panel binding clears, drafts/cache persist | `leave_workspace_view` clears `chat_panel_project` only; `ChatState` untouched | pass | Pinned by `chat_drafts_survive_workspace_close` |
| D. `/tui-stats` and kind display account for the new kind without brittle counts | `TuiTaskRegistry::summary()` groups generically by kind; `Display` + `kind_display` test cover `Workspace` | pass | No hard-coded kind inventory exists outside the enum itself |
| Work package A: regression test (unrelated Command + Workspace task, close, survival + precise accounting proofs) | `tests/workspace_postclosure_m002_task_cancellation.rs::unrelated_command_tasks_survive_workspace_close` | pass | Fails on pre-fix baseline (see §4) |
| Work package A: chat/draft independence test | `chat_drafts_survive_workspace_close` | pass | — |
| Work package B: smallest precise seam | Dedicated kind on 2 spawns + 2 close-path cancels; registry untouched | pass | No stop condition triggered |
| Work package C: M005 behavior re-run | `workspace_m005_selected_project_chat` 9/9, routing/tabs suites green | pass | See §4 |
| Docs: `architecture/tui.md` + tui skill ownership rule | Kind list + "Workspace cancellation ownership" paragraph; skill row documents cancel-only-Workspace + fencing boundary | pass | — |

## 3. Production implementation evidence

- `src/tui/task_lifecycle.rs`: new `TuiTaskKind::Workspace` variant
  (documented as view-owned dashboard refresh/expand fetches cancelled
  on `Route::Workspace` close) + `Display` arm + `kind_display`
  assertion. Registry mechanics (`cancel_kind`, accounting,
  `cancel_for_tab`/`cancel_for_session`/`cancel_all`, `summary`)
  unchanged.
- `src/tui/commands/workspace_dashboard.rs`:
  `leave_workspace_view` cancels `TuiTaskKind::Workspace` (was
  `Command`); `start_dashboard_refresh` and `toggle_dashboard_expand`
  spawn `TuiTaskKind::Workspace` (were `Command`); close doc comment
  states unrelated `Command` tasks keep running. Generation bump,
  `request.cancel()`, route-history navigation, and panel-binding
  clearing unchanged.
- `src/tui/app/mod.rs`: legacy `Dialog::WorkspaceDashboard` teardown
  arm cancels `TuiTaskKind::Workspace` (was `Command`) with an updated
  comment. All other dialog teardown arms (`Tree`, `Import`,
  `Session`, `Connect`, `TaskSchedule`, `TaskView`) intentionally
  unchanged — they own their dialogs' `Command` work and are out of
  scope per plan §5.
- Tests: new `tests/workspace_postclosure_m002_task_cancellation.rs`
  (4 tests, auto-discovered default-features target matching plan
  §10 naming; no `Cargo.toml` entry needed, consistent with
  `workspace_m005_selected_project_chat`).
- Docs: `architecture/tui.md` (kind list + ownership paragraph);
  `.opencode/skills/tui/SKILL.md` (workspace row ownership rule).
- Deliberately not built: no registry redesign, no daemon/protocol
  cancellation change, no route-ephemeral chat, no project/tab
  ownership change, no mass relabel of `Command` spawns. No stop
  condition triggered.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test --test workspace_postclosure_m002_task_cancellation -- --test-threads=1
cargo test --test workspace_m005_selected_project_chat -- --test-threads=1
cargo test -p codegg --lib tui::task_lifecycle -- --test-threads=1
cargo test -p codegg --lib tui::commands::workspace_dashboard -- --test-threads=1
cargo test --test tui_project_routing -- --test-threads=1
cargo test --test tui_project_tabs -- --test-threads=1
python3 scripts/check_tui_project_authority.py
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

### Results

- `cargo fmt --all -- --check`: pass (after one `cargo fmt` for the
  new test file's import wrapping).
- New `workspace_postclosure_m002_task_cancellation`: 4/4 pass
  (`unrelated_command_tasks_survive_workspace_close`,
  `workspace_owned_work_cancelled_and_completion_stale_dropped`,
  `chat_drafts_survive_workspace_close`,
  `scoped_and_shutdown_cancellation_still_work`).
- Existing `workspace_m005_selected_project_chat`: 9/9 pass.
- `tui::task_lifecycle` lib: 22/22 pass (incl. updated
  `kind_display` and deterministic summary ordering).
- `tui::commands::workspace_dashboard` lib: 10/10 pass.
- `tui_project_routing`: 27/27 pass; `tui_project_tabs`: 20/20
  pass.
- `check_tui_project_authority.py`: pass; 
...[truncated 3013 chars]