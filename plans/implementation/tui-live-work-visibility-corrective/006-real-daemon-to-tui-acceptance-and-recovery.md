# TUI Live Work Visibility M006 — Real Daemon-to-TUI Acceptance and Recovery

Status: **conditionally closed — see `plans/closure/tui-live-work-visibility-corrective/006-status.md`**
Primary class: capability
Repository review baseline: `ea5dab90424ba95312310ac7b1072be0f1323742` (`main`, 2026-10-20)
Source roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md` M006.
Long-term: `plans/000-long-term-specification.md#25-tui-target-behavior`, `#26-reliability-and-recovery`, `#27-security-requirements`, `#28-observability`, `plans/001-terminology-and-domain-model.md`.
Applicable accepted decisions: ADR-0031, ADR-0036, ADR-0041; ADR-0043 must have been accepted and implemented via M003.
Corrective origin: commit `8fab1ba7d48d794b397ce4a0a207d5ae547ff976`; its commit receipt explicitly said `no CI receipt; TUI was never launched`. Earlier convergence tests seeded projection rows, not execution producers.

## 1. Objective and readiness

Close the user-visible regression **only** after reproducing genuine jobs and children through scheduler/worker/daemon event publication, socket projection subscription, TUI state, actual slash/key routing and the painted dialog/overlay. Prove this for attached and late-attaching clients, after parent completion and restart, including actionable inspection and supported control. This is the end-to-end capability gate, not a proposal for permanent performance CI or model-provider benchmarking.

## 2. Test topology / real boundaries

Use isolated SQLite temp database and a test-owned CoreDaemon with a loopback Unix socket (or platform-appropriate supported transport). Instantiate the **real** `SocketCoreClient` + ordinary TUI `App::attach_core_client`/active-session binding, without manually calling `install_subscription` or injecting `SubagentRow`/`JobLike`/prebuilt snapshot. Supply a deterministic scripted provider/worker for child task and an explicitly gated compile/test/CI-watch or long-running shell job; the scheduler must actually accept and transition them. Use a controllable latch/barrier to hold jobs **while the UI is observed**; do not race through completion. Exercise the production event-loop core receiver and `App::apply_core_event`; run actual `App::on_key` in Insert/Normal, `/sessions` command input and `Ctrl+T` binding, and inspect the **painted FocusManager clone** in `ratatui::TestBackend`. Add a separate **manual** real-terminal check on the development workstation as release evidence.

## 3. Required scenarios (all must identify same authoritative ids)

A. Bind Session A and start two concurrent subagents plus one independent background job. Wait for durable scheduler acceptance, then for projection high-water. Open `Ctrl+T`; assert two distinct child rows, one independent job, no double count for child run + its scheduler job, statuses and inspect keys; open literal `/sessions` and assert same IDs/status/labels in selected Session A with exact rendered sections.

B. Without terminating work, attach TUI Client B **late** to Session A. Verify initial snapshot carries outstanding jobs/durable runs and live child identities. Client A and B converge without manual refresh or bogus `Nothing pending`. Have Client C bound to Session B and ensure it never sees A's work (also test unauthorized projection subscribe).

C. Complete parent turn while child continues; parent timeout/stop after child start; cancel queued child before worker dequeues; repeat under a new parent turn. Verify no `orphan_subagent_started` loss, no misassociation with current turn, and eventual terminal state. Preserve deterministic instrumented failure/timeout labels.

D. Disconnect/reconnect socket with event cursor gap; restart daemon with durable jobs in flight and ephemeral worker already gone; test resumed/rebuilt authoritative statuses, interruption reconciliation, no phantom running row and no stale stream repaint. Duplicate/reordered terminal events and a rapid created→terminal job must yield monotonic lifecycle.

E. Hold modal open while a job starts or finishes and `/sessions` refreshes; Enter/mouse must target stable entity ids. Inspect job attempts, authorized cancel and resulting server state. Attempt an observer cancel / foreign-session detail / stale parent subagent steer; verify refusal and no escalation. Test `Ctrl+T` Enter/Modes; on Editor route preserve existing file-tree semantics.

F. Force projection unavailability (unsupported, denied, broken subscribe, silent subscription, bound to wrong session). Both surfaces say **unavailable/loading** (as appropriate), never `Nothing pending`. Conversely a valid empty projection does display `Nothing pending`. Verify bounded overflow count and no starvation under >cap outstanding jobs/children.

## 4. Assertions and observability

Record source `job_id`, `run_id` and `child_instance_id` from trusted admission, `session_id`, attempt, CoreEvent/ProjectionEvent sequences, subscription id, cursor and head commit. Assert status transitions against scheduler/AgentRunStore truth; do not compare only substring-rendered labels. Apply timeouts and controlled latches, not arbitrary sleeps or repeated blind polling. Fail tests on missing projection subscription, missing scheduler publisher, wrong session scope, dropped orphan, stale dialog clone, nonactivatable job and false-empty fallback (mutation checks or purposeful fault injection). Never log raw provider prompts/secret stdout as diagnostic evidence.

## 5. Bounds and security

Test provider-independent deterministic fixtures; no external LLM/network dependency, no actual privileged shell, no live CI credentials, no mandatory VM or SBC compile/startup. Keep concurrency bounded to workstation CI capability; scheduler fairness tests if changes touch it. Reuse daemon authentication/control ownership and session privacy; fail closed on foreign-session and observer writes. No side effects outside test temp workspace.

## 6. Commands and verification

Create a named integration target e.g. `tests/tui_live_work_end_to_end.rs` that runs without ignored/manual flags where test environment supports Unix sockets; gate/skip only with explicit platform reason and no false green. Execute `cargo test -p codegg --test tui_live_work_end_to_end --locked` and existing `tui_sessions_pending_sections`, `tui_main_buffer`, `projection_transport_real`, `async_background_c006_tool_to_idle_ci` (if applicable), focused core/protocol projection tests. Run `python3 scripts/check_projection_transport_isolation.py`, `python3 scripts/check_projection_transport_lifecycle.py`, `python3 scripts/check_tui_transport_authority.py`, `python3 scripts/check_scheduler_bypass.py`, `python3 scripts/check_execution_ownership.py`, fmt, supported clippy, `scripts/verify.sh quick`, `git diff --check`. Record hosted CI when available and exact-head test results; do not claim a run that was not executed.

## 7. Manual operator receipt / docs

On real TUI build from verified SHA: launch a regular daemon-backed session, delegate two children and start one long-running background job, open `Ctrl+T` and `/sessions` while they await, select detail, reconnect, terminate one job; record exact observed row ids/status/time and any errors. This is **required** for full user-visible closure, but is not a perpetual daily CI test and must not be fabricated if test hardware/environment unavailable. Document reproducible operator commands in `docs/tui.md`/`architecture/tui.md` and add a truthful troubleshooting view for missing projection.

## 8. Acceptance, stop and closure

Close only when every scenario has passing **real production-path** evidence and manual terminal receipt on an intended development workstation. Stop/record as **conditionally closed** or **blocked** if live TUI cannot be launched, the trace fails before the presentation layer, or upstream lifecycle semantics remain unqualified. Do not substitute synthetic test success for runtime proof. Write `plans/closure/tui-live-work-visibility-corrective/006-status.md` with each scenario/result, test environment, binary/commit, event/stream trace (redacted), exact commands/CI links, cancellation/security/restart/cursor evidence, manual receipt and residual severity; update roadmap and registry only when actual acceptance holds.
