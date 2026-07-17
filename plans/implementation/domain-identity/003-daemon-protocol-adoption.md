# Domain Identity Milestone 003 — Daemon and Protocol Adoption

Status: ready for handoff

Repository baseline: `3ce0a7ea7c1a8baa41a2618eb293291435e9f9f0` (`main`; planning-only commits after this baseline do not alter production behavior)

Production implementation baseline:

- `f203ed9` — typed domain identity primitives and relation contracts.
- `84d92f0` — durable project/repository storage, workspace/session bindings, reconciliation, and migration diagnostics.
- `a2db5e4` — durable project catalog foundation over the canonical identity stores.
- `efe1995` — additive session connection/model selection, demonstrating the current typed daemon/protocol migration pattern.

Source roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestone-3--daemon-and-protocol-adoption`

Long-term requirements:

- `plans/000-long-term-specification.md` — stable project identity, explicit execution context, daemon authority, and compatibility behavior.
- `plans/001-terminology-and-domain-model.md` — Project, Repository, Workspace, Session, locator, compatibility projection, and binding.
- `plans/002-long-term-roadmap.md#phase-0--canonical-domain-and-compatibility-foundation`

Applicable closure evidence:

- `plans/closure/domain-identity/001-status.md`
- `plans/closure/domain-identity/002-status.md`
- `plans/closure/project-catalog/001-status.md`

Applicable ADRs:

- None. The canonical documents already decide that project identity is durable and path-independent. Stop for an ADR only if implementation requires changing project/repository/workspace cardinality or making a filesystem locator authoritative.

Primary class: infrastructure

## 1. Objective

Make canonical `ProjectId + WorkspaceId` context authoritative for new daemon requests, session creation and rebinding, snapshots, and compatibility-facing server routes.

The milestone succeeds when daemon-owned operations no longer manufacture or infer a project identity from `directory`, `project_dir`, `PWD`, a Git root, or another path string. New clients use explicit stable identities; old clients remain readable through bounded compatibility adapters or receive an actionable incompatibility diagnostic.

This milestone establishes the identity-bearing protocol and daemon context consumed by later Runtime Assets, Project Catalog protocol, Multi-Project TUI, and Session Projection work. It does not implement the complete multi-project catalog protocol or remove every single-project server field; Project Catalog Milestone 004 owns that broader server/catalog migration.

## 2. Why this milestone is ready

The hard dependency is closed:

- Domain Identity Milestone 002 provides durable logical-project, repository, workspace-binding, and session-binding stores; deterministic reconciliation; inspection; revisioned rebinding; and explicit unresolved states.

The repository also has a closed project catalog service that can validate project lifecycle and workspace membership without scanning or activating a workspace. Protocol changes can therefore refer to canonical identities without creating a temporary path-keyed catalog.

## 3. Current implementation evidence

At the repository baseline:

- `codegg_core::identity` owns typed `ProjectId`, `RepositoryId`, `WorkspaceId`, `ProjectBinding`, and `SessionBinding` values.
- `codegg_core::project_storage::ProjectStorage` owns canonical project/repository/workspace/session binding persistence and revisioned rebinding.
- `codegg_core::project_catalog::ProjectCatalog` owns durable project lifecycle, locators, and project/workspace lookup.
- `CoreRequest::SessionList` still accepts a string `project_id`; `CoreRequest::SessionCreate` accepts only a `directory`; `SessionCreateFromTemplate` accepts string `project_id` plus `directory`.
- `SessionSnapshot` still carries string `project_id`, optional string `workspace_id`, and `directory`; the schema does not distinguish canonical fields from compatibility projections at the type boundary.
- `ServerState` still contains `project_dir: String`.
- `src/server/routes/project.rs` currently returns `state.project_dir` as the project ID, groups sessions by legacy `session.project_id`, derives display names from path text, and creates project records without writing the canonical catalog.
- Existing legacy session fields remain readable by design. Canonical authority lives in `session_project_binding`, not in the historical path-valued projection.
- `scripts/check_identity_path_usage.py` exists but is intentionally narrow; it does not yet enforce the complete daemon/protocol boundary.

## 4. Invariants that must not regress

- Paths, directories, Git roots, and server-local roots remain locators, never canonical project identity.
- A new session must have one validated canonical `ProjectId` and one validated canonical `WorkspaceId` before it can execute.
- Project and workspace membership must be checked against durable binding stores, not inferred from textual similarity.
- Existing legacy rows must remain loadable; compatibility fields may be projected but may not override a valid canonical binding.
- Ambiguous or unresolved rows must fail actionably and remain inspectable; the daemon must not guess.
- Identity parsing does not grant authorization.
- Protocol additions must be additive until explicit compatibility-removal criteria are accepted.
- No daemon-owned path may use process-global cwd to establish project context.

## 5. Scope

### In scope

- A typed core representation for authoritative project/workspace request context.
- A daemon-owned resolver that validates project lifecycle, workspace existence, project/workspace binding, and optional session binding.
- Canonical context on session create, attach, load, fork, template creation, import, and workspace rebind paths.
- Additive protocol DTOs and request fields for stable project/workspace identity.
- Stable project/workspace identities in session and daemon snapshots.
- Capability/version negotiation or explicit compatibility detection for new identity-aware requests.
- Compatibility adapters for old directory-only and legacy-project requests.
- Server project-route cleanup sufficient to stop returning a path as a project ID.
- Static guards for new authoritative path-derived project identity.
- Focused migration, restart, contention, protocol, and server tests.

### Explicitly out of scope

- Project discovery scanning.
- Full catalog list/register/archive/restore protocol and multi-project REST/WS surface; Project Catalog Milestone 004 owns those operations.
- Removing `ServerState.project_dir` entirely if it remains needed as a compatibility/default locator; it must cease being identity authority.
- Multi-project TUI tabs and project picker.
- Runtime-asset refresh or snapshot generation.
- Team principals, ACL enforcement, remote nodes, or presence.
- Destructive removal of historical `project`, `session.project_id`, or `session.directory` fields.

## 6. Required production changes

### Core/domain

Introduce a daemon-facing context value, naming consistent with the existing terminology, with at least:

- `project_id: ProjectId`;
- `workspace_id: WorkspaceId`;
- optional `repository_id: RepositoryId` when uniquely bound;
- workspace locator/root available only as execution data;
- binding revision/status needed to reject stale or unresolved context.

Add a resolver/service that:

1. parses untrusted IDs at the boundary;
2. loads the logical project and rejects archived/missing lifecycle where the operation requires activity;
3. loads the workspace and canonical workspace binding;
4. verifies the workspace belongs to the requested project;
5. loads or establishes the canonical session binding inside the same write transaction for session creation flows;
6. returns typed `not_found`, `unbound`, `rebind_required`, `archived`, `mismatch`, and `stale_revision` outcomes;
7. never accepts a path as fallback identity.

The resolver should be reusable by daemon requests, server adapters, Runtime Assets, and later Project Catalog protocol code. It must not import TUI or Axum types into `codegg-core`.

### Storage and migrations

Prefer the existing v25 binding tables and v28 catalog tables. Add an additive migration only when a concrete missing constraint/index or compatibility-state field is required.

Required storage behavior:

- new-session creation writes the session row and canonical `session_project_binding` atomically;
- fork/template/import flows preserve or explicitly resolve canonical binding;
- rebinding uses expected revisions and never partially updates only the legacy projection;
- legacy `project_id` and `directory` writes are documented compatibility projections derived from the already-resolved context;
- cancellation before commit leaves no session without its required canonical binding;
- startup does not scan the filesystem to repair bindings.

### Protocol and DTOs

Add bounded, redacted DTOs for authoritative context, for example:

- `ProjectContextDto` or equivalent: stable project/workspace IDs, optional repository ID, binding state/revision, and display-safe locator summary;
- `SessionBindingDto`: canonical project/workspace identity plus compatibility locator fields clearly named as such;
- typed error/outcome values for unresolved or incompatible legacy requests.

Migrate protocol behavior additively:

- add an identity-aware session-create request carrying `project_id` and `workspace_id`, or extend the existing request with optional fields while preserving old decoding;
- add canonical context to session snapshots and session DTOs without removing legacy fields;
- make `SessionList` and related project-scoped requests validate a stable project ID rather than querying historical path projections;
- ensure fork/template/import results retain canonical bindings;
- advertise the capability through the existing initialization/capability mechanism or add a bounded capability field if none exists;
- old clients using directory-only create may be accepted only when the daemon can deterministically map the explicit directory locator to one existing canonical workspace/project binding; otherwise return `project_context_required` rather than creating identity from the path;
- preserve unknown-field/unknown-variant forward compatibility.

Do not bump the protocol version merely for additive fields unless the current decoder contract requires it. If a version bump is necessary, document exact old/new behavior and add negotiation tests.

### Daemon runtime and concurrency

Route all new context-sensitive handlers through the authoritative resolver.

At minimum review and migrate:

- `SessionCreate`;
- `SessionCreateFromTemplate`;
- `SessionAttach` and `SessionLoad` context hydration;
- session fork/import paths;
- `SessionList`;
- turn submission context lookup;
- agent/model selection paths where they still consume legacy project strings;
- goal/task/worktree requests that accept `project_id` or `project_dir` as authority;
- daemon/session snapshots.

Use one transaction or an equivalent atomic store operation when creating a session and its binding. Concurrent duplicate create/rebind requests must converge or return a typed revision conflict.

### Server and compatibility adapters

Update `src/server/routes/project.rs` and supporting state/adapters so:

- `ProjectInfo.id` is a stable `ProjectId`, never a path;
- list/get data comes from `ProjectCatalog` and canonical binding/session counts, not grouping `session.project_id` strings;
- create/register delegates to the canonical catalog/workspace registration path and does not merely create a directory plus return the path as ID;
- path containment remains a locator-security check and does not become identity derivation;
- compatibility single-project endpoints can project one configured/default workspace, but must label the path separately and return an actionable error when no canonical project binding exists.

`ServerState.project_dir` may remain temporarily as a compatibility locator until Project Catalog Milestone 004, but no route or daemon operation may treat its contents as a project ID after this milestone.

### Security and authorization

- Parse and bound all untrusted IDs.
- Do not expose local paths where only a stable project/workspace identity is required.
- Do not allow an archived/unbound project context to become executable by fallback.
- Do not treat a valid ID as proof of access; preserve seams for later principal/ACL checks.
- Ensure protocol/server diagnostics do not echo untrusted oversized path or ID input.

### Documentation and static guards

Update at least:

- `architecture/identity.md`;
- `architecture/project_identity_storage.md`;
- `architecture/project_catalog.md`;
- `architecture/session.md`;
- `architecture/protocol.md`;
- `architecture/core.md`;
- `architecture/server.md` or the current server architecture document;
- `architecture/workspace.md`.

Strengthen or add static checks that reject:

- `ProjectId` construction from `Path`, `PathBuf`, `directory`, `project_dir`, Git root, or cwd;
- use of `ServerState.project_dir` as an ID value;
- new daemon handlers that accept a project path without also resolving canonical context;
- protected-module `PWD`/`current_dir` identity inference.

## 7. Ordered work packages

### Work package A — Authoritative context resolver

Intent: create one reusable project/workspace/session context authority.

Required changes:

- define typed context and typed resolution outcomes;
- integrate `ProjectStorage`, `ProjectCatalog`, and workspace storage;
- expose read and create/bind transaction APIs;
- cover lifecycle, mismatch, unresolved, and stale-revision outcomes.

Acceptance evidence:

- focused core tests for valid, archived, mismatched, unbound, and ambiguous contexts;
- two workspaces under one project resolve distinctly while sharing the same project identity;
- no path-derived-ID construction exists.

### Work package B — Session write-path adoption

Intent: ensure all newly created or rebound sessions have canonical bindings.

Required changes:

- migrate create, template, fork, import, and rebind flows;
- atomically persist session plus canonical binding;
- derive legacy projections only after successful context resolution;
- keep old rows readable.

Acceptance evidence:

- cancellation/failure leaves no partial session/binding pair;
- concurrent create/rebind uses uniqueness/revision checks;
- restart reloads the same binding.

### Work package C — Additive protocol adoption

Intent: let all frontends address stable project/workspace context.

Required changes:

- add context/binding DTOs and request fields/variants;
- update session and daemon snapshots;
- add typed compatibility outcomes and capability advertisement;
- preserve old-client decoding.

Acceptance evidence:

- old and new JSON fixtures round-trip;
- new requests cannot omit canonical context except through deterministic compatibility resolution;
- snapshots expose stable IDs and clearly separate locator fields.

### Work package D — Daemon request migration

Intent: remove legacy path/string authority from daemon handlers.

Required changes:

- route session list/create/load/attach and turn-related context through the resolver;
- review all `project_id`, `project_dir`, `directory`, and workspace request consumers;
- ensure runtime execution uses the resolved workspace root from context.

Acceptance evidence:

- a forged project/workspace mismatch is rejected before execution;
- moving a workspace locator does not change project identity;
- two active projects remain isolated.

### Work package E — Server compatibility cleanup

Intent: stop server routes from exposing paths as IDs without prematurely implementing the complete catalog protocol.

Required changes:

- back project routes with `ProjectCatalog`/context services;
- return stable IDs and separate locators;
- keep path policy for local registration;
- leave full multi-project server removal to Project Catalog 004.

Acceptance evidence:

- project route IDs remain stable after path rename;
- list/get session counts use canonical bindings;
- no response field named `id` is populated from `project_dir` or a path.

### Work package F — Guards, documentation, and closure matrix

Intent: make the new authority boundary difficult to regress.

Required changes:

- strengthen static checks and negative fixtures;
- update architecture docs and compatibility ownership;
- record every remaining legacy projection and its removal prerequisite.

Acceptance evidence:

- guards pass on production code and fail on deliberate path-authority fixtures;
- docs identify the canonical resolver and compatibility-only fields.

## 8. Failure, cancellation, restart, and contention semantics

- Invalid IDs fail before database or filesystem access.
- Missing project/workspace records return typed not-found outcomes.
- Project/workspace mismatch returns a typed mismatch and never rebinds automatically.
- Ambiguous or `rebind_required` state remains non-executable until explicit operator action.
- Archived projects and workspaces fail operations requiring active execution; read-only inspection may remain available.
- Session creation and canonical binding are atomic.
- Cancellation before commit creates neither session nor binding; cancellation after commit returns the durable result on retry through idempotent request/session identity semantics already used by the relevant flow.
- Concurrent binding writes use uniqueness and expected revisions; stale writers receive current state rather than overwriting.
- Daemon restart reconstructs context from durable stores and does not require path scanning.
- Old directory-only requests may resolve only through an existing unique workspace locator. No match or multiple matches returns `project_context_required` or `ambiguous_project_context`.

## 9. Compatibility and migration

- Historical tables and fields remain intact.
- New protocol fields are additive and optional for old decoders.
- New production writes always establish canonical bindings first.
- Legacy path-valued `session.project_id` remains a compatibility projection and must be documented as non-authoritative.
- Existing clients may continue directory-only session creation only through deterministic lookup of a previously registered workspace/project relation.
- Do not silently create a new logical project from an old path-only request.
- Full removal of legacy fields is deferred to Milestone 004 and requires repository-wide evidence.
- Full project catalog operations and removal of single-project server assumptions remain Project Catalog Milestone 004.

## 10. Required tests

### Focused unit tests

- ID/context parser bounds and typed errors.
- project/workspace lifecycle and membership validation.
- compatibility lookup: unique, none, and ambiguous directory mappings.
- snapshot/DTO serialization and redaction.
- static guard positive and negative fixtures.

### Integration tests

- create session with explicit project/workspace context.
- create via old directory-only request when exactly one binding exists.
- reject old request when no or multiple bindings exist.
- fork/template/import preserve canonical binding.
- session list uses canonical project identity rather than legacy projection.
- project route list/get/register returns stable IDs and separate locators.
- two projects with similar paths remain isolated.

### Restart and recovery tests

- daemon restart rehydrates session/project/workspace context without scan.
- injected failure between session row and binding commit leaves neither partial write.
- v25/v28 database fixtures remain readable.

### Contention and cancellation tests

- concurrent create/rebind converges or returns typed stale revision.
- session creation cancellation before commit leaves no row.
- concurrent clients cannot cross-bind one session to another project's workspace.

### Security and negative tests

- path, Git root, cwd, and `ServerState.project_dir` cannot construct a `ProjectId`.
- oversized/control-character IDs fail without echoing full input.
- project/workspace mismatch fails before tool/process execution.
- archived/unbound context cannot execute through fallback.

### Protocol and compatibility tests

- old request/response fixtures decode.
- new context DTOs round-trip.
- capability negotiation behavior is deterministic.
- old clients receive bounded actionable errors when context cannot be resolved.

## 11. Required verification commands

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test -p codegg-core identity
rtk cargo test -p codegg-core project_storage
rtk cargo test -p codegg-core project_catalog
rtk cargo test -p codegg-core session
rtk cargo test -p codegg-protocol
rtk cargo test --test session_crud
rtk cargo test --test storage_migrations
rtk cargo test --test workspace_isolation
rtk cargo test --test workspace_services_isolation
rtk cargo test --lib core::transport::daemon_socket
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_identity_path_usage.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk git diff --check
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 rtk cargo test --workspace --all-features -- --test-threads=14
```

If the previously recorded daemon-socket startup race recurs, run the focused affected tests serially with full output and record the result. Do not claim a fully green workspace suite when unrelated known failures remain.

## 12. Documentation updates

- Explain canonical project/workspace request context and resolver ownership.
- Mark every legacy path/string field as compatibility-only.
- Document old-client directory-only resolution and its failure cases.
- Explain the boundary between this milestone and Project Catalog Milestone 004.
- Document server-route transitional behavior and removal prerequisites.
- Update protocol request/response examples with stable identities.

## 13. Acceptance criteria

- Every new executable session has a durable canonical project/workspace binding.
- New daemon requests and snapshots carry stable project/workspace identities.
- Directory-only compatibility requests cannot create or redefine identity.
- Old clients remain readable or fail with explicit context-required diagnostics.
- `src/server/routes/project.rs` no longer returns a path as `ProjectInfo.id`.
- Session listing and server counts use canonical bindings.
- Project/workspace mismatches and unresolved contexts fail before execution.
- Static guards reject new authoritative path-derived project identity.
- Existing provider selection, workspace execution, and session compatibility behavior remain functional.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- canonical project/workspace membership cannot be validated with the closed stores;
- implementation would require making a path or Git remote authoritative identity;
- project/repository cardinality must change;
- additive protocol compatibility cannot be preserved without a documented version negotiation decision;
- the work expands into complete project-catalog protocol, TUI tabs, ACLs, remote nodes, or runtime-asset refresh;
- a required schema change would destructively rewrite legacy fields;
- repository evidence contradicts the closed Milestone 002 invariants.

## 15. Closure evidence required

The closure record must contain:

- exact implementation commit(s);
- requirement-to-evidence matrix;
- canonical context type/resolver ownership;
- protocol/DTO delta and compatibility fixtures;
- session create/fork/import/rebind atomicity evidence;
- server route before/after authority review;
- path-identity guard and negative-fixture output;
- restart/contention/cancellation results;
- full verification command log with pass/fail counts;
- inventory of remaining legacy fields and named removal prerequisites;
- explicit disposition of Multi-Project TUI and Project Catalog 004 dependencies.

## 16. Handoff notes

- Treat `3ce0a7e` as the reviewed code baseline; inspect current `main` for later planning-only commits before editing.
- Preserve user changes and existing migration ordering.
- The repository intentionally limits parallel test resource use; follow existing build/test constraints.
- Do not conflate stable identity with authorization.
- Do not implement full catalog protocol or remove `ServerState.project_dir` unless that removal is necessary and remains within the narrow compatibility cleanup described above.
- The daemon-socket test race recorded by Milestone 002 is a known verification hazard, not permission to omit focused protocol tests.
