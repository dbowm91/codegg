# Remaining-work review — TUI workspace and panel surface corrective

Date: 2026-11-26. Branch `feat/auto-20261126-cae46b00`, head `700d2e8fd` plus the
uncommitted M005 keymap fix and the M001 acceptance-test extension described below.

This is a review of what still stands between the addendum and closure. It is **not** a
closure record. Every "verified" line below was observed on the live TUI through
`tui-test` in an isolated HOME/data home; every "blocked" line names the exact refusal
that stopped it.

## Verdict

The twelve milestones have code, but the addendum cannot be closed:

- **M007 (Workspace whole view) and M009 (Workspace picker) are verified live.**
- **M005's list controls were not wired at all**; they are now fixed and verified live
  (`w`, `e`, `C`, `W`) on a rebuilt binary, with a regression test for the painted clone.
- **M003's aggregate Diff surface, M008's team action, M004's reasoning-level path and the
  Terminal panel in a draft state are not demonstrated.**
- **M011 cannot be verified on this platform**: a fresh project has no repository identity
  without a remote, and a new checkout fails at containment with
  `protected mount view is available only on Linux`. M012 depends on M011.
- No per-milestone closure records exist, and `scripts/verify.sh full` has not run.

## Per-milestone status

| Milestone | Code | Automated test | Live evidence | Verdict |
|---|---|---|---|---|
| M001 | yes | real-CLI zero-row test extended and passing; mutation check fails as intended | first launch: "no session", `/sessions` "no sessions loaded" (`m001-first-launch.svg`) | verified |
| M002 | yes | panel-fill tests exist | idle body captured for Logs/Tasks; Terminal refuses in a draft | partial / blocked |
| M003 | yes | splitter tests exist; no browser test | `/diff` aggregate worked in one env, fails "unexpected response" in a clean draft; no per-file list | defective |
| M004 | yes | one-CAS store test | model selection persists (checkmark); reasoning level not visible/reachable | partial |
| M005 | yes, plus uncommitted fix | 35 focused tests pass, painted-clone regression added | `w`/`e`/`C`/`W` all respond (`m005-sessions-controls.svg`) | verified |
| M006 | yes | `duplicate_project_commands_are_gone` | not re-driven live this pass | verified by test/guard |
| M007 | yes | route/focus tests | filter, detail, NOR `Shift+X`, INS `X` (`m007-*.svg`) | verified |
| M008 | yes | six lifecycle tests | detail + membership; `s` arms; `x` shows a picker error; `t` silent | partial |
| M009 | yes | panel catalog tests | Workspace and Sessions panels live (`m009-*.svg`) | verified |
| M010 | yes | keymap/render tests | sections render; `run_store_unavailable`; Ctrl+T overlay works | partial / blocked |
| M011 | yes | none | launch sheet 3 steps live; checkout refused (repository identity / macOS containment) | blocked |
| M012 | yes | lineage-grouping test | none: needs a managed checkout | blocked by M011 |

## Defects found

### 1. M005 list controls were wired into a dead router (fixed in working tree)

Live: pressing `w` in `/sessions` launched a session ("Untitled") and left
`scope: this workspace` unchanged; `e` typed `e` into the filter; `C` did the same.
Source: the handlers live in `App::handle_dialog_key`'s `#[cfg(any())]` legacy twin
(`crates/codegg-tui/src/app/mod.rs:4748+`); the live authority is
`FocusManager` -> `SessionDialog::handle_key`, which bound `w` to
`TuiMsg::LaunchSession` and had no scope/empty/cleanup binding.

Working-tree fix: three `TuiMsg` intents, dialog bindings before the filter catch-all,
dispatch to the existing app methods, checkout reuse moved to `W`, footer updated.

The first live re-verify exposed a second layer: the App toggled its owned
`session_dialog`, but the painted dialog is the clone pushed onto the focus manager, so the
header did not move. `toggle_session_scope` / `toggle_session_show_empty` now mutate the
focused clone through `FocusManager::with_dialog_mut` and mirror the result back.
Regression test `scope_and_empty_toggles_reach_the_painted_dialog`
(`crates/codegg-tui/src/commands/sessions.rs`). Focused suite: 35 passed, 0 failed.

Live, on the rebuilt binary: `w` flips "this workspace" → "whole project"; `e` adds
"· incl. empty"; `C` answers "No empty sessions to delete"; `W` answers the
checkout-reuse refusal. Receipt: `m005-sessions-controls.svg`.

### 2. M001 has no zero-row acceptance test

The existing project-identity test checked id reuse only; `load_initial_session_new_creates_session`
asserts the opposite path. `plain_launch_registers_the_launch_directory_as_a_project`
(`tests/core_daemon/single_daemon_lifecycle.inc`) now launches the real CLI twice against
the same isolated homes and asserts: one workspace, one project, one binding, zero durable
`session` rows before a prompt, and the same ids on relaunch. It passes, and it fails as
intended (0 workspaces) when the launch bootstrap is bypassed.

### 3. The draft state lies on session-scoped surfaces

Observed on first launch in a real repo with no session:

- Sidebar Git section: `not a git repo` (the probe only starts when a session exists or
  is selected; `crates/codegg-tui/src/commands/git_sidebar.rs:103`).
- Terminal panel: `No active session; open a project session before creating a terminal`;
  `i` does not enter INS, so M002's required first-launch receipt cannot be produced.
- Runtime policy: `Policy preference read failed (run_store_unavailable): the command's
  workspace could not be resolved, so its run cannot be recorded` when a session is
  created (`/tasks`, terminal, task view all surface it).

This is the §5.5 "surfaces answer truthfully in the draft state" requirement, not a
cosmetic issue.

### 4. M003's aggregate Diff surface is not usable in a clean draft

- The Diff **panel** and bare `/diff` both end in
  `The diff returned an unexpected response` / body `unexpected response` in a fresh
  isolated environment (repeated twice, panel and dialog).
- The one successful `/diff` capture showed a combined patch with both files, but **no
  file list and no per-file selection**; Down/Enter closed the overlay.
- `/diff <path>` works (`Diff: one.txt`, only that file's change).

The panel's `apply_diff_finished` only accepts `CoreResponse::GitDiff`
(`crates/codegg-tui/src/commands/diff.rs:101`); anything else — including a wrapped
refusal — becomes "unexpected response", so the real refusal is hidden.

### 5. M008's detail actions are incomplete

- `s` works: `press s again to archive repo`.
- `x` on an active project opens the project picker with the error
  `Project is no longer inactive; refresh and try again` instead of a clear refusal or a
  counted confirmation.
- `t` produced no visible change in the detail (membership line present).
- `e`/`a` not exercised.

### 6. M011's launch cannot complete on this host

Two independent refusals:

1. A project registered from a bare `git init` has `repository_id = NULL`; the launch
   handler refuses with `project_context_required: project has no resolved repository to
   launch from` (`src/core/daemon_sessions.rs:716`). Repository lineage requires remote
   evidence (`ProjectCatalog::probe_repository_lineage`), so a local-only repo can never
   launch.
2. With a remote-backed repo the wizard reaches checkout creation and fails with
   `session launch checkout failed: ... the worktree child could not be contained:
   protected mount view is available only on Linux`.

M012's live receipt needs a managed checkout, so it inherits the blocker.

### 7. M004's reasoning level is not demonstrable

The model dialog rows render `—` for reasoning; `l` (documented cycle) and Tab produced
no visible level change, and the status line stayed `reasoning:default`. The one-CAS write
is covered by `model_and_reasoning_are_written_in_one_revision_bump`, but the live
"model + level" receipt is still missing. Re-derive whether this is the dialog's capability
projection or the key path before closing M004.

## Receipts captured this pass

Under `plans/closure/tui-workspace-and-panel-surface-corrective/receipts/`:

- `m001-first-launch.svg` — first launch, no session.
- `m005-sessions-controls.svg` — scope + empty toggles live.
- `m007-workspace-filter.svg`, `m007-workspace-detail.svg`, `m007-workspace-ins-x.svg`.
- `m009-panel-manager.svg`, `m009-workspace-panel.svg`, `m009-sessions-panel.svg`.
- `m010-task-view.svg`, `m010-task-overlay.svg`.
- `m002-launch-idle.svg`, `m002-terminal-nor.svg` (the INS capture is a failure frame),
  `m002-panel-walk.svg`, `m003-diff-aggregate.svg`, `m003-diff-file.svg`,
  `m003-diff-single.svg`, `m011-launch-sheet.svg`.

The M002/M009/M010 worker reports are beside them as
`m002-m003-receipt.md` and `m009-m010-receipt.md`.

## Verification run this pass

- `cargo test -p codegg-tui --lib -- scope_and_empty_toggles_reach_the_painted_dialog components::dialogs::session`
  — 35 passed, 0 failed.
- `cargo test -p codegg --test core_daemon -- single_daemon_lifecycle::plain_launch_registers_the_launch_directory_as_a_project --exact`
  — 1 passed; the mutation check (bootstrap bypassed) failed with 0 workspaces.
- `python3 scripts/check_test_assertion_quality.py` and `check_test_duplication.py` — passed.
- `scripts/verify.sh quick` — **passed** on the working tree (after re-banking the comment
  budget by the three comment lines this change adds).
- `scripts/verify.sh full` — **not run**.
- Everything is uncommitted.

## Outstanding work, in order

1. Decide the draft-state policy: fix the Git probe, the Terminal refusal, and the
   `run_store_unavailable` path, or record an explicit limitation.
2. Fix or bound the Diff panel: surface the daemon refusal, then show the file index and
   one patch at a time.
3. Resolve M011: repository identity for local-only repos, and a macOS decision for the
   containment refusal (qualify a backend, or gate the capability).
4. Re-drive M004's level path and M008's `t`/`x`.
5. `scripts/verify.sh full` on the final head.
6. Commit the working-tree fixes (M005 keymap + painted clone, M001 zero-row test).
7. Per-milestone closure records, or one closure record that names each milestone's
   receipt and its limits.

## Decisions the owner still owns

- The `w` collision: the plan's §14 says `w` toggles scope; M006's launch hint said `w`
  reuses a checkout. The working tree follows the plan (`w` scope, `W` reuse) and the
  footer is updated. Say if the launch key should stay on `w`.
- M011 on macOS: qualify a containment backend, or ship the capability as Linux-only and
  mark the milestone conditionally closed.
- Whether local-only repositories should obtain a repository identity at registration.
