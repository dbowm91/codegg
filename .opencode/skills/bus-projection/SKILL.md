---
name: bus-projection
description: Global event bus, permission/question registries, and frontend-neutral session projection in codegg
version: 1.0.0
tags:
  - bus
  - events
  - projection
  - collaboration
  - presence
---

# Bus and Projection Guide

Operational guide for changing events and derived views. The full
contracts live in `architecture/bus.md`, `architecture/projection.md`,
`architecture/collaboration.md`, and `architecture/presence.md`; this
skill covers the sync/async and authority boundaries that are easy to
violate.

## Ownership Map

| Layer | Location | Role |
|-------|----------|------|
| Bus | `crates/codegg-core/src/bus/global.rs`, `events.rs`, `mod.rs` | `GlobalEventBus` (tokio broadcast, cap 4096, sync `publish()`), `AppEvent`, `PermissionRegistry` + `QuestionRegistry` (sync); `events.rs` owns the current event set |
| Projection contract | `crates/codegg-protocol/src/projection/` | `ProjectionEnvelope`/`ProjectionEvent`, `SessionProjectionSnapshot`, deterministic canonical reducer (no I/O), `caps`/`limits`/`dto`/`adapters`/`fixtures`; the projection enum owns the current variant set |
| Replay | `crates/codegg-core/src/projection_replay/` | Durable replay for reconnect/resume |
| Daemon seam | `src/core/daemon_projection.rs`, `daemon.rs` | Projection request family handler; SSE uses the global bus (no per-state `event_bus` field) |
| Collaboration | `crates/codegg-core/src/collaboration.rs`, `src/tui/app/state/chat.rs` | Project channels/messages (threads, mentions, refs, edits/redactions, read markers, retention, bounded sync); bodies stay inert text |
| Presence | `crates/codegg-core/src/presence.rs`, `src/core/daemon.rs` | Ephemeral leases (heartbeat/idle/expiry); liveness-only, never authorizes work |

## Hard Rules

1. **Registries are sync; order matters.** `register`/`respond`/
   `answer_question` are `fn`, not `async`. Register the responder
   BEFORE publishing the Pending event.
2. **The reducer never performs I/O.** No network, filesystem, provider,
   or clock reads inside `reducer.rs`; all frontends (local TUI, remote
   TUI, observers, ACP) consume the same derived model.
3. **Projection is derived, never authoritative.** Mailbox/journal run
   records are execution truth; projection may summarize but never replay
   a journal event as a turn or tool call. Reconnect recovers from durable
   run/control state.
4. **Chat bodies stay inert; presence stays liveness-only.** Collaboration
   text never executes; presence never gates work. Both ride the
   daemon-owned, authorized, privacy-preserving event path.
5. **Transport isolation holds.** Raw broadcast and identity seams are
   guarded; scoped subscriptions + durable replay are the only remote
   history path (guards below).

## Static Guards

```bash
python3 scripts/check_projection_transport_isolation.py  # raw-broadcast/identity guard
python3 scripts/check_projection_disclosure.sh           # disclosure seam
python3 scripts/check_projection_publication_seam.sh     # publication seam
python3 scripts/check_projection_transport_lifecycle.py  # transport lifecycle
python3 scripts/check_websocket_bounds.py                # WS bounds
```

## Testing

```bash
cargo test -p codegg-core bus::
cargo test -p codegg-protocol projection
cargo test --test collaboration_m001_chat
cargo test --test collaboration_m002_chat_tui
cargo test --test collaboration_m003_chat_actions
```

## See Also

- `architecture/bus.md`, `architecture/projection.md`
- `.skills/core/SKILL.md` — projection request family on the daemon
- `.skills/server/SKILL.md` — `/tui` WS event/state protocol (no `RenderFrame`)
- `.skills/tui/SKILL.md` — sidebar/run-tree joins by exact canonical task IDs
