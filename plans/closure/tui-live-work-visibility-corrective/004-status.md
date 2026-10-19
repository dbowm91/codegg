# TUI Live Work Visibility Corrective M004 — Closure Status

Status: **closed**
Date: 2026-10-19
Class: **capability**
Source implementation plan: `plans/implementation/tui-live-work-visibility-corrective/004-unified-session-work-model-and-visibility.md`
Source subsystem roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md#7-milestones`
Implementation commit: recorded in the registry after implementation commit

## Finding

`App::task_rows()` and `/sessions` now derive from one bounded selector over
the authorized session projection. Child generations, durable runs, jobs, and
tool programs are associated only by canonical IDs; live child/job associations
do not produce duplicate rows. The two TUI surfaces and the sidebar preserve
the task address separately from child-generation selection identity.

The production-facing Unix-socket regression attaches the ordinary TUI App,
submits two durable jobs, publishes two session-owned child lifecycle events
through the daemon event log, applies the authorized projection stream, then
asserts the painted `/sessions` clone and `Ctrl+T` overlay show both children
and both jobs. The test also verifies task ID remains the transcript address.

## Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| One ID-based session-work selector associates related child/run/job/program records | `session_work` unit tests | pass | Includes explicit linked/deduped and mixed legacy/new records; unrelated entities remain separate. |
| Production socket subscription and event publication reach both live-work surfaces | `default_tui_socket_path_installs_authorized_session_snapshot_and_live_job` | pass | Real SQLite daemon, Unix socket, `SocketCoreClient`, default App subscription, `CoreEvent` event-log publication, and projected stream application; no injected `SubagentRow` or prebuilt snapshot. |
| Two children and two separate jobs appear in the painted `/sessions` clone and overlay | Same socket integration test | pass | `TestBackend` paints both surfaces; assertions include child names, section headings, and stable child IDs. |
| Session switches do not retain foreign rows; absent projection is unavailable | `tui_sessions_pending_sections` and session authority tests | pass | Switch clears rows for a session without an authorized snapshot; loading/unavailable differs from authoritative empty. |
| Selection cannot slide to a different generation or entity during refresh | session dialog selection race and `child_generation_keys_selection_but_enter_uses_the_actual_task_id` | pass | Child generation is the row key; Enter retains the numeric worker task ID. Job selection is similarly invalidated after removal. |
| Overlay cap and omissions remain visible | `tui_main_buffer` overlay tests | pass | Bounded rows retain the omitted-count group heading behavior. |
| Sidebar and task surfaces use the same live-work truth | `tui_side_panels` and selector mapping tests | pass | The selector is the display source; render-side subagent cache is no longer required before `task_rows()` updates. |

## Verification executed

| Command | Result |
|---|---|
| `rtk cargo test -p codegg --lib default_tui_socket_path_installs_authorized_session_snapshot_and_live_job --locked` | pass (1) |
| `rtk cargo test -p codegg --lib session_work --locked` | pass (3) |
| `rtk cargo test -p codegg --lib child_generation_keys_selection_but_enter_uses_the_actual_task_id --locked` | pass (1) |
| `rtk cargo test -p codegg --test tui_sessions_pending_sections --locked` | pass (12) |
| `rtk cargo test -p codegg --test tui_main_buffer --locked` | pass (58) |
| `rtk cargo test -p codegg --test tui_side_panels --locked` | pass (31) |
| `rtk python3 scripts/check_tui_transport_authority.py` | pass |
| projection transport isolation/lifecycle guards | pass |
| `rtk bash scripts/verify.sh quick` | pass |
| `CARGO_BUILD_JOBS=2 rtk cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `rtk git diff --check` | pass |

Local evidence; no hosted CI or manual terminal receipt is claimed.

## Limits

The socket regression drives real daemon publication and projection delivery,
but constructs child lifecycle `CoreEvent`s as deterministic test inputs. It
does not run a scripted delegated worker from admission through the UI. That
combined producer trace, late attachment/restart matrix, and manual terminal
receipt remain in M006 and are recorded there as unqualified. The M004 UI and
row-derivation acceptance criteria are met without claiming the broader M006
runtime qualification.

## Unblock audit

M001–M003 were closed, so M004 became eligible and is now closed. This unblocked
M005. M006 became eligible after M005 closed; its conditional disposition and
remaining real-worker, recovery, and manual-terminal gates are recorded in
`006-status.md`.
