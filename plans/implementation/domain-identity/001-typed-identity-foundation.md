# Domain Identity Milestone 001 — Typed Identity Foundation

Status: implemented

Repository baseline: `fbae374a2cd6172505204b1bc1bee1ef247afd5f` (production-code baseline; subsequent planning-only commits do not alter implementation state)

Source roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestone-1--typed-identity-primitives-and-relation-contracts`

Long-term requirements:

- `plans/000-long-term-specification.md` — stable project/repository/workspace/session identities and path-independent ownership
- `plans/001-terminology-and-domain-model.md` — canonical terms and relations
- `plans/002-long-term-roadmap.md#phase-0--canonical-domain-and-compatibility-foundation`

Applicable ADRs:

- None. Stop if implementation requires changing the canonical project/repository/workspace relationship.

Primary class: invariant

## 1. Objective

Introduce the typed identity primitives and canonical relation contracts required by later project, provider, asset, TUI, replay, team, and distributed work, without yet changing production storage authority or user-visible behavior.

The milestone succeeds when new core types can express `Project -> Repository -> Workspace/Worktree`, `Session -> Project + Workspace`, and future actor/execution references, while existing `WorkspaceId` behavior and current daemon operation remain intact.

## 2. Why this milestone is ready

- It has no hard dependency beyond the existing singleton-daemon/workspace baseline.
- `crates/codegg-core/src/workspace.rs` already demonstrates the desired typed-ID pattern with stable `WorkspaceId` and explicit `ExecutionContext`.
- Existing serde, SQLite, protocol, and migration infrastructure provide clear integration seams.
- The canonical long-term documents already resolve the architectural choice: paths are locators, not durable identity.

## 3. Current implementation evidence

- `crates/codegg-core/src/workspace.rs` owns `WorkspaceId`, `WorkspaceRecord`, registry/store traits, and immutable execution context.
- `crates/codegg-protocol/src/core.rs` and `dto.rs` use string IDs in session/workspace/job DTOs and retain `project_id`/`directory` compatibility fields.
- `crates/codegg-core/src/session/schema.rs`, `session/mod.rs`, and `migration.rs` support additive migrations and existing workspace binding.
- `src/server/routes/project.rs` currently uses path strings as project IDs, but changing that route is explicitly outside this first milestone.
- Static guards already protect daemon execution from new process-cwd dependence, demonstrating an established guard pattern.

Known gap: there is no common core identity module for project, repository, node, principal, agent-run/task, provider connection, channel, worktree, or audit IDs, and no canonical relation types separating logical project identity from workspace locator.

## 4. Invariants that must not regress

- Existing valid `WorkspaceId` serialization and workspace registry behavior remain compatible.
- No new production path may derive a durable ID from a filesystem path.
- Identity parsing must reject empty, malformed, or unbounded values consistently.
- Typed IDs are opaque; their existence does not grant authorization.
- No process-global cwd dependency is introduced.
- Current sessions, jobs, and daemon startup continue to compile and behave without a data migration in this milestone.

## 5. Scope

### In scope

- A low-level identity module in `codegg-core` or an equivalently dependency-safe core crate.
- Typed newtypes for:
  - `ProjectId`;
  - `RepositoryId`;
  - `WorktreeId`;
  - `NodeId`;
  - `PrincipalId`;
  - `AgentRunId`;
  - `AgentTaskId`;
  - `ProviderConnectionId`;
  - `ChannelId`;
  - `AuditEventId`.
- Consolidation or re-export of existing `WorkspaceId` without unnecessary breaking relocation.
- Shared validation, generation, serde, display, ordering/hash, and database-friendly string conversion rules.
- Core relation contracts representing project/repository/workspace/session bindings.
- Compatibility annotations/helpers showing which current string fields are legacy projections.
- Focused tests and architecture documentation.

### Explicitly out of scope

- Adding project/repository database tables.
- Migrating current session rows.
- Rewriting server project routes.
- Project discovery, TUI tabs, provider connection storage, auth, audit, remote nodes, or worktree orchestration.
- Removing `project_id`, `directory`, or other compatibility fields.
- Generating semantic IDs from Git remotes or paths.

## 6. Required production changes

### Core/domain

Create one authoritative typed-ID implementation pattern. Prefer a macro or small shared helper only if it keeps validation and error reporting explicit. IDs should support:

- construction through generated opaque values;
- validated parsing from persisted/protocol strings;
- `AsRef<str>`/`Display` and explicit `into_string` behavior;
- serde round trips;
- ordering/hash/equality;
- bounded length and allowed character rules;
- no implicit path conversion.

Define relation structs or interfaces sufficient for later storage work, for example a project binding carrying `ProjectId`, optional `RepositoryId`, `WorkspaceId`, and future optional `WorktreeId`/`NodeId`, without forcing all future fields into existing persisted records.

Keep `ExecutionContext` workspace-focused for now, but add a clearly named seam for later project identity propagation rather than overloading workspace ID.

### Storage and migrations

No production schema migration is required. Add SQLx/string conversion tests or helper traits where the current repository pattern supports them. Do not introduce unused tables merely to exercise types.

### Protocol and DTOs

Do not broadly change wire DTOs yet. It is acceptable to add conversion helpers or internal typed wrappers behind existing string fields. Any public DTO change must be additive and proven unnecessary for this milestone before proceeding.

### Runtime and concurrency

ID generation must be thread-safe and collision-resistant. No global mutable registry is needed. Relation values should be cloneable/immutable and suitable for `Arc`-owned runtime context.

### Frontend or operator surface

No user-visible UI is required. Developer diagnostics and debug formatting should remain concise and must not accidentally render paths as IDs.

### Security and authorization

Apply length/format bounds to untrusted protocol parsing. Do not encode secrets, usernames, path text, or authorization decisions into generated IDs.

### Documentation and static guards

- Add or update architecture documentation for the typed identity module and canonical relations.
- Update workspace/session/protocol docs only where needed to distinguish new types from compatibility fields.
- Prepare, but do not yet enable overly broad, a static guard pattern that later milestones can extend to reject path-derived project identity.

## 7. Ordered work packages

### Work package A — Inventory and identity contract

Intent: confirm all current ID patterns and define one compatible convention.

Required changes:

- inspect `WorkspaceId`, job/run/schedule IDs, session IDs, and protocol string fields;
- document the chosen generation and validation rules;
- avoid changing existing IDs whose semantics differ unless a compatibility wrapper is required.

Acceptance evidence:

- a concise mapping of each new type and its owner module;
- no unresolved naming collision with current aliases/types.

### Work package B — Implement typed IDs

Intent: add the reusable identity primitives with strict parsing and generation.

Required changes:

- implement newtypes and shared error type;
- add serde/display/hash/order conversions;
- add deterministic fixture constructors for tests without exposing unchecked constructors in normal production use;
- re-export types from stable core module boundaries.

Acceptance evidence:

- unit tests for every type;
- compile-time use from at least one relation contract.

### Work package C — Define canonical relations

Intent: make project/repository/workspace/session distinctions concrete before storage migration.

Required changes:

- add relation/binding types or traits used by the next milestone;
- identify legacy string projections explicitly;
- ensure several workspaces can reference one project/repository in the model;
- ensure sessions can carry project and workspace separately.

Acceptance evidence:

- relation construction/serde tests;
- tests proving paths are not accepted as implicit project IDs.

### Work package D — Documentation and guard seam

Intent: ensure later agents cannot misunderstand the new types.

Required changes:

- update architecture docs;
- add developer guidance for when to use project vs repository vs workspace vs worktree IDs;
- add focused static test/guard fixture or lintable marker for future path-identity enforcement.

Acceptance evidence:

- documentation matches `plans/001-terminology-and-domain-model.md`;
- no production schema or user behavior change slipped into scope.

## 8. Failure, cancellation, restart, and contention semantics

- ID parsing failure returns a typed error with the identity kind and safe reason.
- Generation has no partial persistent state and is safe under concurrent calls.
- Restart does not affect validity of persisted IDs.
- Duplicate generated IDs must be treated as a hard invariant violation by later stores; this milestone should rely on collision-resistant generation rather than a global lock.
- Test-only unchecked constructors must be clearly scoped and must not leak into untrusted production parsing.

## 9. Compatibility and migration

- Existing string-backed DTO and database fields remain unchanged.
- `WorkspaceId` remains wire/storage compatible.
- New identity types should serialize as strings to support additive future migrations.
- Do not silently reinterpret current path-valued `project_id` strings as valid new `ProjectId`; later migration must explicitly translate them.
- Document removal criteria for any temporary aliases introduced.

## 10. Required tests

### Focused unit tests

- generated ID validity and uniqueness sample;
- parse/display/serde round trip for every type;
- empty, overlong, whitespace, slash/path-like, NUL/control, and invalid-character rejection;
- ordering/hash map behavior;
- relation construction and equality.

### Integration tests

- core crate consumer imports and uses IDs without depending on the application crate;
- existing workspace/session/protocol tests compile unchanged.

### Restart and recovery tests

- serialized IDs remain valid across reconstruct/deserialize fixtures.

### Contention and cancellation tests

- concurrent generation from many tasks produces no duplicates in a bounded stress test.

### Security and negative tests

- path strings are not accepted through accidental `From<Path>` or permissive parsing;
- untrusted oversized values fail before allocation growth becomes unbounded.

### Migration and compatibility tests

- current `WorkspaceId` fixture serialization remains unchanged;
- current DTO JSON fixtures remain unchanged unless an additive field is explicitly justified.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-core identity
cargo test -p codegg-core workspace
cargo test -p codegg-protocol
cargo test --test workspace_isolation
python3 scripts/check_daemon_cwd_usage.py
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Run broader workspace tests only after narrow identity/workspace/protocol tests are green. Respect the repository's serial-test/resource constraints where configured.

## 12. Documentation updates

- new identity/domain architecture document or a clearly scoped section in existing core/workspace docs;
- `architecture/workspace.md` relation clarification;
- `architecture/session.md` compatibility-field clarification;
- `architecture/protocol.md` typed-internal/string-wire guidance;
- module rustdoc for validation/generation guarantees.

## 13. Acceptance criteria

- All required typed IDs exist in a stable core boundary and follow one documented validation contract.
- Existing `WorkspaceId`, daemon startup, session storage, and protocol behavior remain compatible.
- Canonical relation types distinguish project, repository, workspace, worktree, and session.
- No new durable identity is derived from path text.
- Required focused tests and guards pass.
- The closure record can map every roadmap Milestone 1 exit condition to concrete evidence.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the current `WorkspaceId` cannot be preserved without a breaking wire/storage change;
- project/repository cardinality requires changing the canonical model;
- a schema migration appears necessary to make the types compile;
- an existing public API relies on accepting arbitrary path strings as typed project IDs;
- the work expands into project storage, discovery, or server route migration;
- broad ID refactoring would destabilize unrelated job/run/session semantics.

## 15. Closure evidence required

- implementation commit(s);
- type inventory and ownership mapping;
- requirement-to-test matrix for all new IDs;
- exact verification commands and results;
- compatibility evidence for `WorkspaceId` and DTO fixtures;
- static guard result;
- list of legacy path/string projections intentionally left for Milestones 2–3;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 16. Handoff notes

- Preserve the current serial/resource-conscious test configuration.
- Do not mass-rewrite string IDs outside the milestone boundary.
- Prefer small explicit types over a generic `Id<T>` if generic serialization/debug output would obscure wire compatibility.
- Planning-only commits after the stated baseline do not alter production code, but the implementing agent must still inspect `main` before editing and record the actual implementation baseline in closure.
