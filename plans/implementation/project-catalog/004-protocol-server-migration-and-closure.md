# Project Catalog Milestone 004 — Protocol, Server Migration, and Closure

Status: active

Repository baseline: `a827ae8` (`docs(plans): record domain identity closure commit`)

Source roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-4--protocol-server-migration-and-closure`

Long-term requirements:

- `plans/000-long-term-specification.md#10-project-catalog-and-discovery`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-3--project-catalog-and-lazy-discovery`

Applicable ADRs:

- None. Existing typed project/workspace context and catalog contracts are sufficient for this milestone.

Primary class: capability

## 1. Objective

Expose the durable project catalog and the existing lazy activation/health seam through the native protocol and server adapters, while removing single-project authority from `ServerState`. A daemon/server instance must be able to list and operate on multiple projects using explicit project/workspace scope, with bounded compatibility behavior for legacy locator calls.

## 2. Why this milestone is ready

- Project Catalog Milestones 001–003 are closed: durable catalog, bounded discovery/reconciliation, and lazy activation/health.
- Runtime Assets Milestone 004 is closed and supplies the activation refresh contract.
- `ProjectContextResolver` is the authority for explicit project/workspace/session scope and read-only directory compatibility lookup.
- `ProjectCatalog`, `CoreDaemon::activate_project_workspace`, and `CoreDaemon::project_health` already provide the domain/runtime operations to adapt.

## 3. Current implementation evidence

- `crates/codegg-protocol` has workspace and identity-aware session operations but no project catalog request/response/event family.
- `src/core/daemon.rs` dispatches workspace operations and owns project activation/health methods, but does not expose catalog operations through `CoreRequest`.
- `src/server/routes/project.rs` reads catalog data but still uses `ServerState.project_dir` for the default project and creation boundary.
- `src/server/routes/session.rs`, `routes/workspace.rs`, `routes/file.rs`, and `ws.rs` retain single-project/default-locator assumptions.
- `src/server/http.rs` constructs `ServerState` from process cwd and a legacy project-local store.
- Existing core, catalog, activation, transport, and server tests provide focused seams for multi-project, restart, and compatibility coverage.

## 4. Invariants that must not regress

- A directory/path is a compatibility locator only; it never becomes a project or workspace identity.
- Project IDs and workspace IDs are validated and resolved through typed catalog/context services before storage or filesystem work.
- Catalog listing and health are bounded and probe-free; listing does not activate services or start external processes.
- Archive is logical and non-destructive; sessions, workspaces, and catalog history remain durable.
- Remote/placeholder locators remain data and are never executed by the local server.
- Server requests cannot silently fall back to another project, workspace, credential, or process-global cwd.
- Legacy clients remain wire-compatible where safe and receive bounded, actionable errors where explicit scope is required.

## 5. Scope

### In scope

- Bounded project DTOs, catalog request/response variants, catalog lifecycle/health events, and protocol capability negotiation.
- Core daemon dispatch for list/get/register/archive/restore, project health, and the existing activation seam as appropriate for transport.
- REST and JSON-RPC/WebSocket adapter migration to explicit project/workspace scope.
- Removal of `ServerState.project_dir` as authoritative identity; any compatibility locator must be request-scoped and resolved through `ProjectContextResolver`.
- Multi-project, scope-isolation, restart/hydration, bounded-list, capability, and legacy-compatibility tests.
- Architecture and operator documentation plus planning/closure artifacts.

### Explicitly out of scope

- Multi-project TUI tab state or session projection reducers.
- SSH/linked-node execution, remote scanning, authorization, team membership, or project hiding policy.
- New discovery/reconciliation semantics or a second asset refresh/activation coordinator.
- Worktree lifecycle redesign, provider ownership, or unrelated server endpoint rewrites.

## 6. Required production changes

### Core/domain

- Use existing `ProjectCatalog` and `ProjectContextResolver` APIs as the sole authority for project operations and scope.
- Add bounded conversions for catalog records, workspace summaries, and durable health records.
- Keep activation/health aggregation path-free and do not persist transient activation health as durable catalog health.

### Protocol and DTOs

- Add project summary/workspace/health DTOs with bounded list fields and stable string-backed IDs.
- Add request/response variants for project list/get/register/archive/restore and scoped health.
- Add project lifecycle/health events with explicit project/workspace IDs.
- Add client/server capability flags with serde defaults so older clients can negotiate/fallback.

### Runtime and concurrency

- Preserve daemon-owned activation lease, refresh, expiry, and eviction authority.
- Do not hold unbounded project lists or perform synchronous probes in protocol/server list paths.
- Ensure concurrent requests for separate projects remain isolated and same-scope operations retain existing coalescing behavior.

### Server and compatibility

- Remove `ServerState.project_dir` and replace default-root assumptions with explicit scope or a clearly named, non-authoritative server resource needed only for safe compatibility/file operations.
- Migrate project/workspace/session routes and JSON-RPC methods to use explicit IDs; locator-only requests resolve uniquely or fail with a typed context-required response.
- Make file/workspace mutation boundaries explicit and reject ambiguous or out-of-scope paths rather than using cwd.
- Keep legacy endpoints available only where their semantics remain unambiguous and document the compatibility behavior.

### Documentation and static guards

- Update `architecture/project_catalog.md`, `architecture/protocol.md`, and `architecture/server.md` with ownership, DTO, capability, route, compatibility, restart, and bound semantics.
- Run project-catalog, daemon-cwd, core-boundary, execution-ownership, and relevant Git/security guards.

## 7. Ordered work packages

### Work package A — Protocol catalog surface

Intent: establish the stable wire contract before adapters depend on it.

Required changes: add DTOs, request/response/event variants, capability flags, serde defaults, and protocol round-trip/legacy fixture tests.

Acceptance evidence: old capability fixtures deserialize; new project messages round-trip; IDs and list sizes are bounded at the protocol boundary.

### Work package B — Daemon catalog dispatch

Intent: expose existing core-owned catalog operations without duplicating storage logic.

Required changes: add conversion helpers and `CoreDaemon::handle_request` branches for list/get/register/archive/restore/health; map typed failures to stable error codes; emit lifecycle/health events where state changes.

Acceptance evidence: direct daemon tests cover two projects, archive/restore preservation, health scope, malformed IDs, and bounded list behavior.

### Work package C — Server scope migration

Intent: make HTTP/WS adapters project-aware and remove process-global project authority.

Required changes: refactor `ServerState` construction and project/session/workspace/file/WS adapters; route explicit IDs through the daemon/core services; retain only documented locator compatibility with unique resolution and actionable failures.

Acceptance evidence: adapter tests demonstrate two projects can be listed/operated by one server, cross-project session access is rejected, and legacy locator calls neither infer nor switch project identity.

### Work package D — Restart, scale, and operational closure

Intent: prove the migration at the boundaries that motivated the milestone.

Required changes: add restart/hydration and bounded multi-project integration coverage; update architecture/operator docs; run focused and broad verification; record all deviations and residual findings.

Acceptance evidence: restart exposes durable catalog metadata without activation; a bounded list remains bounded; event/request scope is explicit; all required guards pass or are recorded with justified pre-existing findings.

## 8. Failure, cancellation, restart, and contention semantics

- Invalid or archived project/workspace IDs return stable typed errors and never fall back to a different scope.
- Duplicate registration is idempotent according to existing catalog/workspace binding semantics.
- Archive/restore operations are durable and safe to retry; concurrent updates preserve catalog transaction/retry behavior.
- Listing and health remain read-only and do not acquire activation leases. Activation continues to use the existing bounded lease/refresh seam.
- Restart hydrates durable catalog/workspace/health metadata without eagerly recreating transient service leases.
- WebSocket/event consumers receive only events allowed by their negotiated/requested scope; lag or unsupported capabilities result in bounded resync/actionable errors.

## 9. Compatibility and migration

- New project-scoped operations are additive and capability-gated.
- Existing workspace/session wire fields remain available during migration; directory fields remain locators and are never used as durable IDs.
- Legacy server routes either resolve a unique existing context or return `project_context_required`/`ambiguous_project_context`; they do not use a process-global project as authority.
- No destructive schema migration is required; the existing catalog and binding tables remain the durable source.

## 10. Required tests

### Focused unit tests

- protocol DTO/request/response/event serialization and legacy capability defaults;
- daemon catalog conversions, lifecycle operations, errors, limits, and event payloads.

### Integration tests

- one daemon/server listing and operating on multiple projects;
- project/workspace/session scope isolation and archive/restore behavior;
- REST/JSON-RPC/WebSocket project operations and capability negotiation.

### Restart and recovery tests

- catalog and health hydration after daemon restart without activation/service leases.

### Contention and cancellation tests

- concurrent same-project operations, independent project isolation, bounded list limits, and event replay/filter scope.

### Security and negative tests

- malformed/oversized IDs, ambiguous locator, path traversal, remote locator non-execution, and cross-project access denial.

### Migration and compatibility tests

- older capability fixtures, legacy workspace/session requests, and actionable unsupported/explicit-scope errors.

## 11. Required verification commands

```bash
rtk cargo fmt -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test -p codegg-protocol
rtk cargo test -p codegg-core project_catalog
rtk cargo test -p codegg --lib core::daemon
rtk cargo test --features server --lib server
rtk cargo test --workspace --all-features -- --test-threads=14
rtk scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_git_forbidden_patterns.py
rtk git diff --check
```

## 12. Documentation updates

- `architecture/project_catalog.md`
- `architecture/protocol.md`
- `architecture/server.md`
- `plans/subsystems/project-catalog-roadmap.md`
- `plans/registry.md`
- `plans/closure/project-catalog/004-status.md`

## 13. Acceptance criteria

- A single daemon/server can list and operate on several durable projects without a process-global project identity.
- Project operations and health cross the native protocol with bounded DTOs, explicit scope, capability negotiation, and lifecycle/health events.
- REST/WS adapters use explicit project/workspace context; compatibility locator calls are unique-resolution-only and actionable on failure.
- Archive/restore, restart hydration, scope isolation, event filtering, and bounded list behavior are evidenced by tests.
- Required architecture, operator, planning, and closure records are complete, and no high/critical finding remains.

## 14. Stop conditions

Stop and report if implementation requires a new ownership boundary for discovery, authorization, remote execution, the TUI, or asset refresh; if a safe migration requires a schema rewrite; or if current code contradicts the typed project/workspace invariants. Do not silently widen this milestone.

## 15. Closure evidence required

The closure record must include exact implementation commits, a requirement-to-evidence matrix, commands and outcomes, migration/compatibility review, invariant and failure/restart/contention/security review, updated architecture/operations docs, known limitations, unresolved findings by severity, roadmap disposition, registry updates, and explicit status decisions for Multi-Project TUI 001 and Session Projections 001.

## 16. Handoff notes

This handoff was authored because the repository baseline had closed Milestones 001–003 but no registered M004 implementation plan. Keep changes scoped to the project-catalog protocol/server boundary, preserve unrelated user changes, and use capped workspace verification because the suite is large.
