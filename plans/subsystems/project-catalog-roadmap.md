# Project Catalog and Lazy Discovery Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#10-project-catalog-and-discovery`
- `plans/001-terminology-and-domain-model.md` — project, repository, workspace, locator
- `plans/002-long-term-roadmap.md#phase-3--project-catalog-and-lazy-discovery`

Related ADRs:

- None required initially. Create an ADR only if repository identity reconciliation or project-to-repository cardinality must diverge from the canonical model.

## 1. Purpose and ownership boundary

This subsystem owns the daemon's durable catalog of logical projects, repository metadata, configured discovery roots, explicit project registrations, local/remote locator placeholders, project health summaries, lazy activation, archive/restore, and restart hydration.

It consumes the stable identity foundation, workspace registry, project asset service, Git repository facts, workspace-service registry, protocol transport, and server routes. It must not own the TUI tab model, remote SSH execution, worktree lifecycle, authorization, or full repository hosting.

## 2. Work classification

### Invariants

- Catalog identity is stable and independent of path text.
- Discovery is bounded and never eagerly activates expensive project services.
- Archive is logical and never deletes workspaces, repositories, or session history.
- Project operations are request-scoped; no network server component assumes one process-global `project_dir`.
- Repository/path reconciliation is deterministic and conservative.
- Symlinks and permissions cannot escape configured discovery boundaries.

### Capabilities

- The daemon lists several projects before any session is opened.
- Users can register one-off local projects and configured discovery roots.
- Projects can be refreshed, archived, restored, and inspected for health.
- Catalog entries can represent future SSH or linked-node locations without pretending they are locally executable.

### Infrastructure

- Project/repository catalog records and stores.
- Discovery-root configuration and bounded scanner.
- Repository identity reconciliation.
- Lazy workspace-service activation and eviction seams.
- Project protocol operations and server route migration.

### Polish

- Actionable health and duplicate diagnostics.
- Efficient incremental refresh.
- Operator documentation for discovery roots and one-off projects.

## 3. Non-goals

- Executing commands over SSH.
- Cloning or mirroring every discovered repository.
- Starting LSP, Git watchers, indexers, build caches, or providers during catalog scanning.
- Multi-project TUI tabs.
- Team authorization and project membership.
- Deep monorepo subproject inference beyond configured rules.

## 4. Current state

The workspace registry already persists canonical roots and provides lazy workspace service bundles. Sessions are bound to workspaces, and daemon execution can resolve explicit workspace context.

The current server project route derives project IDs from paths and groups sessions by legacy `project_id`. `ServerState` retains one `project_dir`, and project creation is constrained under that root. There is no durable catalog independent of sessions, no discovery-root configuration, no repository record, and no archive/restore lifecycle for logical projects.

Git root discovery and repository status primitives already exist through `egggit` and worktree helpers. Protocol infrastructure already supports workspace list/snapshot and capability negotiation, which can be extended with project operations.

## 5. Target architecture

Persist project and repository catalog records separately from workspace activation. A project record includes stable ID, display name, lifecycle, primary repository relation, tags/metadata, last discovery/health status, and locator summaries. A repository record includes stable ID, VCS kind, canonical remote identities where available, default branch, local lineage fingerprints, and reconciliation metadata.

A discovery-root record includes locator, bounded depth, mode (`git` or `directories`), ignore rules, enabled state, and scan policy. Scans produce candidate facts first, then reconcile candidates against existing projects/repositories transactionally. Path movement updates workspace locators rather than replacing project identity.

Project activation asks the workspace-service registry for only the selected concrete workspace. Catalog listing never initializes expensive services. Remote locator variants are typed placeholders with health states such as unavailable/not-supported until later execution-target phases.

## 6. Dependency graph

```text
Milestone 1: durable project/repository catalog contracts
        |
        v
Milestone 2: bounded discovery and reconciliation
        |
        +--> Milestone 3: lazy activation and project health
        |           |
        |           v
        `--> Milestone 4: protocol/server migration and closure
```

- Milestone 1 has a hard dependency on Domain Identity Milestones 1–2.
- Milestone 2 has a hard dependency on Milestone 1 and a soft dependency on runtime-asset source definitions.
- Milestone 3 has hard dependencies on Milestones 1–2 and an interface dependency on the workspace-service registry.
- Milestone 4 has hard dependencies on Milestones 1–3 and SHOULD consume completed runtime-asset activation refresh behavior.

## 7. Milestones

### Milestone 1 — Durable project and repository catalog

Class: infrastructure

Objective: establish project/repository records, stores, lifecycle, locator types, and protocol-neutral services without filesystem scanning.

Dependencies: hard on domain identity storage.

Deliverable boundary: additive schema, stores, project/repository services, explicit local project registration, archive/restore, locator enum including local/SSH/linked-node placeholders, health model, and migration from existing workspace/session relations.

User or operator value: the daemon has a durable project catalog independent of active sessions.

Exit conditions:

- project list/get/register/archive/restore work through service APIs;
- one project can reference several workspaces;
- archive preserves sessions/workspaces;
- remote locators serialize but cannot execute locally;
- restart hydration is deterministic.

Deferred work: root scanning and TUI.

### Milestone 2 — Bounded discovery and reconciliation

Class: capability

Objective: discover candidate projects under configured roots without expensive activation or identity churn.

Dependencies: hard on Milestone 1.

Deliverable boundary: config schema, scanner, depth/ignore/permission/symlink boundaries, Git/directory modes, repository-fact extraction, candidate reconciliation, incremental refresh, and diagnostics.

Exit conditions:

- large roots are scanned within configured bounds;
- duplicate path aliases and known repository identities converge;
- path rename/move updates locators without creating a new logical project when evidence is sufficient;
- ambiguous candidates remain distinct or require explicit association;
- scanning starts no LSP/indexer/build service.

Deferred work: remote scanning.

### Milestone 3 — Lazy activation and health

Status: closed; see `plans/closure/project-catalog/003-status.md`.

Class: infrastructure

Objective: activate workspace service bundles only for selected projects/workspaces and expose bounded health.

Dependencies: hard on Milestones 1–2; interface dependency on runtime-asset refresh.

Deliverable boundary: activation leases, idle eviction integration, project/workspace selection, asset refresh on activation, health aggregation, stale/unavailable states, and contention behavior.

Exit conditions:

- listing many projects does not retain service leases;
- activating one project starts only its required services;
- inactive projects can evict cleanly;
- project health distinguishes catalog, workspace, asset, and service failures;
- concurrent activation of one workspace coalesces.

Deferred work: TUI tab lifetime rules.

### Milestone 4 — Protocol, server migration, and closure

Status: ready for handoff; implementation plan not yet registered.

Class: capability

Objective: expose complete catalog operations through the native protocol and remove single-project assumptions from network server state.

Dependencies: hard on Milestones 1–3. Milestones 1–3 are closed; the M004
implementation handoff is ready to be authored and registered.

Deliverable boundary: project DTOs/requests/responses/events, capability flags, REST/WS adapter changes, removal of authoritative `ServerState.project_dir`, compatibility behavior, restart and scale tests, and architecture docs.

Exit conditions:

- several projects can be listed and operated through one daemon/server;
- server requests carry explicit project/workspace scope;
- compatibility endpoints either project correctly or fail actionably;
- all Phase 3 exit criteria are evidenced.

## 8. Cross-cutting requirements

### Storage and migration

Catalog schema is additive and idempotent. Existing workspace/session-derived projects migrate conservatively. Discovery refresh must not delete records merely because a root is temporarily unavailable.

### Protocol and compatibility

Use bounded project summaries and paginated/limited lists where appropriate. Local path fields remain locators, not IDs. Add capabilities before changing old routes.

### Security and authorization

Canonicalize and bound discovery paths. Treat remote locators as data only. Future authorization must be able to hide project existence; avoid globally broadcasting unfiltered catalog events.

### Concurrency, cancellation, and recovery

Scans are cancellable and coalesced per root. Reconciliation is transactional. Daemon restart resumes from durable catalog state rather than requiring immediate full rescans.

### Observability and audit

Emit scan counts, durations, ignored candidates, reconciliation reasons, health transitions, and activation/eviction outcomes without exposing secrets.

### Performance and resource use

Scanning must cap depth, entries, stat concurrency, Git probes, and elapsed time. Catalog list paths must not synchronously perform repository operations.

### Documentation and operations

Document root configuration, reconciliation behavior, archive semantics, remote placeholders, and health diagnostics.

## 9. Verification strategy

Use temporary directory forests, Git repositories with aliases/remotes/worktrees, permission and symlink fixtures, large-root bounds, concurrent scan/activation tests, restart hydration, unavailable-root behavior, and server multi-project integration tests.

## 10. Risks and decision points

- Remote URLs and local Git lineage may not uniquely identify forks. Reconciliation must favor false negatives over merging unrelated projects.
- Monorepo discovery can explode candidate counts. Keep bounded explicit modes and defer semantic subproject inference.
- Workspace-service activation ownership may reveal hidden process-global assumptions. Stop and create a corrective dependency plan rather than embedding catalog state into services.
- If a project needs several first-class repositories immediately, record an ADR and revise migration contracts before implementation.

## 11. Completion definition

This roadmap closes when the daemon maintains a durable, bounded, path-independent project catalog; discovery and reconciliation are conservative; activation is lazy; archive is non-destructive; and all server/protocol operations are explicitly project-scoped.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/project-catalog/001-durable-catalog-foundation.md` | `plans/closure/project-catalog/001-status.md` | — |
| 2 | closed | `plans/implementation/project-catalog/002-bounded-discovery-reconciliation.md` | `plans/closure/project-catalog/002-status.md` | — |
| 3 | ready | `plans/implementation/project-catalog/003-lazy-activation-and-health.md` | — | —; Runtime Assets Milestone 3 activation-refresh interface closed |
| 4 | not started | — | — | Milestones 1–3 closure |
