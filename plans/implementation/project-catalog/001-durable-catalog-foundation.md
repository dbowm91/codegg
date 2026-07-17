# Project Catalog Milestone 001 — Durable Catalog Foundation

Status: ready for handoff

Repository baseline: `fbae374a2cd6172505204b1bc1bee1ef247afd5f` (production-code baseline; subsequent planning-only commits do not alter implementation state)

Source roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-1--durable-project-and-repository-catalog`

Long-term requirements:

- `plans/000-long-term-specification.md#10-project-catalog-and-discovery`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-3--project-catalog-and-lazy-discovery`

Applicable ADRs:

- None. Stop if implementation requires changing the canonical logical-project/repository/workspace model or adopting a different project cardinality.

Primary class: infrastructure

## 1. Objective

Add durable logical project and repository catalog records, lifecycle, local/remote locator types, explicit registration, archive/restore, health placeholders, and restart hydration, without implementing filesystem root scanning or multi-project TUI behavior yet.

The milestone succeeds when the daemon can list and operate durable project records independently of active sessions and paths, while existing workspaces/sessions can be associated through the completed domain-identity migration interfaces.

## 2. Why this milestone is ready

Hard dependencies:

- Domain Identity Milestone 001 is closed in `f203ed9`.
- Domain Identity Milestone 002 is implemented at `84d92f0` and provides
  durable project/repository/workspace/session relations through
  `codegg_core::project_storage::ProjectStorage` and schema v25.
- The catalog may consume those typed storage and inspection interfaces without
  reusing path-valued compatibility fields.

The agent must not implement a temporary path-keyed project table or reuse legacy `project_id` path strings as the new authoritative identity.

## 3. Current implementation evidence

- `crates/codegg-core/src/workspace.rs` persists workspace records with stable IDs, canonical roots, display names, archive state, and registry/store traits.
- Sessions carry workspace binding and legacy `project_id`/`directory` compatibility projections.
- `src/server/routes/project.rs` derives project IDs and names from paths, groups sessions by legacy project strings, and uses a process-global `ServerState.project_dir`.
- Project creation currently creates directories under the configured server root rather than creating durable logical catalog records.
- Workspace service bundles already provide an activation/eviction seam, but this milestone only prepares catalog records and does not alter activation behavior.
- Git root and repository fact primitives exist through worktree helpers and `egggit`, but discovery/reconciliation is deferred.

## 4. Invariants that must not regress

- Project IDs are stable and path-independent.
- Archive/restore is logical and never deletes workspaces, repositories, sessions, or files.
- A project can reference several workspaces and at least one primary repository relation.
- Remote locators are inert data in this phase and cannot trigger local path access or command execution.
- Existing workspace/session behavior remains available during migration.
- Listing catalog records performs no LSP, Git scan, indexer, provider probe, or build initialization.
- No network server route becomes less scoped or more permissive.

## 5. Scope

### In scope

- Project and repository domain records using completed typed IDs.
- Lifecycle states and timestamps.
- Project-to-repository and project-to-workspace relations using completed domain contracts.
- Locator types for:
  - local workspace/path reference;
  - SSH placeholder;
  - linked-node placeholder.
- Project health placeholder/state model that can represent unresolved/unavailable/unsupported locators without probing them.
- Additive SQLite migrations, stores, service layer, and restart hydration.
- Explicit local project registration by existing workspace/repository association.
- Project list/get/register/archive/restore service methods.
- Conservative migration/import from existing workspace/session relations.
- Internal/redacted protocol-ready summary DTOs if needed for tests.
- Documentation and tests.

### Explicitly out of scope

- Discovery roots, directory traversal, ignore rules, or repository reconciliation scanning.
- Remote SSH connections or node enrollment.
- Lazy service activation changes.
- TUI project picker or tabs.
- Team authorization/privacy enforcement.
- Deleting/creating project directories as a side effect of catalog operations.
- Full multi-repository project UX.

## 6. Required production changes

### Core/domain

Define project, repository, locator, lifecycle, and health contracts in `codegg-core` or another daemon-independent core boundary. Suggested record content:

Project:

- stable `ProjectId`;
- display name and optional description/tags;
- primary `RepositoryId` or explicit absence;
- lifecycle/archived timestamp;
- creation/update timestamps;
- last selected/opened metadata;
- bounded health summary or status reference.

Repository:

- stable `RepositoryId`;
- VCS kind;
- normalized remote identities where already known;
- optional default branch and lineage facts;
- lifecycle and timestamps.

Locator:

- typed variant and opaque fields;
- no implicit conversion from remote locator to local path;
- validation that distinguishes syntax from reachability.

### Storage and migrations

Add additive tables/indexes for project, repository, project-repository relation, and project-workspace relation as required by the domain milestone. Enforce uniqueness at the correct semantic level without assuming paths are unique project identity.

Migration/import should:

- use existing authoritative project/workspace relations when available;
- associate current valid workspace records conservatively;
- preserve unresolved rows for actionable rebinding;
- be idempotent across restart;
- avoid deleting or rewriting legacy fields in this milestone.

### Protocol and DTOs

Do not expose incomplete network APIs unless necessary for integration testing. If added, use bounded project summaries with typed/string ID serialization, locator summaries, lifecycle, workspace/session counts from indexed queries, and health placeholders. Add capability flags rather than changing old route semantics silently.

### Runtime and concurrency

Implement a daemon-owned `ProjectCatalogService` or equivalent over stores. It should hydrate bounded indexes, serialize conflicting writes through transactions/unique constraints, and expose async APIs. No discovery watcher or activation lease is required.

### Frontend or operator surface

No TUI picker is required. A diagnostic CLI/core request or tests may expose catalog list/get for validation. Do not create a frontend-only catalog.

### Security and authorization

- Validate locator syntax and length.
- Never resolve SSH/linked-node locators as local filesystem paths.
- Do not expose secrets in locator metadata.
- Preserve future ability to filter project existence by principal.
- Avoid path traversal through explicit local registration; bind only canonical workspaces already allowed by workspace policy.

### Documentation and static guards

Update project/workspace/session/storage architecture and document archive semantics, locator inertness, and the distinction between explicit registration and future discovery.

## 7. Ordered work packages

### Work package A — Catalog domain contracts

Intent: make durable project/repository/catalog semantics explicit before schema work.

Required changes:

- define records, lifecycle, health, locator variants, and service/store interfaces;
- define validation and serialization;
- define relation ownership and count/query contracts;
- identify compatibility mapping from current workspaces/sessions.

Acceptance evidence:

- unit tests for all variants and transitions;
- remote locators cannot be coerced into local paths;
- archive lifecycle is non-destructive by contract.

### Work package B — Additive schema and stores

Intent: persist catalog state independently of sessions.

Required changes:

- schema migration and indexes;
- project/repository/relation stores;
- transaction and uniqueness behavior;
- idempotent migration fixtures;
- restart hydration.

Acceptance evidence:

- CRUD/lifecycle tests;
- two workspaces associated with one project;
- archived project remains queryable with opt-in and preserves related rows.

### Work package C — Conservative legacy association

Intent: import current workspaces/sessions without path-derived authority.

Required changes:

- use completed domain migration helpers;
- associate unambiguous workspace/repository relations;
- leave ambiguous rows unbound with diagnostics;
- record compatibility projections but do not rewrite them as new IDs;
- add operator-visible diagnostic data.

Acceptance evidence:

- representative legacy fixtures;
- path rename does not change imported project identity after association;
- invalid/missing paths do not cause data loss.

### Work package D — Catalog service and explicit registration

Intent: provide one daemon-owned catalog API for later protocol/discovery/TUI work.

Required changes:

- service construction/hydration;
- list/get/register/archive/restore;
- explicit registration from an existing allowed workspace/repository;
- bounded counts/health placeholders;
- concurrency handling and typed errors.

Acceptance evidence:

- service restart tests;
- concurrent duplicate registration converges;
- listing triggers no expensive project services or filesystem scan.

## 8. Failure, cancellation, restart, and contention semantics

- Migration and registration are transactional and idempotent.
- Concurrent registration of the same existing workspace/repository must converge or return a typed conflict with the existing project ID.
- Archive/restore races resolve by record revision/transaction order and never delete relations.
- Daemon restart hydrates catalog metadata without probing every locator.
- Unavailable local paths or inert remote locators remain catalog records with health placeholders; they are not automatically removed.
- Cancellation before commit leaves no partial project/repository relation.

## 9. Compatibility and migration

- Preserve legacy session/workspace fields and APIs.
- Existing single-project startup may synthesize or select the associated project through the service, but must not create duplicates on every run.
- Do not change old server project routes until the later protocol/server milestone.
- Explicit registration must return stable IDs suitable for future DTOs.
- Document criteria for removing legacy path-grouped project behavior.

## 10. Required tests

### Focused unit tests

- project/repository/locator validation and serde;
- lifecycle transitions;
- health placeholder states;
- relation cardinality and count behavior.

### Integration tests

- SQLite migration/store/service CRUD;
- explicit registration from existing workspace;
- two workspaces to one project;
- one-off project with no remote;
- remote locator persistence without execution.

### Restart and recovery tests

- migration idempotence;
- restart hydration;
- interrupted migration/registration transaction;
- unavailable workspace after restart.

### Contention and cancellation tests

- concurrent duplicate registration;
- archive/restore race;
- concurrent relation updates;
- cancellation before transaction commit.

### Security and negative tests

- local path outside allowed workspace policy rejected;
- SSH/linked-node locator never reaches local filesystem APIs;
- oversized/invalid locator fields;
- no catalog listing side effects or service activation.

### Migration and compatibility tests

- existing workspace/session fixtures;
- unresolved legacy path diagnostics;
- compatibility APIs remain green;
- path rename after stable association.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-core project
cargo test -p codegg-core workspace
cargo test --test workspace_isolation
cargo test --test single_daemon
cargo test -p codegg-protocol
python3 scripts/check_daemon_cwd_usage.py
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Add focused catalog migration/service integration tests and run them explicitly.

## 12. Documentation updates

- new project catalog architecture document;
- `architecture/workspace.md`;
- `architecture/session.md`;
- storage/migration index;
- protocol notes for any internal/additive DTO seam;
- operator guidance for archive/restore and inert remote locators.

## 13. Acceptance criteria

- The daemon owns durable project and repository catalog records under stable IDs.
- Catalog state exists independently of active sessions and filesystem paths.
- Explicit local registration, list/get, archive, and restore work through one service.
- Existing workspaces/sessions migrate conservatively or remain actionably unbound.
- Restart hydration performs no eager scan or expensive service startup.
- Remote locator placeholders persist safely without execution.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- domain identity/storage dependencies are not closed;
- repository/project cardinality contradicts canonical documents;
- migration requires guessing ambiguous repository identity;
- the only implementation path uses path strings as project primary keys;
- catalog listing requires initializing workspace services;
- scope expands into discovery scanning, server route replacement, TUI tabs, or remote execution.

## 15. Closure evidence required

- implementation commit(s);
- schema and migration matrix;
- relation/cardinality evidence;
- explicit registration and lifecycle tests;
- restart/no-eager-activation evidence;
- security evidence for locator inertness and allowed paths;
- exact verification commands/results;
- list of deferred discovery/protocol/TUI work;
- closure recommendation.

## 16. Handoff notes

- This plan remains blocked until domain identity relations/storage are authoritative.
- Preserve current workspace registry ownership rather than duplicating workspace records in the catalog.
- Prefer conservative unresolved state over incorrect repository merging.
- Inspect current `main` before implementation and record the actual code baseline in closure.
