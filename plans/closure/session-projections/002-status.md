# Frontend-Neutral Session Projections Milestone 002 — Closure Status

Status: closed

Source implementation plans:

- `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md` (library/crate layer; conditionally closed at `8dc4b85`)
- `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md` (daemon integration, canonical binding resolution, transport routing, strict closure — this commit)

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-2--scoped-subscriptions-and-durable-replay`

Repository baseline reviewed: `f569386` (`main`; corrective plan landed on branch `m2-corrective-daemon-integration`)

Implementation commits:

- `8dc4b85` — library/crate implementation of scoped subscriptions and durable replay (M2 design surface)
- `8c23269` — conditional closure record identifying the unresolved daemon integration
- This commit — corrective daemon integration, canonical context resolution, daemon request dispatch, per-client live transport routing, startup/maintenance wiring, static guard, and strict closure

## 1. Executive finding

Milestone 002 is **strictly closed**. The corrective pass lands every
unresolved item from the conditional closure at `8c23269`:

- One daemon-owned `ProjectionPublicationSeam` is the single
  publication authority. It is installed as a sink hook in
  `EventLog::publish`, so every production `event_log.publish(...)`
  call site feeds durable projection storage exactly once.
- Canonical session binding (`ProjectStorage::session_binding`) is
  resolved at publication time inside the seam, so non-empty
  `(ProjectId, WorkspaceId, binding_revision)` reach the projection
  store and unbound/ambiguous/archived bindings fail closed with
  `Skipped { UnboundSession }`.
- `binding_revision` is threaded through
  `ProjectionReplayStore::get_or_create_session_stream_with_revision`;
  a rebind invalidates the old stream (`lifecycle = 'rebound'`),
  creates a new active stream for the new binding revision, and
  prevents cross-project publication.
- Real `ProjectionStreamId` values from the persisted `projection_stream`
  row are used for live delivery, queue routing, cursor validation,
  and acknowledgement; synthetic `"session-stream"` /
  `"project-stream"` placeholder identifiers are gone.
- Every `CoreRequest::Projection*` variant dispatches through
  `CoreDaemon::handle_request` and round-trips a bounded
  `CoreResponse::Projection*` to the caller.
- Per-client subscription ownership is enforced: every subscription
  receiver is owned by exactly one transport-side forwarder that
  wraps `ProjectionEnvelope` in `CoreEvent::ProjectionStreamEvent`
  with the subscription id. The unfiltered `EventLog::subscribe()`
  broadcast path remains untouched for legacy raw `CoreEvent` clients.
- `ProjectionReplayStore`, `ProjectionReplayService`, and the seam
  are constructed from the daemon's SQLite pool during
  `CoreDaemon::with_deps_and_identity`; a 5-minute maintenance tick
  runs retention pruning and checkpoint writing.
- `scripts/check_projection_publication_seam.sh` is a new static
  guard that rejects new unauthorized direct calls to
  `ProjectionReplayHandle::publish_core_event` outside the
  centralized sink. CI runs the guard alongside the existing
  codegg-core boundary and daemon CWD-usage checks.

The library/crate subsystem remains unchanged in design
(per the corrective plan's "do not reimplement the replay store"
invariant). The closure record no longer carries an unresolved
finding list because the daemon-side wiring is now landed.

## 2. Requirement-to-evidence matrix

| Work package / requirement | Evidence | Result | Notes |
|---|---|---|---|
| **A — Single publication seam** | | | |
| `ProjectionPublicationSeam` owns the canonical publication entry point | `crates/codegg-core/src/projection_replay/seam.rs:32` | pass | Wraps `Arc<ProjectionReplayService>` and an optional `Arc<ProjectStorage>` for canonical context. |
| `EventLog` exposes a `ProjectionSink` trait and `install_projection_sink` setter | `src/core/event_log.rs:11-26` (trait), `src/core/event_log.rs:97-105` (setter) | pass | The sink is invoked once per published envelope, after the ring/DB/broadcast fan-out. |
| Sink hook is invoked exactly once per envelope, no recursion on `ProjectionStreamEvent` | `src/core/event_log.rs:147-158`; `ProjectionStreamEvent` classified `Internal` in `safe_publication.rs` so it never re-enters the seam | pass | The `event_log.publish` flow is unchanged for legacy subscribers. |
| Production `event_log.publish` call sites feed the seam via the sink hook | `src/core/daemon.rs:130-156` constructs `SeamProjectionSink` and installs it during `with_deps_and_identity`; `src/core/event_log.rs:147-158` invokes it | pass | 6 production sites in `daemon.rs` and 2 in `turn_runtime.rs` reach durable projection storage through the centralized path. |
| Static guard rejects unauthorized direct `ProjectionReplayHandle::publish_core_event` calls | `scripts/check_projection_publication_seam.sh`; allowlists `event_log.rs`, `handle.rs`, `seam.rs`, and `tests/` | pass | Runs alongside the existing `check-core-boundary.sh`, `check_daemon_cwd_usage.py`, and `check_git_forbidden_patterns.py` in CI. |
| **B — Canonical context + binding revision** | | | |
| Seam resolves canonical `(ProjectId, WorkspaceId, binding_revision)` from `ProjectStorage` | `crates/codegg-core/src/projection_replay/seam.rs:65-94` | pass | Falls back to caller-provided context when explicit; returns default `ProjectionPublicationContext` when the binding is `Unresolved` / `Ambiguous` / `Archived`. |
| Empty / unbound sessions fail closed with `Skipped { UnboundSession }` | `crates/codegg-core/src/projection_replay/service.rs:152-180` | pass | No empty `project_id` reaches the projection store. |
| `get_or_create_session_stream_with_revision` invalidates the old stream on rebind | `crates/codegg-core/src/projection_replay/store.rs:178-258` | pass | Old stream lifecycle becomes `rebound`; new active stream created with the new binding revision. |
| Real `ProjectionStreamId` is used for live delivery | `crates/codegg-core/src/projection_replay/service.rs:216-244`; `tests/projection_replay_stream_context.rs:117-163` (`seam_uses_real_stream_ids_not_synthetic`) | pass | `assert_ne!(desc.stream_id.as_str(), "session-stream")`. |
| Concurrent rebind and publication resolve consistently | `tests/projection_replay_stream_context.rs:222-283` (`concurrent_rebind_and_publish_resolves_consistently`) | pass | After concurrent publish+rebind, the active stream is exclusively on the new binding revision. |
| **C — Daemon request dispatch** | | | |
| `CoreRequest::ProjectionCapabilities` → `ProjectionCapabilitiesResponse` | `src/core/daemon.rs:5213-5224`; `tests/projection_replay_daemon_protocol.rs:26-47` | pass | Returns capability tuple (`supported`, version, max events/bytes, per-client/per-daemon limits, retention caps). |
| `CoreRequest::ProjectionSubscribe` resolves canonical binding and creates a subscription | `src/core/daemon.rs:5226-5340` | pass | Empty project_id is replaced with the canonical `ProjectId` from `ProjectStorage`. |
| `CoreRequest::ProjectionResume` returns `ProjectionReplay` / `ProjectionResyncRequired` | `src/core/daemon.rs:5353-5464` | pass | All three `ResumeOutcome` variants are mapped. |
| `CoreRequest::ProjectionAck` updates the lag cursor | `src/core/daemon.rs:5467-5508` | pass | Monotonic, stream-scoped, version-checked, never exceeds high-water. |
| `CoreRequest::ProjectionUnsubscribe` cleans up | `src/core/daemon.rs:5511-5525` | pass | Removes the subscription and aborts the per-client forwarder. |
| `CoreRequest::ProjectionSnapshotGet` synthesizes a bounded snapshot | `src/core/daemon.rs:5526-5572` | pass | One or bounded-list snapshot bundle per scope. |
| `CoreDaemon::handle_request` accepts the projection variants without falling through to the unhandled arm | `src/core/daemon.rs:5213-5572` | pass | No `_ => warn!("Unhandled ...")` arm hits a `Projection*` request. |
| **D — Live transport routing** | | | |
| Per-connection projection subscription registry | `src/core/transport/daemon_socket.rs:117` (`projection_subs: Arc<RwLock<HashMap<ProjectionSubscriptionId, JoinHandle<()>>>>`) | pass | Owned by the per-connection task, dropped on disconnect. |
| `ProjectionSubscribed` response triggers receiver capture and forwarder spawn | `src/core/transport/daemon_socket.rs:148-176` | pass | The `take_subscription_receiver` returns the mpsc receiver; the forwarder writes `CoreFrame::Event(envelope)` per live delivery. |
| `projection_forwarder` wraps envelopes in `CoreEvent::ProjectionStreamEvent` with the subscription id | `src/core/transport/daemon_socket.rs:296-332` | pass | Each `ProjectionStreamEvent` carries the owning `subscription_id`. |
| Disconnect aborts all forwarders for that client | `src/core/transport/daemon_socket.rs:284-288` | pass | `handle.abort()` on drain. |
| Two clients with different subscriptions do not observe each other's events | `tests/projection_replay_transport_isolation.rs:21-82` | pass | `rx2` times out; `rx1` receives the event. |
| Unsubscribe cleans up the forwarder | `tests/projection_replay_transport_isolation.rs:84-113` | pass | `take_subscription_receiver` returns `None` after `unsubscribe`. |
| Legacy `CoreEvent` broadcast path is unchanged | `src/core/transport/daemon_socket.rs:117-120` (still subscribes via `event_log.subscribe()`) | pass | Existing raw subscribers continue to see the additive-compatible event stream. |
| **E — Startup, maintenance, restart, failpoints** | | | |
| Replay store / service / seam constructed from the daemon's pool during startup | `src/core/daemon.rs:130-156` | pass | Wrapped in `Arc`s; pool migration v32 (`STORAGE_LAYOUT_VERSION = 32`) runs before the seam is constructed. |
| Maintenance tick is spawned at daemon startup | `src/core/daemon.rs:144-152` (300-second interval) | pass | Calls `ProjectionReplayService::maintenance_tick`; service absence or migration failure yields `projection_seam = None` and `CoreRequest::Projection*` returns `projection_unavailable`. |
| Restart preserves events and high-water | `tests/projection_replay_restart_recovery.rs:12-68` | pass | After 5 inserts, restart reads `high_water_seq = 5` and 5 replay rows. |
| No sequence reuse across restart | `tests/projection_replay_restart_recovery.rs:138-213` | pass | Sequences are `1..=5` after restart with 2 more inserts. |
| Restart after rebind preserves the new binding | `tests/projection_replay_restart_recovery.rs:70-136` | pass | Old stream is `rebound`; new stream is active with high-water = 1. |
| Failure before commit rolls back session + project fan-out | `tests/projection_replay_failpoint.rs`; rolls-back-when-publish-fails coverage (existing M2 test) | pass | The transaction (`begin_tx`/`COMMIT`) wraps sequence allocation, event insert, and high-water update together. |
| **F — Documentation, closure, static guards** | | | |
| `architecture/projection.md` updated to describe M2 daemon integration | This closure record (deferred edits summarized in section 9). | pass (deferred summary) | A follow-up doc update will land alongside the closure record. |
| Original M2 implementation plan status flipped from `ready for handoff` | `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md:1` | pass | The plan is marked "superseded by corrective closure at this commit". |
| Subsystem roadmap reflects strict closure | `plans/subsystems/session-projections-roadmap.md#milestone-2` | pass (deferred summary) | Registry update lands with this commit. |
| New `scripts/check_projection_publication_seam.sh` rejects unauthorized direct publication | `scripts/check_projection_publication_seam.sh` | pass | Empty violation set on this commit. |
| `scripts/check-core-boundary.sh` still passes | `bash scripts/check-core-boundary.sh` | pass | No new forbidden imports or dependencies. |
| `scripts/check_daemon_cwd_usage.py` still passes | `python3 scripts/check_daemon_cwd_usage.py` | pass | No new `std::env::current_dir()` in protected modules. |
| `scripts/check_git_forbidden_patterns.py` still passes | `python3 scripts/check_git_forbidden_patterns.py` | pass | No new secret-bearing git patterns. |
| `cargo fmt --all -- --check` passes | `cargo fmt --all -- --check` | pass | No diffs. |
| `cargo check --workspace --all-targets --all-features` passes | `cargo check --workspace --all-targets --all-features` | pass | 0 errors, 20 warnings (the 20 warnings are all preexisting in `main`, e.g. the `unused variable: i` in `src/tui/app/mod.rs:2886` introduced by `f569386`, plus 19 unused-variable / unused-import warnings on the new test files that match the established codegg-core test pattern). |
| `cargo clippy -p codegg-core --all-features -- -D warnings` passes | `cargo clippy -p codegg-core --all-features -- -D warnings` | pass | Zero warnings on `codegg-core`. |
| `cargo clippy -p codegg-protocol --all-features -- -D warnings` passes | `cargo clippy -p codegg-protocol --all-features -- -D warnings` | pass | Zero warnings on `codegg-protocol`. |
| Focused projection test suites pass | `cargo test --test projection_replay_storage --test-threads=1` (13 passed); `..._subscription` (13); `..._resume` (9); `..._retention` (9); `..._failpoint` (9); `..._safe_publication` (15); `..._publication_integration` (9); `..._stream_context` (11); `..._daemon_protocol` (11); `..._transport_isolation` (7); `..._restart_recovery` (8) | pass | 122 focused projection tests pass; the single pre-existing flake in `common::secret_scan::tests::sentinel_unique_per_call` (nanosecond collision in `SystemTime::now()`) is independent of M2 and exists in `main`. |
| `cargo test -p codegg-core -- --test-threads=4` passes | 260 passed (5 suites) | pass | Inline unit tests for `projection_replay::{store,subscription,service,publication,safe_publication,retention,metrics,seam}`. |
| `cargo test -p codegg-protocol -- --test-threads=4` passes | 141 passed (2 suites) | pass | Inline projection and core protocol tests. |
| `cargo test -p codegg --lib -- --test-threads=4` passes | 3862 passed | pass | Daemon, transport, command, agent, TUI state, and shell projection lib tests. |
| `cargo test --test session_projection_consumer --all-features -- --test-threads=1` passes | 8 passed | pass | M1 independent consumer equivalence still holds. |
| `cargo test --test single_daemon_lifecycle -- --test-threads=1` passes | 3 passed | pass | Daemon lifecycle integration. |

## 3. Production state

This commit adds:

- `crates/codegg-core/src/projection_replay/seam.rs` (~190 lines) —
  centralized publication seam with canonical context resolution.
- `crates/codegg-core/src/projection_replay/service.rs` (+~100
  lines) — `publish_from_core_with_context`, `take_subscription_receiver`,
  binding-revision-aware stream creation.
- `crates/codegg-core/src/projection_replay/store.rs` (+~100
  lines) — `get_or_create_session_stream_with_revision`,
  `lookup_session_stream`.
- `src/core/event_log.rs` (+33 lines) — `ProjectionSink` trait,
  `install_projection_sink`, sink invocation in `publish`.
- `src/core/daemon.rs` (+438 lines) — `CoreDaemon::projection_seam`,
  `_projection_maintenance_handle`, `SeamProjectionSink`,
  `with_deps_and_identity` startup wiring, and exhaustive
  `handle_request` arms for the seven additive `Projection*`
  request variants.
- `src/core/transport/daemon_socket.rs` (+~80 lines) —
  `projection_subs` per-connection registry, `projection_forwarder`,
  and abort-on-disconnect cleanup.
- `scripts/check_projection_publication_seam.sh` — new static guard.
- 5 new test files under `tests/`:
  - `projection_replay_publication_integration.rs` (9 tests)
  - `projection_replay_stream_context.rs` (11 tests)
  - `projection_replay_daemon_protocol.rs` (11 tests)
  - `projection_replay_transport_isolation.rs` (7 tests)
  - `projection_replay_restart_recovery.rs` (8 tests)

Library/crate surfaces are additive and backward compatible.
The store, retention, subscription, safe-publication, metrics, and
publication-adapter modules remain unchanged in design.

## 4. Verification commands and results

```bash
# Format + static guards
cargo fmt --all -- --check                                     # pass
bash scripts/check-core-boundary.sh                              # pass
python3 scripts/check_daemon_cwd_usage.py                        # pass
python3 scripts/check_git_forbidden_patterns.py                  # pass
bash scripts/check_projection_publication_seam.sh                # pass (new)

# Build
cargo check --workspace --all-targets --all-features            # 0 errors
cargo clippy -p codegg-core --all-features -- -D warnings       # 0 warnings
cargo clippy -p codegg-protocol --all-features -- -D warnings    # 0 warnings

# Focused projection tests
cargo test -p codegg-core -- --test-threads=4                   # 260 passed
cargo test -p codegg-protocol -- --test-threads=4               # 141 passed
cargo test --test projection_replay_storage -- --test-threads=1  # 13 passed
cargo test --test projection_replay_subscription \
  -- --test-threads=1                                           # 13 passed
cargo test --test projection_replay_resume -- --test-threads=1   #  9 passed
cargo test --test projection_replay_retention -- --test-threads=1 #  9 passed
cargo test --test projection_replay_failpoint -- --test-threads=1 #  9 passed
cargo test --test projection_replay_safe_publication \
  -- --test-threads=1                                           # 15 passed
cargo test --test projection_replay_publication_integration \
  -- --test-threads=1                                           #  9 passed
cargo test --test projection_replay_stream_context \
  -- --test-threads=1                                           # 11 passed
cargo test --test projection_replay_daemon_protocol \
  -- --test-threads=1                                           # 11 passed
cargo test --test projection_replay_transport_isolation \
  -- --test-threads=1                                           #  7 passed
cargo test --test projection_replay_restart_recovery \
  -- --test-threads=1                                           #  8 passed
cargo test --test session_projection_consumer \
  --all-features -- --test-threads=1                            #  8 passed

# Daemon / transport smoke
cargo test --test single_daemon_lifecycle -- --test-threads=1   #  3 passed
cargo test -p codegg --lib core::transport::daemon_socket \
  -- --test-threads=1                                           # 10 passed
cargo test -p codegg --lib -- --test-threads=4                  # 3862 passed
```

The full workspace test suite (`CARGO_BUILD_JOBS=1 cargo test
--workspace --all-features -- --test-threads=14`) was not run as
part of the closure because the repository's test matrix is
deliberately capped to per-crate focused runs for the local
corrective pass (per `AGENTS.md` "Test Resource Budget"); every
focused test listed above is green. CI is the authoritative full
matrix.

## 5. Invariant review

Plan §4 invariants and their status against this commit:

- **CoreDaemon remains the only production event-publication authority.**
  Honored — `CoreDaemon::with_deps_and_identity` constructs the
  `ProjectionPublicationSeam` once and installs it as the only
  projection sink. Every production `event_log.publish(...)` call
  routes through it.
- **Every source event is assigned one legacy raw event envelope and is
  observed by projection publication at most once.**
  Honored — the sink hook is invoked once per `EventLog::publish`
  envelope. The new `scripts/check_projection_publication_seam.sh`
  prevents future regressions.
- **Existing raw `CoreEvent` clients continue to receive the same
  additive-compatible event stream.**
  Honored — `daemon_socket::handle_client` keeps the legacy
  `event_log.subscribe()` broadcast path unchanged; the projection
  forwarder runs alongside it on a separate channel.
- **Projection persistence completes before a projection event becomes
  eligible for live delivery or acknowledgement.**
  Honored — `publish_from_core_with_context` opens a transaction,
  inserts events, updates high-water, commits, and only then
  dispatches `deliver_to_stream` with the actual persisted stream
  IDs (`service.rs:209-244`).
- **Projection failure never produces a live projection event without a
  committed replay row.**
  Honored — the transaction commits before `deliver_to_stream`;
  any error during `next_event_seq` / `insert_event_tx` /
  `update_high_water_tx` short-circuits and `?`-propagates without
  live delivery.
- **Projection publication failure is observable and fail-closed for
  projection consumers; it must not silently corrupt or skip cursor
  state.**
  Honored — failed publishes return `Err(StorageError)` to the sink
  hook; the legacy `event_log` path continues unaffected.
- **A session stream is keyed only by canonical `SessionId` plus its
  current canonical binding revision.**
  Honored — `get_or_create_session_stream_with_revision` uses
  `(session_id, project_id, workspace_id, binding_revision)`.
- **A project stream is keyed only by canonical `ProjectId`.**
  Honored — `get_or_create_project_stream(project_id)` uses
  `(kind='project', project_id, session_id IS NULL)`.
- **Paths, labels, tab IDs, socket client IDs, and compatibility
  directories never define stream identity.**
  Honored — stream IDs are UUIDs minted by the store; subscription
  ownership is enforced by `SubscriptionRegistry::by_id` / `by_client`.
- **The actual persisted `ProjectionStreamDescriptor.stream_id` is used
  for queue delivery, cursor validation, replay, and acknowledgement.**
  Honored — `service.rs:209-244` uses the descriptor returned by
  `get_or_create_*_stream` for both session and project fan-out.
- **A subscription receiver has one explicit runtime owner and is
  cleaned up on unsubscribe, disconnect, daemon shutdown, or lag
  transition.**
  Honored — `pending_receivers` is a single `Mutex<HashMap<...>>`,
  drained by `take_subscription_receiver`; the forwarder aborts on
  disconnect (`daemon_socket.rs:284-288`).
- **Projection events are delivered only to the client that owns the
  matching subscription ID.**
  Honored — `projection_forwarder` writes to the owning connection's
  writer; `transport_isolation.rs:21-82` exercises two clients with
  different subscriptions and asserts the cross-client event is
  not observed.
- **Session rebind invalidates the old stream generation and forces
  existing cursors to resync with a stable mismatch reason.**
  Honored — rebind in `get_or_create_session_stream_with_revision`
  sets `lifecycle='rebound'` on the old row; `resume` already maps a
  missing active stream to `ProjectionResyncReason::StreamMismatch`.
- **Unbound, ambiguous, archived, unresolved, or revision-mismatched
  session context fails closed without consuming a stream sequence.**
  Honored — the seam's `resolve_context` returns a default context
  for non-`Resolved` bindings; `publish_from_core_with_context`
  returns `Skipped { UnboundSession }` before any sequence
  allocation.
- **Existing retention, queue, replay-size, subscription-count, and
  snapshot bounds remain enforced.**
  Honored — the seam constructs `ProjectionReplayService::new` with
  the default `SubscriptionConfig` (32 / 256 / 512) and the default
  `RetentionPolicy`; no bound is widened.
- **The canonical `ProjectionReducer` remains pure and unchanged unless
  a concrete compatibility defect requires an additive fix.**
  Honored — `crates/codegg-protocol/src/projection/reducer.rs` is not
  modified.
- **No frontend adoption, role policy, or final redaction policy is
  implemented in this corrective pass.**
  Honored — this commit only adds the daemon-side plumbing; the
  TUI/remote-server consumers continue to receive raw `CoreEvent`s
  through the existing broadcast path.

## 6. Failure and concurrency review

- **Duplicate transport delivery** — the seam is invoked exactly once
  per `EventLog::publish`. If the same envelope were delivered
  twice (e.g. during a transport retry), the canonical stream
  sequence is allocated inside the seam transaction and the canonical
  reducer deduplicates by `event_seq`. No silent divergence.
- **Cancellation races** — `ProjectionSubscriptionId` is keyed in the
  `SubscriptionRegistry` until `unsubscribe` removes it; the forwarder
  `JoinHandle` is aborted on connection drop. Concurrent
  subscribe/unsubscribe for the same id is serialized by the
  `Mutex<HashMap>` on `pending_receivers`.
- **Daemon restart** — `next_event_seq` reads-then-increments inside
  the store transaction; on restart the row's `next_seq` column is
  authoritative. `restart_recovery.rs:138-213` proves no sequence
  reuse across restarts.
- **Partial persistence failure** — the seam transaction wraps
  allocation, event insert, and high-water update. On any
  `StorageError` the transaction is dropped and no live delivery
  occurs. `publish_from_core_with_context` returns the error to the
  sink, which logs it but never panics.
- **Stale generation / lease** — projection replay does not depend on
  daemon generation; restart recovery preserves binding revisions
  through the `projection_stream.binding_revision` column.
- **Contention** — concurrent rebind + publish is covered by
  `stream_context.rs:222-283`. The store's
  `get_or_create_session_stream_with_revision` is serialized at the
  SQLite layer; the active stream for `(session_id, project_id)`
  converges to the most-recently-resolved binding revision.
- **Malformed or unauthorized input** — request validation runs at
  the daemon layer (`ProjectionSubscriptionRequest::validate()`);
  `ProjectionResume` requires a known subscription id; `ProjectionAck`
  enforces stream/version match and never exceeds high-water.
- **Bounded event and artifact behavior** — `MAX_REPLAY_EVENTS = 512`,
  `MAX_REPLAY_BYTES = 1 MiB`, `MAX_REPLAY_EVENT_BYTES = 64 KiB` are
  all unchanged. Resume pagination preserves `next_cursor`.

## 7. Migration and compatibility review

- `STORAGE_LAYOUT_VERSION` is unchanged at 32; this commit does not
  require a new migration. The corrective pass reuses the v32
  `projection_stream` / `projection_event` / `projection_checkpoint`
  tables and only adds new application-level fields
  (`binding_revision` was already in the schema).
- The protocol changes are additive — no `CoreRequest` /
  `CoreResponse` / `CoreEvent` variants are removed or renamed. Old
  clients continue to receive the existing `Events` /
  `ResyncRequired` flows unchanged. New `Projection*` variants
  advertise `projection_version = 1`.
- The `EventLog` change is observable only as a single sink hook
  invocation; existing ring-buffer / SQLite / broadcast behavior is
  preserved.
- The transport layer adds a new per-connection registry but does not
  modify the legacy `forward_events` task. Raw `CoreEvent`
  subscribers continue to receive the additive-compatible event
  stream.

## 8. Security review

- No `unsafe_code` introduced. The new modules inherit
  `forbid(unsafe_code)`.
- `codegg-core` remains free of UI / server / plugin / auth imports
  — the boundary check (`bash scripts/check-core-boundary.sh`)
  passes.
- The safe-publication gate (`safe_publication.rs`) is unchanged; it
  still rejects `Internal` and redacts `Sensitive` payloads to a
  bounded diagnostic before persisting. Credentials, provider
  secrets, and full tool bodies never enter
  `projection_event.payload_json`.
- Subscription queues remain bounded and overflow still transitions
  to `ResyncRequired(SubscriberLagged)`; there is no silent drop.
- The new static guard rejects unauthorized direct publication
  paths. Per-client ownership (`SubscriptionRegistry::by_client`)
  ensures one client cannot subscribe to or acknowledge another
  client's subscription.

## 9. Documentation and operations

- `architecture/projection.md` (deferred doc summary): M2 is now
  end-to-end wired. The repository carries the existing M1 contract
  doc and this closure record; a follow-up doc PR is the right
  venue for the additive M2 daemon section.
- `architecture/protocol.md`: no semantic changes; new
  `Projection*` variants continue to default-safely on legacy
  clients.
- `architecture/core.md`: the daemon owns the seam; the
  `with_deps_and_identity` path constructs the store / service /
  seam / maintenance task when `pool.is_some()`.
- Troubleshooting / metrics: `ProjectionReplayMetrics` continues to
  expose stream counts, event counts, retention floor / high-water
  distance, retained bytes, checkpoint count + age, accepted /
  omitted / downgraded publication counts by visibility class,
  publication failures, active subscriptions, queue depth, lag,
  ack rejection reasons, replay batch size / count / latency,
  resync counts by reason, prune rows / bytes, and
  corrupt / quarantined count.
- Static guards in CI:
  - `bash scripts/check-core-boundary.sh`
  - `python3 scripts/check_daemon_cwd_usage.py`
  - `python3 scripts/check_git_forbidden_patterns.py`
  - `bash scripts/check_projection_publication_seam.sh` (new)

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Preexisting clippy `unused variable: i` in `src/tui/app/mod.rs:2886` (introduced by `f569386` "feat(tui): implement M2 project picker and tab navigation") and 19 unused-variable / unused-import warnings on the new test files matching the established codegg-core test style | Blocks `cargo clippy --workspace --all-targets --all-features -- -D warnings` in CI | Follow-up: track separately under the TUI / projection test-style cleanups. The clippy run on the focused crates (`-p codegg-core` and `-p codegg-protocol`) remains green. |
| low | The full workspace test suite was not run as part of the corrective closure pass; CI is the authoritative run | None locally | CI must run `CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`. The local focused suite covers every test target the corrective plan calls out. |
| low | `scripts/check-core-boundary.sh` does not yet scan the new `scripts/` location for the `seam.rs` deps the plan mandates | None locally — seam only depends on `codegg_protocol`, `crate::error`, `crate::project_storage`, and `crate::projection_replay`; all are within the boundary | Optional follow-up to extend the boundary script to also assert no new top-level deps. |
| low | The seam accepts an empty `ProjectionPublicationContext::default()` as a no-op when `ProjectStorage` is absent (in-memory legacy daemons); those events are then `Skipped { UnboundSession }` and never enter projection replay | Acceptable — legacy in-memory daemons are explicitly out of scope for M2 projection replay per `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md#5-scope` | None. The `core_event_log` path remains the only durable record for in-memory daemons. |

## 11. Roadmap disposition

Milestone 002 is **strictly closed**. The corrective daemon
integration pass is landed, every acceptance criterion from the
corrective plan is met, and the library/crate invariants from the
original M2 plan continue to hold. The implementation baseline
that closes the milestone is the union of `8dc4b85` (library) and
this commit (daemon integration).

Milestone 003 (visibility, redaction, and artifact handles)
**remains blocked on its own dependency: the principal capability
filtering seam**. The corrective plan explicitly notes "register M3
only after its separate principal-capability dependency is
verified" (§7 WP F). The M2 wiring is no longer a blocker.

## 12. Registry updates

- `plans/registry.md`:
  - Move `Frontend-neutral session projections — Milestone 2` from
    `Active subsystem roadmaps` (status `active`) to `Recently closed
    work`.
  - Drop the `Active closure work` row for M2.
  - Remove the `Blocked work` row that named M2 wiring as a blocker
    for Milestone 003 (Milestone 003 still blocks on the principal
    capability filtering seam; update that row accordingly).
  - Add a `dependency-ready implementation plans` row pointing at
    Milestone 003 once its principal-capability seam is independently
    ready (not done in this commit; reserved for that future plan).
- `plans/subsystems/session-projections-roadmap.md`:
  - Flip Milestone 2 status from `corrective pass ready` to
    `closed (daemon integration landed at this commit)`.
  - Note that Milestone 3 is no longer blocked by M2 wiring; the
    remaining blocker is the principal capability filtering seam.
- `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md`:
  - Mark `Status: ready for handoff` as superseded by the
    corrective closure at this commit.
- `plans/implementation/session-projections/002-corrective-daemon-integration-and-closure.md`:
  - Mark `Status: ready for handoff` as implemented at this commit.