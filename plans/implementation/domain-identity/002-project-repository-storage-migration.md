# Domain Identity Milestone 002 — Project and Repository Storage Migration

Status: ready for handoff

Repository baseline: `9dcde707f6fe001cc6d73e7f562ccccf9f782f1a` (`main`; Domain Identity Milestone 001 and Provider Connections Milestone 001 are closed)

Production implementation baseline:

- `f203ed9` — typed domain identity primitives and relation contracts.
- `bccca00` — additive provider-connection foundation consuming the typed identity layer.

Source roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestone-2--project-and-repository-storage-migration`

Long-term requirements:

- `plans/000-long-term-specification.md` — stable project/repository/workspace/session identities and path-independent ownership.
- `plans/001-terminology-and-domain-model.md` — Project, Repository, Workspace, Session, compatibility projection, binding, and locator.
- `plans/002-long-term-roadmap.md#phase-0--canonical-domain-and-compatibility-foundation`

Applicable closure evidence:

- `plans/closure/domain-identity/001-status.md`

Applicable ADRs:

- None. The canonical documents already decide that logical project identity is path-independent and that migration must preserve ambiguous legacy data rather than guess. Stop for an ADR only if implementation requires a materially different project/repository cardinality or authority model.

Primary class: infrastructure

## 1. Objective

Add durable logical-project and repository persistence, explicit workspace/session binding storage, and deterministic migration/rebinding behavior for existing CodeGG databases.

The milestone succeeds when existing valid workspaces and sessions can resolve to stable `ProjectId` and optional `RepositoryId` values without making filesystem paths authoritative, while ambiguous legacy data remains accessible through an explicit unresolved/rebind-required state.

This milestone establishes the authoritative storage and migration interface consumed by Runtime Assets and Project Catalog. It does not yet make every daemon request, protocol DTO, server route, or TUI path use the new identities; that is Domain Identity Milestone 003.

## 2. Why this milestone is ready

The only hard dependency is closed:

- Domain Identity Milestone 001 introduced and closed the typed `ProjectId`, `RepositoryId`, `WorkspaceId`, relation contracts, lexical validation, compatibility annotations, and path-identity guard seam in `f203ed9`.

The current catalog already has transactional SQLite migrations, workspace persistence, session persistence, a user-scoped catalog migration path, and restart-safe migration conventions. The next work can therefore add storage authority without inventing new identity primitives or replacing the existing database framework.

## 3. Current implementation evidence

At the repository baseline:

- `codegg_core::identity` owns validated `ProjectId`, `RepositoryId`, `WorkspaceId`, `ProjectRepositoryBinding`, `ProjectBinding`, and `SessionBinding` contracts.
- `WorkspaceRegistry` persists path-independent `WorkspaceId` values but `WorkspaceRecord` currently contains only canonical-root and display/lifecycle metadata.
- SQLite migration version 24 is current. Migrations run under `BEGIN IMMEDIATE`, update `migration_version` only on success, and roll back on failure.
- The legacy `project` table originated in migration v1 and is coupled to historical path/worktree semantics. The existing `session.project_id` column is required and may contain a path-valued compatibility projection.
- `session.workspace_id` is additive and nullable. Existing daemon execution rejects unbound sessions before execution, but session storage still treats string `project_id` as a compatibility field.
- `migrate_legacy_project_database` imports project-local session databases into the daemon catalog, registers/reuses a workspace, and currently writes the canonical workspace root into `CreateSession.project_id` and `directory`.
- The migration marker table makes legacy database import idempotent by source path, but it does not establish logical project/repository identity.
- Server route and protocol authority remain explicitly deferred to Milestone 003.

Known gap: there is no authoritative persisted relationship among logical project, repository lineage, workspace, and session. The historical `project` row and `session.project_id` value cannot be reinterpreted as stable identity without an explicit additive migration.

## 4. Invariants that must not regress

- A filesystem path is a locator and MUST NOT be converted into a `ProjectId` or `RepositoryId`.
- The historical `project` table and `session.project_id` field MUST remain readable as compatibility data until Milestone 003 and later removal criteria close.
- Existing session IDs, message IDs, workspace IDs, history, parent relationships, and archive/delete state MUST remain unchanged.
- Migration MUST be additive, idempotent, restart-safe, and transactionally fail before the daemon serves partially migrated authority.
- Ambiguous repository lineage MUST produce explicit unresolved/rebind-required state; it MUST NOT merge projects based only on similar names, directory basenames, or guessed remotes.
- One project MAY own several workspaces. One workspace MUST have at most one active authoritative project binding.
- A session MUST have at most one authoritative project/workspace binding at a time.
- Repository identity is VCS lineage metadata, not authorization and not a secret.
- Migration and registration MUST converge under concurrent callers through uniqueness constraints and transactional conflict handling.
- Existing singleton-daemon, workspace-registry, scheduler, and execution-context ownership boundaries MUST remain intact.
- No process-global cwd dependency may be introduced.

## 5. Scope

### In scope

- Durable logical-project records with stable `ProjectId`, display metadata, lifecycle, and timestamps.
- Durable repository records with stable `RepositoryId` and bounded lineage metadata.
- An explicit project-to-repository relationship consistent with the canonical primary-repository assumption.
- Authoritative workspace-to-project and optional workspace-to-repository binding storage.
- Authoritative session-to-project/workspace binding storage.
- Explicit binding status and migration diagnostics for resolved, unresolved, ambiguous, stale-locator, and rebind-required cases.
- Additive SQLite migrations and trait-backed stores or equivalent core-owned storage interfaces.
- Deterministic reconciliation of existing workspace/session rows.
- Integration with `migrate_legacy_project_database` so new imports create/reuse stable identity bindings rather than writing a path as new authority.
- Bounded repository-lineage probing needed to reconcile one known workspace at a time.
- Operator/core inspection APIs sufficient to enumerate identity bindings and diagnostics without reading raw SQL.
- Static guard expansion sufficient to prevent new storage code from deriving `ProjectId` from paths.
- Architecture and storage documentation.

### Explicitly out of scope

- Project-root discovery scans, configured discovery roots, project archive UI, or catalog protocol routes.
- Making project identity authoritative in all daemon commands, protocol snapshots, server project routes, or TUI state.
- Remote repositories, SSH locators, execution nodes, or cross-machine reconciliation.
- Worktree lifecycle and `WorktreeId` persistence beyond an optional nullable relation seam.
- Team principals, authorization, ownership enforcement, or project privacy.
- Deleting or renaming historical `project` or `session.project_id` columns.
- Automatically merging repositories with conflicting remotes or rewritten/unrelated histories.
- Full Git-host identity, network lookups, or remote API calls during migration.
- Background scanning of every workspace at daemon startup.

## 6. Required production changes

### Core/domain

Introduce core-owned records and result types, naming them consistently with the canonical terminology. The exact module split may follow existing `codegg-core` conventions, but it must expose:

- `ProjectRecord` keyed by `ProjectId`;
- `RepositoryRecord` keyed by `RepositoryId`;
- project/repository relationship records;
- workspace binding records containing `WorkspaceId`, `ProjectId`, optional `RepositoryId`, optional future `WorktreeId`/`NodeId`, timestamps, revision, and status;
- session binding records containing session ID, `ProjectId`, `WorkspaceId`, status, source/provenance, revision, and timestamps;
- bounded migration/reconciliation diagnostics with machine-readable reason codes;
- create/get/list/update/rebind interfaces that accept typed IDs at the core boundary.

Repository lineage metadata should be sufficient for deterministic local reconciliation without pretending to be a universal repository fingerprint. Prefer bounded normalized values such as:

- canonicalized remote identities with credentials/query/fragment removed;
- local Git common-directory or object-lineage evidence where safe and stable;
- VCS kind;
- optional default branch or head metadata for diagnostics only;
- provenance and confidence/reconciliation status.

No lineage field may contain credentials or unbounded command output.

### Storage and migrations

Add a new migration after v24. Do not silently repurpose the historical `project` table unless implementation first proves a lossless, explicit compatibility model. The preferred additive shape is separate canonical tables for:

- logical projects;
- repositories;
- project/repository relationships;
- workspace/project bindings;
- session/project/workspace bindings;
- migration or rebinding diagnostics/provenance where needed.

Required database properties:

- stable typed IDs stored as validated text;
- foreign keys for authoritative relations;
- uniqueness preventing multiple active project bindings for one workspace or session;
- indexes for project, repository, workspace, session, status, and lineage lookup;
- revision or equivalent optimistic-concurrency protection for manual rebind operations;
- timestamps and lifecycle/binding state;
- additive/idempotent DDL under the current migration transaction framework.

Historical rows and fields remain untouched except for safe additive references or compatibility annotations. Never overwrite path-valued legacy `session.project_id` data with a generated ID unless the migration also preserves the original value in a documented compatibility location and the write is atomic.

### Deterministic migration and reconciliation

Implement a bounded reconciler that operates on existing persisted workspace/session records. It must separate evidence gathering from mutation and produce a candidate plan/report before applying changes.

Minimum deterministic rules:

1. Reuse an existing authoritative workspace binding when one is already valid.
2. Reuse an existing repository/project only when a unique, validated lineage match exists.
3. Allow several workspace records to bind to one project/repository when the lineage evidence is uniquely consistent.
4. Create a new project/repository for one unambiguously independent workspace.
5. Treat missing Git metadata as a valid non-repository project case or explicit unresolved repository relation; do not invent a repository ID merely from a path.
6. Mark conflicting or multiple plausible matches as ambiguous and retain legacy locators for operator rebinding.
7. Bind a session from its valid `workspace_id` first. Its legacy `project_id` and `directory` are diagnostics/projections, not authority.
8. Sessions lacking a resolvable workspace remain accessible but rebind-required; do not attach them to an unrelated current workspace.

The applied migration must be idempotent. A rerun after success must produce no duplicate records or changed IDs. A rerun after interruption must safely resume or roll back.

### Legacy database import

Refactor `migrate_legacy_project_database` so imported sessions receive canonical binding records created/reused through the new storage service. Preserve the existing source database, session IDs, messages, and provenance marker behavior.

The import path must no longer establish new project authority by assigning `workspace.canonical_root` to an authoritative identity field. It may continue populating historical `project_id` and `directory` compatibility projections until Milestone 003 defines the replacement DTO/write behavior.

### Runtime and concurrency

- Run schema migration before serving requests, using the existing fail-before-serve behavior.
- Keep repository probing bounded to explicitly migrated/registered workspaces; no recursive discovery.
- Coalesce or serialize concurrent reconciliation for the same workspace/repository lineage.
- Use transaction and uniqueness constraints as final authority rather than process-local locks alone.
- Manual rebind operations must use expected revision/generation and return typed conflicts for stale callers.
- A failed probe or unsupported repository must not corrupt an existing valid binding.

### Protocol and DTOs

Do not perform broad protocol adoption in this milestone. Add only core-facing or admin/diagnostic DTO seams needed to inspect migration outcomes if they can remain additive and frontend-neutral.

Any wire-visible addition must:

- serialize typed IDs as strings;
- preserve current fields and protocol compatibility;
- avoid making a path-valued compatibility field authoritative;
- be clearly marked as preview/diagnostic if full capability negotiation belongs to Milestone 003.

### Frontend or operator surface

Provide at least one bounded inspection/rebinding surface through the core command layer, CLI diagnostics, or existing status/doctor mechanisms. It must allow an operator or closure test to determine:

- project ID;
- repository ID, if any;
- bound workspace IDs and locators;
- bound/unresolved session counts;
- migration source/provenance;
- actionable reason for any unresolved or ambiguous binding.

A full project picker or catalog UI is not required.

### Security and authorization

- Strip credentials, query strings, fragments, and control characters from remote lineage metadata before persistence or diagnostics.
- Bound all stored lineage/display/diagnostic fields.
- Do not execute hooks or contact remotes while probing repository identity.
- Do not follow filesystem aliases outside the registered workspace boundary.
- Typed identity or successful repository matching grants no authorization.

### Documentation and static guards

Update at minimum:

- `architecture/identity.md`;
- `architecture/workspace.md`;
- `architecture/session.md`;
- `architecture/storage.md`;
- `architecture/core.md` or workspace-services documentation where ownership changes;
- migration/operator documentation.

Expand `scripts/check_identity_path_usage.py` or add an equivalent focused guard so new authoritative project storage/binding code cannot use path-to-`ProjectId` construction or write path text into canonical project ID fields. Keep allowlists narrow and documented.

## 7. Ordered work packages

### Work package A — Canonical storage model

Intent: establish the durable project/repository/binding schema and core records without migrating live rows yet.

Required changes:

- define canonical records, statuses, diagnostics, and store contracts;
- add additive schema migration and indexes;
- add typed row conversion with strict validation;
- add in-memory/test store support where existing core patterns require it;
- document the legacy-table compatibility boundary.

Acceptance evidence:

- clean database creates all canonical tables;
- v24 database migrates without data loss;
- typed records round-trip;
- malformed stored IDs fail actionably rather than being accepted unchecked;
- repeated migration is idempotent.

### Work package B — Repository lineage normalization

Intent: derive bounded, credential-free evidence for one registered workspace without making paths into identity.

Required changes:

- implement local-only VCS evidence extraction;
- normalize remotes and reject/strip secret-bearing URL material;
- distinguish unique match, no repository, insufficient evidence, and ambiguity;
- add deterministic equality/reconciliation tests across equivalent remote spellings and workspace renames.

Acceptance evidence:

- two workspaces for the same uniquely identified repository reconcile to one repository/project;
- path rename alone does not create a new project;
- conflicting remotes do not merge;
- repositories without remotes do not receive an overconfident cross-workspace merge.

### Work package C — Workspace and session reconciliation

Intent: produce and apply a deterministic migration plan for existing catalog rows.

Required changes:

- enumerate existing workspace and session rows in bounded batches;
- build candidate project/repository/binding actions with diagnostics;
- apply each coherent migration transactionally;
- preserve unresolved rows and original compatibility values;
- add revision/provenance records sufficient for restart and audit seams.

Acceptance evidence:

- valid existing sessions resolve through workspace bindings to stable project IDs;
- unresolved/ambiguous rows remain listable with reason codes;
- duplicate and concurrent migration attempts converge;
- rerun after success changes nothing.

### Work package D — Legacy database import integration

Intent: ensure future imports enter the canonical relation model.

Required changes:

- route workspace/project/repository resolution through the new service;
- preserve existing migration markers and source database immutability;
- bind imported sessions canonically while retaining legacy projections;
- handle preexisting session IDs and partial imports without duplicate authority.

Acceptance evidence:

- one legacy project database imports once and binds sessions to stable identities;
- repeated import reports already migrated;
- an interrupted or conflicting import does not leave duplicate projects or bindings;
- source databases are unchanged.

### Work package E — Inspection, rebinding, and guards

Intent: make unresolved migration state actionable and prevent regression.

Required changes:

- add bounded inspect/list APIs or commands;
- add explicit rebind operation using typed IDs and expected revision;
- add redacted structured diagnostics;
- enable or extend path-identity static guards for the new authority modules;
- update architecture and operator documentation.

Acceptance evidence:

- operator can distinguish resolved, ambiguous, stale-locator, and rebind-required cases;
- stale concurrent rebind fails with a typed conflict;
- static guard rejects a fixture that derives `ProjectId` from a path;
- no raw SQL inspection is needed for closure evidence.

## 8. Failure, cancellation, restart, and contention semantics

- Schema migration failure rolls back the current migration version and prevents request serving.
- Reconciliation must be resumable. Durable records written before a process crash must either form a valid complete binding or carry an explicit non-active/provisional state that recovery can finish or discard.
- Do not hold a database transaction open while running Git subprocesses. Gather bounded evidence first, then open the transaction and revalidate affected revisions before applying.
- Concurrent registration/reconciliation of equivalent repository evidence must converge through uniqueness constraints. One caller may win; other callers reload the winning records rather than creating duplicates.
- Cancellation before mutation leaves no records. Cancellation after a committed coherent unit leaves that unit valid and a subsequent run resumes remaining work.
- A workspace path disappearing during migration produces stale-locator/rebind-required status; it does not delete project/session history.
- A manual rebind cannot silently override a newer binding. Expected revision is mandatory.
- Repository probe timeout or failure leaves prior valid bindings unchanged and reports bounded diagnostics.

## 9. Compatibility and migration

- Keep legacy `project`, `session.project_id`, `session.directory`, and nullable `session.workspace_id` readable.
- Treat existing path-valued `session.project_id` values as compatibility provenance only.
- New canonical tables/relations are the sole authority introduced by this milestone.
- Do not bump the public protocol version unless an unavoidable wire-visible addition requires it; prefer additive diagnostic APIs.
- Existing clients and session history must continue to operate through current paths while Milestone 003 migrates daemon/protocol authority.
- Preserve the legacy database import marker format or provide an additive versioned extension.
- Document exact criteria under which historical compatibility values can later stop being written.

## 10. Required tests

### Focused unit tests

- canonical record validation and serde;
- row conversion rejects malformed IDs and oversized fields;
- remote normalization and secret stripping;
- repository reconciliation result classification;
- migration diagnostic reason codes;
- binding revision conflict behavior.

### Storage and migration tests

- clean database migration from version 0 through the new version;
- direct v24-to-new-version migration;
- idempotent rerun;
- injected failure before version recording rolls back;
- existing legacy project/session/workspace/provider-connection rows survive unchanged;
- uniqueness and foreign-key behavior;
- malformed legacy values remain preserved and unresolved rather than aborting all migration.

### Integration tests

- one project with one workspace and several sessions;
- one repository/project with two workspace paths;
- workspace rename with unchanged repository lineage;
- two unrelated repositories with the same basename;
- repository with no remote;
- conflicting/multiple remotes producing ambiguity;
- unbound session producing actionable rebind state;
- explicit rebind followed by session resolution;
- legacy project-local database import into canonical bindings.

### Restart and recovery tests

- restart after schema migration but before reconciliation;
- restart after one committed reconciliation batch;
- stale/missing workspace locator after restart;
- repeated import/reconciliation after simulated crash;
- existing IDs remain stable across hydration.

### Contention and cancellation tests

- concurrent registration of two workspaces for one repository;
- concurrent reconciliation of the same workspace;
- stale manual rebind revision;
- cancellation before apply and between bounded batches;
- no database transaction remains open during delayed Git probing.

### Security and negative tests

- remote URL containing username/password/token is redacted before storage;
- query/fragment secrets are not persisted or logged;
- path traversal/symlink escape in workspace probing is rejected;
- malformed/path-like typed IDs are rejected;
- no external network request occurs during migration;
- static guard catches path-derived project authority.

### Compatibility tests

- existing session and workspace APIs continue to compile and pass;
- historical `project_id` and `directory` values remain readable;
- provider-connections v24 data remains valid;
- protocol serialization remains unchanged unless an explicitly tested additive DTO is introduced;
- current daemon startup and legacy migration commands remain compatible.

## 11. Required verification commands

The implementation agent must inspect package/test names and adjust only when repository reality requires it. At minimum run:

```bash
rtk cargo fmt --all -- --check

rtk cargo test -p codegg-core identity
rtk cargo test -p codegg-core workspace
rtk cargo test -p codegg-core project
rtk cargo test -p codegg-core repository
rtk cargo test -p codegg-core migration
rtk cargo test --test storage_migrations
rtk cargo test --test workspace_isolation

rtk cargo test -p codegg-protocol
rtk cargo test -p codegg-core

rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_identity_path_usage.py
rtk python3 scripts/check_daemon_cwd_usage.py
rtk git diff --check
```

Run the broader workspace suite when the implementation touches shared session/storage initialization beyond the focused core boundary. Preserve the repository's intentional resource constraints and serial-test requirements.

## 12. Documentation updates

- Document canonical project/repository/binding tables and ownership.
- Document deterministic migration and ambiguity rules.
- Document how `migrate_legacy_project_database` now establishes canonical bindings.
- Document operator inspection and rebind workflow.
- Clearly label all historical path/string projections and name their future removal owner.
- Update migration version references and schema diagrams.

## 13. Acceptance criteria

- A migrated session resolves to one stable `ProjectId` and `WorkspaceId` through authoritative storage.
- Two uniquely related workspaces can share one `ProjectId` and `RepositoryId`.
- Renaming/moving a workspace does not change project identity when lineage remains uniquely matched.
- Ambiguous rows remain accessible and carry actionable rebind diagnostics.
- Migration is additive, idempotent, restart-safe, and protected against concurrent duplication.
- New legacy database imports establish canonical bindings without path-derived identity.
- No plaintext credentials or secret-bearing remote material enters canonical records or diagnostics.
- Runtime Assets can consume an explicit project/workspace binding interface without reading `PWD` or path-valued `project_id`.
- Project Catalog has a stable storage interface to consume after this milestone closes.
- Historical compatibility fields remain intact for Milestone 003.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- implementation requires path text to become a `ProjectId` or `RepositoryId`;
- a lossless additive migration cannot be achieved without deleting or overwriting legacy information;
- project-to-repository cardinality must differ materially from the canonical primary-repository assumption;
- repository equivalence requires network access or speculative history rewriting;
- a needed change would make server routes/protocol authority part of this milestone rather than Milestone 003;
- unresolved sessions can only be made executable by guessing a binding;
- migration cannot fail before serving requests;
- scope expands into project discovery, multi-project TUI, team authorization, or worktree lifecycle.

## 15. Closure evidence required

The closure record must include:

- implementation commits;
- final schema version and table/index/constraint summary;
- requirement-to-evidence matrix for every acceptance criterion;
- representative pre-migration and post-migration database evidence;
- one-project/two-workspace fixture evidence;
- workspace rename stability evidence;
- unresolved/ambiguous legacy-row evidence and rebind demonstration;
- restart, idempotency, injected-failure, and concurrent-registration results;
- legacy project-database import evidence;
- proof that historical fields were preserved;
- proof that no secret-bearing remote data was persisted;
- static guard output;
- exact commands run and outcomes;
- documentation changes;
- unresolved findings classified by severity;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 16. Handoff notes

- Milestone 001 is closed at `f203ed9`; do not redesign the typed identity lexical contract in this milestone.
- Current `main` includes Provider Connections Milestone 001. Preserve migration v24 data and provider-connection behavior.
- The historical v1 `project` table is not automatically the canonical logical-project table merely because its name matches. Treat its semantics as legacy until proven and documented.
- `migrate_legacy_project_database` currently writes a canonical path into `CreateSession.project_id`; this is a compatibility write that must not become the new authority.
- Avoid holding SQLite write locks while invoking Git.
- Do not enable broad background repository scanning.
- Preserve unrelated user changes and inspect current `main` before implementation.
