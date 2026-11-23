# TUI Panel Reachability Corrective Addendum

Status: closed

Audit date: 2026-11-22
Repository baseline: `be6d68b18`.
The panels were exercised end to end with `tui-test` against a real daemon and
a real provider before any of these fixes.

Predecessor, not reopened:
- `plans/subsystems/tui-editor-ide-presence-roadmap.md` and the closed terminal,
  panel-surface and subagent-visibility lines. This addendum owns panel
  *reachability*: whether the gesture that selects a panel loads it, and whether
  a panel works before a turn has been sent.

## 1. Purpose and ownership boundary

A side panel must work without starting a session. Selecting one with
`Shift+`directional must **load** it — the same act a manager commit performs —
rather than tell the user to go and load it somewhere else.

Owns: the walk/selection loader, the chat panel's project binding and channel
load, the configured placement's effect on the visible edge, the human
interactive PTY's profile on a platform without a constrained backend, and the
command executor's routing of pre-routed request families.

Does not own: panel registry contents, the box/keymap contract, the agent shell
surfaces (`bash`/`terminal` on `ManagedProcessService`), or Linux PTY
containment.

## 2. Findings

**F1 — the walk bypassed the loader.** `step_panel` called
`panel_host.step`, which only changes the slot. `select_panel` — the seam the
manager, saved hotkeys, `dock_open_box` and `focus_side_panel` all use — is what
materializes a hosted panel and readies the terminal. Walking therefore landed
on "Nothing loaded. Open it from the panel manager (⇧S)."

**F2 — Chat was unbound on a fresh session.** `resolve_chat_project` read the
workspace-dashboard selection and the active tab's `project_id`. A session the
daemon created carries its project from creation, but the tab surface is
populated later, so before the first turn the panel reported "Chat unavailable".

**F3 — binding was not loading.** Even bound, the panel rendered "Chat
unavailable" because nothing ever asked for its channel list; only `/chat` did.

**F4 — a viewless Chat consumed every key.** `handle_key` returned
`Consumed` when it had no view, so a focused panel swallowed `i` and stranded
the composer. A viewer with nothing to view now passes through.

**F5 — a configured placement never moved the frame.** `with_placement`
repointed `selected` without moving the visible edge, so with
`panel_placement: "left"` the frame drew the right column while every selection
occupied the collapsed left one. The first panel opened appeared in a second
column beside the session panel.

**F6 — the human terminal could not start on macOS.** The daemon resolved the
PTY profile from the stored sandbox preference (`workspace_write`), and the
constrained PTY owner is Linux-only, so every spawn was refused. The panel still
offered "press i for a shell".

**F8 — Chat advertised a verb it could not receive.** The panel renders
"press i to write", but the walk left the host unfocused, so `i` fell through to
the app's insert verb and armed the *agent* composer; the badge read
`[build][LLM]` while the panel asked for a message.

**F9 — the `/files` box survived opening a document.** The tree's editor
handoff blurred the panel and put the editor in Normal, but never closed the
modal it was opened from, so the box stayed mounted on top of the file it had
just opened.

**F10 — the human shell is the operator's own terminal.** It must open as an
interactive shell awaiting input (INS, not "press i"), its keystrokes must reach
the PTY unfiltered, and it must not be held to the turn's sandbox profile: the
interactive PTY is an IDE terminal, not an agent effect. Agent shells stay on
`bash`/`terminal` over `ManagedProcessService` with their resolved profile.

**F7 — command effects skipped the pre-routers.** `dispatch_command_effect`
dispatched an inner request by family, which bypasses the Chat/Team/Interactive/
HumanShell/WorkOrders pre-routers. `terminal_attach` therefore returned
`unimplemented`, so a created shell never attached.

## 3. Evidence after the fixes

- Files loads from `Shift+↑` (one gesture from the default Session panel) and
  replaces the column it lands on.
- Chat renders `#general 0/0` and "no messages — press i to write" on a fresh
  session.
- Terminal attaches a live `bash-3.2$` prompt and executes
  `echo $((40+2))` → `42` through the panel.
- `cargo test -p codegg-tui --test tui_render` — 1252 passed.
- `cargo test -p codegg-tui --lib` — 3208 passed.
- `cargo test -p codegg --lib daemon_commands` — 20 passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean.
- `scripts/verify.sh quick` — passed.

## 3a. Open findings for the next corrective

These were found while exercising the surfaces with `tui-test` and are **not**
fixed by this milestone. They are recorded so the next pass starts from
evidence rather than from a fresh survey.

**F11 — a fresh session shows error boxes from an earlier one.** Every launch
paints the transcript area with `ERROR` boxes carrying old daemon text (a
research-report validation failure and a `wire_encode` failure from October)
while the buffer says "No messages yet". The text matches historical daemon log
entries and no such request is issued at launch, so these are replayed or
retained `CoreEvent::Error` entries that a new session does not clear.
`new_session` clears `messages_state.messages`; whatever holds these boxes is a
different container or a restored view, and finding it is the next step.

**F12 — walking past Terminal spawns a shell.** Stepping *through* Terminal (on
the way to Help or Tree) starts a PTY because a step is a selection and a
selection loads. Acceptable while the terminal is the only panel whose load is a
process, but a walk that lands on three panels should not leave a shell running
for each visit. A "load on settle" rule would need a bounded dwell, which is
worse than the current honesty; leaving it, and recording the cost, is the
decision for now.

**F13 — `/editor` with no recent file only explains itself.** The palette entry
opens "No recently edited files"; the natural next step (open the file manager)
is not offered. A hint, not a defect, but it is the first thing a new user hits
when they reach for the editor.

## 4. Non-goals

No new panel kind, no second loader, no containment change on Linux, no change
to the manager's keymap, and no attempt to constrain the human shell where no
backend exists (the daemon instead stops promising it).

## 5. Milestone

| Milestone | Scope | Status |
|---|---|---|
| C001 | Panels load and work without a session | closed; `plans/closure/tui-panel-reachability-corrective/001-status.md` |
