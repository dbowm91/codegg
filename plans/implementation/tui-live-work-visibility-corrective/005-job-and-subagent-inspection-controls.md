# TUI Live Work Visibility M005 — Identity-Keyed Inspection and Authorized Actions

Status: **closed** — closure: `plans/closure/tui-live-work-visibility-corrective/005-status.md`
Primary class: capability
Repository review baseline: `61cbe9222c442c443cc7dfb3bd3a5b7d99ecd649` (`main`, 2026-10-19; re-pin)
Source roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md` M005.
Long-term: `plans/000-long-term-specification.md#25-tui-target-behavior`, `#27-security-requirements`, `#28-observability`; `plans/001-terminology-and-domain-model.md`.
Applicable accepted decisions: ADR-0007 (session controller), ADR-0036 (TurnSteer), ADR-0041 **Option A** (subagent messages addressed to parent, not child). M003/ADR-0042 must have yielded stable child identity before handoff.
Corrective origin: `8fab1ba...`; previous C002 explicitly consumed `Enter` on job rows with no action; C007 added only `/subagent-send` through the controlling parent turn, not direct child interaction.

## 1. Objective and readiness

Selecting a visible job or child is never a dead key. The user can inspect an authorized job's status/attempts/summary and request supported controls; select a durable subagent to see its run state and, where already authorized by existing APIs, perform run controls. All actions remain daemon-owned and identity-keyed, with clear disabled/rejected states. Hard dependency M004's canonical `SessionWorkRow`; no new steering authority is inferred.

## 2. Code evidence and bounded capability

`src/tui/components/dialogs/session.rs::activate_selected` returns `None` on `Job`; `src/tui/app/state/tasks.rs::activatable_target` targets only subagents; `App::open_selected_task` silently returns false for a job. Available public protocol candidates: `CoreRequest::JobGet`, `JobList`, `JobAttempts`, `JobCancel` and `RunGet` in `crates/codegg-protocol/src/core.rs`; inspect actual daemon owner checks, response DTOs and `JobWait` semantics first. `/subagent-send` uses `TurnSteer` on a controlling **parent** turn and cannot send to detached children; preserve that distinction.

## 3. Invariants / non-goals

- The selected entity's stable job/run/child id—not the list index, current highlight text, or agent name—is passed to daemon APIs. An entity finishing or disappearing mid-confirmation must not retarget the action.
- Inspection does not implicitly send, start, cancel, grant authority, move a worktree or reopen a parent turn.
- Every action uses existing authenticated CoreClient requests and server-side authorization; an observer sees read-only affordances, not enabled cancel/steer.
- `TurnSteer` is best-effort and only targets current controlled parent turn under ADR-0041; no direct child prompt API is invented. A detached child may be inspected but must show parent-steer unavailable.
- No full transcript reconstruction from logs the daemon does not expose; prefer honest detail summaries, optional explicit `RunGet`, and clear disclosure of missing child output.
- Out: new scheduler commands, user-to-child direct steering decision, implicit agent resumption, uncontrolled polling, extra keybinds.

## 4. Ordered work packages

A. **Inventory control authority.** Read `src/core/daemon_jobs.rs` and `CoreRequest::{JobGet,JobAttempts,JobCancel,RunGet}` handlers, related tests, permissions and status taxonomy. Verify which operations truly allow an interactive TUI user and how errors are returned. Identify genuine missing protocol; if a new cross-boundary run-control request is necessary, stop at a separate ADR/interface proposal rather than routing through a model/tool command.

B. **Inspect surface.** Add a shared work detail dialog or panel route reachable from both `/sessions` and `Ctrl+T`. For jobs show job id, kind, session, state, active attempt, bounded summary/error and attempt history when available; for durable subagents show run/agent, state, parent relation, worktree/branch (if authorized), recent progress and terminal result (only projected/disclosed fields). For transient child show known projection info with unavailable fields explicitly flagged. Keep the current subagent transcript tab for a child where it is genuinely backed; allow a separate detail inspection without silently replacing that tab.

C. **Actions and feedback.** Provide explicit identity-keyed `Cancel job` if `JobCancel` authorization supports it; show pending/ack/refusal/terminal conflict, do not optimistically declare it cancelled. For durable run, implement only actions already supported by an authorized typed Core API (e.g. inspect); no unapproved direct message path. Keep `/subagent-send` as parent-steer with clear addressee and turn ownership; disable when the controlling parent turn is absent, stale or controlled elsewhere. If cancellation is unsupported for a given job/source, show why instead of doing nothing.

D. **Input/selection correctness.** Wire Enter/mouse/modal activation via typed `TuiMsg`/panel intents; a job row should activate detail. Maintain row-key identity through concurrent updates; do not allow a clone-owned modal to operate on obsolete index after refresh. Preserve composer text on refused addressed steer. Do not add fallback `TurnSubmit`.

## 5. Security, failure, recovery and compatibility

Reject unauthorized foreign session/job ids identically to unavailable ones where canonical policy requires non-disclosure; show bounded user messages without raw stdout/secrets. Model-controller vs human-controller conflicts, stale turn, already terminal job, cancel in flight, daemon disconnect, duplicate commands and rapidly replaced detail target must behave deterministically. No new storage required; reuse existing typed CoreRequest/Response variants and keep remote `/tui` / observer read-only.

## 6. Tests, docs and required verification

Use a fake CoreClient for request-shape / refusal tests **and** a real daemon + authorized owner for negative cross-session and cancellation tests. Drive Enter/mouse on *both* surfaces; assert exact job/run id, detail DTO and server-side actual status change after supported cancel. Try race: select J1, J1 completes, J2 shifts into its row, Enter/cancel must never address J2. Exercise stale parent `TurnSteer`, not-controller refusal, unsupported direct steering, modal clone, focus restoration, job without attempts, detached subagent. Tests must fail if Job Enter returns `None`.

Run `cargo test -p codegg --test tui_sessions_pending_sections --locked`, `cargo test -p codegg --test tui_main_buffer --locked`, run/control-focused module tests, `python3 scripts/check_tui_transport_authority.py`, `python3 scripts/check_execution_ownership.py`, fmt, supported clippy, `scripts/verify.sh quick`, `git diff --check`. Update `docs/tui.md`, `architecture/tui.md`, `architecture/jobs.md` and in-surface labels.

## 7. Acceptance, stop, closure

Pass: job rows in both entry points open meaningful real detail, supported cancel is daemon-authorized and visibly acknowledged, subagent run detail works by stable identity, parent-turn steering remains correctly gated; no Enter silently discarded on a selectable work row. Stop for any new run-specific human control authority that requires an ADR, missing canonical API, or accidental observer privilege. Closure `plans/closure/tui-live-work-visibility-corrective/005-status.md` must include exact supported/unsupported action matrix, integrated request/response and owner authorization evidence, simultaneous lifecycle/selection negative tests, verified code/head and limitations.
