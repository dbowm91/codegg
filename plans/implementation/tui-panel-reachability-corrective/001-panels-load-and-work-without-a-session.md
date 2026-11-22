# TUI Panel Reachability C001 — Panels Load and Work Without a Session

Status: closed
Repository baseline: `be6d68b18`

Source roadmap: `plans/subsystems/tui-panel-reachability-corrective-addendum.md`
Primary class: invariant

## 1. Objective

Selecting a panel with `Shift+`directional loads it, and Terminal, Chat and
Files work on a fresh session before any turn is sent.

## 2. Current implementation evidence (at the baseline)

- `src/commands/panels.rs` `step_panel` called `panel_host.step`; `select_panel`
  is the loader.
- `resolve_chat_project` read the dashboard selection and `active_project_id()`.
  A fresh session's tab has no `project_id`; the daemon session does.
- `panels/chat.rs` `handle_key` returned `Consumed` without a view.
- `panels/mod.rs` `with_placement` only called `select_placement`.
- `src/core/daemon.rs` PTY create resolved the profile from
  `runtime_preferences.sandbox_profile`; `build_m004_pty_spec` refuses any
  constrained profile off Linux.
- `src/core/daemon_commands.rs` `dispatch_command_effect` used
  `dispatch_family_request` directly.

## 3. Scope

In: `step_panel` → `select_panel`; the session-project chat fallback and the
channel load on selection (skipped while the Workspace view owns the route);
`PassThrough` for a viewless Chat; `with_placement` moving the visible edge;
`human_pty_profile` + `constrained_pty_supported` and the daemon clamp;
command effects re-entering the pre-router path for pre-routed families; the
unhandled-variant warning naming the variant.

Out: new panel kinds, a second loader, Linux containment changes, frontend
option-surface changes.

## 4. Acceptance

1. `Shift+↑` from Session loads Files and replaces the column.
2. Chat binds and loads on a fresh session.
3. Terminal attaches a live shell on macOS under the default profile.
4. A command effect of a pre-routed family is served, not `unimplemented`.
