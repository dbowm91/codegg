# TUI Live Work Visibility M004 — Unified Session Work Model and Both Entry Points

Status: **blocked — M001, M002 and M003 must close**
Primary class: capability
Repository review baseline: `61cbe9222c442c443cc7dfb3bd3a5b7d99ecd649` (`main`, 2026-10-19; re-pin)
Source roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md` M004.
Long-term: `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`, `#25-tui-target-behavior`, `#28-observability`; `plans/001-terminology-and-domain-model.md`.
Applicable accepted decisions: ADR-0041 (human interaction), ADR-0031 (idle continuation); proposed `plans/adrs/ADR-0042-session-owned-subagent-lifecycle-projection.md` must be resolved through M003.
Original closure paths: `plans/closure/tui-subagent-and-job-visibility-convergence/001-status.md`, `002-status.md`, `003-status.md`, `004-status.md`. Those tests seeded rows directly and did not demonstrate runtime liveness.

## 1. Objective and dependencies

The **same authoritative session work** must appear in the normal `Ctrl+T` overlay and the actual `/sessions` dialog, plus applicable sidebar surfaces, regardless of which worker representation emits it. Present a bounded, deduplicated view of active subagent executions, durable agent runs, scheduler jobs and long-running tool programs; preserve the user-visible source identity and status. Hard prerequisites: verified projection transport (M001), job lifecycle/snapshot (M002), and child lifespan contract (M003); do not substitute temporary frontend state if those are blocked.

## 2. Current implementation evidence

`src/tui/app/mod.rs::task_rows` builds from `ui_state.subagents.rows` and `active_snapshot().jobs`, not `snapshot.agent_runs` or `snapshot.tool_programs`. `src/tui/app/render.rs::sync_subagents_from_snapshot` only reads the active or latest recent turn, while sidebar already consumes `agent_runs`, `tool_programs` and `jobs`. `session_dialog_pending_rows` derives from `task_rows` but is currently scoped to the active snapshot, even though `/sessions` is a list of sessions. `Ctrl+T` has a bounded-row overlay and different Editor-route behavior (`Ctrl+T` opens file tree); preserve that route distinction. `/sessions` paints a cloned `SessionDialog` in `FocusManager`, not only the `dialog_state` backing field.

## 3. Invariants / non-goals

- Every visible work row originates in exactly one authoritative projected entity or an explicitly deduplicated relation of them. No client-side invented identity, status or display name. Membership is scoped to session; no foreign-session rows under current session.
- A child with both `AgentRun` and `Job` and session subagent node is **one logical execution**, with accessible secondary identities; different jobs under one run are not conflated. Join only on canonical persisted `job_id`, `run_id`, `program_id` or accepted `child_instance_id`, never on label or decimal/UUID task ids.
- Liveness reflects normalized status, is tolerant of unknown nonterminal states, and honors authoritative terminal/recovery. An absent projection is *unavailable*, not empty.
- Bounded work overview, stable selection identity under reordered/terminal rows, no UI mutation of scheduler state, no new blanket keybind, no movement of editor's file-tree chord.
- Out: job lifecycle publication (M002), child-identity schema (M003), job/detail interaction (M005), direct steering, generalized multi-frontend dashboard rewrite.

## 4. Required production changes

A. Define one bounded, readonly `SessionWorkRow` (or equivalent) and selector operating on a **specific authoritative session projection**. Include stable logical key, entity kind, source ids, parent relation if known, status/phase, title/description, display name/descriptor and available action metadata. Keep `Todo` separate: todo entries are agent-authored intentions, not scheduler execution. Document precedence when related `AgentRun`/`Job`/`ToolProgram` statuses temporarily diverge.

B. Replace `App::task_rows`'s fragmented projection reads with the canonical view, threading the active session id and projection validity. Keep `Ctrl+T` row budget/fair-share, report omitted-count explicitly, and let a sidebar show more detail without a second truth derivation. Preserve existing finished-subagent tab detachment semantics.

C. Bind `/sessions`'s work sections to a labeled, **selected/active session**. The default current session must immediately show its work. On selecting/switching to another session, load only that session's authorized work; an unrelated session's rows must never remain under the next selected session. If cross-session counts/previews are added, require separate authorized summaries rather than pretending one active snapshot covers all sessions. Refresh the **painted** focus-manager dialog clone as well as backing state, retaining row-keyed selection.

D. Provide explicit `Loading live work…`, `Live work unavailable` (with bounded non-secret reason) and `Nothing pending` states based on M001's authority status. Job/child rows must not disappear merely because parent turn completes, nor be displayed live after authoritative terminal/reconciliation.

## 5. Failure, races, security and compatibility

Late attach, session switch while modal open, child start+finish during a frame, duplicate updates, row cap, narrow terminal, stale projection, raw-compat mode, reconnect, terminal status from another stream, tab closure: all must maintain row-keyed selection and fail safe. No separate permission model: only the canonical authorized session projection may be rendered. Reuse existing snapshot DTOs and preserve `/sessions` command, `Ctrl+T` Insert/Normal behavior, `/panels` and editor file tree route. No storage migration in this milestone.

## 6. Tests and verification

Tests MUST install the projection through M001's production-facing path and assert the result of *real* event publication (M002/M003), not construct the desired row vector first. Include one running `AgentRun` + `Job` + child node deduped into one row, a separate CI/tool-program job, two sessions each with work, late attach, modal already open when job arrives, race reorder while Enter is pressed, cap overflow and disconnected/authoritative empty distinction. Drive `App::on_key` in Insert and Normal, type literal `/sessions` through command route, and inspect `TestBackend` painted dialog clone and overlay; negative test other session has no leaked row.

Run focused `tui_sessions_pending_sections`, `tui_main_buffer`, `tui_side_panels` and new session-work integration test, `python3 scripts/check_tui_transport_authority.py`, projection transport guards, fmt, supported clippy, `scripts/verify.sh quick`, `git diff --check`. Update `architecture/tui.md`, `docs/tui.md`, `architecture/projection.md` and slash help as required. Don't add a separate job shortcut or alter the Editor-route file tree.

## 7. Acceptance, stop and closure

Pass: both interfaces show at least two real live children and a distinct job with correct identities/status; a child with a corresponding durable job is not duplicated; after completion/status update rows reconcile correctly; `/sessions` never attributes another session's work to the active one; missing subscription isn't reported as nothing pending. Stop if upstream M001–M003 can't supply trustworthy identity/scoping, or if multi-session support would bypass authorization. Closure `plans/closure/tui-live-work-visibility-corrective/004-status.md` requires tested real projection provenance, screenshot-equivalent TestBackend assertions, real modal clone checks, row association matrix, exact verification and residuals.