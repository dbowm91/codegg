# Presence and Observation Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/presence-observation/001-project-presence-leases.md`

Source subsystem roadmap:

- `plans/subsystems/presence-observation-roadmap.md#M001--project-scoped-presence-leases`

Repository baseline reviewed: `3a02ffff5f503b58020d2715ad809669c9dac135`

Implementation commits or pull requests:

- `3a02ffff` — feat(presence): M001 project-scoped presence leases (domain service, protocol DTOs + CoreRequest/Response/Event, auth matrix, daemon handlers + activity touches + disconnect expiry, disconnect wiring, 11 unit + 15 integration tests, presence/protocol/core/authorization docs).

## 1. Executive finding

M001 is closed. The daemon owns ephemeral project-scoped presence
leases for canonical principals, client attachments, and bounded
session/agent activity summaries with heartbeat/idle/expiry/reconnect
and privacy semantics. Authorized members receive bounded accurate
collaborator/session activity; stale disconnects expire; multi-client
aggregation works; restart clears without fabricating durable
presence; unauthorized principals get no project/activity signal.
Presence is never consulted to authorize an operation. No unresolved
high, medium, or low M001 finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Lease/activity DTOs + aggregation rules for multiple clients/sessions (package A) | `PresenceActivityDto`, `PresenceHeartbeatRequestDto`, `PresencePrincipalDto`, `PresenceSnapshotDto`, `PresenceCapabilitiesDto` in `crates/codegg-protocol/src/core.rs`; `PresenceActivity` rank `AgentRunning > Active > Observing > Idle`, per-principal max-rank + sorted session union + client count + max timestamp in `crates/codegg-core/src/presence.rs::snapshot` | pass | Labels coarse; no content/secrets. |
| Daemon presence service with monotonic expiry/idle + bounded cleanup (package B) | `PresenceService` (`heartbeat`/`touch`/`remove`/`disconnect_client_in_project`/`remove_client`/`evict_expired`/`clear`/`snapshot`/`metrics`); `lease_ttl` 90s / `idle_after` 30s defaults; bounds 4096 contributions / 512 projects / 256 principals/project / 16 sessions/principal; single `evict_expired` scan, no spawned task | pass | Time is caller-supplied `Instant` for determinism. |
| Connection/session/agent activity updates without correctness coupling (package C) | `CoreDaemon::note_presence_activity` (best-effort `touch` with tracked generation); `SessionAttach/Load` touches `Active`; `TurnSubmit` touches `AgentRunning`; failures ignored | pass | Session/agent systems do not depend on presence. |
| Authorized snapshot/subscription or event integration + privacy negatives (package D) | `PresenceCapabilities` / `PresenceHeartbeat` / `PresenceSnapshotGet` (`project.observe`, DirectProject); `PresenceHeartbeatAck` / `PresenceSnapshot` / `PresenceCapabilities` responses; `PresenceUpdated { project_id }` liveness hint (no collaborator detail, `Safe` like other project notices); denials map to `project_not_found` | pass | Snapshot requires `evict_expired` first; event receivers re-fetch via authorized snapshot. |
| Restart/churn/bounds docs + tests (package E) | `architecture/presence.md`; `architecture/protocol.md`, `core.md`, `codegg_core.md`, `authorization.md` updates; 11 unit + 15 integration tests (see §4) | pass | No storage migration; restart `clear()` specified. |

## 3. Production implementation evidence

- `crates/codegg-core/src/presence.rs` (new, ~1100 lines with 11
  tests): `PresenceActivity` + `parse`/`rank`, `PresenceConfig`
  (ttl/idle/bounds + `capabilities_dto`), `PresenceError`
  (`InvalidInput`/`Capacity`/`StaleGeneration`), `PresenceService`
  with `DashMap` contributions + bounded generation-tombstone table +
  atomic gauges, `heartbeat` (stale-generation reject, bound
  enforcement), `touch` (tracked-generation renewal, never
  stale-rejects), deterministic `remove`, `disconnect_client_in_project`,
  `remove_client`, `evict_expired` (single scan), `clear` (restart),
  `snapshot` (bounded per-principal aggregation), `metrics` (no
  secrets), `empty_snapshot`.
- `crates/codegg-core/src/lib.rs`: `pub mod presence;`
  (boundary-clean: only `identity`, `dashmap`, `serde`, `thiserror`,
  `codegg-protocol` used).
- `crates/codegg-protocol/src/core.rs`: presence DTOs +
  `CoreRequest::Presence{Capabilities,Heartbeat,SnapshotGet}` +
  `CoreResponse::Presence{HeartbeatAck,Snapshot,Capabilities}` +
  `CoreEvent::PresenceUpdated { project_id }`. Additive; old clients
  that never request the capability never see the variants.
- `crates/codegg-core/src/authorization.rs`: `operation_descriptor`
  entries (capabilities global; heartbeat/snapshot `DirectProject` +
  `project.observe`) + 3 `representative_requests` rows. Adding a
  `CoreRequest` variant remains a compile error until classified;
  `scripts/check_authorization_matrix.py` passes.
- `crates/codegg-core/src/projection_replay/safe_publication.rs`:
  `PresenceUpdated` classified `Safe` (project id only, no
  collaborator detail; receivers re-authorize via snapshot).
- `src/core/daemon.rs`: `presence: Arc<PresenceService>` owned by
  `CoreDaemon` (constructed in `with_deps_and_identity`);
  `authorization_denial` maps heartbeat/snapshot denials to
  `project_not_found`; `resolve_authorization_project` resolves both
  from direct `project_id`; `PresenceCapabilities`/`PresenceHeartbeat`
  (transport-derived principal/client, `presence_stale_generation` /
  `presence_capacity` / `presence_invalid_*` codes,
  `PresenceUpdated` publish)/`PresenceSnapshotGet` (bounded evict +
  snapshot) arms; `note_presence_activity` + `note_client_disconnected`
  helpers; `SessionAttach/Load` → `Active`, `TurnSubmit` →
  `AgentRunning` touches.
- `src/core/transport/daemon_socket.rs` + `src/server/ws.rs` (3
  close paths): `note_client_disconnected` (`remove_client`) after
  every `unregister`. Disconnect shortens/expires contributions;
  reconnect with newer generation renews without duplicates.
- `src/core/mod.rs`: no change needed (`core_event_type` wildcard
  covers `PresenceUpdated` as `other`; presence events are not
  persisted via `should_persist`).
- `tests/presence_m001_leases.rs` (new, 15 tests): 8 service
  (heartbeat/expiry/idle, two-clients, sessions, aggregation,
  disconnect/reconnect, stale-generation, restart, churn bound) + 7
  daemon boundary (capabilities, round-trip, privacy negatives,
  DTO-authority negative, disconnect/history survival, restart,
  stale-generation, two-clients over protocol).
- Docs: `architecture/presence.md` (new, ownership/state
  machine/protocol/boundary/invariants/testing); presence sections in
  `architecture/protocol.md`, `core.md`, `codegg_core.md`
  (module table + related link), `authorization.md` (privacy note + 3
  matrix rows).

Distinguished as absent (downstream milestones, not M001 scope): TUI
collaborator rendering (M002), authorized read-only observation over
projection replay (M003), chat typing/read markers, durable history,
distributed node presence.

## 4. Verification executed

All local (no hosted `CI / verify` claimed).

### Commands run

```bash
cargo test -p codegg-core --lib presence
cargo test -p codegg-core --lib authorization
cargo test --lib client_registry
cargo test -p codegg-protocol --lib
cargo test --test presence_m001_leases
cargo test --test identity_m003_daemon_authorization
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_authorization_matrix.py
bash scripts/check-core-boundary.sh
bash scripts/check_projection_disclosure.sh
python3 scripts/check_project_catalog_invariants.py
bash scripts/verify.sh quick
```

### Results

- `cargo test -p codegg-core --lib presence`: 11 passed.
- `cargo test -p codegg-core --lib authorization`: 21 passed.
- `cargo test --lib client_registry`: 8 passed (existing
  register/count/unregister/attach/session/name/principal-immutability
  suite still green after disconnect-expiry wiring).
- `cargo test -p codegg-protocol --lib`: 177 passed.
- `cargo test --test presence_m001_leases`: 15 passed (8 service + 7
  daemon).
- `cargo test --test identity_m003_daemon_authorization`: 9 passed
  (M003 regression still green after matrix + denial-shape additions).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass.
- `check_authorization_matrix.py`: all invariants verified.
- `check-core-boundary.sh`: passed (one doc-comment false positive
  fixed by avoiding a raw `crate::authorization` token in
  `presence.rs` comments).
- `check_projection_disclosure.sh`: OK.
- `check_project_catalog_invariants.py`: 7/7 passed.
- `scripts/verify.sh quick`: passed (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution-ownership,
  `cargo check --workspace --all-targets --locked`).

Plan §11 lists `cargo test --workspace presence` and `cargo test
--workspace client_registry`: no such workspace crates exist, so the
justified substitutes above were used (`-p codegg-core --lib
presence`, `--test presence_m001_leases`, `--lib client_registry`).
No verification was skipped without a substitute.

## 5. Invariant review

| Plan invariant | Evidence | Result |
|---|---|---|
| Presence is ephemeral; membership/authorization is durable authority | `clear()` on restart; no migration/`STORAGE_LAYOUT` change; `AuthorizationService` + `TeamStore` decide visibility; presence never appears in `authorize_request` | pass |
| Unauthorized projects indistinguishable from absent | Heartbeat/snapshot denials map to `project_not_found`; `daemon_unauthorized_project_is_indistinguishable_from_absent` pins outsider == absent code for both ops and zero side effect | pass |
| One principal may have multiple clients/sessions | `two_clients_one_principal_aggregate` (service + protocol), `several_sessions_aggregate_and_agent_rank_wins` | pass |
| Lease expiry cannot delete session history | `daemon_disconnect_expires_without_deleting_history` asserts session row survives `remove_client`; `evict_expired` touches only the presence tables | pass |
| High churn cannot create unbounded timers/tasks/maps | No `tokio::spawn` in `presence.rs`; one contribution map + one bounded tombstone table; `high_churn_stays_bounded` (64 heartbeats into cap-8 table, single-scan reclaim) | pass |
| Presence never authorizes | Handoff note enforced: no `presence` read in any `authorize*` path; activity touches are write-only best-effort | pass |

## 6. Failure and recovery review

- Duplicate delivery: heartbeats are idempotent renewals keyed by
  `(project, principal, client, session)`; reconnect with newer
  generation renews without duplicate principal rows (pinned).
- Cancellation races: N/A (no async lease tasks exist).
- Daemon restart: `clear()` drops leases + tombstones; snapshot after
  restart is empty; rebuild comes only from active connections
  (pinned by service + daemon restart tests).
- Partial persistence failure: N/A (no durable presence writes).
- Stale generation/lease: expired rows evicted by single scan; late
  heartbeat with old generation rejects as `presence_stale_generation`
  and cannot resurrect (pinned service + daemon); contended
  renew/remove deterministic via exact-generation `remove` (pinned).
- Contention/resource release: capacity rejects as
  `presence_capacity` instead of growing; generation tombstones
  bounded to `max_contributions` via `bound_generation_tracker`.
- Malformed input: overlong/NUL/control locators reject as
  `presence_invalid_input`/`presence_invalid_session`; unknown
  activity strings fail closed (`parse` → `None`); unknown audit-style
  filters N/A.
- Bounded event/artifact behavior: `PresenceUpdated` carries only
  `project_id`; snapshots truncate principals/sessions with
  `truncated` flag; metrics carry gauges only.

## 7. Migration and compatibility review

- No durable migration: no schema change, no `STORAGE_LAYOUT_VERSION`
  bump, `check_project_catalog_invariants.py` 7/7 green.
- Protocol is additive: 3 requests + 3 responses + 1 event; existing
  client snapshots remain compatible (new fields are new variants,
  not changed shapes). Older clients that never request the presence
  capability never see the variants; `PresenceCapabilities` negotiates
  bounds. No `PROTOCOL_VERSION` bump (same additive convention as
  prior milestones).
- Config: no new settings surface; lease intervals are
  `PresenceConfig` defaults (90s/30s) with capability-advertised hints.
- Rollback: dropping the daemon binary clears all presence (ephemeral
  by design); no data to roll back.

## 8. Security review

- Authorization precedes lookup: M003 gate enforces `project.observe`
  before any presence read/write; `resolve_authorization_project`
  uses the direct `project_id` locator.
- Principal derived from connection: `request_authority_for_client`
  supplies principal + `trusted_client_id`; DTOs carry no
  principal/role/capability/client identity (pinned by
  `daemon_request_dtos_carry_no_authority` at wire shape + nested
  body).
- Privacy: single-project denials use `project_not_found` for both
  heartbeat and snapshot; outsider == absent pinned; events carry no
  membership detail.
- Secrets: snapshots/metrics contain ids/activity/timestamps only;
  `snapshots_carry_no_secrets` pins absence of
  `token/secret/password/api_key/bearer` substrings.
- Path validation: client/session locators length + NUL/control
  checked; project/principal use the typed identity lexical contract.
- DoS bounds: contribution/project/principal/session caps + 128-byte
  locators + bounded snapshot/event shapes; no per-lease task to
  exhaust.
- Audit: presence reads/writes intentionally emit no audit events
  (liveness projection, not durable control-plane action); M003 gate
  denials still flow through the existing denial-audit path.

## 9. Documentation and operations

- New: `architecture/presence.md` (purpose, ownership, state machine,
  protocol, boundary, invariants, testing, related docs).
- Updated: `architecture/protocol.md` (ephemeral/non-authoritative
  note + related link), `architecture/core.md` (presence request
  family), `architecture/codegg_core.md` (module table + related
  link), `architecture/authorization.md` (privacy note + 3 matrix
  rows).
- Terminology cross-links: plan §11 terms (`Presence lease`,
  `Principal presence`, `Session activity` at
  `plans/001-terminology-and-domain-model.md#11`) already cover this
  milestone; no terminology change needed.
- Operator diagnostics: `PresenceCapabilities` (bounds + hints),
  `PresenceMetrics` gauges (`live_leases/tracked_clients/...`),
  `presence_stale_generation` / `presence_capacity` codes.
- Static guards: `check_authorization_matrix.py`,
  `check-core-boundary.sh` both green; no new CI lane added per
  verification policy.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

No critical/high/medium/low findings remain. The `crate::authorization`
doc-token guard trip was fixed in-tree before closure (comment
reworded; guard green).

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. Presence M002 (TUI
collaborator surface) is unblocked to `ready`; M003 remains blocked on
M002 (session-projections M012 interface already satisfied).
Project-collaboration M001 remains blocked on presence-observation
M003 (unchanged).

## 12. Registry updates

- `plans/registry.md`: Presence row `M001 ready` → `M002 ready`;
  dependency-ready table: remove M001 presence-leases row, add M002
  collaborator-surface row (unblocked by this closure); execution
  order §3 reworded (M001 closed, M002 may proceed); blocked work:
  remove `Presence M002` row, narrow `Presence M003` blocker to M002
  closure; closure control points: add M001 closed row.
- `plans/subsystems/presence-observation-roadmap.md`: M001 `ready` →
  `closed` with closure link; M002 `blocked` → `ready` with plan link
  and cleared blocker.
- `plans/implementation/presence-observation/001-project-presence-leases.md`:
  `ready for handoff` → `implemented` with closure link.
- `plans/implementation/presence-observation/002-tui-collaborator-presence-surface.md`:
  `blocked` → `ready for handoff` with unblocked-by-M001 note.
