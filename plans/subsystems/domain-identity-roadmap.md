# Domain Identity and Compatibility Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md` — domain model, stable identity, ownership invariants
- `plans/001-terminology-and-domain-model.md` — canonical terminology and compatibility mappings
- `plans/002-long-term-roadmap.md#phase-0--canonical-domain-and-compatibility-foundation`

Related ADRs:

- None required initially. The canonical documents already decide that project identity is durable and path-independent. Create an ADR only if implementation reveals a materially different identity or migration model.

## 1. Purpose and ownership boundary

This subsystem establishes the durable identity primitives and relations used by every later CodeGG subsystem. It owns typed identifiers, project/repository/workspace/worktree relations, compatibility projections, storage migration, protocol representation, and guards against reintroducing path-derived project identity.

It consumes the existing workspace registry, session persistence, protocol envelopes, and singleton-daemon execution context. It must not implement project discovery, project-tab UX, team authorization, remote nodes, worktree lifecycle, or provider connections; those subsystems consume the identities defined here.

## 2. Work classification

### Invariants

- Paths are locators, never durable project identity.
- A session is bound to one logical project and one concrete workspace at a time.
- A workspace can move or be renamed without changing the logical project identity.
- Several workspaces may represent one repository/project.
- New daemon-owned operations carry explicit typed context rather than inferring identity from process cwd.
- Compatibility fields remain readable during migration but are not authoritative for new writes.

### Capabilities

- Existing sessions can be migrated or produce actionable rebinding diagnostics.
- Protocol clients can distinguish project, repository, workspace, and worktree.
- Operators can inspect identity relationships without reading raw database rows.

### Infrastructure

- Typed IDs and validation.
- Project and repository persistence seams.
- Session/workspace relation migration.
- Compatibility DTOs and protocol capabilities.
- Static guards and schema migration tests.

### Polish

- Clear diagnostics for unresolved legacy rows.
- Architecture documentation and developer guidance.
- Compact debug/status rendering for identity relationships.

## 3. Non-goals

- Project root scanning or repository discovery.
- Remote project locators or execution nodes.
- Team principals, memberships, or authorization.
- Automated worktree creation and cleanup.
- Removing every compatibility field in this roadmap.
- Renaming all historical database columns merely for cosmetic consistency.

## 4. Current state

The repository already has a strong `WorkspaceId` and daemon-owned `WorkspaceRegistry` in `crates/codegg-core/src/workspace.rs`. Workspace IDs are generated independently of path text, sessions carry `workspace_id`, and `ExecutionContext` propagates canonical workspace roots into daemon-owned execution.

The current model still treats `project_id` and `directory` as compatibility projections in several session DTOs and storage paths. Some server project routes derive project IDs directly from filesystem paths, and `ServerState` retains a process-global `project_dir`. Existing architecture documentation explicitly describes `Session -> WorkspaceId -> canonical root`, while the long-term model requires `Session -> ProjectId -> WorkspaceId -> NodeId -> locator`.

The protocol already transports workspace snapshots and session snapshots, which provides a migration seam. The core session schema and migration modules already support additive SQLite migrations. Static guards already prohibit new process-global cwd use in protected daemon execution modules, but there is no equivalent guard against path-derived project identity.

## 5. Target architecture

Introduce typed identifiers in a low-level core module with consistent parsing, serialization, database conversion, and display behavior:

- `ProjectId`
- `RepositoryId`
- `WorkspaceId` (retain existing type)
- `WorktreeId`
- `NodeId`
- `PrincipalId`
- `AgentRunId`
- `AgentTaskId`
- `ProviderConnectionId`
- `ChannelId`
- `AuditEventId`

Phase 0 only needs full persistence and relation behavior for project, repository, workspace, and session. The remaining IDs should be introduced as stable primitives or reserved contract types without prematurely creating their subsystem storage.

Persist logical projects and repositories separately from workspace locators. A project record owns display metadata and lifecycle. A repository record owns durable VCS lineage metadata. A workspace record references a project and optionally a repository. Sessions reference both project and workspace explicitly.

Legacy `project_id` path values and `directory` remain compatibility projections. Migration resolves canonical workspace records first, then associates or creates a logical project according to deterministic rules. Ambiguous rows are retained but marked unbound rather than guessed across unrelated repositories.

## 6. Dependency graph

```text
Milestone 1: typed identity primitives and relation contracts
        |
        v
Milestone 2: project/repository storage and migration
        |
        v
Milestone 3: daemon/protocol adoption and compatibility guards
        |
        v
Milestone 4: closure, migration evidence, and legacy-removal criteria
```

- Milestone 1 has no hard dependency beyond the current daemon/workspace baseline.
- Milestone 2 has a hard dependency on Milestone 1.
- Milestone 3 has a hard dependency on Milestone 2.
- Milestone 4 has a hard dependency on Milestones 1–3 and an operational dependency on migration evidence from representative existing databases.

## 7. Milestones

### Milestone 1 — Typed identity primitives and relation contracts

Class: invariant

Objective: define typed IDs and canonical project/repository/workspace/session relations without changing user-visible behavior.

Dependencies: none.

Deliverable boundary: core identity module, conversion/validation tests, relation structs/interfaces, compatibility annotations, and architecture updates.

User or operator value: later features can rely on unambiguous durable identities rather than paths.

Exit conditions:

- typed IDs round-trip through serde and SQLite-compatible representations;
- invalid/empty IDs fail consistently;
- project/repository/workspace/session relations are represented in core contracts;
- existing `WorkspaceId` remains source compatible or receives a controlled migration;
- no storage migration is required yet beyond test fixtures.

Deferred work: data migration and production protocol adoption.

### Milestone 2 — Project and repository storage migration

Class: infrastructure

Objective: persist projects/repositories and migrate existing workspaces/sessions into explicit relations.

Dependencies: hard on Milestone 1.

Deliverable boundary: additive schema migration, stores, deterministic migration/rebinding logic, rollback-safe startup behavior, and migration diagnostics.

User or operator value: projects survive path changes and can own several workspaces.

Exit conditions:

- existing valid sessions resolve to stable project IDs;
- two workspace paths can map to one project/repository;
- path rename does not create a new project when repository lineage is unchanged;
- ambiguous legacy rows remain accessible with actionable rebinding state;
- migration is restart-safe and idempotent.

Deferred work: project discovery and UI.

### Milestone 3 — Daemon and protocol adoption

Class: infrastructure

Objective: make stable project identity authoritative in new daemon requests, responses, session creation, and server routes while retaining compatibility fields.

Dependencies: hard on Milestone 2.

Deliverable boundary: DTO additions, capability negotiation, session binding, project-aware request context, server route cleanup, and static identity guards.

User or operator value: all frontends can address the same logical project independent of path.

Exit conditions:

- new sessions persist typed project/workspace relations;
- protocol snapshots carry stable project IDs;
- old clients continue through compatibility projections or receive explicit incompatibility diagnostics;
- server routes no longer use a path as the project ID;
- a static guard rejects new authoritative path-derived project identity in daemon-owned code.

Deferred work: project catalog scanning and multi-project TUI.

### Milestone 4 — Closure and legacy-removal criteria

Class: polish

Objective: close migration correctness, document residual compatibility paths, and define when legacy fields can be removed.

Dependencies: hard on Milestones 1–3.

Deliverable boundary: closure matrix, representative database migration evidence, restart/contention tests, architecture docs, and tracked removal criteria.

Exit conditions:

- all Phase 0 exit criteria are evidenced;
- unresolved legacy rows are classified;
- no silent path-identity fallback remains in production writes;
- compatibility fields have named owners and removal prerequisites.

## 8. Cross-cutting requirements

### Storage and migration

Use additive, idempotent SQLite migrations. Migration must be restart-safe and must not delete unresolved legacy information. Stores should be trait-backed where consistent with existing `codegg-core` patterns.

### Protocol and compatibility

Add fields and capabilities before removing legacy fields. Unknown variants and fields should preserve current forward-compatibility behavior. New clients use typed IDs; compatibility clients receive path/directory projections.

### Security and authorization

Identity types must not imply authorization. Project IDs are opaque identifiers, not secrets. Future principal and node IDs must not be trusted merely because they parse.

### Concurrency, cancellation, and recovery

Concurrent registration of the same repository/workspace must converge. Migration and registration need transaction or uniqueness protection. Failed startup migration must leave the prior schema/data usable or fail clearly before serving requests.

### Observability and audit

Emit structured diagnostics for migration, rebinding, duplicate reconciliation, and compatibility fallback. Reserve correlation seams for later audit integration.

### Performance and resource use

Hydration must remain bounded and indexed. Repository identity probing must not become an implicit full project scan.

### Documentation and operations

Update `architecture/workspace.md`, session/storage/protocol docs, and static-guard documentation. Provide operator-visible diagnostics for unbound sessions.

## 9. Verification strategy

Use typed-ID unit tests, schema migration fixtures, property-style path rename tests, concurrent registration tests, protocol compatibility tests, daemon restart tests, and static source guards. Include at least one fixture with two workspaces for one repository and one unresolved legacy path.

## 10. Risks and decision points

- Repository identity matching may be ambiguous for repositories without remotes or with rewritten history. Default to explicit association rather than overconfident merging.
- Existing `project_id` semantics may be embedded in tests and APIs. Preserve compatibility projections while migrating authority.
- Introducing all future IDs at once can create unused abstractions. Keep only validation/serialization in Phase 0 for IDs whose stores land later.
- If project-to-repository cardinality becomes contentious, record an ADR before changing the canonical one-project/primary-repository migration assumption.

## 11. Completion definition

This roadmap closes when project identity is durable and path-independent across storage, daemon execution, and protocol surfaces; existing sessions migrate or fail actionably; compatibility behavior is explicit; and no new daemon-owned operation can establish project authority from path text alone.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/domain-identity/001-typed-identity-foundation.md` | `plans/closure/domain-identity/001-status.md` | — |
| 2 | closed | `plans/implementation/domain-identity/002-project-repository-storage-migration.md` | `plans/closure/domain-identity/002-status.md` | — |
| 3 | not started | — | — | Implementation plan not yet authored |
| 4 | not started | — | — | Milestones 1–3 closure evidence |
