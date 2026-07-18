# Project Catalog Milestone 004 — Protocol, Server Migration, and Closure

Status: ready for handoff

Repository baseline: `466356f8bef4242e24bafea1a4d5603e91d9f197` (`main`; planning-only commits after this baseline do not alter production behavior)

Production implementation baseline:

- `84d92f0` — canonical project/repository/workspace/session identity storage and reconciliation.
- `ec42dce` — identity-aware daemon requests, session binding DTOs, canonical server project IDs, and bounded directory compatibility resolution.
- `a2db5e4` — durable project catalog service with project lifecycle, locators, health placeholders, and probe-free hydration.
- `5974976` — bounded discovery roots, scans, reconciliation, cancellation/status, unresolved observations, and schema v29.
- `972c286` and `2293a11` — runtime-asset refresh protocol, lifecycle triggers, immutable generations, and activation-safe runtime pinning.
- `27cbd43` — owner-scoped lazy project activation, asset refresh integration, bounded health aggregation, lease expiry, and restart-safe transient state.

Source roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-4--protocol-server-migration-and-closure`

Long-term requirements:

- `plans/000-long-term-specification.md` — durable multi-project catalog, project-scoped daemon authority, lazy activation, project-relative frontend operations, and TUI catalog consumption.
- `plans/001-terminology-and-domain-model.md` — Project, Repository, Workspace, ProjectLocator, health, discovery observation, activation, and compatibility projection.
- `plans/002-long-term-roadmap.md#phase-3--project-catalog-and-lazy-discovery`

Applicable closure evidence:

- `plans/closure/domain-identity/002-status.md`
- `plans/closure/domain-identity/003-corrective-status.md`
- `plans/closure/runtime-assets/003-status.md`
- `plans/closure/runtime-assets/004-status.md`
- `plans/closure/project-catalog/001-status.md`
- `plans/closure/project-catalog/002-status.md`
- `plans/closure/project-catalog/003-status.md`

Applicable ADRs:

- None. The canonical documents already select daemon-owned catalog authority and explicit project/workspace context. Stop for an ADR if implementation requires changing project/repository cardinality, exposing daemon-local paths as remote identity, or introducing a second activation/discovery authority.

Primary class: capability

## 1. Objective

Expose the complete closed project-catalog, discovery, activation, and health services through the native protocol and server adapters; remove the process-global `ServerState.project_dir` as project authority; preserve bounded compatibility behavior for older clients; and close Phase 3 with multi-project restart, isolation, scale, and transport evidence.

The milestone succeeds when several projects can be listed, inspected, registered, discovered, activated, health-checked, archived/restored, and session-scoped through one daemon/server using stable `ProjectId + WorkspaceId` context. REST, WebSocket, in-process, socket, and stdio adapters must consume the same daemon authority, and no server route may infer a project identity from a default path.

This milestone provides the stable protocol contract required by Multi-Project TUI Milestone 001. It does not implement TUI tabs, the `Space f` picker, remote SSH execution, team authorization, presence, or session projection reducers.

## 2. Why this milestone is ready

All hard dependencies are closed:

- Project Catalog 001 provides durable catalog records and lifecycle.
- Project Catalog 002 provides bounded discovery/reconciliation and operation status/cancellation.
- Project Catalog 003 provides lazy activation, refresh composition, health aggregation, and transient lease ownership.
- Domain Identity 003 provides canonical request context and additive compatibility DTOs.
- Runtime Assets 003–004 provide refresh/generation and in-flight snapshot semantics required by activation.

No downstream TUI or authorization implementation is needed to define and verify a frontend-neutral catalog protocol.

## 3. Current implementation evidence

At the repository baseline:

- `ProjectCatalog` exposes list/get/register/archive/restore, workspace/session/locator inspection, durable health, and probe-free restart hydration.
- `DiscoveryCoordinator` exposes root validation/list/get, preview, refresh/start-refresh, refresh-all, cancellation, scan status, unresolved observations, and explicit revision-checked workspace association.
- `CoreDaemon::activate_project_workspace` resolves canonical project/workspace binding, acquires a bounded owner-scoped activation lease, invokes the existing runtime-asset refresh authority, and returns transient health.
- `CoreDaemon::project_health` is read-only and does not activate services.
- `ProjectActivationRegistry` has bounded TTL/capacity and an eviction seam, but no native protocol consumer or recurring maintenance caller.
- `crates/codegg-protocol` has canonical `ProjectContextDto`/`SessionBindingDto` and runtime-asset refresh DTOs, but no project catalog/discovery/activation DTO/request/response/event family.
- `CoreRequest::SessionList` still accepts a required string `project_id`; session creation has additive optional canonical fields.
- `src/server/routes/project.rs` returns stable catalog project IDs, but compatibility `get_project` and relative `create_project` still use `state.project_dir` as the default locator/root.
- `ServerState` still stores `project_dir: String`.
- Current HTTP project creation may create a directory beneath the server root before registering it; remote clients therefore still interact with daemon-local path semantics.
- Multi-Project TUI 001 is blocked specifically on stable project list/get and project/workspace summaries over `CoreClient`.

## 4. Invariants that must not regress

- The daemon and catalog services remain the sole project authority; protocol/server/TUI layers are adapters only.
- Project IDs are stable typed identities, never paths or locator strings.
- Catalog list/get and health reads remain bounded and probe-free.
- Discovery is explicit, bounded, cancellable, and never activates project services.
- Activation is explicit and scoped to one validated `ProjectId + WorkspaceId` plus a server-derived client owner.
- Runtime-asset refresh remains owned by the existing coordinator; the protocol does not create a second refresh path.
- Archive is logical and non-destructive.
- Remote locator variants remain inert; no SSH/linked-node locator is coerced into a local path.
- Server compatibility defaults may select one existing canonical context but may not create identity from path text.
- Protocol payloads are bounded, redacted, and contain no credentials or unrestricted daemon-local paths.
- Catalog events include explicit project scope and are not emitted as unfilterable global detail streams.

## 5. Scope

### In scope

- Project/catalog/discovery/activation/health DTOs and capability negotiation.
- Native `CoreRequest`, `CoreResponse`, and bounded `CoreEvent` variants.
- Daemon handlers using existing catalog/discovery/activation authorities.
- Client ownership and lifecycle for serializable activation handles.
- REST and WebSocket project route migration.
- Removal of authoritative `ServerState.project_dir`.
- Explicit compatibility-default resolution for old routes/clients.
- Multi-project list/get/register/archive/restore/workspace/session/locator/health operations.
- Discovery root inspection, scan start/status/cancel, unresolved observation inspection, and explicit association.
- Lazy activation/release/health and maintenance eviction integration.
- Pagination/limits, stale revision outcomes, event scoping, restart, scale, and multi-client isolation tests.
- Complete Phase 3 closure evidence and documentation.

### Explicitly out of scope

- Multi-project TUI state, picker, tabs, or navigation.
- Creating or editing arbitrary daemon-local directories from a remote client.
- Remote SSH or linked-node discovery/execution.
- Team membership/ACL enforcement; retain authorization seams and safe filtering defaults.
- Presence, chat, observer mode, ACP, web-specific project models, or frontend-local catalog authority.
- Starting LSP/indexer/build/provider services during list/get/discovery.
- Changing project/repository cardinality or merging ambiguous repositories.
- Removing historical session compatibility fields owned by Domain Identity 004.

## 6. Required protocol surface

### Capability negotiation

Add a stable capability identifier such as `project_catalog.v1` and explicit feature flags for:

- catalog list/get/lifecycle;
- workspace/session/locator summaries;
- discovery inspection and refresh;
- activation and health;
- compatibility-default availability.

Older clients that ignore unknown capabilities/variants remain decodable. Do not bump the protocol version unless current envelope compatibility cannot represent the additive surface.

### Bounded DTOs

Add frontend-neutral DTOs equivalent to:

- `ProjectSummaryDto` — project ID, display name, lifecycle, bounded tags/description summary, primary repository summary, last-opened/updated timestamps, workspace/session counts, locator/health summary, and revision where applicable;
- `ProjectDetailDto` — bounded project metadata, repository relations, locator summaries, workspace summaries, durable health, and diagnostics without eager session bodies;
- `ProjectWorkspaceSummaryDto` — project/workspace IDs, display label, lifecycle/binding state, locator summary, activity/activation summary, and asset generation summary;
- `ProjectLocatorDto` — typed local/SSH/linked-node kind, display-safe summary, availability/support status, and revision; no remote secret/user credential material or raw local path for remote clients;
- `ProjectHealthDto`/`HealthLayerDto` — catalog/workspace/asset/service states, bounded codes/messages, timestamps, and stale state;
- `ProjectActivationDto` — lease ID, project/workspace IDs, owner/client identity class, acquired/expiry timestamps, refresh outcome/generation, health, and binding revision;
- `DiscoveryRootSummaryDto` — root ID/name, mode, enabled/revision, display-safe locator summary, limits summary, and last status;
- `DiscoveryScanStatusDto`/`DiscoveryRefreshResultDto` — operation/root/generation/status, bounded counts, duration, truncation/cancellation flags, and diagnostics;
- `UnresolvedDiscoveryObservationDto` — bounded observation ID/root/generation/status/outcome/diagnostic and display-safe locator summary;
- typed stale/conflict/not-found/unsupported outcomes rather than error-string parsing.

Set explicit maximum list sizes, diagnostics, text lengths, locator summaries, and serialized payload sizes. Large session lists/messages remain behind existing session APIs.

### Native requests and responses

Add requests/responses equivalent to:

Catalog:

- project list with `include_archived`, limit, and cursor/offset contract;
- project get by stable ID;
- register existing local workspace by `WorkspaceId` plus bounded metadata;
- archive/restore with expected revision where the store supports it;
- list project workspaces;
- list project session summaries/counts;
- list project locators;
- get durable/transient health.

Discovery:

- list/get configured roots;
- validate/preview an explicitly configured root;
- start refresh for one root or all enabled roots;
- get scan status;
- cancel scan;
- list unresolved observations with optional root filter;
- explicitly associate an existing workspace to a project using expected binding revision.

Activation:

- activate project/workspace;
- release activation lease;
- get activation/health status;
- optional lease renewal only if required by client lifetime semantics;
- maintenance/eviction remains daemon-owned, not an unrestricted client operation.

All handlers must route through existing `ProjectCatalog`, `DiscoveryCoordinator`, `ProjectContextResolver`, `ProjectActivationRegistry`, and runtime-asset refresh services. No handler may directly recreate their SQL or filesystem behavior unless a narrowly reviewed adapter is required.

### Events

Add bounded events for meaningful state transitions, not every read:

- project registered/updated/archived/restored;
- discovery scan started/completed/cancelled/failed;
- project locator or reconciliation state changed;
- activation acquired/released/expired;
- project health changed materially;
- asset generation changed through the existing asset refresh event, referenced rather than duplicated.

Each event carries stable project/root/workspace scope, correlation/operation ID, bounded summary, and visibility classification/filter seam. Do not broadcast raw discovery candidates, absolute paths, or unbounded diagnostics.

## 7. Daemon ownership and activation lifetime

### Core daemon handlers

Add daemon methods that:

- parse and validate typed IDs at the boundary;
- call the closed service APIs;
- map typed errors to stable protocol outcomes;
- enforce list/report bounds;
- publish scoped events after committed changes;
- never activate on list/get/health/discovery inspection;
- preserve cancellation and coalescing semantics from the underlying coordinator.

### Serializable activation facade

`ProjectActivationLease` is intentionally non-serializable. Add a daemon-owned facade that maps one opaque protocol lease ID to the in-memory lease and derives owner identity from the transport/client context rather than trusting a caller-supplied arbitrary owner string.

Required behavior:

- one client/owner may idempotently activate the same project/workspace;
- different clients may hold separate protocol handles while sharing one workspace bundle;
- release, disconnect, expiry, and daemon shutdown drop the underlying lease exactly once;
- stale/unknown lease IDs return typed outcomes;
- a client cannot release another client's lease;
- capacity/TTL remain bounded by the existing activation policy;
- activation failure with no usable asset generation leaves no retained lease;
- restart has zero active leases and requires explicit reactivation.

Integrate `evict_project_activation_leases` into an existing daemon maintenance loop or a new bounded interval task with clear shutdown ownership. Do not create a free-running untracked task.

## 8. Server and transport migration

### Remove `ServerState.project_dir` authority

Replace `ServerState.project_dir: String` with daemon/catalog services plus, if compatibility requires it, an explicitly typed optional default context such as:

- `default_project_id`;
- `default_workspace_id`;
- display-only default locator summary.

At server startup, any legacy path argument must resolve once through `ProjectContextResolver` to one existing canonical context. Missing or ambiguous resolution produces an actionable startup/route diagnostic; it must not synthesize a project ID.

No route may use a raw path as a cache key, project ID, subscription scope, or ownership boundary.

### REST routes

Provide explicit canonical routes equivalent to:

- `GET /projects`;
- `GET /projects/{project_id}`;
- `POST /projects/register` with existing `workspace_id` and bounded metadata;
- `POST /projects/{project_id}/archive|restore`;
- `GET /projects/{project_id}/workspaces|sessions|locators|health`;
- discovery list/scan/status/cancel/unresolved routes;
- activation/release routes using canonical IDs.

The old no-ID `get_project` compatibility route may project the typed default context when one exists. Otherwise return `project_context_required`. It must not derive the ID from `project_dir`.

Replace or deprecate arbitrary path-creating `POST /projects` behavior. A remote request must not create directories on the daemon filesystem. Local-only explicit workspace registration may remain behind a separately named and permission-ready operation that validates an already registered workspace or uses a local administrative boundary.

### WebSocket/socket/stdio/in-process adapters

- Route all native project requests through `CoreDaemon`.
- Apply the same bounds and typed outcomes across transports.
- Derive activation owner from the connection/client identity available to the transport.
- On disconnect, release client-owned activation handles.
- Apply project/root subscription filters to scoped events.
- Preserve existing remote secret restrictions and avoid adding path-bearing privileged payloads.
- Add `CoreClient` methods consumed by Multi-Project TUI 001; no TUI-specific protocol variants.

## 9. Compatibility requirements

- Existing identity-aware and legacy session operations remain available.
- Old REST clients using one default project may receive a projection only when startup resolved one canonical default context.
- Legacy path arguments are locators used for unique existing-context lookup, never registration or authority.
- Unknown new request/response/event variants retain current forward-compatibility behavior.
- Additive DTO fields use serde defaults where old fixtures require them.
- Explicitly document deprecated endpoints and their removal prerequisites; Domain Identity 004 owns the broader compatibility inventory.
- Do not silently choose the first catalog project as a default.

## 10. Performance and resource requirements

- Project lists are bounded and paginated/limited; no unbounded `Vec` from the full catalog crosses the protocol.
- List/get/health operations do not perform Git probes, scans, activation, LSP/index/build/provider initialization, or asset body loading.
- Discovery uses existing depth/entry/candidate/time/output/stat/Git concurrency limits.
- Concurrent refreshes for one root coalesce; global scan concurrency remains bounded.
- Activation capacity/TTL remain bounded; disconnect cleanup and periodic eviction prevent leaked leases.
- Events carry summaries only and do not replay large discovery reports or project details.
- Scale tests include hundreds/thousands of catalog records and verify bounded response/latency behavior without service activation.

## 11. Security and authorization seams

- Stable IDs are not authorization. Every protocol handler must have a clear future principal/project authorization insertion point.
- Until team authorization lands, do not introduce broader visibility than existing local/trusted daemon clients already possess.
- Project-scoped events include filter metadata and are not globally broadcast with sensitive detail.
- Local paths are omitted or reduced to display-safe summaries for remote clients.
- SSH/linked-node locators expose bounded inert summaries only; no credentials, private keys, host-key material, or secret references.
- Discovery cannot escape configured canonical roots through symlinks/permissions.
- Activation owners are server-derived; clients cannot impersonate another owner or release another lease.
- Remote clients cannot create arbitrary local directories.
- Diagnostics are bounded/redacted and never include provider secrets or untrusted full path dumps.

## 12. Ordered work packages

### Work package A — DTO and capability contract

Intent: establish the frontend-neutral contract before changing server routes.

Required changes:

- define bounded catalog/detail/workspace/locator/health/discovery/activation DTOs;
- define capability flags and typed error/outcome enums;
- add serialization fixtures for old/new clients;
- document limits and path-redaction rules.

Acceptance evidence:

- protocol crate tests cover all variants and bounds;
- old fixtures decode with defaults;
- no DTO contains credential material or unrestricted remote path authority.

### Work package B — Daemon catalog and discovery handlers

Intent: expose closed services without duplicating authority.

Required changes:

- add catalog list/get/register/archive/restore and inspection handlers;
- add discovery root/scan/status/cancel/unresolved/association handlers;
- map errors to typed outcomes;
- publish bounded scoped events after committed mutations;
- preserve probe-free reads and scan coalescing.

Acceptance evidence:

- fake/in-process client tests operate two projects independently;
- listing does not activate services;
- scan cancellation/status survives normal completion and restart metadata lookup;
- stale association revision is rejected without mutation.

### Work package C — Activation protocol facade and maintenance

Intent: safely bridge non-serializable leases to client lifetimes.

Required changes:

- opaque server-owned activation-handle table;
- client-derived ownership and disconnect cleanup;
- activate/release/status/health handlers;
- bounded recurring expiry eviction with explicit shutdown;
- scoped activation/health events.

Acceptance evidence:

- same-client idempotency and cross-client isolation;
- disconnect/expiry releases services exactly once;
- restart creates no phantom activation;
- activation refresh failure retains no lease;
- capacity cannot be exceeded under contention.

### Work package D — Server state and REST/WS migration

Intent: remove the last process-global single-project authority.

Required changes:

- remove `ServerState.project_dir`;
- resolve optional typed compatibility default at startup;
- add canonical project/discovery/activation REST routes;
- deprecate arbitrary path-creating project route;
- route WebSocket/socket/stdio/inproc through the same daemon handlers;
- add project/root event filtering.

Acceptance evidence:

- no server authority derives project ID from path;
- two projects are operated through one server concurrently;
- compatibility route projects one typed default or fails actionably;
- remote arbitrary directory creation is impossible;
- disconnect releases activation handles.

### Work package E — CoreClient contract for downstream TUI

Intent: unlock Multi-Project TUI without implementing it here.

Required changes:

- add transport-neutral `CoreClient` methods for bounded project list/get/workspaces/health and activation/release;
- expose capability/unsupported outcomes;
- ensure stale completions can carry request/project/workspace IDs;
- provide fake-client fixtures for downstream tests.

Acceptance evidence:

- all transports implement the same interface;
- fake client can return multiple same-named projects with distinct IDs;
- list/get does not load sessions/messages for inactive projects;
- no TUI state or renderer change is required in this milestone.

### Work package F — Scale, restart, compatibility, and closure

Intent: close Phase 3 with realistic evidence.

Required changes:

- multi-project server/socket integration tests;
- large-catalog bounded list tests;
- discovery operation restart/status tests;
- activation disconnect/expiry/shutdown tests;
- old/new protocol and REST compatibility fixtures;
- architecture updates and final closure record.

Acceptance evidence:

- all Phase 3 exit criteria map to passing evidence;
- no critical/high unresolved catalog correctness issue remains;
- Multi-Project TUI 001 dependency is explicitly marked unblocked only after closure.

## 13. Failure, recovery, and concurrency requirements

- Catalog mutations are transactional and emit events only after commit.
- Discovery cancellation preserves durable prior catalog records and records operation status.
- Unavailable roots update observations/health; they do not archive/delete projects automatically.
- Concurrent scan requests for one root coalesce; different roots obey global caps.
- Concurrent archive/restore/register/association operations use revision/uniqueness behavior and return typed conflicts.
- Activation handle release is idempotent; disconnect, explicit release, expiry, and shutdown races cannot double-release or leak the underlying lease.
- Daemon restart hydrates catalog/discovery metadata without scans or activation.
- Server startup with an invalid legacy default fails clearly or starts without a default; it never creates a path-keyed project.

## 14. Documentation

Update at least:

- `architecture/project_catalog.md`;
- `architecture/protocol.md`;
- `architecture/server.md`;
- `architecture/core.md`;
- `architecture/workspace.md`;
- `architecture/workspace_services.md`;
- `architecture/session.md`;
- `architecture/tui.md` with the downstream contract only;
- REST/WebSocket API documentation;
- capability and compatibility/deprecation documentation.

Document protocol bounds, activation ownership/lifetime, event scoping, optional typed default behavior, disabled path-creation behavior, discovery operation lifecycle, restart semantics, and downstream TUI expectations.

## 15. Verification commands

The implementation agent must run focused commands equivalent to:

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test -p codegg-protocol
rtk cargo test -p codegg-core project_catalog
rtk cargo test -p codegg-core project_discovery
rtk cargo test -p codegg-core project_discovery_service
rtk cargo test --lib core::project_activation
rtk cargo test --lib core::daemon::tests
rtk cargo test --test project_catalog
rtk cargo test --test project_discovery
rtk cargo test --test project_activation
rtk cargo test --test <new_project_protocol_target>
rtk cargo test --test <new_multi_project_server_target>
rtk cargo test --lib core::transport::daemon_socket
rtk cargo test --lib remote_core_loader_tests
rtk python3 scripts/check_project_catalog_invariants.py
rtk python3 scripts/check_identity_path_usage.py
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_discovery_invariants.py
rtk bash scripts/check-core-boundary.sh
rtk git diff --check
```

Run the capped workspace suite using the repository's documented test resource policy. Any environment-restricted socket binding failure must be recorded separately and supplemented with deterministic in-process/socket tests where possible; it does not excuse missing multi-project transport evidence.

## 16. Stop conditions

Stop and record a corrective dependency or ADR if:

- a protocol/server handler must derive project identity from a path;
- catalog/discovery/activation logic would be duplicated outside existing authorities;
- remote clients require arbitrary daemon-local path creation;
- authorization must be fully implemented to avoid a new data exposure;
- project/repository cardinality must change;
- activation ownership cannot be bound to transport/client identity safely;
- event filtering cannot prevent unscoped sensitive project broadcasts;
- list/get requires eager activation or repository probing;
- a protocol-breaking removal is required rather than an additive capability.

## 17. Acceptance criteria

All criteria are required:

1. Native protocol capabilities and bounded DTOs cover catalog list/get/lifecycle, project workspaces/locators/health, discovery status/control, and activation/release.
2. Every handler delegates to the existing catalog, discovery, context, activation, and asset-refresh authorities; no parallel authority is introduced.
3. `ServerState.project_dir` is removed as project authority.
4. Legacy default-project behavior resolves one typed canonical context or fails with an actionable diagnostic; it never synthesizes identity from a path or selects the first project silently.
5. Several projects can be listed and operated concurrently through one daemon/server with stable IDs and no cross-project state leakage.
6. Catalog list/get/health remain bounded and probe-free and do not retain activation/service leases.
7. Discovery scans remain bounded, cancellable, coalesced, restart-inspectable, and non-destructive when roots are unavailable.
8. Protocol activation handles are owner-scoped, capacity/TTL bounded, released on explicit release/disconnect/expiry/shutdown, and restart-empty.
9. Remote clients cannot create arbitrary local directories or coerce SSH/linked-node locators into local execution.
10. Project events are bounded, explicitly scoped, and filterable; no raw candidates, secrets, or unrestricted paths are broadcast.
11. All CoreClient transports expose the same project list/get/workspace/health/activation contract required by Multi-Project TUI 001.
12. Old protocol/REST compatibility fixtures remain decodable or fail actionably through documented deprecation behavior.
13. Multi-project restart, scale, concurrency, cancellation, and transport tests pass under the repository's bounded test policy.
14. All Phase 3 exit criteria are evidenced in the closure record with no critical/high unresolved catalog correctness issue.

## 18. Required closure evidence

Create `plans/closure/project-catalog/004-status.md` containing:

- executive closure finding;
- Phase 3 requirement-to-evidence matrix;
- protocol capability/DTO/request/response/event inventory;
- daemon ownership review;
- server-state and route migration evidence;
- multi-project transport/isolation results;
- catalog list probe-free and scale evidence;
- discovery cancellation/restart/reconciliation evidence;
- activation ownership/disconnect/expiry/shutdown evidence;
- compatibility-default and deprecated endpoint behavior;
- security/path/locator/event-filtering review;
- exact verification commands and results;
- unresolved findings with severity and owner;
- downstream dependency disposition.

After accepted closure, mark the Project Catalog roadmap closed and mark Multi-Project TUI Milestone 001 dependency-ready. Session Projections 001 remains blocked until Multi-Project TUI 001 closes.