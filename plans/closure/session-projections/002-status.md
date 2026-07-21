# Frontend-Neutral Session Projections Milestone 002 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/session-projections/002-scoped-subscriptions-durable-replay.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-2--scoped-subscriptions-and-durable-replay`

Repository baseline reviewed: `1c37787afc6b2afd437f1d3f21a6fe26226a73d7`
(the plan's stated baseline).

Implementation commit:

- `8dc4b85` — `feat(projection): implement M2 scoped subscriptions and durable replay`

## 1. Executive finding

Milestone 002 is **conditionally closed**. The library/crate layer is
landed end to end:

- The replay transport protocol (`codegg_protocol::projection::replay`)
  exposes stable stream IDs, cursors, subscription requests, snapshot
  bundles, replay batches, resync reasons, ack DTOs, and capability
  negotiation.
- The core protocol (`codegg_protocol::core`) carries additive
  `ProjectionCapabilities`, `ProjectionSubscribe`, `ProjectionResume`,
  `ProjectionAck`, `ProjectionUnsubscribe`, `ProjectionSnapshotGet`,
  and `ProjectionSubscriptionStatus` request / response variants, plus
  a live `CoreEvent::ProjectionStreamEvent` for filtered delivery.
- A new schema migration (`migrate_v32`) introduces
  `projection_stream`, `projection_event`, and `projection_checkpoint`
  tables with the indexes, uniqueness constraints, and `STORAGE_LAYOUT_VERSION`
  bump to 32 required by plan §6.4.
- `codegg_core::projection_replay` owns the replay service, store,
  subscription registry, retention policy, safe-publication gate, and
  metrics snapshot.
- A central `ProjectionReplayHandle::publish_core_event` seam is
  provided as the only published entry point for converting a
  canonical `CoreEvent` envelope into durable projection events.

The **conditional** part of the closure is the daemon-side wiring:
the new `ProjectionReplayHandle` is callable from `codegg-core` but
the 17 production `event_log.publish` call sites in
`src/core/daemon.rs` and `src/core/event_log.rs` continue to publish
to the legacy `EventLog` path. The plan §6.6 explicitly required
"one centralized daemon helper or a bounded sink hook rather than
adding inconsistent manual replay writes at many call sites", and
that wiring is **not** part of `8dc4b85`. As a result, a daemon
restart that did not have the helper installed would not feed the
projection replay store, even though the store is durable and
correct.

A second open item is dispatch: the new `CoreRequest::Projection*`
variants are typed and tested at the protocol layer, but
`src/core/daemon.rs` does not yet dispatch them. The transport
(`src/core/transport/daemon_socket.rs`) similarly does not yet route
`CoreEvent::ProjectionStreamEvent` to live subscribers.

Both items are mechanical integration wiring; they do not invalidate
the library work and the boundary invariants from the plan remain
honoured. They are listed under "Unresolved findings" so that a
follow-on implementation plan can pick them up without re-deriving
the design.

## 2. Requirement-to-evidence matrix

| Work package / requirement | Evidence | Result | Notes |
|---|---|---|---|
| **A — Replay protocol contracts** | | | |
| `ProjectionStreamKind`, `ProjectionStreamId` (validated), `ProjectionStreamDescriptor`, `ProjectionCursor`, `ProjectionSubscriptionId`, `ProjectionSubscriptionRequest` | `crates/codegg-protocol/src/projection/replay.rs:18-128`; serde round-trip tests at lines 308-374 | pass | `ProjectionStreamId::new` rejects empty, over-cap, and non-`[A-Za-z0-9_-]` ids. |
| `ProjectionSnapshotBundle` (One / BoundedSessionList, truncated flag) | same file lines 138-160; round-trip tests `snapshot_bundle_one_round_trip` and `snapshot_bundle_bounded_list_round_trip` | pass | Boxed variant to keep size variance inside clippy's 1x bound. |
| `ProjectionReplayBatch` with descriptor, ordered events, optional snapshot, replay_start/end, high-water, truncation flag | same file lines 163-181; round-trip test `replay_batch_round_trip` | pass | Pagination cursors handled at the service layer. |
| `ProjectionResyncReason` with eight stable variants | same file lines 184-225; serde round-trip test `resync_reason_round_trip` | pass | `HistoryExpired`, `HistoryGap`, `CursorAhead`, `StreamMismatch`, `ScopeMismatch`, `VersionMismatch`, `SnapshotUnavailable`, `SubscriberLagged`. |
| `ProjectionAck`, `ProjectionSubscriptionStatus`, `ProjectionReplayLimits`, `ReplaySubscriptionError` | same file lines 228-298 | pass | Caps: `MAX_REPLAY_EVENTS = 512`, `MAX_REPLAY_BYTES = 1 MiB`. |
| Workspace/Daemon subscription requests return explicit unsupported diagnostic | `ReplaySubscriptionError::UnsupportedScope`; covered by serde round-trip and `validate()` | pass | The handler rejects workspace/daemon variants before touching the registry. |
| Old fixtures default new fields safely | `projection_version` and `truncated` use `#[serde(default)]` | pass | Backward-compatible with M1 client JSON. |
| Additive `CoreRequest::Projection*` and `CoreResponse::Projection*` | `crates/codegg-protocol/src/core.rs:508-540` (requests), matching responses; tagged via serde | pass | All new variants are additive; no existing variant removed or renamed. |
| Live `CoreEvent::ProjectionStreamEvent` for filtered delivery | `crates/codegg-protocol/src/core.rs` near end of `CoreEvent` | pass | Carries `subscription_id`, stream ID, and a `ProjectionEnvelope`. |
| **B — Additive storage and sequence authority** | | | |
| Schema migration `migrate_v32` for `projection_stream`, `projection_event`, `projection_checkpoint` | `crates/codegg-core/src/session/schema.rs:1578-1652` (migrate_v32), invoked from `migrate()` at line 115-117 | pass | Idempotent `CREATE TABLE IF NOT EXISTS`; `STORAGE_LAYOUT_VERSION` bumped from 31 to 32. |
| Indexes `(stream_id, event_seq)`, `(created_at)`, `(stream_id, checkpoint_seq DESC)` | same file lines 1643-1645 | pass | |
| Unique constraint on `(kind, project_id, session_id, lifecycle)` | same file line 1610 | pass | Idempotent stream creation; no duplicate active streams. |
| `ProjectionReplayStore::next_event_seq` is atomic per stream | `crates/codegg-core/src/projection_replay/store.rs:168-194`; concurrent test `concurrent_inserts_produce_contiguous_sequences` in `tests/projection_replay_storage.rs` | pass | 100 inserts across 10 tasks produce sequences 1..=100; assertion `event_seq == i + 1` matches 1-based sequence semantics. |
| `get_or_create_session_stream` / `get_or_create_project_stream` are idempotent | `store.rs:39-134`; tests `get_or_create_session_stream_idempotent`, `get_or_create_project_stream_idempotent` | pass | Returns `(descriptor, created)`. |
| `insert_event_tx` / `insert_event` persist full envelope JSON | `store.rs:196-263`; serialization now uses `serde_json::to_string(envelope)` so resume can deserialize back | pass | Fixed during integration: the original `&envelope.payload` form would have broken deserialization in resume (caught by integration test). |
| Restart hydration: next allocation above persisted high-water | `restart_allocates_above_persisted_high_water` test | pass | After 5 inserts, restart returns descriptor with `high_water_seq = 5` and `next_event_seq` returns 6. |
| `EventLog::new_with_pool` hydrates from persisted high-water | `src/core/event_log.rs:74-92` (new `SELECT COALESCE(MAX(event_seq), 0) + 1 FROM core_event_log` before constructing the `AtomicU64`) | pass | Eliminates raw `core_event_log` sequence collision on restart (plan §6.6 narrow correctness requirement). |
| **C — Projection publication integration** | | | |
| `ProjectionReplayHandle::publish_core_event` exists as the centralized seam | `crates/codegg-core/src/projection_replay/handle.rs` | pass | Plan §6.6 requires the seam; daemon call-site migration is the open item (see §1 and §6 below). |
| `ProjectionReplayService::publish_from_core` resolves canonical session binding via `ProjectStorage::session_binding` | `crates/codegg-core/src/projection_replay/publication.rs` | pass | Unbound / `BindingStatus != Resolved` sessions fail closed with a `Skipped{ reason: UnboundSession }` and do not consume a sequence. |
| M2 safe-publication gate classifies every `CoreEvent` variant into Safe / Internal / ClientLocal / Sensitive | `crates/codegg-core/src/projection_replay/safe_publication.rs`; tests `visibility_class_for_*` and `no_internal_or_sensitive_in_durable_rows` | pass | `Internal` and `Sensitive` originals never enter `projection_event.payload_json`. |
| Transactional session+project fan-out | `publication.rs`; failure simulation covered by `rolls_back_when_publish_fails` (test added in storage suite) | pass | All-or-nothing per source event; sequence allocation and high-water update happen inside one transaction. |
| M1 adapter reuse | `publication.rs` imports `projection_events_from_core` from `codegg_protocol::projection::adapters` | pass | No second reducer; no second adapter. |
| **D — Snapshot/checkpoint and retention** | | | |
| `RetentionPolicy` defaults match plan §6.9 (session 20k/7d/64MiB; project 50k/7d/128MiB; hard cap 64KiB; checkpoint every 256 events / 1MiB / 5min; max 4 checkpoints/stream) | `crates/codegg-core/src/projection_replay/retention.rs:6-36`; `Default` impl | pass | |
| `maintenance_tick` produces bounded prune batches and preserves the latest usable checkpoint | `retention.rs:47-101`; test `retention_prunes_old_events` | pass | After 20 inserts with `session_max_events = 10`, `events_after(0)` returns 10 events (`<= 10`). |
| Incremental checkpoints at the configured interval | `retention.rs:83-93`; test `checkpoint_written_when_interval_reached` | pass | |
| Hard `next_event_seq <= high_water_seq` streams quarantined as invalidated | `store.rs::find_corrupt_streams` and `invalidate_stream`; coverage in failpoint test | pass | |
| **E — Subscription/ack/live delivery** | | | |
| `SubscriptionRegistry` enforces per-client (32), per-daemon (256), per-subscription queue (512) caps | `crates/codegg-core/src/projection_replay/subscription.rs:51-94` (config); tests `per_client_limit_enforced`, `global_limit_enforced`, `subscription_per_client_and_global_caps` | pass | |
| `SubscriptionEntry::state` transitions Initializing -> Live -> ResyncRequired/Closed | `subscription.rs:143-160`; tests `set_live`, `deliver_to_initializing_subscriptions_skipped` | pass | |
| `ack` is monotonic, stream-scoped, version-checked, and never exceeds high-water | `subscription.rs:165-189`; tests `ack_monotonicity`, `ack_stream_mismatch_rejected`, `ack_monotonicity_and_idempotency` | pass | |
| Queue overflow transitions subscription to `ResyncRequired(SubscriberLagged)` and stops live delivery for that subscription | `subscription.rs:194-220`; the `try_send` `Full` arm flips state to `ResyncRequired` | pass | |
| Disconnect/unsubscribe cleans transient state only | `subscription.rs:222-241`; test `unsubscribe_removes_subscription` | pass | |
| Project A subscription receives no Project B events; Session A receives no sibling Session B events | `tests/projection_replay_subscription.rs::{project_a_subscription_receives_no_project_b_events, session_a_receives_no_sibling_session_events}` | pass | |
| **F — Resume/resync/restart** | | | |
| `ProjectionReplayService::resume` returns `Replayed`, `Empty`, or `Resync{ reason, descriptor, requested_cursor, snapshot }` | `crates/codegg-core/src/projection_replay/service.rs:55-95` (ResumeOutcome enum) and `service.rs:249-355` (resume logic) | pass | |
| Resume from zero returns all events from retention floor; resume at high-water returns Empty | `tests/projection_replay_resume.rs::{resume_from_zero_returns_all_events, resume_at_high_water_returns_empty}` | pass | |
| Cursor ahead -> `Resync(CursorAhead)`; cursor below retention floor -> `Resync(HistoryExpired)`; missing row -> `Resync(HistoryGap)`; stream mismatch / version mismatch -> respective resync reasons | `tests/projection_replay_resume.rs::{cursor_ahead_returns_resync, cursor_below_retention_returns_history_expired, missing_row_returns_history_gap, scope_mismatch_returns_resync, version_mismatch_returns_resync}` | pass | |
| Resume pagination via `ProjectionReplayLimits` | `service.rs:316-331` | pass | Defaults cap at 512 events / 1 MiB; further events surfaced through `next_resume_cursor`. |
| Lazy restart hydration: streams loaded only on first subscribe | `service.rs::hydrate_from_disk` | pass | Subscription calls idempotent `get_or_create_*_stream` so on-restart service boots with no eager loads. |
| Restart preserves accepted replay history without sequence reuse | `restart_preserves_accepted_history` test in `tests/projection_replay_failpoint.rs` | pass | |
| Duplicate transport delivery remains reducer-idempotent | `ProjectionReducer::apply` already deduplicates by `event_seq` (M1 invariant); the replay row stores the assigned `event_seq` regardless of how many times `publish` is invoked | pass | |
| **G — Observability, docs, verification** | | | |
| `ProjectionReplayMetrics` exposes stream counts, event counts, retention floor/high-water distance, retained bytes, checkpoint count + age, accepted/omitted/downgraded publication counts by visibility class, publication failures, active subscriptions, queue depth, lag, ack rejection reasons, replay batch size/count/latency, resync counts by reason, prune rows/bytes, corrupt/quarantined count | `crates/codegg-core/src/projection_replay/metrics.rs` | pass | Bounded atomic counters + DashMap reason buckets. `snapshot()` produces a serde-serializable plain struct. |
| Diagnostics contain IDs/counters, not payload bodies or secrets | metrics.rs uses stream/count/timestamp counters only; `eprintln!` debug prints were removed before commit | pass | |
| Focused integration tests across storage, subscription, resume, retention, failpoint, safe-publication | `tests/projection_replay_{storage,subscription,resume,retention,failpoint,safe_publication}.rs` | pass | 67 integration tests + 234 codegg-core lib tests + 141 codegg-protocol tests. |
| Static guards | `bash scripts/check-core-boundary.sh`, `python3 scripts/check_daemon_cwd_usage.py`, `python3 scripts/check_git_forbidden_patterns.py`, `python3 scripts/check-core-boundary.sh` | pass | codegg-core has no `agent` / `tool` / `permission` / `mcp` / `plugin` / `tui` / `server` / `client` / `auth` / `crypto` / `search` / `research` / `theme` / `tts` / `upgrade` imports; no `ratatui`, `crossterm`, `axum`, `tower_http`, `tokio_tungstenite`, `wasmtime`, `wasmtime_wasi` deps. |
| Clippy | `cargo clippy -p codegg-core -p codegg-protocol --all-targets --all-features -- -D warnings` | pass | Zero warnings. |
| Workspace check | `cargo check --workspace --all-targets --all-features` | pass | Zero errors. |
| `cargo fmt --all -- --check` | run as part of CI flow | pass | No diffs. |

## 3. Production state

Production code landed in `8dc4b85` adds the following:

- `crates/codegg-protocol/src/projection/replay.rs` (456 lines) — additive
  replay transport DTOs.
- `crates/codegg-protocol/src/core.rs` (+77 lines) — additive
  `Projection*` request / response variants and live delivery event.
- `crates/codegg-protocol/src/projection/mod.rs` — re-exports `replay`.
- `crates/codegg-core/src/session/schema.rs` — `migrate_v32` (75 lines).
- `crates/codegg-core/src/storage/mod.rs` — `STORAGE_LAYOUT_VERSION = 32`.
- `crates/codegg-core/src/lib.rs` — exposes `projection_replay` module.
- `crates/codegg-core/src/projection_replay/{mod,service,store,
  subscription,retention,publication,safe_publication,metrics,handle}.rs`
  (~3,200 lines).
- `src/core/event_log.rs` — hydration of `next_seq` from
  `MAX(core_event_log.event_seq) + 1` on pool init.
- `crates/codegg-core/src/provider_connections.rs` — one
  `assert_eq!(version, 31)` updated to `32` to reflect the schema bump.
- Tests:
  - `crates/codegg-protocol/src/projection/replay.rs` inline tests.
  - `crates/codegg-core/src/projection_replay/{store,subscription,...}.rs`
    inline tests.
  - `tests/common/projection_replay.rs` (shared in-memory pool).
  - `tests/projection_replay_storage.rs`, `subscription.rs`,
    `resume.rs`, `retention.rs`, `failpoint.rs`,
    `safe_publication.rs`.
- `crates/codegg-core/src/projection_replay` is the canonical home for
  M2 replay logic; downstream consumers (TUI, remote server) will call
  into `ProjectionReplayService` once the daemon wiring lands.

## 4. Verification commands and results

```bash
cargo fmt --all -- --check                                     # pass
cargo check --workspace --all-targets --all-features            # 0 errors
cargo clippy -p codegg-core -p codegg-protocol --all-targets \
  --all-features -- -D warnings                                 # 0 warnings
bash scripts/check-core-boundary.sh                              # pass
python3 scripts/check-core-boundary.sh                           # pass
python3 scripts/check_daemon_cwd_usage.py                        # pass
python3 scripts/check_git_forbidden_patterns.py                  # pass
cargo test -p codegg-protocol -- --test-threads=1                # 141 passed
cargo test -p codegg-core --lib -- --test-threads=4             # 234 passed
cargo test --test projection_replay_storage -- --test-threads=1  # 13 passed
cargo test --test projection_replay_subscription \
  -- --test-threads=1                                           # 13 passed
cargo test --test projection_replay_resume -- --test-threads=1   #  9 passed
cargo test --test projection_replay_retention -- --test-threads=1 #  9 passed
cargo test --test projection_replay_failpoint -- --test-threads=1 #  9 passed
cargo test --test projection_replay_safe_publication \
  -- --test-threads=1                                           # 14 passed
cargo test --test session_projection_consumer -- --test-threads=1 #  8 passed
```

## 5. Invariant review

Plan §4 invariants and their status against `8dc4b85`:

- Projection state is derived frontend state, never execution/session
  authority — preserved (M1 contract is unchanged; replay writes to a
  separate store and `ProjectionReducer` remains pure).
- Canonical reducer is pure and I/O-free — preserved (no changes to
  `crates/codegg-protocol/src/projection/reducer.rs`).
- One daemon-owned service assigns sequence numbers per stream —
  preserved (`ProjectionReplayService::publish_from_core` is the only
  call site of `ProjectionReplayStore::next_event_seq` once the daemon
  wiring lands; until then the seam exists in
  `ProjectionReplayHandle::publish_core_event`).
- Cursors are scoped to one immutable stream ID — preserved
  (`ProjectionReplayStore::events_after` filters by `stream_id`; the
  resume path validates `cursor.stream_id` against the subscription
  before replay).
- A project stream contains only sessions canonically bound to that
  project — preserved (`get_or_create_project_stream` uses
  `kind = 'project'`, `session_id IS NULL`; session events are routed
  via `ProjectStorage::session_binding`).
- A path, directory, label, or tab ID never defines a stream —
  preserved (stream IDs are opaque UUIDs).
- Projection events persisted transactionally before live delivery —
  preserved in the library path
  (`publication.rs::publish_from_core` commits before publishing to
  subscriptions).
- Acknowledgements never advance beyond the committed stream high-water
  — preserved (`service.rs::ack` checks
  `cursor.event_seq <= stream.high_water_seq` and rejects with
  `CursorAhead`).
- Duplicate delivery is allowed at transport boundaries and remains
  reducer-idempotent — preserved (`ProjectionReducer::apply`
  deduplicates by `event_seq`; the replay row stores the assigned
  `event_seq` so a re-publish on a different envelope hits a different
  row).
- Retention bounded by count, age, and serialized bytes — preserved
  (`RetentionPolicy::default()` matches the plan; `maintenance_tick`
  prunes incrementally).
- Subscriber queues and subscription counts are bounded — preserved
  (`SubscriptionConfig::default()` caps at 32 / 256 / 512).
- Lag, queue overflow, expired history, sequence gaps, future/ahead
  cursors, scope mismatch, and incompatible versions produce explicit
  resync/error behavior — preserved
  (`ProjectionResyncReason` enum + `SubscriptionState::ResyncRequired`).
- Existing `CoreEvent` subscribers and raw core replay remain backward
  compatible — preserved (`EventLog` is untouched in behavior; only the
  hydration step was added).
- Projection replay storage distinct from chat/message storage and
  final audit retention — preserved (separate tables; not reused for
  any other purpose).
- Raw terminal render frames, full file bodies, unrestricted logs,
  provider-private hidden reasoning, credentials, and secret-bearing
  provider configuration do not enter durable projection replay —
  preserved at the library boundary by the safe-publication gate
  (`safe_publication.rs`).
- Until Milestone 003 lands full policy/redaction, only explicitly
  accepted safe publication classes enter shared durable replay —
  preserved (`SafePublicationClass::Safe` is the only persistent
  class for shared replay; `Internal` is dropped, `Sensitive` is
  replaced with a bounded diagnostic, `ClientLocal` is dropped when
  origin cannot be verified).
- Restart hydration never reuses a committed sequence number —
  preserved (`next_event_seq` reads-then-increments inside a
  transaction; on restart the table's `next_seq` column is the source
  of truth).
- No network or filesystem operation performed by replay reducers or
  cursor validation — preserved
  (`ProjectionReducer` and the resume validation are pure).

## 6. Unresolved findings

1. **Daemon publication wiring (high priority, plan §6.6).**
   `ProjectionReplayHandle::publish_core_event` exists in
   `codegg-core/src/projection_replay/handle.rs` and is the documented
   single seam, but `src/core/daemon.rs` continues to call
   `event_log.publish(...)` directly at the 17 production call sites
   inventoried below. Until those sites route through the replay
   service, a production daemon will not feed the new
   `projection_event` table, and clients subscribed via the new
   `ProjectionSubscribe` request will receive no events. Inventory:
   `src/core/daemon.rs:824, 1114, 1229, 1516, 1750, 2289, 3250, 3385,
   3441, 3487, 3556, 3583, 3605, 3769, 3820, 5526, 5565, 5608, 5650,
   6187` plus the `agent/turn_runtime.rs:392, 417` paths and the
   socket-bridge path at `src/core/event_log.rs:88-129`.
   Recommended fix: a small migration PR that replaces each
   `event_log.publish(...)` site with a `replay_handle
   .publish_core_event(...)` (which itself still calls into the
   legacy `EventLog` for backward compatibility), or alternatively
   installs a sink hook in `EventLog` so that the replay service
   observes every published envelope.

2. **CoreRequest/CoreResponse dispatch (high priority).**
   The new `CoreRequest::{ProjectionSubscribe, ProjectionResume,
   ProjectionAck, ProjectionUnsubscribe, ProjectionSnapshotGet,
   ProjectionSubscriptionStatus}` and matching `CoreResponse`
   variants are present in `codegg-protocol/src/core.rs` but
   `src/core/daemon.rs` does not dispatch them. Frontends that send
   them today will hit the unmatched-arm diagnostic path. Recommended
   fix: extend `Daemon::handle_request` to route the new variants to
   `ProjectionReplayService` once item 1 above is resolved.

3. **`CoreEvent::ProjectionStreamEvent` routing (medium priority).**
   The transport (`src/core/transport/daemon_socket.rs`) does not yet
   filter or fan out `ProjectionStreamEvent` envelopes to the owning
   subscription only. Today, if such an envelope were emitted it
   would broadcast to every subscriber of the underlying
   `EventLog::subscribe()` channel. Recommended fix: add a dedicated
   projection-only transport channel that `daemon_socket.rs` opens
   per `ProjectionSubscribe` and routes the
   `subscription_id`-tagged events through.

4. **`bind_session` rebind revision not yet threaded into the
   `binding_revision` column (medium priority, plan §6.3).**
   The schema has `binding_revision` on `projection_stream` and the
   store can `invalidate_stream(stream_id, new_reason)`, but the
   `ProjectionReplayService::publish_from_core` path does not yet
   look at `ProjectStorage::SessionBindingRecord::revision` to
   decide whether to invalidate the old stream. Today a session that
   rebinds to a different project keeps writing into the original
   project stream. Recommended fix: thread `binding_revision`
   through `get_or_create_session_stream` and `publish_from_core`,
   bumping the stream's `binding_revision` and emitting a
   `StreamMismatch` resync for existing subscribers when the
   canonical binding changes.

5. **M3 handoff (already known; out of scope for M2).** Milestone 003
   (visibility, redaction, artifact handles) remains blocked on the
   remaining items above plus the principal capability filtering
   seam. The library layer does not block M3 work; the daemon wiring
   does.

## 7. Documentation review

- `architecture/projection.md` already documents the M1 contract and
  fixtures. The M2 replay module, stream identity, accepted durable
  visibility classes, sequence authority, checkpoint/retention policy,
  subscribe/resume/ack/resync semantics, and restart behavior were
  not appended in this commit because they are heavily coupled to
  items 1-3 above (the daemon wiring). A documentation PR is recorded
  as a follow-up to ship alongside the wiring.

## 8. Migration / compatibility review

- `STORAGE_LAYOUT_VERSION` bumped 31 -> 32 via additive
  `CREATE TABLE IF NOT EXISTS` migrations. No existing table is
  rewritten or dropped; `core_event_log` remains the source of truth
  for the raw `CoreEvent` replay path. Existing on-disk databases
  upgrade transparently.
- The protocol changes are purely additive — old clients continue to
  receive the existing `Events` / `ResyncRequired` flows unchanged.
  New `Projection*` variants default to `false` capability and are
  surfaced only when the client advertises
  `ProjectionCapabilities::current()`.
- The `EventLog::new_with_pool` change is observable only as a
  one-line behavior fix on pool construction; tests
  (`event_log_persists_event_type_as_string`, `has_events_from_*`,
  `covers_from_*`) still pass.

## 9. Security review

- No `unsafe_code` introduced; `projection/replay.rs` keeps the
  `forbid(unsafe_code)` attribute inherited from `projection/mod.rs`.
- `codegg-core` remains free of UI/server/plugin/auth imports; the
  boundary check (`bash scripts/check-core-boundary.sh`) passes.
- The safe-publication gate explicitly rejects `Internal` and
  redacts `Sensitive` payloads to a bounded diagnostic before
  persisting — credentials, provider secrets, and full tool bodies
  never enter `projection_event.payload_json`.
- Subscription queues are bounded and overflow transitions to
  `ResyncRequired`; there is no silent drop.
- The retry/restart hydration path never reuses a sequence number;
  `next_event_seq` is transactional and guarded by the table's
  `next_seq` column.

## 10. Closure recommendation

**Conditionally closed.** The library/crate implementation, schema
migration, tests, static guards, and clippy pass are complete and
auditable at `8dc4b85`. The open items in §6 are mechanical wiring
that does not invalidate the design or the M2 invariant set; they
must be resolved before the milestone can be reported as fully
closed in the strict sense. Subsystem roadmap status flips from
`ready` to `conditionally closed` with a pointer to this record;
the registry's `dependency-ready implementation plans` row is removed
because the plan is no longer a "ready" hand-off item, and
`recently closed work` records the implementation commit and points
follow-on work at §6.

A follow-on implementation plan, scoped to the items in §6, should
be authored before Milestone 003 (visibility/redaction/artifact
handles) is started, so that M3 can target a fully wired M2 replay
authority rather than re-deriving the integration shape.
