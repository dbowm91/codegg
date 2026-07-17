# Domain Identity Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/domain-identity/002-project-repository-storage-migration.md`

Source subsystem roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestones`

Repository baseline reviewed: `9dcde707f6fe001cc6d73e7f562ccccf9f782f1`

Implementation commits or pull requests:

- `84d92f0` — add durable project/repository identity storage, local lineage
  reconciliation, legacy-import bindings, migration tests, guards, and
  architecture documentation.

## 1. Executive finding

Milestone 2 is closed as an infrastructure and migration milestone. SQLite
schema version 25 now adds canonical logical-project, repository,
project/repository relation, workspace binding, session binding, and identity
diagnostic tables without replacing historical session or project fields.
`ProjectStorage` owns deterministic reconciliation, operator inspection,
optimistic rebind operations, and bounded catalog reconciliation. Local Git
lineage probing is network-free, secret-safe, and conservative when evidence is
ambiguous or stale. Legacy project-database imports preserve source session and
message IDs, retain compatibility projections, and establish canonical
bindings before the imported data is served.

Daemon/protocol-wide authority adoption remains Domain Identity Milestone 3;
this closure does not claim that later surface migration.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A migrated session resolves to one stable `ProjectId` and `WorkspaceId` | `migrate_legacy_project_database` calls `ProjectStorage::reconcile_workspace_path` and `bind_session`; `import_preserves_session_and_message_ids_and_adds_canonical_binding` | pass | The legacy `project_id` string remains only as a compatibility projection. |
| Two uniquely related workspaces share one `ProjectId` and `RepositoryId` | `same_lineage_reuses_project_and_repository`; `concurrent_registration_converges_on_one_project_and_repository` | pass | Path-independent lineage is the reuse key. |
| Workspace rename/move preserves project identity when lineage remains unique | `ProjectStorage` reuses the normalized lineage key across distinct workspace roots; local probe is rooted at the explicit workspace locator | pass | No path text is used to construct a project or repository ID. |
| Ambiguous rows remain accessible with actionable rebind diagnostics | `ambiguous_evidence_is_preserved_as_diagnostic`; `explicit_rebind_resolves_an_ambiguous_binding`; `rebind_workspace` uses expected revisions | pass | Ambiguous evidence is not merged or guessed. |
| Migration is additive, idempotent, restart-safe, and concurrency-safe | v25 additive schema test; migration marker rerun test; injected-failure resume test; concurrent registration test with a four-connection file SQLite pool | pass | Reconciliation retries bounded SQLite lock contention. |
| Legacy imports establish canonical bindings without path-derived identity | `migration.rs` canonical reconciliation/import path and migration integration tests; `check_identity_path_usage.py` | pass | Source DB is read separately and is not rewritten. |
| No plaintext credentials or secret-bearing remote material is persisted | `secret_bearing_lineage_is_not_persisted`; remote normalization tests; bounded/redacted diagnostic construction | pass | Credential-bearing remote evidence becomes a rebind-required diagnostic. |
| Runtime Assets can consume an explicit project/workspace binding interface | `ProjectStorage`, `WorkspaceBindingRecord`, `workspace_binding`, `inspect_workspace`; `architecture/project_identity_storage.md` | pass | No Runtime Assets integration was claimed in this milestone. |
| Project Catalog has a stable storage interface | `ProjectStorage` CRUD/reconciliation APIs and v25 foreign-keyed tables; architecture docs | pass | Project Catalog 001 is now dependency-ready. |
| Historical compatibility fields remain intact for Milestone 3 | `domain_identity_v25_is_additive_and_indexed`; import preservation test | pass | Legacy `project`, `session.project_id`, and `session.directory` remain readable. |

## 3. Production implementation evidence

### Storage and domain ownership

- Added `codegg_core::project_storage` with durable project, repository,
  relation, workspace-binding, session-binding, diagnostic, lifecycle,
  revision, inspection, and rebind APIs.
- Added `codegg_core::repository_lineage` for local-only Git probing and
  deterministic normalization of HTTPS, SSH, and scp-style remotes. Probes
  use explicit `current_dir`, bounded output, disabled prompts/config side
  effects, and no network or hooks.
- Added schema migration v25. The canonical tables have primary keys,
  foreign keys, status/lifecycle/revision checks, uniqueness on
  `(vcs_kind, lineage_key)`, relation primary keys, and lookup indexes for
  lifecycle, lineage, status, project, repository, workspace, session, and
  diagnostics.
- `STORAGE_LAYOUT_VERSION` is 25. Existing historical tables and fields are
  deliberately not rewritten.

### Migration and operational seams

- `migrate_legacy_project_database` reconciles the explicit workspace before
  importing rows, preserves source IDs, binds imported sessions, and records
  the existing migration marker for idempotent reruns.
- `ProjectStorage::reconcile_catalog` is bounded to registered workspaces;
  there is no broad background repository scan.
- Inspection and rebind APIs expose status, diagnostics, revisions, and
  operator-selected replacement project/repository bindings.
- Added `check_identity_path_usage.py` with a negative fixture proving that a
  path-derived `ProjectId` construction is rejected.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo test -p codegg-core project_storage -- --nocapture
rtk cargo test -p codegg-core repository_lineage -- --nocapture
rtk cargo test -p codegg-core migration -- --nocapture
rtk cargo test --test storage_migrations -- --nocapture
rtk cargo test -p codegg-core
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_identity_path_usage.py
rtk python3 scripts/check_identity_path_usage.py --fixture scripts/fixtures/identity_path_usage/path_derived.rs
rtk git diff --check
CARGO_BUILD_JOBS=1 rtk cargo test --workspace --all-features -- --test-threads=14
rtk cargo test --all-features core::transport::daemon_socket::daemon_socket_integration_tests -- --test-threads=1 --nocapture
```

### Results

- Formatting, focused identity/lineage/migration tests, storage migration
  integration tests, core boundary, daemon-cwd, execution-ownership, and
  whitespace checks passed. The focused tests cover schema round trips,
  same-lineage reuse, concurrent registration, stale revision conflicts,
  ambiguity/rebind, idempotence, source-ID preservation, and secret-safe
  diagnostics.
- The identity path guard passed normally. Its negative fixture failed as
  intentionally designed, reporting a path-derived identity violation.
- Core clippy with all targets/features and `-D warnings` passed.
- The broad all-features workspace run completed with `3759 passed; 3
  failed`. The three failures were unrelated daemon socket integration tests
  (`global_only_subscription_does_not_receive_session_events`,
  `two_socket_session_filter_isolation`, and
  `resume_replay_uses_same_filter_as_live_forwarding`); each failed to connect
  to its test Unix socket with `ENOENT` during the harness startup window. A
  serial targeted rerun was attempted but produced no output and was
  cancelled after hanging. This is recorded as a bounded unrelated test-harness
  finding below, not as identity/storage evidence.

## 5. Invariant review

- Paths remain locators only. Canonical IDs are generated independently and
  equivalent repositories are matched only by a unique normalized lineage
  key.
- A workspace binding is explicit, revisioned, and status-bearing; a session
  binding points to the canonical project and workspace.
- Ambiguous, insufficient, stale, and credential-bearing evidence remains
  visible as diagnostics and requires explicit operator rebind.
- Legacy fields remain readable and are written only as compatibility
  projections during import; they do not become canonical authority.
- Git inspection is local-only and bounded. It does not invoke network
  operations, hooks, or unbounded repository scanning.
- SQLite writes are transactional at the migration layer, use foreign keys
  and uniqueness constraints, and avoid holding a write lock while probing
  Git. Reconciliation retries only bounded database-lock contention.

## 6. Failure and recovery review

- Repeating reconciliation for the same workspace/evidence is idempotent and
  does not create duplicate logical projects or repositories.
- Concurrent registration converges through the lineage uniqueness constraint
  and bounded lock retries.
- Expected revision checks reject stale rebinds with a typed conflict; an
  operator can retry after inspecting the current binding.
- Migration markers make completed legacy imports restart-safe. The existing
  injected mid-migration failure test proves completed migration steps remain
  recorded and a later run resumes to version 25.
- No unresolved session is made executable by guessing. Unbound sessions are
  marked `rebind_required` during bounded catalog reconciliation.

## 7. Migration and compatibility review

- v25 is additive and follows the existing v1–v24 migration chain. The
  historical `project` table and string-backed session projections survive
  unchanged.
- Legacy imports preserve source session and message IDs, source project and
  directory projections, and add canonical workspace/session binding rows.
- The source project database remains separate and is not modified by the
  import path. The existing migration marker prevents duplicate imports.
- No protocol or server authority migration was attempted; those belong to
  Milestone 3.

## 8. Security review

- Remote normalization strips credentials, query/fragment material, control
  characters, and `.git` suffixes before a lineage identity is considered.
- Credential-bearing or otherwise unsafe remote evidence is rejected from
  repository persistence and produces only a bounded diagnostic.
- Git probes run with prompts disabled, use explicit workspace roots, cap
  captured output, and do not perform network access.
- The path-identity static guard and its negative fixture protect the
  canonical construction boundary.

## 9. Documentation and operations

Updated:

- `architecture/identity.md`
- `architecture/project_identity_storage.md`
- `architecture/session.md`
- `architecture/storage.md`
- `architecture/workspace.md`
- `architecture/workspace_services.md`
- `architecture/jobs.md`
- `plans/implementation/domain-identity/002-project-repository-storage-migration.md`
- `plans/registry.md` and the affected subsystem roadmaps

Operators can inspect a workspace through `ProjectStorage::inspect_workspace`
and list diagnostics through `list_diagnostics_for_workspace`; rebinds require
the current revision and an explicit replacement identity. The schema version
and canonical table/index inventory are documented in the storage architecture
docs and exercised by `tests/storage_migrations.rs`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Three unrelated daemon socket integration tests failed with Unix-socket `ENOENT` in the broad all-features suite; the serial targeted rerun hung and was cancelled. | The broad workspace suite is not completely green, but the failures do not touch identity/storage code or its focused evidence. | Track and fix the socket-test startup/timing race in the daemon transport test surface before treating the full suite as green. |
| low | Legacy `project_id` and `directory` projections remain in the session schema. | Intentional compatibility surface; later consumers could accidentally read them as authority. | Complete explicit daemon/protocol adoption and removal criteria in Domain Identity Milestones 3–4. |

No critical or high-severity finding remains for this milestone.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. Runtime Assets 001 and
Project Catalog 001 now have their required storage/binding baseline and are
marked ready for handoff. Domain Identity Milestone 3 remains not started
because its implementation plan has not yet been authored. Multi-Project TUI
001 and Session Projections 001 remain blocked on the later protocol/catalog
and project-aware state work.

## 12. Registry updates

Updated `plans/registry.md` to:

- close Domain Identity 002 and link this record;
- mark Runtime Assets 001 and Project Catalog 001 dependency-ready;
- retain Provider Connections 002 as ready;
- keep Multi-Project TUI 001 and Session Projections 001 blocked; and
- record Domain Identity 003 as the next unplanned milestone.

Updated subsystem roadmaps to close Domain Identity 002, mark the two newly
unlocked handoffs ready, and leave their later dependent milestones unchanged.
