# Presence and Observation M001 — Project-Scoped Presence Leases

## Purpose

Daemon-owned ephemeral project-scoped presence for canonical
principals, client attachments, and bounded session/agent activity
summaries, with heartbeat/idle/expiry/reconnect and privacy semantics.
This document covers M001 only (leases). TUI rendering is M002;
authorized read-only observation is M003.

Presence is a **projection of liveness/activity only**. It is never
consulted to authorize an operation, never becomes durable history or
audit truth, and never carries secrets, file content, prompts, or
reasoning.

Long-term requirements: `plans/000-long-term-specification.md#14`,
`#27`; `plans/002-long-term-roadmap.md#phase-7`.
Terminology: `plans/001-terminology-and-domain-model.md#11`
(Presence lease, Principal presence, Session activity).
Roadmap: `plans/subsystems/presence-observation-roadmap.md#M001`.
Closure: `plans/closure/presence-observation/001-status.md`.

## Where It Lives

```
crates/codegg-core/src/presence.rs        # domain service (ephemeral, bounded, task-free)
crates/codegg-core/src/lib.rs             # pub mod presence
crates/codegg-protocol/src/core.rs        # PresenceActivityDto, heartbeat/snapshot/capabilities DTOs,
                                          # CoreRequest::Presence{Capabilities,Heartbeat,SnapshotGet},
                                          # CoreResponse::Presence{HeartbeatAck,Snapshot,Capabilities},
                                          # CoreEvent::PresenceUpdated
crates/codegg-core/src/authorization.rs   # operation_descriptor + representative requests
                                          # (heartbeat/snapshot require project.observe, DirectProject)
src/core/daemon.rs                        # daemon-owned PresenceService, handlers, activity touches,
                                          # disconnect expiry, restart clear
src/core/transport/daemon_socket.rs       # disconnect expiry (Unix socket)
src/server/ws.rs                          # disconnect expiry (WebSocket x3)
tests/presence_m001_leases.rs             # service + daemon boundary tests
```

## How It Works

### Domain service (`codegg-core::presence`)

`PresenceService` is thread-safe (`DashMap` + atomics), task-free, and
strictly bounded. All time is caller-supplied `Instant` (monotonic) plus
wall-clock millis for DTOs, so tests are deterministic.

- **Key**: `(ProjectId, PrincipalId, client_id, session_id?)`.
  Principals/projects are typed identities; client/session locators are
  bounded opaque strings (≤128 bytes, no NUL/control).
- **Activity**: `Active | Idle | Observing | AgentRunning` with rank
  `AgentRunning > Active > Observing > Idle`. Labels are coarse; no
  command text or content is carried.
- **Heartbeat** (`heartbeat` / `heartbeat_dto`): creates or renews one
  contribution, bumping `expires_at = now + lease_ttl` (default 90s)
  and `idle_at = now + idle_after` (default 30s). Carries a
  per-connection monotonic `connection_generation`; a heartbeat older
  than the tracked maximum for that `(project, principal, client)` is
  rejected as `StaleGeneration` and cannot resurrect expired state.
- **Implicit touch** (`touch`): session/agent progress renews with the
  tracked maximum generation, so daemon-internal touches never
  stale-reject against an explicit heartbeat generation.
- **Effective activity**: past `idle_at` a contribution reads as `Idle`
  but stays present until `lease_ttl`.
- **Removal**: `remove` is deterministic (only when stored generation
  equals supplied); `disconnect_client_in_project` expires one
  connection in one project; `remove_client` expires one connection
  across all projects (socket disconnect); `evict_expired` is the
  single bounded cleanup scan (no task-per-lease); `clear` drops
  everything on restart.
- **Snapshot**: per-principal aggregation (max activity rank, sorted
  union of sessions truncated to `max_sessions_per_principal`, client
  count, max timestamp), sorted by principal id, truncated to
  `max_principals_per_project` with a `truncated` flag.
- **Bounds** (defaults): 4096 contributions, 512 projects, 256
  principals/project, 16 sessions/principal. New contributions beyond a
  bound fail as `Capacity` rather than growing memory.
- **Metrics**: `live_leases / tracked_clients / total_heartbeats /
  total_expired / total_stale_rejected / total_capacity_rejected`.
  Gauges only; no identity secrets. Snapshots serialize without
  `token/secret/password/api_key/bearer` substrings (pinned by test).

### Protocol

- `PresenceCapabilities` (global): negotiates `lease_ttl_secs`,
  `idle_after_secs`, `heartbeat_interval_hint_secs` (= ttl/3), and
  bounds. Unknown capabilities degrade to `supported: false`.
- `PresenceHeartbeat { request }` (`project.observe`, DirectProject):
  payload carries only `project_id`, optional `session_id`, `activity`,
  `connection_generation`. Principal/client come from transport
  authority. Replies `PresenceHeartbeatAck { project_id,
  expires_at_ms }` or `presence_capacity` / `presence_stale_generation`
  / `presence_invalid_*`. Denials use `project_not_found` (see below).
- `PresenceSnapshotGet { project_id }` (`project.observe`,
  DirectProject): replies `PresenceSnapshot { snapshot }` after one
  bounded `evict_expired` scan. Denials use `project_not_found`.
- `CoreEvent::PresenceUpdated { project_id }`: liveness hint only, no
  collaborator detail. Receivers re-fetch through the authorized
  snapshot path. Classified `Safe` like other project-scoped notices.
  Older clients that never request the presence capability never see it.

### Daemon boundary

- `CoreDaemon.presence: Arc<PresenceService>` (constructed in
  `with_deps_and_identity`, cleared on restart — never fabricated).
- The M003 gate runs first: heartbeat/snapshot require
  `project.observe` on the resolved project. `authorization_denial`
  maps both to `project_not_found` so unauthorized projects are
  indistinguishable from absent (same code as genuinely absent).
- `resolve_authorization_project` resolves both from the direct
  `project_id` locator; DTOs carry no principal/role/capability (pinned
  by `daemon_request_dtos_carry_no_authority` test).
- **Activity integration** (`note_presence_activity`, best-effort):
  `SessionAttach/Load` success touches `Active`; `TurnSubmit`
  acceptance touches `AgentRunning`. Failures are ignored so session /
  agent correctness never depends on presence.
- **Disconnect**: Unix-socket and all three WebSocket close paths call
  `note_client_disconnected` (`remove_client`) after `unregister`.
  Disconnect shortens/expires contributions; reconnect with a newer
  generation renews without duplicate principal rows; session history
  is untouched (pinned by test).
- **Restart**: `clear()` drops leases + generation tombstones; rebuild
  comes only from active connections afterwards.

## Invariants & Gotchas

- **Presence never authorizes**: it is never consulted in
  `authorize_request` or any execution path. Handoff note from the
  plan is load-bearing.
- **Ephemeral, not durable**: no storage migration, no `STORAGE_LAYOUT`
  bump. `clear()` on restart is the specified behavior.
- **Privacy negatives**: unauthorized heartbeat/snapshot deny as
  `project_not_found`; enumeration is unchanged (presence has no
  listing operation). `PresenceUpdated` carries no membership detail.
- **One principal, many clients/sessions**: aggregation is
  deterministic; tests pin two-clients-one-principal and
  several-sessions-per-principal plus `AgentRunning` rank.
- **No timers/tasks**: `evict_expired` is called before snapshots and
  on the daemon's existing paths; this module spawns no background
  task (high-churn test pins bounded memory).
- **Stale generation**: late heartbeats with old generations reject;
  contended renew/remove is deterministic (exact-generation remove).
- **Core boundary**: `presence.rs` must not gain `crate::agent/tool/
  permission/mcp/plugin/tui/server/client/auth/...` imports (not even
  in doc comments — the guard greps raw text). Run
  `bash scripts/check-core-boundary.sh` after changes.
- **Auth matrix**: adding a `CoreRequest` variant requires
  `operation_descriptor` + `representative_requests` entries; run
  `python3 scripts/check_authorization_matrix.py`.

## Testing

```bash
cargo test -p codegg-core --lib presence
cargo test --test presence_m001_leases
cargo test -p codegg-core --lib authorization
python3 scripts/check_authorization_matrix.py
bash scripts/check-core-boundary.sh
```

Key suites: `tests/presence_m001_leases.rs` (8 service + 7 daemon
tests: heartbeat/expiry/idle, disconnect/reconnect, two-clients,
several-sessions, aggregation, privacy negatives, churn bound,
restart, stale generation, DTO authority negative, capabilities).

## M002 — TUI Collaborator Surface

The TUI renders daemon-owned presence as a bounded per-project
projection. It owns no presence truth.

```
src/tui/app/state/presence.rs      # reducer keyed by project_id + request/reconnect epoch
src/tui/commands/presence.rs       # capability + snapshot fetch (spawn-and-complete)
src/tui/app/mod.rs                 # App.presence, header count, /collaborators, reconnect/switch hooks
src/tui/components/dialogs/info.rs # InfoType::Collaborators (scrollable panel)
src/tui/components/component.rs    # DialogType::Collaborators (focus slot)
src/tui/command.rs                 # /collaborators (/presence, /team) registry
src/tui/input.rs                   # help entries
tests/presence_m002_collaborators.rs
```

- **Reducer** (`PresenceState`): per-project entries with
  `request_id` + `reconnect_epoch` stale guards, `sequence` ordering,
  `needs_resync` flag, and LRU eviction at 16 projects. Snapshots only
  update the matching `project_id` (rapid-switch leak prevention).
  Unauthorized (`project_not_found`) and unsupported (old daemon) both
  clear to `Unavailable` and render identically; transient errors retain
  stale rows with a resync flag.
- **Fetch**: `start_refresh_presence` negotiates `PresenceCapabilities`
  then `PresenceSnapshotGet` in one registered task (`TuiTaskKind::Command`).
  No polling storm: `needs_refresh` coalesces while loading, and
  `PresenceHint` (`PresenceUpdated { project_id }`) only flags resync —
  the active project re-fetches, inactive tabs refresh on foreground.
- **Header**: `presence.header_summary(project_id)` renders `👥 N`
  (plus `· agent running` / `(stale)`) only for authorized `Ready`
  data; unavailable/loading renders nothing (hidden, identical for
  unauthorized and absent).
- **Panel**: `/collaborators` (`/presence`, `/team`; `refresh`
  subcommand forces re-fetch) opens a scrollable info dialog from
  `presence.panel_lines(project_id)`: stable activity-rank + id order,
  32-row display bound with `+N more`, coarse labels only
  (`active`/`idle`/`observing`/`agent running`), empty/loading/error/
  unavailable states. Focus follows the standard info-dialog convention
  (`j`/`k` scroll, `Esc`/`Enter` close); opening never mutates sessions.
- **Lifecycle**: tab switch triggers a bounded refresh for the new
  active project; tab close drops the project when no remaining tab
  holds it; `on_projection_reconnect` bumps both presence and routing
  epochs and re-fetches the active project. Authorization loss clears
  the cache (`clear_project`) so hidden data is never rendered locally.
- **Compatibility**: older daemons without the presence capability show
  the generic unavailable panel; project tabs keep working. No storage
  migration.

```bash
cargo test --test presence_m002_collaborators
cargo test -p codegg --lib tui::app::state::presence
```

## Related Docs

- `architecture/authorization.md` — `project.observe` gate + denial shape
- `architecture/protocol.md` — presence wire DTOs and capability
- `architecture/core.md` — daemon ownership and transports
- `architecture/codegg_core.md` — core module boundary
- `architecture/projection.md` — observation reuses projection replay (M003)
