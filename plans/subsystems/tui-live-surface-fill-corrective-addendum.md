# TUI Live Surface Fill Corrective

Status: conditionally closed — M001–M003 implemented; live receipts outstanding

Long-term references:
- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`

Related ADRs:
- None required. The daemon already owns the diff read and the session projection. This workstream does not add a transport, a protocol variant, or a second source of subagent truth.

## 1. Purpose and ownership boundary

A real TUI still fails three user-visible cases after the closed panel, terminal, and subagent workstreams. Selecting the Terminal panel does not leave a shell ready for input, and `i` does not take the keyboard. The Diff panel never paints a diff. Subagents that exist do not appear on the Session panel, the sessions dialog, or `Ctrl+T`.

This workstream owns only those three fill failures. It does not reopen the closed predecessors, and it does not treat "the function exists" or "the same arm exists on `origin/main`" as acceptance.

## 2. Work classification

### Invariants
- A frontend paints only what an authorized projection or a daemon command result vouches for. It does not invent a subagent, a diff, or a local git read.
- A panel the user is looking at shows the outcome of the load it started: ready, empty, refused, or failed. It does not stay on a loading placeholder after the answer has arrived.

### Capabilities
- The Terminal panel is input-ready when it is selected.
- The Diff panel displays the daemon's answer.
- A live subagent appears on every named surface.

### Infrastructure
- None. Existing spawn, attach, diff, and projection machinery is the substrate. A new helper that nothing paints is not completion.

### Polish
- Hint wording may change only where it would otherwise describe a gesture that no longer matches the load behavior.

## 3. Non-goals

- No ADR-0064 change, no restored root `codegg` binary, and no edit to `scripts/annotate_nextest_junit.py`, `scripts/iter_selection.py`, or `stash@{0}`.
- No new terminal transport. The closed WS-attach refusal stays the refusal.
- No second diff implementation and no frontend `git` spawn.
- No synthesized subagent row for a child the authorized snapshot does not contain.
- Tree, the hosted "open it from the panel manager" placeholder, and the `Shift+H` dock-only versus `Enter` select split are out of scope except where one of those paths is the mechanism of a named failure here.
- No claim that `origin/main` is the oracle. Presence of the same arm there is not evidence the live TUI works.

## 4. Current state

Repository baseline `d319afec15b8b8b33ced2c0af7a3ee50eb1a82c3`. The TUI on this branch is `crates/codegg-tui`. No live TUI session was captured, so the projection and controller state at the user's failure are unverified. This section records **what the baseline code did** on the paths the user named; the milestones below record what landed and each closure record carries the evidence.

**Terminal.** `SidePanelId::Terminal` is bespoke. Selecting it does not spawn a shell and does not focus a PTY. `TerminalPanel::on_activate` only adopts an already-active handle. `i` reaches the panel only when the panel already owns the keyboard. `claims_insert_keys` is true only when the controller already says the handle is focused, so an unfocused terminal does not take Insert-mode keys. A successful panel spawn calls `show_terminal` and does not call `InteractiveTerminalController::focus`. `focus` itself returns false unless the link allows mutation and an attachment id is set. A refused focus re-attaches without the panel-spawn bit, so that completion opens the terminal box instead of taking the panel keyboard. The required behavior is the one that used to work: the terminal is open and ready for input as soon as it loads.

**Diff.** The panel arm starts a bare daemon diff and records Loading. Every completion leaves that placeholder in place. A transport error, a typed refusal, an unexpected response, and equal old/new content only toast. A successful `GitDiff` enqueues a modal `Dialog::Diff` and never adopts it into the panel. The bare daemon branch returns an empty old side and the unified `git diff` text as the new side, so a clean tree compares empty with empty, toasts "No changes detected", and the panel stays Loading. A dirty tree can open a modal that is not the panel the user selected. The closed panel-surface record already noted that Diff reaches a placeholder on the ordinary clean-tree path; that closure did not require the panel to paint the answer.

**Subagents.** The Session panel, the sessions dialog, and `Ctrl+T` all derive rows from `select_session_work` on `active_snapshot()`. A missing snapshot becomes an empty vector for `Ctrl+T`. The sessions dialog and `Ctrl+T` then keep only `SubagentRow::is_running` rows. That predicate treats an unrecognized non-empty status as not running, while the job predicate's comment claims the opposite and claims the two rules match. Durable run status overwrites the child status before that filter. The sidebar omits the Subagents section when the derived vector is empty. `select_session_work` reads `snapshot.subagents`, the active turn, and only the newest recent turn. A controlled socket test can populate `snapshot.subagents`; that does not prove the snapshot a manual TUI holds while a child is live contains the child. `tui-live-work-visibility-corrective` M006 closed conditionally and left manual terminal receipt as a re-entry condition.

## 5. Target architecture

No new surface. Each milestone makes the existing surface show the outcome of the load the user already performed.

- Selecting the Terminal panel starts the default shell when none is live, and a successful attach focuses the controller so the next key is PTY input.
- The Diff panel paints the daemon answer in that panel: a diff, an explicit empty state, or a typed refusal. Loading does not outlive the completion.
- A live child that the authorized snapshot contains is painted on the Session panel, the sessions dialog, and `Ctrl+T`. A snapshot that does not contain a child the user can otherwise observe is a publication defect, reported rather than papered over.

## 6. Dependency graph

The three milestones are independent. No hard or interface dependency exists between them. Each may be handed off alone.

- Hard: none. The closed terminal, panel, and projection contracts are the substrate, not work this line reopens.
- Interface: daemon diff command and session projection, both already shipped.
- Soft: none.
- Operational: a manual or socket-attached TUI receipt is required for closure of each milestone. A unit test that calls the function directly is not that receipt.

## 7. Milestones

### M001 — Terminal panel is input-ready on load
Class: capability. Dependencies: none. Deliverable: selecting the Terminal panel leaves a live shell focused for input, or a visible typed refusal. Exit: the regression fails on this baseline and passes after the change; a key after a successful load is queued as PTY input.

### M002 — Diff panel paints the daemon answer
Class: capability. Dependencies: none. Deliverable: the Diff panel the user selected shows the diff, an explicit empty state, or the refusal. Exit: Loading does not survive completion; a clean tree and a dirty tree are both visible in the panel.

### M003 — Live subagents reach the named surfaces
Class: capability. Dependencies: none. Deliverable: a live child in the authorized snapshot appears on the Session panel, the sessions dialog, and `Ctrl+T`. Exit: the hiding filter is gone or justified against the producer's real status token, and a snapshot that lacks the child stops the milestone instead of being painted around.

## 8. Cross-cutting requirements

- Storage: none. No migration and no `STORAGE_LAYOUT_VERSION` bump.
- Protocol: none unless M003's stop condition finds the child missing from the snapshot. That finding is reported; it is not fixed by a silent TUI field.
- Security: terminal input still goes through the existing attach and input path. Diff remains a daemon read. No new writer.
- Concurrency: a second Terminal select while a create is in flight must not orphan a shell. Diff completion must not open a dialog for a panel the user has since left without clearing the placeholder it set.
- Observability: a failed spawn, attach, or diff is a toast or an in-panel refusal, not a silent return.
- Docs: `docs/tui.md` only if a named gesture changes. No canonical long-term edit.

## 9. Verification strategy

Each milestone's new test must fail on this baseline before the production change. Closure requires the focused TUI target plus `scripts/verify.sh scoped`. A green unit test that constructs the success state by hand is not acceptance. Manual or socket-attached receipt is named in each plan and is part of closure, not a follow-up.

## 10. Risks and decision points

- If panel-spawn attach cannot focus because the link is not yet mutable, the milestone waits for that completion and focuses then. It does not focus a handle that cannot take input and call that ready.
- If the bare diff text is not a side-by-side file diff, M002 renders that text in the panel. It does not add a second git read to manufacture old/new file bodies.
- If a live child is absent from the authorized snapshot, M003 stops. Synthesizing the row in the TUI is the prohibited choice.

## 11. Completion definition

All three milestones are closed, each with a closure record, and a manual or socket-attached TUI shows: a selected Terminal panel accepting input without a second `i`; a Diff panel that shows a diff or an explicit empty/refusal state; and a live subagent on the Session panel, the sessions dialog, and `Ctrl+T`.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | conditionally closed | `plans/implementation/tui-live-surface-fill-corrective/001-terminal-input-ready-on-load.md` | `plans/closure/tui-live-surface-fill-corrective/001-status.md` | live keystroke receipt |
| 2 | conditionally closed | `plans/implementation/tui-live-surface-fill-corrective/002-diff-panel-paints-the-daemon-answer.md` | `plans/closure/tui-live-surface-fill-corrective/002-status.md` | live dirty/clean tree receipt |
| 3 | conditionally closed | `plans/implementation/tui-live-surface-fill-corrective/003-live-subagents-reach-named-surfaces.md` | `plans/closure/tui-live-surface-fill-corrective/003-status.md` | live subagent receipt; M003 stop condition undischarged |
