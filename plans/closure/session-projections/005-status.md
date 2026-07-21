# Session Projections Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-5--remote-transport-isolation-resume-and-compatibility-closure`

Repository baseline reviewed: `bdc2138b7923592d08057485341d4168d504eb14`

Implementation commit:

- `4c751ff` — connection-owned projection transport, cursor resume, typed lifecycle operations, bounded queues, identity guard, tests, and documentation.

## 1. Executive finding

Milestone 005 is complete. Unix-socket, `/core`, and `/tui` projection
delivery now use connection-local ownership around the daemon-issued receiver.
Replay and live handoff retain the persisted descriptor and real stream ID;
resume returns canonical replay or typed resync; foreign lifecycle and
artifact operations fail closed; raw compatibility is bounded and filtered;
disconnect, downgrade, unsubscribe, lag, and shutdown clean up transient
receivers without deleting replay history.

The M4 conditional findings are therefore strictly closed. No unresolved high
or medium transport-isolation findings remain.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Shared connection-local ownership | `src/core/transport/projection.rs`; 5 focused unit tests | pass | Bounded subscriptions, artifacts, diagnostics, generation, lifecycle, cancellation. |
| Unix receiver ownership and real stream ID | `src/core/transport/daemon_socket.rs`; `daemon_socket_integration` tests; static guard | pass | Receiver is taken once; forwarder uses the descriptor stream ID. |
| `/core` receiver ownership and filtering | `src/server/ws.rs`; shared owner/forwarder path; workspace check | pass | Projection responses install the receiver before control response release. |
| `/tui` receiver ownership and typed outcomes | `src/server/ws.rs`; protocol tests; TUI tests | pass | Subscribe/resume/replay/resync/ack/unsubscribe/status/artifact paths are typed. |
| Cursor resume and typed resync | daemon `ProjectionResume`; replay/resume and daemon protocol suites | pass | Stream mismatch, expiry, ahead, gap, scope, version, and lag reasons remain typed. |
| Replay-to-live handoff | install-before-response ordering and cancellation-race tests | pass | `Notify::notify_one` preserves the live-start permit when scheduling is delayed. |
| Foreign subscription/scope rejection | trusted `handle_request_for_client`; owner checks; transport isolation suite | pass | Client-supplied IDs are locators, not authority. |
| Bounded queues and tasks | separate 256-entry control/live/raw WebSocket queues; owner caps | pass | Live overflow emits typed subscriber-lagged resync and stops the forwarder. |
| Artifact lifecycle | typed list/read protocol; per-connection cap and project ownership checks | pass | Existing disclosure and artifact policy remains daemon-owned. |
| Version-4/raw compatibility | protocol serde tests; additive defaults; raw compatibility path | pass | Legacy raw messages remain available and projection-private events are excluded. |
| Static regression protection | `scripts/check_projection_transport_isolation.py` | pass | Rejects raw projection forwarding and subscription-derived stream IDs. |

## 3. Production implementation evidence

- `ProjectionConnectionState` is the shared transport-neutral owner. It keeps
  trusted connection identity, negotiated mode/version, reconnect generation,
  bounded owned subscriptions, artifact permits, diagnostics, and cancellation.
- The replay service still owns durable stream sequence, receiver registration,
  cursor validation, and persistence-before-delivery. Unsubscribe now also
  drops a receiver that is still pending during a failed install or disconnect.
- `CoreDaemon::handle_request_for_client` supplies a transport-derived owner
  identity. Resume refuses to rebound an actively owned stream to a different
  connection and creates a daemon-issued subscription only for an unowned
  reconnect.
- Unix socket, `/core`, and `/tui` each take the receiver once, retain the real
  `ProjectionStreamDescriptor`, and forward exact subscription/stream IDs.
- `/core` and `/tui` raw broadcast tasks explicitly discard
  `CoreEvent::ProjectionStreamEvent`. WebSocket control, projection-live, and
  raw-compatibility traffic use separate bounded queues with control priority.
- TUI protocol version 5 adds explicit cursor resume, typed ack/unsubscribe/
  status/artifact operations, optional unbound resync identity, and a raw
  compatibility diagnostic. Older projection event/snapshot/resync fixtures
  remain decodable through additive defaults.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt -- --check
rtk cargo check -p codegg --all-features
rtk proxy env CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
rtk cargo test -p codegg-protocol -- --nocapture
rtk cargo test -p codegg-core -- --nocapture
rtk cargo test -p codegg --lib core::transport::projection -- --nocapture
rtk cargo test -p codegg --lib daemon_socket_integration -- --nocapture
rtk cargo test --test projection_replay_daemon_protocol -- --nocapture
rtk cargo test --test projection_replay_subscription -- --nocapture
rtk cargo test --test projection_replay_resume -- --nocapture
rtk cargo test --test projection_replay_restart_recovery -- --nocapture
rtk cargo test --test projection_replay_transport_isolation -- --nocapture
rtk cargo test --test projection_disclosure_invariants -- --nocapture
rtk cargo test --test projection_artifact_handles -- --nocapture
rtk cargo test --test tui -- --nocapture
rtk cargo test --test tui_render -- --nocapture
rtk cargo test --test single_daemon_lifecycle -- --nocapture
rtk python3 scripts/check_projection_transport_isolation.py
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_git_forbidden_patterns.py
rtk python3 scripts/check_scheduler_bypass.py
rtk bash scripts/check_projection_disclosure.sh
```

### Results

All listed formatting, workspace check, focused tests, regression tests, and
static guards passed. Recorded counts include: protocol 157, core 299,
transport-owner 5, daemon protocol 13, subscription 13, resume 9, transport
isolation 7, disclosure 16, artifacts 13, TUI 164, render 99, and daemon
lifecycle 3.

`cargo clippy -p codegg-protocol --all-targets -- -D warnings` passed. The
workspace `clippy --all-targets --all-features -- -D warnings` invocation
remains blocked only by three pre-existing `question_mark` suggestions in
`crates/egglsp/src/edit.rs`, already recorded by the prior closure; no new
M005 warning was introduced.

## 5. Invariant review

- Stream, subscription, project, workspace, session, and connection identities
  remain distinct. No transport constructs a stream ID from a subscription ID.
- Only the receiver taken for the owned daemon subscription produces live
  projection envelopes. Raw broadcast paths filter projection events.
- Cursor validation and sequence authority remain in the replay service/daemon.
- Persistence precedes delivery; duplicate reconnect boundaries remain
  reducer-idempotent.
- Expired, ahead, gapped, mismatched, version-incompatible, rebound, and
  lagged cursors produce typed resync/error outcomes.
- Disconnect, unsubscribe, downgrade, cancellation, and shutdown remove
  transient forwarders while retaining replay history.
- Raw compatibility is mode-isolated and bounded.
- Per-connection queues, subscriptions, artifact reads, diagnostics, and
  replay receiver capacity are bounded.

## 6. Failure and recovery review

The receiver-install path unsubscribes when a seam, receiver, duplicate ID,
or capacity check fails. Forwarders observe cancellation even before the live
ready notification is consumed. Queue overflow marks the owned subscription
resync-required, emits `SubscriberLagged` when the control queue accepts it,
and stops delivery without advancing acknowledgements. Service unsubscribe
drops pending receivers, making repeated cleanup safe at the transport layer.
Daemon restart/replay behavior remains covered by the existing restart
recovery and resume suites; replay history is not removed by connection cleanup.

## 7. Migration and compatibility review

The remote TUI protocol is version 5. Version-4/raw variants remain present;
new cursor, snapshot metadata, event stream identity, and resync identity
fields use additive defaults where old fixtures need them. Projection-primary
clients negotiate capabilities before projection operations. Legacy raw resume
continues only in raw compatibility mode and receives an explicit deprecation
diagnostic when projection capability is accepted. No storage schema or replay
authority was replaced.

## 8. Security review

Transport calls use server-derived connection IDs rather than client-supplied
IDs. Acknowledgement, resume, and unsubscribe look up daemon ownership; an
active stream owned by another connection cannot be rebound by cursor alone.
Artifact list/read requires an owned project subscription, bounded concurrent
reads, and the existing daemon disclosure policy. Secret-bearing provider
operations remain denied on the remote core WebSocket. Diagnostics contain
codes and bounded reasons, never payload bodies, artifact content, or secrets.

## 9. Documentation and operations

Updated `architecture/{client,core,overview,projection,protocol,server,testing,tui}.md`,
`AGENTS.md`, the M4 closure correction, the projection roadmap, and the plan
registry. Added `scripts/check_projection_transport_isolation.py` and wired it
into the repository guard documentation. Queue bounds, lag behavior,
compatibility mode, disconnect cleanup, and typed resync recovery are now
documented.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Workspace clippy still reports three pre-existing `question_mark` suggestions in `crates/egglsp/src/edit.rs`. | Prevents a clean repository-wide `-D warnings` result but is unrelated to M005. | Track with the existing clippy cleanup; no transport action. |

No high or medium M005 findings remain.

## 11. Roadmap disposition

Milestone closed and the session-projections subsystem roadmap may proceed as
strictly closed. There are no dependency-ready correctness plans currently
blocked by M005. Deferred UX/product items remain explicitly unregistered and
out of scope; they are not silently promoted to implementation plans.

## 12. Registry updates

- Marked M005 and the session-projections subsystem closed in
  `plans/registry.md` and `plans/subsystems/session-projections-roadmap.md`.
- Removed the M005 dependency-ready, active-closure, and blocked rows.
- Moved M004 to strict closed status and added M005 to recently closed work.
- Corrected stale implementation-plan headers for M001, M003, and M004 so
  closed closure records no longer appear blocked or ready.
