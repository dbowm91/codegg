# TUI Live Work Visibility Corrective M001 — Closure Status

Status: **closed**
Date: 2026-10-19
Class: **infrastructure**
Source implementation plan: `plans/implementation/tui-live-work-visibility-corrective/001-default-tui-session-projection-lifecycle.md`
Source subsystem roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md#7-milestones`
Repository baseline reviewed: `df923802` (rebased target branch)
Implementation commit: `a7774c1e7bc23047bac3076ee236e9f26b2b39f0` — default TUI session subscription and honest live-work state

## 1. Executive finding

The ordinary socket-connected `/core` TUI now binds the active session to an
authorized projection subscription, installs its validated snapshot, applies
matching live envelopes, and cleans up on session/tab changes and reconnect.
`Ctrl+T`, `/sessions`, and the sidebar distinguish loading or unavailable work
state from an authoritative empty snapshot. `/observe` retains its separate
read-only subscription.

The integration fixture uses a real SQLite-backed `CoreDaemon`, Unix socket,
`SocketCoreClient`, and the production `App::set_session`/command-dispatch path.
It does not inject a projection snapshot or claim a manual terminal run.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Default socket path subscribes and installs the owning session snapshot | `default_tui_socket_path_installs_authorized_session_snapshot_and_live_job` | pass | The test creates the session through the socket and observes the production subscription completion. |
| Live event updates the installed snapshot through the socket | `default_tui_socket_path_installs_authorized_session_snapshot_and_live_job` | pass | A second durable scheduler admission after subscription arrives as a matching projection stream event. |
| Session/project/tab/request generations fence stale completions | projection-client binding tests and primary completion validation | pass | Stale completions cannot replace the active binding. |
| Opaque stream identity is accepted without weakening session isolation | `transport_stream_id_is_opaque_but_session_scope_is_still_checked` | pass | A matching opaque stream applies; a foreign-session envelope requests resync. |
| Loading/unavailable differs from authoritative empty in both entry points | `tui_sessions_pending_sections` and default projection status wiring | pass | The painted focus-manager dialog clone receives the current status. |
| Pre-install traffic is bounded and drained after snapshot installation | bounded projection-client queue tests and socket subscribe path | pass | The queue is capped at 256 envelopes; overflow starts a fresh authoritative subscribe. |
| Observer authority remains separate | projection transport isolation/lifecycle guards and unchanged `/observe` route | pass | No observer subscription is reused as the controller's primary session stream. |
| Reconnect clears stale rows and rebuilds the active session | `on_projection_reconnect` state test and production rebind path | pass | Reconnect takes a fresh authoritative snapshot rather than replaying from a retained primary cursor. |

## 3. Production implementation evidence

`src/tui/commands/primary_projection.rs` owns capability negotiation,
subscription, descriptor validation, stale-response cleanup, bounded pending
envelopes, acknowledgements, and resync. `App::set_session`, active-tab
switching, and reconnect invoke that lifecycle. Socket events carry an opaque
stream ID; the controller checks that ID plus the installed session scope.

The availability state reaches `task_rows`, the `Ctrl+T` overlay, the painted
`/sessions` dialog, and the sidebar. Partial job-snapshot diagnostics also show
an unavailable reason while retaining rows that were successfully loaded.

## 4. Verification executed

| Command | Result |
|---|---|
| `rtk bash scripts/verify.sh quick` | pass, local working tree |
| `CARGO_BUILD_JOBS=2 rtk cargo clippy --workspace --all-targets --locked -- -D warnings` | pass, local working tree |
| `rtk cargo test -p codegg --lib default_tui_socket_path_installs_authorized_session_snapshot_and_live_job --locked` | pass (1) |
| `rtk cargo test -p codegg --test tui_sessions_pending_sections --locked` | pass (11) |
| `rtk cargo test -p codegg-protocol --lib projection --locked` | pass (96) |
| `rtk python3 scripts/check_projection_transport_isolation.py` | pass |
| `rtk python3 scripts/check_projection_transport_lifecycle.py` | pass |
| `rtk python3 scripts/check_tui_transport_authority.py` | pass |
| `rtk git diff --check` and `rtk cargo fmt --all -- --check` | pass |

These are local receipts; no hosted CI run or manual terminal run is claimed.

## 5. Invariant review

- Only the active bound session's descriptor may install as the primary view.
- Live events must match both the active subscription and opaque stream ID;
  session-scope identity is checked against the descriptor.
- Missing primary projection state never becomes `Nothing pending`.
- Event processing and rendering perform no projection storage query.
- Observer state remains read-only and separately subscribed.

## 6. Failure and recovery review

Session switches invalidate the old request and subscription; stale responses
are unsubscribed. Reconnect clears old work rows and starts a new authorized
subscribe/snapshot cycle. The bounded pre-install queue resyncs on overflow.
The primary TUI does not retain a cursor across reconnect; a fresh snapshot is
the recovery authority, which avoids a cursor-floor gap at the cost of another
bounded snapshot query.

## 7. Migration and compatibility review

No DTO or storage migration was introduced. Existing `/observe` and remote
`/tui` attach contracts remain separate. Older remote attach events that omit
the stream ID retain their compatibility path.

## 8. Security review

Subscription authorization remains daemon-side. The client validates exact
session, project, stream kind, and workspace descriptor identity before
installation. Foreign-session envelopes do not reduce into the active
snapshot. Projection transport ownership and TUI transport-authority guards
passed.

## 9. Documentation and operations

Updated `architecture/tui.md`, `architecture/projection.md`, `docs/tui.md`, and
`.opencode/skills/tui/SKILL.md` with lifecycle, status, isolation, and reconnect
behavior.

## 10. Unresolved findings

No high- or medium-severity finding remains for M001. Cursor-based resume is
not implemented for the primary TUI; reconnect always takes a fresh
authoritative snapshot. Manual interactive terminal qualification belongs to
M006 and is not claimed here.

## 11. Roadmap disposition

M001 is closed. The fresh-snapshot reconnect behavior satisfies active-session
recovery without requiring a new protocol or schema. This closure does not
unblock M004 because M002 and M003 remain dependencies.

## 12. Registry updates

The M001 implementation plan and roadmap row are marked closed and this record
is listed under Recently closed. M003 remains blocked on explicit acceptance of
ADR-0042; therefore M004–M006 remain blocked in order.
