# Session Projections Milestone 004 — Closure Status

Status: conditionally closed — remote transport corrective Milestone 005 required

Source implementation plan:

- `plans/implementation/session-projections/004-frontend-adoption-compatibility-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-4--frontend-adoption-compatibility-closure`

Repository baseline reviewed: `bac73ce` (`main`); closure branch constructed atop M3 closure `c4be4c1`.

Implementation commit:

- `bdc2138b7923592d08057485341d4168d504eb14` — frontend controller, local/remote adoption surface, compatibility annotations, tests, and original closure record.

Corrective follow-up:

- `plans/implementation/session-projections/005-remote-transport-isolation-resume-closure.md`

## 1. Executive finding

The frontend-neutral controller, local TUI projection state, protocol additions, artifact caches, and independent reducer/controller equivalence work landed. Those portions remain accepted.

A post-closure production audit found that the remote transport evidence does not satisfy the isolation and resume claims required for strict closure. Milestone 004 is therefore **conditionally closed**, not strictly closed, until Milestone 005 corrects the WebSocket and Unix-socket transport findings below.

### Accepted M4 work

- `ProjectionClientController` provides transport-neutral `ProjectionMode::{ProjectionPrimary, RawCompatibility, Unsupported}`, capability negotiation, subscription/replay/reducer state, acknowledgement cadence, bounded diagnostics, and reconnect epochs.
- `tests/session_projection_m4_controller.rs` drives the canonical fixture scripts through both the controller and reducer and verifies equivalent logical digests.
- `ProjectionClientState` adds bounded per-tab summaries, cursors, artifact handles, excerpts, and reconnect cleanup to the TUI.
- `REMOTE_TUI_PROTOCOL_VERSION = 4` adds projection capability, subscribe, snapshot, replay, resync, acknowledgement, and live-event shapes.
- Legacy `RenderFrame`, `StateSnapshot`, and raw-core messages remain available as a bounded compatibility path.
- Static projection-disclosure, core-boundary, cwd, scheduler, and forbidden-pattern checks were reported green by the implementation pass.

### Post-closure findings requiring M5

1. **`/tui` has no connection-local projection ownership.** `TuiSessionState` stores session/model/rate-limit data only. It does not retain daemon-issued projection subscription IDs, stream descriptors, cursors, receiver tasks, or reconnect generations.
2. **`/tui` uses the daemon-wide raw event broadcast for live delivery.** `upgrade_tui` calls `daemon.subscribe()`, and `convert_core_event_to_tui` accepts any `CoreEvent::ProjectionStreamEvent` without checking connection ownership.
3. **The `/tui` subscribe handler does not take the subscription receiver.** It forwards `ProjectionSubscribe` but never calls the existing `take_subscription_receiver` seam, so the correct owned live stream is not installed.
4. **Remote projection resume is not wired.** Existing `TuiMessage::Resume` replays raw `EventLog` sequence numbers, not a stream-scoped `ProjectionCursor` through `CoreRequest::ProjectionResume`.
5. **Identity is synthesized incorrectly.** A `ProjectionReplay` response is assigned a `ProjectionSubscriptionId` constructed from `descriptor.stream_id`; subscription and stream identities are distinct.
6. **Typed resync is lost.** `ProjectionResyncRequired` is converted to a generic error instead of `TuiMessage::ProjectionResync`.
7. **`/core` WebSocket has the same ownership gap.** Its event task forwards the daemon-wide event broadcast and does not take/own per-subscription projection receivers after projection requests.
8. **Unix-socket live events carry a synthetic stream ID.** Its owned forwarder constructs `ProjectionStreamId` from `ProjectionSubscriptionId` instead of retaining the real descriptor stream ID.
9. **Remote protocol lifecycle is incomplete.** There are no explicit projection resume, unsubscribe, status, or remote artifact-read/list request operations.
10. **Projection-facing WebSocket queues are unbounded.** The adapters use unbounded MPSC queues instead of bounded lag/resync behavior.

These findings affect connection isolation, durable reconnect, and identity correctness. They are not deferred UX polish.

## 2. Original requirement-to-evidence summary

| Work package | Landed evidence | Current result |
|---|---|---|
| Shared controller and negotiation | `crates/codegg-protocol/src/projection/controller.rs` | pass |
| Independent controller/reducer equivalence | `tests/session_projection_m4_controller.rs` | pass |
| Local TUI bounded projection state | `src/tui/app/state/projection_client.rs` | pass |
| Remote protocol DTO additions | `crates/codegg-protocol/src/tui.rs` | pass |
| Remote subscribe/ack request bridge | `src/server/ws.rs` | partial — request/response surface exists, ownership/live/resume lifecycle is incomplete |
| Projection live delivery isolation | `/tui`, `/core`, Unix socket transports | fail pending M5 |
| Durable remote cursor resume | client controller + remote server bridge | fail pending M5 |
| Artifact cache bounds | `ProjectionClientState` | pass for local state; remote request lifecycle pending M5 |
| Raw compatibility path | legacy TUI messages retained | pass, removal deferred |
| Strict subsystem closure | roadmap and registry | fail pending M5 |

## 3. Compatibility and deprecation position

`RenderFrame`, `StateSnapshot`, and raw-core event envelopes remain supported under bounded `RawCompatibility`. M5 must add explicit deprecation diagnostics and removal criteria, but must not remove these variants in the same corrective release.

The compatibility sequence is:

- M4 implementation: projection controller and additive protocol surface landed.
- M5 corrective: fix transport isolation/resume/lifecycle; surface compatibility-channel deprecation.
- A later release, only after the documented compatibility window: evaluate removal of legacy variants.

## 4. Resource bounds retained from M4

| Bound | Constant / policy | Effect |
|---|---|---|
| Controller diagnostics | `MAX_CONTROLLER_DIAGNOSTICS = 32` | Drop oldest on overflow. |
| Controller subscriptions | `MAX_CONTROLLER_SUBSCRIPTIONS = 16` | Refuse new subscriptions. |
| Outstanding lag per subscription | `MAX_OUTSTANDING_LAG = 1024` | Force resync request. |
| Ack cadence | `DEFAULT_ACK_CADENCE = 16` | Ack every 16 envelopes or on resync. |
| Artifact handles per tab | `MAX_ARTIFACT_HANDLES_PER_TAB = 32` | Bound metadata cache. |
| Artifact excerpts per tab | `MAX_ARTIFACT_EXCERPTS_PER_TAB = 8` | Bound excerpt cache. |
| Artifact excerpt bytes | `MAX_ARTIFACT_EXCERPT_BYTES = 8KiB` | Reject oversized excerpts. |

M5 must add explicit bounds for WebSocket outbound queues, receiver-forwarder tasks, replay batches in flight, subscriptions per connection, and remote artifact reads.

## 5. Deferred non-blocking product work

The following remain future product/UX candidates and are not closure blockers for M5:

- numeric acknowledgement/resync hot-key UX;
- cross-tab artifact hand-off;
- plugin-specific `ProjectionEvent::PluginUi` semantics;
- final team roles/presence/chat;
- cross-daemon replay replication.

## 6. Verification recorded by the M4 implementation

The M4 implementation reported these commands green for its landed scope:

```bash
cargo check -p codegg --all-features
cargo check -p codegg-protocol
cargo test -p codegg-protocol
cargo test -p codegg-core
cargo test -p codegg --lib projection_client::tests
cargo test --test session_projection_m4_controller
cargo test --test tui_render
cargo test --test projection_disclosure_invariants
cargo test --test projection_artifact_handles
cargo test --test projection_replay_daemon_protocol
cargo test --test projection_replay_subscription
cargo test --test projection_replay_resume
cargo test --test session_projection_consumer
cargo clippy -p codegg-protocol --all-targets -- -D warnings
cargo fmt -- --check
bash scripts/check-core-boundary.sh
bash scripts/check_projection_disclosure.sh
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_scheduler_bypass.py
```

Those results do not include the two-connection WebSocket isolation, owned-receiver, remote projection-resume, unsubscribe/disconnect, real-stream-ID, or bounded-queue tests now required by M5.

Pre-existing unrelated issues recorded at the time were clippy findings in `crates/egglsp/src/edit.rs` and `python_script::executor::tests::execute_sets_os_filesystem_isolation`.

## 7. Strict closure criteria

Milestone 004 and the session-projections roadmap may return to strict `closed` only when the M5 closure record proves:

- connection-local projection ownership on Unix socket, `/core`, and `/tui`;
- no projection-private event delivery through generic daemon-wide broadcasts;
- real stream ID preservation;
- end-to-end `ProjectionCursor` resume and typed resync;
- unsubscribe/disconnect/shutdown cleanup;
- bounded queue/lag behavior;
- foreign subscription and artifact-operation rejection;
- version-4 compatibility behavior;
- zero unresolved high or medium transport-isolation findings.
