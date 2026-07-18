# Domain Identity Milestone 004 — Closure and Legacy-Removal Criteria

Status: ready for handoff

Repository baseline: `466356f8bef4242e24bafea1a4d5603e91d9f197` (`main`; planning-only commits after this baseline do not alter production behavior)

Production implementation baseline:

- `f203ed9` — typed identity primitives and relation contracts.
- `84d92f0` — durable project/repository storage, canonical workspace/session bindings, reconciliation, and migration diagnostics.
- `ec42dce` — canonical daemon request context, atomic identity-aware session writes, additive protocol binding DTOs, compatibility adapters, server identity cleanup, and expanded path-identity guard.
- `5974976` — bounded project discovery and conservative reconciliation, providing representative moved/missing/ambiguous locator behavior.
- `27cbd43` — explicit project/workspace activation and bounded health, demonstrating the current canonical context consumer boundary.

Source roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestone-4--closure-and-legacy-removal-criteria`

Long-term requirements:

- `plans/000-long-term-specification.md` — durable path-independent project identity, explicit daemon context, migration safety, compatibility, and multi-project authority.
- `plans/001-terminology-and-domain-model.md` — Project, Repository, Workspace, Session, binding, locator, compatibility projection, and unresolved/rebind-required state.
- `plans/002-long-term-roadmap.md#phase-0--canonical-domain-and-compatibility-foundation`

Applicable closure evidence:

- `plans/closure/domain-identity/001-status.md`
- `plans/closure/domain-identity/002-status.md`
- `plans/closure/domain-identity/003-status.md`
- `plans/closure/domain-identity/003-corrective-status.md`
- `plans/closure/project-catalog/002-status.md`
- `plans/closure/project-catalog/003-status.md`

Applicable ADRs:

- None. The canonical documents already require additive migration and path-independent identity. Stop for an ADR if implementation proposes destructive schema removal, changes project/repository/workspace cardinality, or makes compatibility telemetry an authorization mechanism.

Primary class: polish

## 1. Objective

Close the Domain Identity and Compatibility roadmap with reproducible migration evidence, a complete inventory of remaining compatibility projections, strengthened static/runtime guards against silent path-derived authority, operator-visible classification of unresolved legacy state, and explicit prerequisites for eventually removing each legacy field or request shape.

The milestone succeeds when the repository can demonstrate that representative historical databases migrate or remain actionably unresolved; canonical writes never silently fall back to path identity; every remaining compatibility field has one named owner and measurable removal gate; and the roadmap can close without deleting data or breaking older clients prematurely.

This milestone defines removal criteria. It does not automatically drop historical columns, bump the wire protocol incompatibly, or remove compatibility adapters whose consumers have not yet migrated.

## 2. Why this milestone is ready

All hard dependencies are closed:

- Milestone 001 established typed identity and relation contracts.
- Milestone 002 established durable canonical storage, conservative reconciliation, idempotent migration, and explicit unresolved states.
- Milestone 003 established canonical daemon/protocol context and bounded compatibility adapters.

Project Catalog discovery and activation are also implemented, so migration evidence can cover moved locators, multiple workspaces, missing roots, archived projects, and lazy activation without inventing temporary test-only identity behavior.

## 3. Current implementation evidence

At the repository baseline:

- `codegg_core::context::ProjectContextResolver` validates typed project/workspace membership, lifecycle, and session bindings.
- `SessionStore::create_with_binding` and `import_session_with_binding` commit compatibility projections and canonical binding rows atomically.
- New daemon session creation, template creation, import, fork, list, load/attach hydration, runtime binding, and snapshots use canonical context.
- `ProjectContextDto` and `SessionBindingDto` are additive protocol DTOs; legacy session fields remain present for wire compatibility.
- Directory-only compatibility resolution succeeds only when one existing canonical binding is found; missing or ambiguous mappings fail actionably.
- `src/server/routes/project.rs` returns stable catalog project IDs but still uses the server's default `project_dir` locator for compatibility `get` and relative-create behavior.
- `ServerState.project_dir` remains a compatibility/default-locator concern owned by Project Catalog Milestone 004.
- `CoreRequest::SessionList.project_id` is still a required string and may carry a legacy directory projection; identity-aware creation requests use optional canonical `project_id` and `workspace_id` fields.
- Protocol `Session` and `SessionSnapshot` still carry legacy `project_id`, optional `workspace_id`, and `directory` beside canonical `binding`.
- Historical SQLite `project`, `session.project_id`, `session.workspace_id`, and `session.directory` fields remain readable and intentionally unremoved.
- `scripts/check_identity_path_usage.py` rejects known path-derived project-ID construction patterns, but removal readiness and compatibility ownership are not yet represented as a single maintained inventory.

## 4. Invariants that must not regress

- A path, directory, Git root, or server default locator is never canonical project identity.
- Compatibility reads may resolve one existing canonical binding; they never create or merge identity from path text.
- New executable session writes require validated canonical project/workspace context.
- Ambiguous, missing, archived, mismatched, or unresolved rows remain inspectable and fail actionably rather than being guessed.
- Migration and evidence gathering are additive, restart-safe, idempotent, and non-destructive.
- Legacy fields remain readable until their documented removal gates are satisfied.
- Identity validity does not imply authorization.
- Diagnostics are bounded and do not expose credentials, credential-bearing remotes, or unnecessary absolute paths.
- No closure claim may rely only on current-schema databases; representative historical fixtures are required.

## 5. Scope

### In scope

- A maintained compatibility-surface inventory with owner, authority status, read/write sites, consumers, diagnostics, and removal prerequisites.
- Representative historical database fixtures and migration/restart/contention evidence.
- Classification and inspection of unresolved, ambiguous, archived, missing, moved, and multi-workspace identity states.
- Static guards preventing new authoritative writes to legacy/path-valued identity fields.
- Runtime assertions or typed helpers preventing compatibility adapters from becoming identity creators.
- A machine-readable or test-validated legacy-removal readiness report.
- Architecture and operator documentation.
- Final Phase 0 requirement-to-evidence closure matrix.

### Explicitly out of scope

- Dropping or renaming historical SQLite columns in this milestone.
- Removing `ProjectContextDto`, `SessionBindingDto`, canonical stores, or compatibility readers.
- Breaking old clients by requiring a protocol version bump without measured readiness.
- Project catalog protocol/server migration; that is Project Catalog Milestone 004.
- Multi-project TUI, authorization, remote-node identity, worktree lifecycle, or distributed identity replication.
- Guessing unresolved project/repository relationships to make fixtures pass.

## 6. Required production and evidence changes

### Compatibility-surface inventory

Create one authoritative inventory, preferably under `architecture/` with a small machine-validated companion file or script. At minimum inventory:

- historical SQLite `project` table;
- `session.project_id`;
- `session.workspace_id`;
- `session.directory`;
- protocol `Session.project_id`, `Session.workspace_id`, and `Session.directory`;
- protocol `SessionSnapshot.project_id`, `workspace_id`, and `directory`;
- `CoreRequest::SessionList.project_id` legacy directory interpretation;
- optional `SessionCreate.directory` and `SessionCreateFromTemplate.directory` compatibility locator behavior;
- `ServerState.project_dir` and default-locator server routes, owned by Project Catalog 004;
- any root/TUI/session model field still carrying a path-keyed project projection.

Each entry must record:

- canonical replacement;
- current authority classification (`canonical`, `compatibility projection`, `compatibility input`, `display locator`, or `deprecated write`);
- production read sites;
- production write sites;
- test/fixture consumers;
- owning subsystem and milestone;
- failure behavior when canonical context is unavailable;
- required telemetry/evidence before removal;
- minimum release/deprecation window if relevant;
- explicit removal blocker.

Add a guard that fails when a new compatibility field or authoritative write site appears without inventory ownership.

### Representative migration fixture corpus

Add versioned, deterministic SQLite fixtures or fixture builders covering at least:

1. pre-workspace legacy sessions whose `project_id` and `directory` are path-valued;
2. workspace-aware sessions without canonical project/session binding rows;
3. a canonical project with one repository and two workspaces/worktrees;
4. a workspace moved to a new path with unchanged unique repository lineage;
5. two unrelated repositories with superficially similar names/paths;
6. ambiguous or credential-bearing remote lineage that must remain `rebind_required`;
7. missing/unavailable workspace roots;
8. archived project and archived workspace/session combinations;
9. partially completed migration followed by restart/resume;
10. concurrent registration/import attempts for the same repository/workspace;
11. source databases imported through `migrate_legacy_project_database` with preserved session/message IDs;
12. already-current databases to prove idempotent no-op behavior.

Fixtures must not embed live credentials or machine-specific absolute paths. Use deterministic temporary roots and redacted remote examples.

### Migration evidence harness

Provide a focused harness or integration test target that:

- migrates every fixture from its recorded source layout to the current layout;
- validates canonical project/workspace/session bindings;
- validates preserved historical rows and compatibility projections;
- verifies unresolved/ambiguous states and diagnostics;
- restarts and reruns migration to prove idempotence;
- injects a bounded failure between migration steps and proves resume behavior;
- exercises concurrent registration/import where applicable;
- verifies no broad repository/network scan occurs;
- emits a compact machine-readable summary suitable for the closure record.

The evidence harness must fail closed if a fixture silently becomes executable through path fallback.

### Legacy-state inspection and readiness report

Add a bounded core/operator inspection seam that reports counts, not raw unbounded rows, for:

- canonically bound sessions;
- unbound/rebind-required sessions;
- ambiguous workspace/project bindings;
- sessions still relying on legacy-only protocol/storage projections;
- archived canonical contexts;
- compatibility writes observed or still enabled by code path;
- unknown/inconsistent states.

The report may be implemented as a core service plus CLI/debug operation, or as a deterministic offline inspection command. It must not expose secret-bearing remote text or unrestricted absolute paths.

Define a stable readiness result per compatibility entry, such as:

- `blocked_by_active_writer`;
- `blocked_by_legacy_consumer`;
- `blocked_by_unresolved_rows`;
- `blocked_by_server_default_locator`;
- `eligible_for_deprecation`;
- `eligible_for_removal_after_window`.

Do not report `eligible` merely because one test database is clean.

### Static and runtime guards

Extend identity guards to cover:

- writes to historical `session.project_id` or equivalent fields outside named compatibility projection helpers;
- construction of canonical `ProjectId`/`RepositoryId` from `Path`, `PathBuf`, directory, cwd, Git root, or locator strings;
- server/TUI code treating `path`, `directory`, or `project_dir` as a stable project key;
- compatibility adapters that create canonical records rather than resolve one existing unique binding;
- new session creation APIs that omit canonical binding persistence;
- tests or fixtures that bypass the canonical context resolver for executable paths.

Where static matching would be brittle, centralize compatibility projection writes behind one clearly named API and guard direct call sites.

### Removal criteria

For each compatibility field/request shape, define objective prerequisites. At minimum:

- no canonical production writer depends on the field as authority;
- all supported clients consume the canonical DTO/request replacement;
- compatibility usage can be measured or bounded through tests/diagnostics;
- representative fixture corpus has zero silently executable unresolved rows;
- unresolved real-world rows have an operator rebind path;
- Project Catalog 004 has removed server `project_dir` authority where applicable;
- Multi-Project TUI and remote transports use catalog IDs rather than paths;
- deprecation is documented for at least the required release window;
- rollback/upgrade behavior is understood;
- a separate destructive-removal plan and migration review is approved.

Classify current fields honestly. Most are expected to remain `blocked`, not removed, at the end of this milestone.

### Documentation

Update at least:

- `architecture/identity.md`;
- `architecture/project_identity_storage.md`;
- `architecture/session.md`;
- `architecture/storage.md`;
- `architecture/protocol.md`;
- `architecture/server.md`;
- `architecture/workspace.md`;
- a new or dedicated compatibility/removal-readiness document;
- static-guard documentation.

Document which fields remain, why they remain, who owns them, how failure behaves, and what future plan may remove them.

## 7. Ordered work packages

### Work package A — Inventory and authority classification

Intent: establish one source of truth before adding more tests or guards.

Required changes:

- enumerate all storage/protocol/server/TUI compatibility fields and call sites;
- classify each as canonical, projection, input, locator, or deprecated write;
- assign owner and removal blocker;
- add machine validation that inventory and known production surfaces remain synchronized.

Acceptance evidence:

- no known compatibility field lacks an owner;
- no legacy field is mislabeled as canonical authority;
- adding an unowned compatibility write makes the guard fail.

### Work package B — Historical fixture corpus and migration runner

Intent: make migration correctness reproducible rather than anecdotal.

Required changes:

- create the twelve representative fixture classes;
- add migration/restart/failure/concurrency runner;
- preserve source IDs and unresolved evidence;
- produce a compact result summary.

Acceptance evidence:

- every fixture has explicit expected canonical and compatibility state;
- reruns are idempotent;
- injected failure resumes safely;
- ambiguous data remains unresolved and non-executable.

### Work package C — Inspection and removal-readiness report

Intent: provide bounded evidence for future deprecation/removal decisions.

Required changes:

- count/classify canonical and legacy states;
- map inventory entries to readiness outcomes;
- bound/redact diagnostic output;
- expose through a focused operator or offline inspection seam.

Acceptance evidence:

- mixed fixture databases produce correct classifications;
- secrets and unrestricted paths are absent;
- active writers/consumers and unresolved rows block eligibility.

### Work package D — Guard hardening and compatibility centralization

Intent: prevent future regressions while compatibility remains present.

Required changes:

- centralize projection writes where needed;
- extend path-identity/static guards;
- add negative fixtures for path-derived authority, direct legacy writes, and compatibility-created identity;
- add runtime invariant tests around canonical session creation/import/fork/rebind.

Acceptance evidence:

- production passes guards;
- every negative fixture fails for the intended reason;
- new executable session paths cannot omit canonical binding.

### Work package E — Final Phase 0 closure matrix and documentation

Intent: close the roadmap without overclaiming destructive cleanup.

Required changes:

- map every Phase 0 requirement to code/tests/evidence;
- list residual compatibility fields and readiness states;
- document unresolved findings and future owners;
- update roadmap, registry, and architecture docs;
- create `plans/closure/domain-identity/004-status.md`.

Acceptance evidence:

- closure record distinguishes implemented canonical authority from deferred removal;
- every remaining field has named owner and prerequisites;
- no critical/high unresolved identity correctness issue remains.

## 8. Compatibility and migration requirements

- No destructive migration or field removal in this milestone.
- Existing clients and databases remain readable.
- Compatibility projections are written only after canonical context is resolved.
- Historical fixture formats are immutable once committed; corrections create a new fixture/version rather than rewriting evidence silently.
- Unknown future protocol fields retain current serde forward-compatibility behavior.
- Project Catalog 004 may remove `ServerState.project_dir` authority while retaining an explicit optional default locator for compatibility; Domain 004 records the final disposition but does not duplicate that migration.

## 9. Failure, recovery, and concurrency requirements

- Migration failure before a recorded step leaves prior data readable or startup fails before serving requests.
- Rerunning migration does not duplicate projects, repositories, workspaces, sessions, or diagnostics.
- Concurrent reconciliation/import converges or returns typed contention/conflict; it never produces divergent canonical IDs for one uniquely identified repository.
- Inspection is bounded and does not hold long write transactions.
- Missing roots and transient filesystem failures update diagnostics/observations, not canonical identity deletion.
- Cancellation of evidence tooling leaves the database usable and rerunnable.

## 10. Security requirements

- Fixture and report output contains no live secrets.
- Credential-bearing remote lineage is rejected/redacted and remains unresolved.
- Absolute paths are omitted or deterministically sanitized from portable summaries.
- Identity reports do not imply authorization or disclose projects beyond the caller's existing local/operator boundary.
- Git inspection remains local-only, bounded, non-interactive, hook-free, and network-free.
- Static guards cover server, daemon, core, protocol adapter, and TUI authority surfaces.

## 11. Verification commands

The implementation agent must run focused commands equivalent to:

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test -p codegg-core identity
rtk cargo test -p codegg-core context
rtk cargo test -p codegg-core project_storage
rtk cargo test -p codegg-core migration
rtk cargo test -p codegg-protocol
rtk cargo test --test session_crud
rtk cargo test --test storage_migrations
rtk cargo test --test <new_identity_fixture_target>
rtk cargo test --test workspace_isolation
rtk cargo test --test workspace_services_isolation
rtk python3 scripts/check_identity_path_usage.py
rtk python3 <new_compatibility_inventory_guard>
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk bash scripts/check-core-boundary.sh
rtk git diff --check
```

Run the capped workspace suite using the repository's documented resource policy. Environment-restricted socket/network failures must be separated from identity failures and recorded precisely; they cannot be silently ignored.

## 12. Stop conditions

Stop and record a corrective dependency or ADR if:

- destructive schema removal is required to gather evidence;
- a canonical identity must be derived from a path to preserve behavior;
- project/repository/workspace cardinality must change;
- migration cannot preserve ambiguous/unresolved information;
- compatibility usage cannot be distinguished from canonical authority without a protocol redesign;
- an inspection endpoint would expose secret-bearing or unauthorized project data;
- a downstream subsystem must be implemented inside the identity layer to make tests pass.

## 13. Acceptance criteria

All criteria are required:

1. One maintained compatibility inventory names every known legacy storage/protocol/server/TUI identity field, its classification, owner, and removal blocker.
2. Representative fixtures cover legacy, canonical, moved, multi-workspace, ambiguous, missing, archived, partial-migration, concurrent, imported, and current databases.
3. Every fixture migrates or remains actionably unresolved without silent path-derived identity.
4. Migration reruns and restart-after-failure are idempotent and safe.
5. Concurrent registration/import converges or fails with typed conflict without duplicate canonical authority.
6. A bounded inspection/readiness report classifies canonical, unresolved, compatibility-dependent, and archived states without exposing secrets or unrestricted paths.
7. Static/runtime guards reject new path-derived identity, direct unowned legacy writes, and executable session creation without canonical binding.
8. No production write silently falls back to a path-valued project identity.
9. Every remaining compatibility field has objective removal prerequisites and a named future owner.
10. No destructive schema or protocol removal occurs in this milestone.
11. The final closure matrix evidences every Phase 0 exit criterion and records all residual compatibility debt honestly.
12. Focused identity/migration/protocol tests and repository boundary guards pass, with unrelated environment restrictions documented separately.

## 14. Required closure evidence

Create `plans/closure/domain-identity/004-status.md` containing:

- executive closure finding;
- full Phase 0 requirement-to-evidence matrix;
- compatibility inventory summary;
- fixture corpus and migration result matrix;
- restart/failure/concurrency evidence;
- unresolved/ambiguous state classification;
- static/runtime guard results and negative-fixture evidence;
- security and redaction review;
- removal-readiness table for every compatibility entry;
- exact verification commands and results;
- unresolved findings with severity and owner;
- roadmap and registry disposition.

The subsystem roadmap may close only if canonical authority is evidenced end-to-end and all residual compatibility fields have explicit ownership and removal prerequisites. Closure does not require those fields to be removed.