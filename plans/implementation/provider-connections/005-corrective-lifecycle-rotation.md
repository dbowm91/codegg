# Provider Connections Milestone 005 — Corrective Lifecycle, Rotation, Health, and Closure

Status: closed; see `plans/closure/provider-connections/005-status.md`.

Repository baseline: `213272c3720a8fdd1694452c37553e012c645230` (`main`; the handoff baseline recorded by the plan; production implementation landed later in `0eadc85`)

Implementation commit: `0eadc85`.

Source original milestone:

- `plans/implementation/provider-connections/004-lifecycle-rotation-health-closure.md`
- `plans/closure/provider-connections/004-status.md` (closure record: corrective pass required)

Source subsystem roadmap:

- `plans/subsystems/provider-connections-roadmap.md#milestone-5--corrective-lifecycle-rotation-health-and-closure`

Long-term references:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool`
- `plans/001-terminology-and-domain-model.md` — ProviderConnection, SecretRef, lifecycle, revision, model catalog, scope, and session selection.
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`

Applicable closure evidence from prior milestones (read for context, not reopened):

- `plans/closure/provider-connections/001-status.md`
- `plans/closure/provider-connections/002-status.md`
- `plans/closure/provider-connections/003-status.md`
- `plans/closure/provider-connections/004-status.md`

Applicable ADRs:

- None. The canonical documents already decide daemon ownership, secret references, bounded probes, revisioned selection, and no silent fallback. Stop for an ADR if the implementation requires a new secret backend or materially changes connection lifecycle semantics.

Primary class: capability

## 1. Why this is a corrective pass

The original Milestone 004 plan (`004-lifecycle-rotation-health-closure.md`) was authored and registered as `ready for handoff` at commit `5fc689c`. The closure record (`plans/closure/provider-connections/004-status.md`) establishes that no production work landed against it. Per `plans/003-planning-process.md` §7, a corrective pass is a new implementation plan that lists each unclosed requirement, explains why original verification did not catch the gap, includes regression tests or guards preventing recurrence, and avoids reopening already closed scope without evidence.

The original plan was not opened into a verified closed state; this plan does not adopt the original document's status header and instead treats its work as scoped-but-unexecuted. Where the original plan is precise (e.g., the rotation sequence in §6 Rotation transaction), this plan inherits the requirement verbatim and adds the implementation seams the original plan left to the implementer.

## 2. Why original verification did not catch the gap

The original plan was authored as a handoff but never executed against a baseline. There is no commit that introduces Milestone 004 production code; the only commits touching the plan's scope are planning-only commits (`5fc689c`, `583f51e`). The `plans/003-planning-process.md` §2.5 rule that "a code commit message saying a plan is closed is not sufficient closure evidence" was honored by never claiming closure; the corrective closure record at `004-status.md` records the gap precisely.

This plan adds two static guards to ensure the corrective pass itself lands:

1. A new `scripts/check_provider_connections_m4_coverage.sh` that fails CI if `src/core/provider_connections.rs` lacks `pub fn rotate`, `pub fn refresh`, `pub fn disable`, `pub fn enable`, `pub fn delete`, `pub fn restore`, and a `ProviderConnectionState` variant for `Tombstoned`.
2. The closure record for this milestone must include a verified full verification log per the plan §10. Failure to execute the full verification loop is itself a stop condition.

## 3. Objective

Implement the Milestone 004 deliverable surface in one corrective pass and close the Provider Connections and Eggpool roadmap:

- Staged, atomic, rollback-safe credential and endpoint rotation with revision-safe runtime invalidation.
- Bounded, coalesced, cancellable health/model refresh with backoff/jitter and last-good preservation.
- Lifecycle state machine including enable/disable/delete/restore/purge with explicit reference blockers.
- Selected-session lifecycle projection and turn-submit typed connection-state failures.
- TUI lifecycle controls on `/connections` with secret-free local input and confirmations.
- Deterministic end-to-end fake-daemon/CoreClient lifecycle harness.
- Stabilization of the originally flaky provisioning test under broad parallel load.
- Architecture documentation for the lifecycle state machine, rotation semantics, refresh defaults, deletion/reference policy, and legacy-removal criteria.

The milestone succeeds when the plan §13 acceptance criteria for Milestone 004 are evidenced and the Phase 2 roadmap exit criteria are closed.

## 4. Current implementation evidence at baseline `213272c`

Inherited from prior milestones and reused unchanged:

- `ProviderConnectionStore` (`crates/codegg-core/src/provider_connections.rs:590-846`) — additive migrations v24 (`provider_connections`) and v26 (`provider_provisioning`, `provider_connection_health`, `provider_connection_models`) at `crates/codegg-core/src/session/schema.rs:1052-1298`. Revision-safe `update` (`:702`), `transition` (`:748`), and `disable` (`:789-796`). `delete` is a hard row DELETE (`:798-831`).
- `ConnectionManager` (`src/core/provider_connections.rs:94-208`) — lazy resolution, `OnceCell` cache keyed by `(id, revision)` (`:151-161`), `invalidate` (`:183-186`), `invalidate_revision` (`:189-195`), `clear` (`:197`). The in-flight-pinning primitive already exists; rotation and refresh are what is missing.
- `ProviderConnectionState` (`crates/codegg-core/src/provider_connections.rs:339-343`) — exactly three variants: `Active`, `Disabled`, `CredentialMissing`. Source comment at `:335-336` notes "Deleted is represented by row absence after `ProviderConnectionStore::delete_metadata`."
- `digest_models` (`crates/codegg-providers/src/eggpool.rs:511-518`) — SHA-256 digest of sorted `(id, name)` model rows; private to the eggpool module; consumed only by initial provisioning.
- `SelectionService` (`src/core/session_selection.rs:38-64, :507+`) — five-outcome update protocol including `StaleRevision`, `StaleCatalog`, `ConnectionNotSelectable`, `UnknownModel`, `Updated`. `ConnectionNotSelectable` carries the disabled/deleted state string.
- `SessionSelectionDto` (`crates/codegg-protocol/src/provider.rs:168-184`) — `Selected` carries `connection_revision` and `catalog_revision`. Annotated `#[allow(clippy::large_enum_variant)]` at `:170`; the original plan §6 instructed to box or split the large variant rather than adding more unboxed payload weight.
- `SelectionUpdateOutcome::StaleRevision` / `StaleCatalog` (`src/core/session_selection.rs:42-52`) — return `current_connection_id`, `current_revision`, `current_catalog_revision` but not `current_selected_model_id`.
- `/connections` dialog (`src/tui/components/dialogs/connection_selection.rs:23-282`) — selection-only keymap (`Esc`/`q`/`Down`/`Up`/`Tab`/`BackTab`/`Enter`) at `:125-153`. No refresh/rotate/enable/disable/delete/restore action.
- Server secret-transport denial (`src/server/ws.rs:930-941`) — guards `CoreRequest::EggpoolConnectionCreate` against remote WebSocket carriage of secrets. The matches! arm does not cover any future rotate/refresh variant.
- Flaky test `core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog` (`src/core/eggpool.rs:1077-1122`) — uses `#[tokio::test(flavor = "current_thread")]` and `fake_eggpool(Duration::ZERO)`; synchronization is via `server.join()` on a `std::thread::JoinHandle` at `:1095`. Unchanged.
- `STORAGE_LAYOUT_VERSION = 28` (`crates/codegg-core/src/storage/mod.rs:39`); `migrate_v28` (`crates/codegg-core/src/session/schema.rs:1324`) adds Project Catalog M1 columns.

Missing (the scope of this corrective pass):

- Rotation transaction, staged credential write, atomic revision commit, old-runtime pinning guarantees, typed rotation errors, rotation protocol variants.
- Refresh coordinator (single-flight, backoff, jitter, per-connection/global caps, cancellation, last-good preservation, manual refresh handler, deterministic catalog normalization/digest for refresh).
- Lifecycle state machine additions: `ProvisioningRotating`, `Tombstoned`, `Error`, `Stale`. Reference counting, blocker enumeration, restore, purge eligibility.
- Session/protocol reconciliation: current selected-model in stale outcomes, lifecycle projection for selected sessions, turn-submit typed connection-state failures, additive lifecycle protocol operations/events.
- TUI lifecycle controls: refresh, rotate, enable/disable, delete/restore, masked local-only secret input, confirmations, blocker rendering.
- End-to-end fake-daemon/CoreClient lifecycle harness.
- Flaky-test stabilization with deterministic fake-server readiness.
- Architecture doc updates and Phase 2 closure matrix.

## 5. Invariants that must not regress

Inherited from the original plan §4 (reaffirmed, not amended):

- Plaintext credentials remain confined to bounded secret-input memory and protected credential-store operations.
- Secrets never enter SQLite connection metadata, protocol DTOs, TUI snapshots, logs, diagnostics, project configuration, chat, or audit metadata.
- A connection ID and provider implementation ID remain distinct.
- In-flight requests retain the provider instance/revision captured at request start.
- New requests use the latest successfully committed active revision.
- Rotation or refresh failure leaves the previous valid active connection usable unless the operator explicitly disables it.
- Disable/delete never silently selects another connection or credential.
- Health/model probing remains explicit or bounded background work and cannot stall daemon startup.
- Concurrent rotation/refresh/lifecycle operations are revision-safe and coalesced where appropriate.
- Scope metadata does not itself grant authorization.

Two additional invariants added by this corrective plan:

- The static guard at `scripts/check_provider_connections_m4_coverage.sh` must pass on the closing commit.
- The rotation path must extend the server WS guard at `src/server/ws.rs:930-941` to deny any future `ConnectionRotate` request carrying a secret over remote WebSocket, before the first commit lands the request variant.

## 6. Scope

### In scope

This plan inherits the in-scope items from `plans/implementation/provider-connections/004-lifecycle-rotation-health-closure.md` §5 verbatim:

- Credential and endpoint rotation transaction.
- Revisioned runtime cache invalidation and in-flight pinning semantics.
- On-demand health/model refresh.
- Bounded optional background refresh policy with backoff/jitter and global/per-endpoint concurrency caps.
- Model-catalog diffing and deterministic revision changes.
- Enable/disable, soft-delete/tombstone, restore where policy allows, and explicit purge eligibility.
- Active-session behavior and typed lifecycle diagnostics.
- Additive protocol operations/DTOs for rotate, refresh, enable/disable, delete/restore, status, and purge.
- `/connections` lifecycle actions and local-only secret input.
- End-to-end fake-daemon/TUI harness for connect/select/refresh/rotate/disable/delete/restore flows.
- Stale-state reconciliation improvements.
- Provider-related flaky-test stabilization.
- Closure matrix for the complete provider-connections roadmap.
- Static guard `scripts/check_provider_connections_m4_coverage.sh`.
- Extension of `src/server/ws.rs:930-941` matches! arm to cover any new secret-bearing request variant.

### Explicitly out of scope

Inherited verbatim from the original plan §5:

- Team ACL enforcement or project membership policy.
- Replacing the credential store or introducing a pluggable secret backend without an ADR.
- Reimplementing Eggpool routing, accounting, or fallback.
- Generic provider marketplace/configuration UI beyond lifecycle actions for durable connections.
- Automatic migration of ambiguous legacy provider configs.
- Cross-node secret distribution.
- Project catalog/TUI multi-project work.

This corrective pass does not reopen already closed scope (Milestones 001–003). The TUI dialog render path, the SelectionService outcomes, the daemon handler routing, the credential store, and the eggpool probe are not redesigned — they are extended.

## 7. Required production changes

This section inherits the original plan §6 requirements verbatim and adds the implementation seams the original plan left to the implementer.

### 7.1 Rotation transaction (work package B)

Implement a daemon-owned rotation workflow supporting credential-only rotation and endpoint/TLS/display metadata changes where allowed. Required sequence:

1. Validate connection ID, expected revision, lifecycle state, and requested metadata. Reject if the connection is `Tombstoned`, `Error`, or `Stale`. Reject if `expected_revision` differs from current.
2. Accept secret input through the existing local-only secret transport (`src/auth/secret_input.rs` or equivalent, as established by Milestone 002).
3. Write a staged credential record under a new opaque binding/account reference without replacing the active binding. Use the existing `CredentialStore` interface; do not introduce a new secret backend.
4. Construct/probe a staged provider instance with bounded timeout and cancellation. Reuse the bounded probe from Milestone 002 (`crates/codegg-providers/src/eggpool.rs::probe_eggpool_models`).
5. Discover and validate a bounded model catalog. Reuse `digest_models` (`crates/codegg-providers/src/eggpool.rs:511-518`) for the catalog revision; if the digest equals the current catalog revision, preserve the existing catalog rows and only update the digest-bearing health row.
6. Transactionally update connection metadata, binding reference, health, and catalog within a single SQLite write transaction. Increment revision. Use additive storage only — no column drops.
7. Invalidate future resolution of older cached revisions by calling `ConnectionManager::invalidate_revision(id, old_revision)` and ensure `ConnectionManager::resolve` does not return the old revision for new callers.
8. Preserve any already-captured old runtime for in-flight requests; the existing `OnceCell` cache key `(id, revision)` (`src/core/provider_connections.rs:151-161`) provides this guarantee when `invalidate_revision(old_revision)` is called.
9. Retire/remove the prior credential only after the new revision commits and according to an explicit cleanup policy (a documented `delete_previous_on_commit` flag on the rotation request, defaulting to `true` for credential-only rotations and `false` for endpoint-only rotations). The cleanup deletes only the previous binding reference; never an unrelated credential-store record.
10. On any pre-commit failure, remove staged metadata/credential and retain the prior active revision. The transaction's rollback path must be tested.

Do not overwrite the active credential in place before validation. Rotation errors must be typed (`RotationError`) and redacted.

Required protocol variants (additive):

- `CoreRequest::ConnectionRotateBegin { request_id: String, connection_id: ProviderConnectionId, expected_revision: u64, change: ConnectionRotateChange, secret: SecretInputRef }` — secret-bearing; denied by `src/server/ws.rs` matches! arm.
- `CoreRequest::ConnectionRotateCancel { request_id: String }`.
- `CoreRequest::ConnectionRotateStatus { request_id: String }`.
- `CoreResponse::ConnectionRotateStatus { result: ConnectionRotateStatusDto }`.
- `CoreEvent::ConnectionRotated { connection_id, new_revision, catalog_revision, actor_seam }`.

The `ConnectionRotateChange` enum covers `CredentialOnly`, `EndpointOnly`, and `CredentialAndEndpoint`. Endpoint changes must re-run scheme/host/port/TLS/redirect validation identical to provisioning.

The `SecretInputRef` is a handle into the bounded secret-input buffer; the request does not carry the credential bytes over any transport.

### 7.2 Runtime revision and in-flight behavior (work package B)

- A request/turn resolves a connection once via `ConnectionManager::resolve(connection_id, expected_revision: Option<u64>)` and pins the returned runtime plus revision for its lifetime. When `expected_revision` is `None`, the current committed revision is used.
- Cache invalidation: `ConnectionManager::invalidate_revision(connection_id, old_revision)` (already at `src/core/provider_connections.rs:189-195`) prevents new resolutions from returning the old revision after a successful rotation/endpoint update.
- An in-flight request using the old revision completes normally on the captured old revision unless explicitly cancelled for security policy. The chosen default is documented in the closure record: "no automatic cancellation of in-flight requests on rotation; security policy can be added later via a typed seam."
- A failed or cancelled rotation does not invalidate the active runtime.
- Concurrent rotations use expected revision and one winner; stale callers receive current redacted state.
- Restart reconstructs only the committed active revision lazily. The existing `ConnectionManager::resolve` already does not probe.

### 7.3 Health and model refresh coordinator (work package C)

Implement a daemon-owned single-flight refresh path per connection:

- Single-flight key: `ProviderConnectionId`. A `tokio::sync::Mutex<HashMap<ProviderConnectionId, Arc<RefreshState>>>` (or `DashMap`) on the `ConnectionManager`.
- Explicit manual refresh operation: `CoreRequest::ModelsRefresh` (already declared at `crates/codegg-protocol/src/core.rs:400`) must be wired to the refresh coordinator. Add `CoreRequest::ConnectionRefreshBegin { connection_id, expected_revision }` for explicit refresh and `CoreRequest::ConnectionRefreshCancel { operation_id }` for cancellation.
- Optional bounded background refresh: `Config::provider_connections.background_refresh` defaults to `disabled` (off). When enabled, a per-connection `next_refresh_at` is computed with exponential backoff and jitter after failures.
- Connection and global concurrency caps: `Config::provider_connections.max_concurrent_refreshes` (default 1) and `Config::provider_connections.global_refresh_cap` (default 4).
- Connect/read/overall timeouts: `Config::provider_connections.refresh_connect_timeout_ms` (default 3000), `refresh_read_timeout_ms` (default 5000), `refresh_overall_timeout_ms` (default 10000).
- Exponential backoff with jitter after failures: `backoff = base * 2^attempt`, `jitter = uniform(0, backoff * 0.2)`. Deterministic clock/RNG seams for tests.
- No synchronous startup probe.
- Cancellation via `tokio_util::sync::CancellationToken` or `tokio::sync::Notify`.
- Redirect/TLS/endpoint policy identical to provisioning (reuse `crates/codegg-providers/src/eggpool.rs` validation).
- Redacted error classification: `RefreshError` enum with `Disabled`, `CredentialMissing`, `Timeout`, `Tombstoned`, `EndpointPolicy`, `BoundedBody`, `Unknown`. Error variants do not carry secrets, URLs with credentials, or credential headers.
- Bounded model count (existing cap from Milestone 002), field lengths, and payload bytes (existing cap from Milestone 002).
- Deterministic catalog normalization/digest/revision: extract `digest_models` to a public helper in `crates/codegg-providers/src/eggpool.rs` (or `crates/codegg-providers/src/catalog.rs`) and reuse it for refresh. Normalization: sort by `(id, name)`, lowercase ASCII, drop duplicates, apply length cap.
- No catalog revision change when normalized content is unchanged.
- Health timestamps/status transitions and stale threshold (`Config::provider_connections.health_stale_after_ms`, default 5 minutes).
- Coalescing of concurrent selection/UI refresh requests: a single in-flight refresh shares its result with all callers via `tokio::sync::watch` or a `OnceCell` + `Notify`.

A failed refresh updates bounded health diagnostics according to policy but must not erase the last valid model catalog or active runtime. The last-good catalog is preserved in `provider_connection_models`; only `provider_connection_health` rows are updated on failure.

### 7.4 Lifecycle state machine (work package A)

Extend `ProviderConnectionState` (`crates/codegg-core/src/provider_connections.rs:339-343`) with:

```text
pub enum ProviderConnectionState {
    Active,
    Disabled,
    CredentialMissing,
    ProvisioningRotating,   // new
    Tombstoned,             // new (replaces hard row delete)
    Error,                  // new (terminal health failure)
    Stale,                  // new (health past stale threshold but not yet Error)
}
```

Required behavior:

- `disable` prevents new provider resolution and new selections. Reuse `ConnectionError::Disabled` (`src/core/provider_connections.rs:38`).
- Existing sessions retain their explicit selection but resolve to typed disabled/unavailable status rather than fallback. `SelectionService` already returns `ConnectionNotSelectable`; add `Reason::Disabled | Reason::CredentialMissing | Reason::Tombstoned` and a `current_selected_model_id` field.
- `enable` requires a valid credential/binding and may optionally require a successful probe according to documented policy. Add `CoreRequest::ConnectionEnable { connection_id, expected_revision, require_probe: bool }`.
- `delete` defaults to logical tombstone/soft delete so references remain explainable. `ProviderConnectionStore::delete` becomes a state transition to `Tombstoned` instead of a row DELETE.
- `restore` is supported when metadata/credential policy allows. `CoreRequest::ConnectionRestore { connection_id, expected_revision }` returns the connection to `Active` (or `CredentialMissing` if the binding is gone).
- `hard purge` is an explicit administrative operation only when no session/reference/provisioning/runtime dependency remains, or otherwise returns typed blockers (`PurgeBlocker::SelectedSessions(n)`, `PurgeBlocker::ProvisioningOperation(id)`, `PurgeBlocker::ActiveRuntime`). `CoreRequest::ConnectionPurge { connection_id, expected_revision }` returns `PurgeOutcome::Purged | PurgeOutcome::Blocked(Vec<PurgeBlocker>)`.
- Credential removal transitions affected connections to `CredentialMissing` without deleting identity/history.
- Lifecycle transitions use expected revision and are idempotent where appropriate. Idempotent transitions (`Active → Active`, `Disabled → Disabled`) are explicitly tested.

Reference counting:

- Add a `ProviderConnectionReferenceStore` table (migration v29) that records `connection_id, reference_kind, reference_id` where `reference_kind ∈ { SelectedSession, ProvisioningOperation, ActiveRuntime }`.
- `SelectionService::update` increments/decrements `SelectedSession` references on transition to/from `Selected` state.
- `ConnectionManager::resolve` increments `ActiveRuntime`; the runtime's `Drop` decrements.
- `EggpoolProvisioner` increments/decrements `ProvisioningOperation` for the lifetime of an in-flight provision.
- `purge_eligibility(connection_id)` reads the reference table and returns `Vec<PurgeBlocker>`.

Tombstone preservation:

- `delete` writes a `provider_connection_tombstones` row (migration v29) with `connection_id, tombstoned_at, tombstoned_by_actor, last_known_revision, last_known_catalog_revision, last_known_endpoint_authority`. The tombstone row survives any future restore but is removed by `hard_purge`.

### 7.5 Session and selection behavior (work package D)

Extend selection outcomes/projections so clients can reconcile lifecycle changes without guessing:

- Stale revision/catalog outcomes include `current_connection_id`, `current_connection_revision`, `current_catalog_revision`, and `current_selected_model_id`. Update `SelectionUpdateOutcome::StaleRevision` and `SelectionUpdateOutcome::StaleCatalog` (`src/core/session_selection.rs:42-52`).
- Selected sessions surface disabled, deleted, credential-missing, unhealthy/stale, or removed-model state. Add `SessionLifecycleProjection { connection_id, state, last_health_at, current_selected_model_id, removed_models: Vec<String> }` returned by `CoreRequest::SessionLifecycleGet { session_id }`.
- Removed models preserve the connection choice and require explicit replacement. `SessionLifecycleProjection::removed_models` enumerates models that were in the catalog at selection time but are not in the current digest.
- Active sessions do not silently clear or change selection. This invariant already holds; the lifecycle projection surfaces it.
- Turn submission fails with a typed actionable connection-state error before model invocation when the selected connection is unusable. Extend `src/core/daemon.rs:945-968` to check the connection's lifecycle state and return `TurnSubmitError::ConnectionState { connection_id, state, last_health_at }`.
- No implicit selection of another active connection occurs. Already enforced by `SelectionService`.

If lifecycle additions enlarge `SessionSelectionDto`, box or split the large variant. The current `Selected` variant (`crates/codegg-protocol/src/provider.rs:172-177`) carries `connection_revision: u64` and `catalog_revision: String`; the proposed `current_selected_model_id: Option<String>` addition is small. If the proposed `removed_models: Vec<String>` is added to a `Selected` variant, that variant must be boxed (`Box<ProviderConnectionSummaryDto>` or split into a sibling variant). Verify `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes without `#[allow(clippy::large_enum_variant)]` on `SessionSelectionDto`.

Additive protocol operations/events:

- `CoreRequest::SessionLifecycleGet { session_id }` → `CoreResponse::SessionLifecycle { projection: SessionLifecycleProjection }`.
- `CoreEvent::ConnectionStateChanged { connection_id, old_state, new_state, actor_seam, at }`.

### 7.6 Protocol and DTOs (work packages B, C, D)

Additive redacted operations:

- `ConnectionGet { connection_id }` → `ConnectionDetail { connection_id, display_name, endpoint_authority, tls_policy, scope, state, revisions, last_health, current_catalog_revision, actor_seam }`.
- `ConnectionListDetail {}` → `Vec<ConnectionDetail>` (admin/operator view).
- `ConnectionRefreshBegin/Cancel/Status/Result`.
- `ConnectionEnable/Disable`.
- `ConnectionDelete/Restore`.
- `ConnectionPurge { connection_id, expected_revision }` → `PurgeOutcome`.
- `ConnectionLifecycleEvent { ... }` (CoreEvent).
- `SessionLifecycleGet { session_id }` → `SessionLifecycle { projection: SessionLifecycleProjection }`.

DTOs may contain stable IDs, display name, endpoint authority, TLS policy, scope label, lifecycle/health state, revisions, model summaries, durations, timestamps, and bounded error codes/messages. They must not contain secret references where unnecessary, credentials, encrypted payloads, auth headers, or secret-derived fingerprints.

Secret-bearing inputs (`ConnectionRotateBegin.secret: SecretInputRef`) remain local-only. The server WS guard at `src/server/ws.rs:930-941` must be extended to match `CoreRequest::ConnectionRotateBegin { .. }` and return `secret_operation_remote_denied` for any remote WebSocket carriage. This guard extension is a precondition for landing the request variant.

### 7.7 TUI/operator surface (work package E)

Extend the existing `/connections` dialog rather than creating disconnected provider-management state.

Required actions:

- Inspect redacted connection detail/status. `r` opens a redacted detail panel that renders `ConnectionDetail` from the protocol surface.
- Manual health/model refresh. `R` (shift-r) triggers `ConnectionRefreshBegin` and shows a progress indicator; `Esc` during refresh triggers `ConnectionRefreshCancel`.
- Rotate credential and optionally endpoint metadata through masked/local-only input. `Ctrl-R` opens a masked secret-input overlay that uses the local-only secret transport and never echoes the typed secret. Endpoint metadata changes use a separate dialog because they do not require a secret.
- Enable/disable. `e` toggles. Disabled connections show `Disabled` in the list and require `e` to enable.
- Delete/restore with confirmation and reference blockers. `d` opens a confirmation dialog that lists `PurgeBlocker` items; the operator can confirm `Soft delete (tombstone)` or `Hard purge (only if no blockers)`. `Ctrl-D` initiates restore from tombstone.
- Show active session/reference counts and lifecycle consequences. The detail panel renders `SelectedSession(n)`, `ActiveRuntime(n)`, `ProvisioningOperation(n)`.
- Show last successful probe, stale state, current revision, catalog revision, and bounded diagnostics. Rendered from `ConnectionDetail.last_health`.
- Handle stale revisions by applying current state from the outcome or refreshing automatically. On `StaleRevision`, the dialog re-fetches `SessionLifecycleGet` and shows the new state.
- Never place secret input into normal command history, prompt text, remote snapshots, or reusable TUI state. Secret input lives only in the masked overlay's local buffer; cleared on submit/cancel/close.

A deterministic fake-daemon/CoreClient harness drives keyboard/action flows through connect, select, refresh, rotate, enable/disable, delete/restore, and purge flows without requiring a real terminal service or external Eggpool. Reuse the existing `tests/fake_eggpool_mcp`-style pattern at `tests/` for a fake daemon. The harness lives at `tests/provider_connections_lifecycle.rs` (or `tests/fake_daemon_lifecycle.rs` if a single harness is preferred).

### 7.8 Flaky-test stabilization (work package F)

Investigate and fix `core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog` (`src/core/eggpool.rs:1077-1122`) under broad parallel load. Preferred approaches:

1. Replace `fake_eggpool(Duration::ZERO)` with a deterministic readiness barrier: a `tokio::sync::oneshot` channel that the fake server signals once `bind` returns. The test awaits the barrier before issuing the provisioning request.
2. Use `tokio::time::pause()` + `advance` for deterministic clock.
3. Move the fake server to a dedicated `tokio::task::spawn_local` so its lifecycle is owned by the test.
4. Use isolated TCP ports per test invocation (a port allocator helper in `tests/common/`).

Do not increase arbitrary sleeps.

The closure record must include evidence of repeated broad-load runs (50 iterations minimum) without reproduction. The same repeated-load loop must run for the new refresh coordinator tests (work package C) and rotation transaction tests (work package B).

### 7.9 Security and authorization

- Exact provider/account binding remains mandatory; no fallback account lookup. Already enforced by `ProviderConnectionStore::update` and `ConnectionManager::resolve`.
- Rotation cleanup must not delete an unrelated credential record. The cleanup path uses the `SecretRef` previously associated with the connection and is scoped to that reference.
- Endpoint changes re-run all scheme/host/port/TLS/redirect validation.
- Secret input and errors remain redacted, including failure bodies and URLs. The `RefreshError` and `RotationError` enums do not carry secret-bearing fields.
- Lifecycle operations retain capability/authorization seams for later team roles but do not infer access from connection scope. The `actor_seam` field on protocol DTOs is the future hook.
- Audit-ready events record actor seam, connection ID, action, revisions, endpoint authority, outcome, and duration, never secrets. Add a `ProviderConnectionAuditEvent` table (migration v29) with bounded column types.

### 7.10 Static guards

Add `scripts/check_provider_connections_m4_coverage.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

# ConnectionManager must expose lifecycle operations
for sym in rotate refresh disable enable delete restore; do
    grep -q "pub fn ${sym}\|pub async fn ${sym}" src/core/provider_connections.rs \
        || { echo "missing ConnectionManager::${sym}"; exit 1; }
done

# ProviderConnectionState must include Tombstoned and Error
grep -q "Tombstoned" crates/codegg-core/src/provider_connections.rs \
    || { echo "missing ProviderConnectionState::Tombstoned"; exit 1; }
grep -q "Error\b" crates/codegg-core/src/provider_connections.rs \
    || { echo "missing ProviderConnectionState::Error"; exit 1; }

# Server WS guard must cover ConnectionRotateBegin
grep -q "ConnectionRotateBegin" src/server/ws.rs \
    || { echo "missing server WS guard for ConnectionRotateBegin"; exit 1; }

# CoreRequest variants must exist
for v in ConnectionRotateBegin ConnectionRotateCancel ConnectionRotateStatus \
         ConnectionRefreshBegin ConnectionRefreshCancel ConnectionRefreshStatus \
         ConnectionEnable ConnectionDelete ConnectionRestore ConnectionPurge \
         SessionLifecycleGet; do
    grep -q "${v}" crates/codegg-protocol/src/core.rs \
        || { echo "missing CoreRequest::${v}"; exit 1; }
done

echo "ok"
```

Add this script to the AGENTS.md static-guard list and to CI.

### 7.11 Documentation updates

- `architecture/provider.md` — complete lifecycle state machine, rotation sequence, refresh defaults, deletion/reference policy.
- `architecture/auth.md` — local-only secret transport, redaction policy, secret-input buffer lifecycle.
- `architecture/session.md` — selected-session lifecycle projection, stale-revision reconciliation, turn-submit typed failures.
- `architecture/protocol.md` — additive protocol variants and the WS guard extension.
- `architecture/storage.md` — additive migrations v29 (reference store, tombstone, audit event).
- `architecture/tui.md` — `/connections` dialog lifecycle actions and fake-daemon harness.
- `architecture/core.md` — rotation and refresh coordinator seams, in-flight pinning guarantees.
- Operator examples: connect → rotate → refresh → disable → delete → restore → purge for Eggpool.
- Legacy-removal criteria: legacy `provider`/`model` strings remain via `LegacyResolution` until the corrective plan's closure record accepts explicit removal criteria. This corrective plan does not commit to a removal date; it records the criteria.

## 8. Ordered work packages

The original plan's A–F ordering is preserved. Each work package lists the required production changes from §7.

### Work package A — Lifecycle state machine and reference policy

Required changes: §7.4, §7.10 (Tombstoned/Error grep), schema migration v29 (reference table, tombstone table, audit event table).

Acceptance evidence:

- Transition table tests cover valid/invalid/idempotent/stale cases for all seven states.
- Selected sessions never fall back (regression test against `SelectionService::ConnectionNotSelectable`).
- Delete/restore preserves explainable identity/history (tombstone row survives restore; restore round-trip tests pass).
- `purge_eligibility` returns typed blockers for each reference kind.

### Work package B — Rotation transaction and runtime invalidation

Required changes: §7.1, §7.2, §7.6 (rotate protocol variants), §7.10 (rotate grep, WS guard extension).

Acceptance evidence:

- Failed/cancelled rotation retains old active revision.
- New requests use new revision after commit (verified via cache invalidation + resolve with expected_revision).
- In-flight request finishes on captured old revision (concurrent resolve + rotate test).
- Concurrent rotations yield one winner and typed stale outcome.
- Rotation cleanup deletes only the previous binding reference.
- WS guard denies rotate-with-secret over remote WebSocket.

### Work package C — Health/model refresh coordinator

Required changes: §7.3, §7.6 (refresh protocol variants).

Acceptance evidence:

- Concurrent refresh coalesces (single in-flight probe; multiple callers share result).
- Unchanged catalog keeps revision (digest equality test).
- Failed refresh keeps last-good catalog/runtime (catalog rows untouched on failure).
- Repeated failures honor backoff and do not overload fake Eggpool (repeated-load test with deterministic clock).
- Manual refresh operation works end-to-end through `CoreRequest::ModelsRefresh`.

### Work package D — Session/protocol reconciliation

Required changes: §7.5, §7.6 (lifecycle projection variants).

Acceptance evidence:

- Old clients remain decodable (additive DTO fields only; serialized form remains backward compatible).
- No secret fields serialize (snapshot diff against pre-existing fixtures).
- Disabled/deleted/credential-missing/removed-model states are explicit (`SessionLifecycleProjection` round-trip tests).
- No silent connection/model mutation (regression test against `SelectionService`).
- Large-variant clippy lint resolved (verified by `cargo clippy -- -D warnings`).
- Turn submission against each unusable lifecycle state returns a typed actionable error.

### Work package E — TUI lifecycle controls and end-to-end harness

Required changes: §7.7, §7.10 (Coverage guard passes).

Acceptance evidence:

- Deterministic keyboard/action tests cover connect/select/refresh/rotate/enable/disable/delete/restore/purge.
- Secret input never enters normal TUI snapshots/history (snapshot diff after masked input submit/cancel/close).
- Stale response reconciles without blind overwrite (re-fetch on StaleRevision observed by test).

### Work package F — Flaky-test, docs, and roadmap closure

Required changes: §7.8, §7.9, §7.11, schema migration v29.

Acceptance evidence:

- Repeated broad-load test runs no longer reproduce the provider timing failure (50 iterations).
- Provider-focused clippy/tests are clean or remaining unrelated findings are explicit (closure record enumerates).
- Every roadmap exit criterion has evidence (closure matrix in `plans/closure/provider-connections/005-status.md`).
- Architecture docs and operator examples updated.

## 9. Failure, cancellation, restart, and contention semantics

Inherited from the original plan §8 verbatim and reaffirmed:

- Rotation validation failure makes no persistent change.
- Cancellation before rotation commit removes staged credential/metadata and preserves the active revision.
- Cancellation after commit returns/reconstructs the committed result; cleanup is retryable/idempotent.
- Crash with staged rotation/provisioning is recovered from durable operation state and either completed safely or rolled back; no staged secret becomes active accidentally.
- Concurrent rotations/endpoint updates use expected revision; one wins, others receive current state.
- Refresh cancellation keeps last-good health/catalog and records cancellation separately from endpoint failure.
- Concurrent refresh calls single-flight and share one result.
- Disable is immediate for new resolutions; in-flight requests follow documented captured-runtime semantics (no automatic cancellation; security policy seam available).
- Delete/tombstone prevents new resolution/selection but preserves references and diagnostics.
- Hard purge fails with explicit blockers while sessions, credentials, operations, or runtime references remain.
- Restart reconstructs committed lifecycle/revision state lazily and does not probe synchronously.
- Credential-store record missing on restart yields credential-missing state without selecting another account.

Additions specific to this corrective pass:

- Tombstone state survives restart; restore from restart is idempotent.
- Reference table is the authoritative source for purge blockers; missing references (e.g., session row deleted out-of-band) do not block purge silently — `purge_eligibility` reads the table directly.

## 10. Compatibility and migration

Inherited from the original plan §9 verbatim and reaffirmed. Storage migration v29 is additive:

- `provider_connection_references(connection_id, reference_kind, reference_id, created_at)` — append-only.
- `provider_connection_tombstones(connection_id, tombstoned_at, tombstoned_by_actor, last_known_revision, last_known_catalog_revision, last_known_endpoint_authority)` — append-only; one row per tombstoned connection.
- `provider_connection_audit_events(event_id, connection_id, action, actor_seam, old_revision, new_revision, endpoint_authority, outcome, duration_ms, at)` — append-only.
- Existing v24/v25/v26/v27/v28 tables untouched.

The `ProviderConnectionStore::delete` method's hard row DELETE behavior is replaced by a state transition to `Tombstoned` (migration v29). Existing call sites that relied on hard row deletion must be updated to treat the connection as present-but-tombstoned. A `scripts/check_provider_connections_tombstone_compat.sh` static guard verifies no production code path relies on the row-absence behavior of the old `delete`.

## 11. Required tests

Inherited from the original plan §10 and extended:

### Focused unit tests (added)

- `ProviderConnectionState::can_transition_to` covers all 49 pairs (7×7) with explicit allow/deny table.
- Catalog normalization/digest stability (sorted, lowercased, deduplicated, length-capped).
- Refresh backoff/jitter/cap calculations with deterministic clock/RNG seams.
- Redacted DTO serialization and absence of secret fields (snapshot diff for each new DTO).
- Purge blocker calculation for each reference kind.
- Idempotent lifecycle transitions (`Active → Active`, `Disabled → Disabled`).

### Integration tests (added)

- Successful credential rotation and endpoint rotation.
- Invalid credential/unavailable endpoint rotation rollback.
- In-flight old revision plus new-request new revision.
- Concurrent rotation winner/stale loser.
- Health/model refresh success, unchanged catalog, changed catalog, failure, stale, cancellation, and coalescing.
- Disable/enable/delete/restore with selected sessions.
- Credential deletion transitions to `CredentialMissing`.
- Turn submission against each unusable lifecycle state.
- Multiple sessions sharing one connection.
- Hard purge blocked by selected sessions / provisioning operation / active runtime.

### Restart and recovery tests (added)

- Crash/failpoints at each staged rotation boundary.
- Committed revision survives restart; old staged revision does not activate.
- Interrupted refresh preserves last-good catalog.
- Tombstone/restore and reference counts survive restart.
- No startup probe (timer assertion).

### Contention and cancellation tests (added)

- Many concurrent refresh callers result in one probe.
- Global/per-connection probe caps.
- Rotation vs refresh race resolves by revision/generation.
- Disable/delete during in-flight request follows documented semantics.
- Cancellation cleanup is idempotent.

### TUI/protocol tests (added)

- Fake-daemon/CoreClient lifecycle flow.
- Stale revision response includes current bounded state (including `current_selected_model_id`).
- Rotate secret rejected over disallowed remote transport.
- Dialog state contains no secret after submit/cancel/close.
- Old protocol fixtures still decode.
- Lifecycle DTO large-variant warning is resolved (clippy clean).

### Security and negative tests (added)

- Secret absent from SQLite, logs, DTO JSON, errors, snapshots, debug output, and test artifacts.
- Credential cleanup never deletes unrelated provider/account records.
- Endpoint userinfo/query/fragment/redirect/TLS violations rejected.
- No fallback account/connection after disable/delete/missing credential.
- Bounded response/error body handling.

### Flake and scale tests (added)

- Repeat provisioning/rotation/refresh tests under broad parallel load (50 iterations minimum, recorded in closure record).
- Deterministic fake-server readiness without sleeps.
- Model-catalog maximum bounds.
- Probe-storm/backoff simulation.

## 12. Required verification commands

Inherited from the original plan §11 verbatim and extended with the new static guards:

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test -p codegg-providers
rtk cargo test -p codegg-providers connection
rtk cargo test -p codegg-core provider_connections
rtk cargo test -p codegg-core session::legacy_resolution
rtk cargo test -p codegg-core lifecycle
rtk cargo test -p codegg-protocol
rtk cargo test --test session_selection
rtk cargo test --test session_crud
rtk cargo test --test storage_migrations
rtk cargo test --test provider_connections_lifecycle
rtk cargo test --test rotation
rtk cargo test --test refresh_coordinator
rtk cargo test -p codegg --lib core::eggpool
rtk cargo test -p codegg --lib core::provider_connections
rtk cargo test -p codegg --lib connect
rtk cargo test -p codegg --lib session_selection
rtk cargo test -p codegg --lib tui::commands::connection_lifecycle
rtk cargo test -p codegg --lib tui::components::dialogs::connection_selection
rtk bash scripts/check-core-boundary.sh
rtk bash scripts/check_provider_connections_m4_coverage.sh
rtk bash scripts/check_provider_connections_tombstone_compat.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_scheduler_bypass.py
rtk python3 scripts/check_git_forbidden_patterns.py
rtk git diff --check
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 rtk cargo test --workspace --all-features -- --test-threads=14
```

The formerly flaky provisioning test must be run 50 times in a tight loop under broad parallel load, with iteration count and environment recorded in the closure record. The same loop applies to the rotation and refresh tests.

## 13. Documentation updates

Inherited from the original plan §12 and confirmed:

- Complete provider connection lifecycle state machine.
- Explain staged rotation and old/new revision behavior.
- Document health/model refresh scheduling, caps, backoff, stale thresholds, and manual refresh.
- Document disable/delete/restore/purge and selected-session consequences.
- Document local-only secret transport and redaction.
- Document operator workflows and recovery from missing credentials/unavailable endpoints.
- Record legacy config/model-string removal prerequisites.

## 14. Acceptance criteria

The original plan §13 acceptance criteria are reaffirmed verbatim. This plan adds:

- `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes without `#[allow(clippy::large_enum_variant)]` on `SessionSelectionDto` (large-variant resolved by boxing or splitting).
- `scripts/check_provider_connections_m4_coverage.sh` passes on the closing commit.
- `scripts/check_provider_connections_tombstone_compat.sh` passes on the closing commit.
- Hard purge returns typed `PurgeBlocker` items; no silent soft-delete fallback to `delete`.
- Reference table is the authoritative source for purge blockers; `purge_eligibility` reads it directly.
- Server WS guard extension denies `ConnectionRotateBegin` over remote WebSocket.

## 15. Stop conditions

The agent must stop and report rather than improvise when:

Inherited from the original plan §14 verbatim, plus:

- A new secret backend would be required to implement rotation.
- Hard purge would orphan session/history references and the implementation cannot enumerate blockers.
- The lifecycle state machine would silently change session selection or introduce fallback.
- Team authorization is required to define personal/project/deployment access.
- Background refresh would require unbounded startup or probe behavior.
- Work expands into distributed secret transport or project/TUI roadmap scope.
- The new static guards (`check_provider_connections_m4_coverage.sh`, `check_provider_connections_tombstone_compat.sh`) cannot be written without scanning production code paths the agent does not own.

## 16. Closure evidence required

The closure record at `plans/closure/provider-connections/005-status.md` must contain:

- Exact implementation commit(s).
- Complete lifecycle transition matrix (all 49 pairs).
- Staged rotation sequence and failpoint results.
- In-flight/new-request revision evidence.
- Refresh bounds/backoff/coalescing/catalog-revision evidence.
- Selected-session disable/delete/missing-credential behavior.
- Redaction matrix across storage/protocol/TUI/logs/errors.
- Fake-daemon/TUI lifecycle harness results.
- Formerly flaky test root cause, fix, and repeated-load evidence (50 iterations).
- Compatibility/migration review.
- Full verification log with pass/fail counts.
- Complete Phase 2 requirement-to-evidence matrix and roadmap disposition.
- Confirmation that the static guards pass on the closing commit.

## 17. Handoff notes

- Treat `213272c` as the reviewed production baseline; inspect current `main` before editing.
- Preserve the existing credential store and local-only secret transport.
- Extend `src/server/ws.rs:930-941` to cover `ConnectionRotateBegin` BEFORE introducing the request variant.
- Use deterministic fake-server readiness and clocks instead of arbitrary sleeps.
- Follow the repository's resource-conscious test configuration per `AGENTS.md`.
- Scope metadata is not authorization; leave role enforcement for the later identity/ACL roadmap.
- The closure record for Milestone 003 explicitly recorded that "rotation, health refresh, deletion, and team authorization remain Milestone 004 scope"; team authorization remains deferred per this plan and is unaffected.
- The original plan's handoff notes (§16) remain authoritative for the rotation sequence, refresh defaults, and lifecycle state machine description.
