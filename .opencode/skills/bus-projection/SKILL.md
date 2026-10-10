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
| Bus | `crates/codegg-core/src/bus/global.rs`, `events.rs`, `mod.rs` | `GlobalEventBus` (tokio broadcast, cap 4096, sync `publish()`) in `global.rs`; `AppEvent` in `events.rs`, which owns the current event set; `PermissionRegistry` + `QuestionRegistry` (sync) in `mod.rs` |
| Projection contract | `crates/codegg-protocol/src/projection/` | `ProjectionEnvelope`/`ProjectionEvent` (`event.rs`), `SessionProjectionSnapshot` (`snapshot.rs`), deterministic canonical reducer (no I/O, `reducer.rs`), plus `caps`/`limits`/`dto`/`adapters`/`consumer`/`controller`/`replay`/`fixtures`; the projection enum owns the current variant set |
| Replay | `crates/codegg-core/src/projection_replay/` | Durable replay for reconnect/resume |
| Daemon seam | `src/core/daemon_projection.rs`, `src/core/daemon.rs` | Projection request family handler; the compatibility SSE route (`src/server/routes/event.rs`) reads the global bus, and there is no per-state `event_bus` field |
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
bash    scripts/check_projection_disclosure.sh           # disclosure seam
bash    scripts/check_projection_publication_seam.sh     # publication seam
python3 scripts/check_projection_transport_lifecycle.py  # transport lifecycle
python3 scripts/check_websocket_bounds.py                # WS bounds
```

None of these run in `scripts/verify.sh quick`; they are change-triggered.
Run all five when you touch `crates/codegg-protocol/src/projection/`,
`crates/codegg-core/src/projection_replay/`, a transport adapter, or a WS
route. All five pass on the current tree.

`check_projection_transport_lifecycle.py` splits each Rust source at its
test module so `#[cfg(test)]` fixtures are not scanned as production
transport. The gate is any `cfg` predicate mentioning `test`, not just
the bare `#[cfg(test)]` — `src/core/transport/daemon_socket.rs` gates its
module with `#[cfg(all(test, unix))]`.

## Testing

```bash
cargo test -p codegg-core bus::
cargo test -p codegg-protocol projection
cargo test --test collaboration_m001_chat
cargo test --test collaboration_m002_chat_policy
cargo test --test collaboration_m002_chat_tui
cargo test --test collaboration_m003_chat_actions
```

## See Also

- `architecture/bus.md`, `architecture/projection.md`
- `.opencode/skills/core/SKILL.md` — projection request family on the daemon
- `.opencode/skills/server/SKILL.md` — `/tui` WS event/state protocol (no `RenderFrame`)
- `.opencode/skills/tui/SKILL.md` — sidebar/run-tree joins by exact canonical task IDs

## Source verification

Verified 2026-10-06 against `crates/codegg-core/src/bus/{global,events,mod}.rs`,
`crates/codegg-core/src/presence.rs`,
`crates/codegg-core/src/collaboration.rs`,
`crates/codegg-core/src/projection_replay/`,
`crates/codegg-protocol/src/projection/*.rs`,
`src/core/daemon_projection.rs`, `src/core/daemon.rs`,
`src/server/routes/event.rs`, `src/tui/app/state/chat.rs`, and
`scripts/check_projection_*`, `scripts/check_websocket_bounds.py` — with
all five guards executed, not just read. Confirmed correct as written: the
bus-row attribution (`AppEvent` in `events.rs`, registries in `mod.rs`),
the 4096 broadcast capacity, the full projection module inventory
including `consumer`, `controller`, and `replay`, the
`GlobalEventBus::subscribe()` read behind the SSE route, and the four
`collaboration_m00*` test targets (none feature-gated).

Corrected the previous revision's stale note that
`check_projection_transport_lifecycle.py` failed on
`src/core/transport/daemon_socket.rs`; the guard now exits 0 after its
test-module stripping was fixed to match any `cfg` predicate mentioning
`test` (`scripts/check_projection_transport_lifecycle.py:25`), which is
the fact the Static Guards section now records. Added that these five
guards are change-triggered and absent from `scripts/verify.sh quick`.
