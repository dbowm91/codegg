# TUI Live Work Visibility M001 — Production Session Projection Lifecycle

Status: **closed**
Primary class: infrastructure
Repository review baseline: `df923802` (`plan/tui-live-work-visibility-corrective`, rebased on `origin/main`, 2026-10-19)
Source roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md` M001.
Requirements: `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`, `#25-tui-target-behavior`, `#28-observability`; `plans/001-terminology-and-domain-model.md`.
Applicable accepted decisions: ADR-0007 (session control), ADR-0036 (steering). ADR-0042 is **not** a dependency for this milestone.
Corrective origin: `8fab1ba7d48d794b397ce4a0a207d5ae547ff976`, prior `plans/closure/tui-subagent-and-job-visibility-convergence/001-status.md` and `002-status.md` (synthetic projection-to-render coverage, no launched TUI/real daemon subscription).

## 1. Objective and readiness

The **ordinary** `/core` socket-connected TUI must subscribe to the authorized active session's projection, install the initial authoritative snapshot, apply live envelopes, resume on reconnect, and provide an explicit unavailable/loading/error state. No new child/job schema is required. This milestone is independently executable with the current `CoreRequest::ProjectionSubscribe/Resume/Unsubscribe` and `ProjectionClientController` contracts.

## 2. Current evidence / why old tests missed it

`src/main.rs` attaches `CoreClient` and a raw event receiver. `src/tui/app/mod.rs::attach_core_client` subscribes raw events, while `ensure_session_event_stream` only adjusts a raw session event filter. `app::apply_core_event` handles `ProjectionStreamEvent`; `handle_remote_event` installs `TuiMessage::ProjectionSnapshot` for the distinct `/tui` attach mode; `src/tui/commands/observe.rs` explicitly subscribes for `/observe`. Search found no normal-session `ProjectionSubscribe` issuance or initial snapshot install in production TUI code. `ProjectionClientState::active_snapshot` returns none when no stream is installed. Previous `tests/tui_sessions_pending_sections.rs` installs a hand-built snapshot directly, bypassing all these paths. Verify source at pinned head before edits; treat hypothesis as falsifiable.

## 3. Invariants and scope

- Session-bound stream ownership: neither project-global nor another session's snapshot may drive the visible tab; verify stream descriptor `session_id`/`project_id`, not `ends_with(tab_id)` or arbitrary first snapshot for multi-stream cases.
- Response-before-live ordering, bounded buffering and cursor/ack policy, exactly-once visible effects on duplicate delivery, explicit resync on gaps; no blocking requests in `render`.
- Input mode, editor-route `Ctrl+T` override and `/sessions` clone/focus behavior remain unchanged.
- Separate three states: **authoritative empty**, **loading**, and **unavailable/error/unsupported**. Never translate absence of a subscription into `Nothing pending`.
- Out: scheduler/worker publication changes (M002/M003), new access scopes, perpetual polling, subagent/user controls, a second reducer, cross-session observer authority.

## 4. Required production work

A. **Capture failing production fixture first.** Launch a test-owned `CoreDaemon`/socket with a known session containing one projection job/event. Enter the normal `src/main.rs`/TUI client path (not `/observe`, not synthetic `App::new_for_testing` subscription injection). Show `active_snapshot` stays absent before this fix.

B. **Lifecycle owner.** Introduce/restore a single active-session subscription coordinator within `src/tui/app/{project_session,mod}.rs` or a small `commands` module: start after binding session on startup/restore/switch; send `ProjectionCapabilities` (as needed) then `ProjectionSubscribe` via `CoreClient`, accept `ProjectionSnapshotBundle::One` with validated descriptor/scope, and install through `ProjectionClientState/ProjectionClientController`. Preserve explicit `/observe` subscription isolation. Ensure `SocketCoreClient` transport takes its owning projection receiver and forwards live envelopes; don't confuse this with raw `subscribe_session_events`.

C. **Generation fences and resync.** Key pending requests and installed subscriptions on session, project, tab and reconnect epoch. Unsubscribe old stream on switch/close, reject old responses/events, handle `ProjectionResume`/cursor floor/`ProjectionResyncRequired`, send ACKs under existing cadence, and avoid double installing a `stream_id`. Reconnect must start from an authoritative snapshot if resume cannot cover the gap. Only the matching active subscription changes UI; observers remain read-only.

D. **Honest view state.** Feed a typed `loading | authoritative | unavailable(reason)` status to `App::task_rows`, `Ctrl+T`, `/sessions` and sidebar so transport failure cannot silently render `Nothing pending`. Keep the existing row builder for actual projection data; do not fabricate jobs to signal connection status. Ensure modal's focus-manager clone receives refreshed state.

## 5. Failure, cancellation, restart, contention

Startup with no session defers subscription; failed authorization leaves a visible non-leaking reason and safe retry. Session A→B switch while A's subscribe is pending must never show A's rows on B. Session close revokes receiver; a stale envelope is ignored. Daemon restart and reconnection recover subscription/cursor without leaving ghost rows or a falsely authoritative empty state. Burst traffic must not starve key handling; bounded queue/ACK failure logs diagnostics.

## 6. Protocol, security and compatibility

Prefer existing `ProjectionSubscribe` and `ProjectionStreamEvent`; no DTO or storage migration expected. Existing `/tui` attach and `/observe` retain their contracts. All access via daemon authorization and canonical session identity; no TUI direct SQL reads. TUI raw compatibility mode explicitly reports unavailability or uses a deliberately specified authorized fallback—never reports false emptiness.

## 7. Required tests

Focused: subscription request ordering, descriptor mismatch, duplicate install, close/switch epoch, no-tab single stream vs >1 stream, unavailable/empty semantics, delayed initial snapshot, stream lag/resync, retry and ACK. Integration: real SQLite daemon + SocketCoreClient `ProjectionSubscribe`/snapshot/live event → App state → `Ctrl+T` and painted `/sessions`, including event-after-open. Security negatives: unauthorized session, cross-session event, observer not-controller. Regression should fail if the production subscribe is removed or if initial snapshot install is replaced with an empty stub.

## 8. Verification and docs

Run narrow tests for `projection_client`, `tui_sessions_pending_sections`, `tui_main_buffer`, `projection_transport_real`; `python3 scripts/check_projection_transport_isolation.py`, `python3 scripts/check_projection_transport_lifecycle.py`, `python3 scripts/check_tui_transport_authority.py`; `cargo fmt --all -- --check`; supported `cargo clippy --workspace --all-targets --locked -- -D warnings`; `scripts/verify.sh quick`; `git diff --check`. Record **actual commands executed**, not aspirational passes. Update `architecture/tui.md`, `architecture/projection.md`, `docs/tui.md` and relevant transport docs.

## 9. Acceptance, stop and closure

Pass: default socket TUI receives its own authoritative initial snapshot and live projected updates without manual `/observe`; both entry points distinguish no jobs from no projection; switch/reconnect/isolation tests pass. Stop if daemon stream ownership requires changing authorization or a new protocol/ADR, or if real transport evidence contradicts diagnosis—record evidence and replan. Closure `plans/closure/tui-live-work-visibility-corrective/001-status.md`: exact commit/binary, failing-before/passing-after real test, request/response/stream proof, row render, security/race matrix, verification receipts and unresolved risks. No claims of live-terminal proof from `TestBackend` alone.
