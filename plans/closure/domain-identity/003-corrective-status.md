# Domain Identity Milestone 003 — Corrective Closure Status

Status: closed

Source implementation plans:

- `plans/implementation/domain-identity/003-daemon-protocol-adoption.md` —
  original unimplemented handoff, now superseded.
- `plans/implementation/domain-identity/003-corrective-daemon-protocol-adoption.md` —
  corrective implementation handoff.

Source subsystem roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestone-3--daemon-and-protocol-adoption`

Historical finding:

- `plans/closure/domain-identity/003-status.md` remains unchanged and records
  the original `corrective pass required` finding.

Repository baseline: `27a2e54`

Implementation commit:

- `ec42dce` — `feat(domain-identity): adopt daemon canonical context`

## 1. Executive finding

The corrective pass closes the original Milestone 003 finding. Daemon-owned
session creation, template creation, import, fork, list, load/attach hydration,
runtime binding, snapshots, and server compatibility adapters now use canonical
project/workspace context. Legacy session rows remain readable, but directory
compatibility is a bounded lookup of one existing resolved binding; it never
creates a project identity from path text.

No new database migration was required. The existing canonical project,
workspace, and binding tables are used, and new session writes commit the
compatibility projection and `session_project_binding` together.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Authoritative context type and resolver | `crates/codegg-core/src/context.rs` owns bounded typed request parsing, lifecycle checks, workspace/project binding validation, session binding validation, and unique directory compatibility lookup. | pass |
| New session binding atomicity | `SessionStore::create_with_binding` inserts the session and resolved canonical binding in one transaction; `crates/codegg-core/tests/context.rs::canonical_session_create_and_list_are_atomic_and_identity_backed` verifies the row and canonical list. | pass |
| Import binding atomicity | `SessionStore::import_session_with_binding` writes imported contents and the canonical binding in one transaction; daemon import resolves context before invoking it. | pass |
| Fork/template/rebind behavior | Daemon/server fork paths resolve context before cloning and compensate on binding failure; template creation uses `create_with_binding`; runtime rebinding consumes canonical context. Legacy storage-level fork/import APIs remain readable for old rows. | pass |
| Additive protocol adoption | `ProjectContextDto`, `SessionBindingDto`, optional request identity fields, snapshot binding, serde defaults, old/new fixture tests, and `identity_aware_context` capability were added without a protocol version bump. | pass |
| Legacy compatibility | Directory-only create/list/session paths resolve only an existing unique canonical binding; missing or ambiguous mappings return `project_context_required`. Legacy load remains inspectable and does not silently execute without context. | pass |
| Stable server project identity | `src/server/routes/project.rs` returns catalog `ProjectId` values, preserves paths as locators, and sources session counts/listing from catalog and canonical bindings. REST and WebSocket adapters use the same authority. | pass |
| Path-derived identity guard | `scripts/check_identity_path_usage.py` scans core and daemon/server authority surfaces. Production guard passes; the negative fixture is rejected with `path-derived ProjectId construction detected`. | pass |
| Documentation and boundary | The eight required architecture documents were updated; core-boundary and daemon-CWD guards pass. | pass |

## 3. Verification record

Passing focused tests:

- `rtk cargo test -p codegg-core --test context` — 6 passed.
- `rtk cargo test -p codegg-core session` — 42 passed.
- `rtk cargo test -p codegg-core identity` — 10 passed.
- `rtk cargo test -p codegg-core project_storage` — 7 passed.
- `rtk cargo test -p codegg-core project_catalog` — 11 passed.
- `rtk cargo test -p codegg-protocol` — 84 passed.
- `rtk cargo test --test session_crud` — 32 passed.
- `rtk cargo test --test storage_migrations` — 4 passed.
- `rtk cargo test --lib core::daemon::tests` — 27 passed.
- `rtk cargo test --lib core::tests` — 14 passed.
- `rtk cargo test --lib remote_core_loader_tests` — 9 passed.
- `rtk cargo test --test workspace_isolation` — 6 passed.
- `rtk cargo test --test workspace_services_isolation` — 11 passed.
- `rtk cargo test --lib core::transport::daemon_socket` — 10 passed.
- `rtk cargo test --test scheduler_contention` — 14 passed.
- `rtk cargo test --test scheduler_cancellation` — 10 passed.
- Daemon recovery/cancellation filters — 2 and 3 passed respectively.

Repository gates:

- `rtk cargo fmt --all -- --check` — pass.
- `rtk cargo check --workspace --all-targets --all-features` — pass.
- `rtk bash scripts/check-core-boundary.sh` — pass.
- `rtk python3 scripts/check_daemon_cwd_usage.py` — pass.
- `rtk python3 scripts/check_project_catalog_invariants.py` — 7/7 pass.
- `rtk python3 scripts/check_execution_ownership.py` — pass.
- `rtk python3 scripts/check_identity_path_usage.py` — pass.
- `rtk git diff --check` — pass.

The capped workspace command
`CARGO_BUILD_JOBS=1 rtk cargo test --workspace --all-features -- --test-threads=14`
recorded 3,814 passed and 5 environment-restricted failures. The five
failures were three Unix-socket integration tests unable to create/connect a
socket in this sandbox (`No such file or directory`) and two fake Eggpool
server tests unable to bind (`Operation not permitted`). No changed identity,
protocol, session, daemon, server, workspace, or TUI test failed.

The repository-wide clippy command remains non-clean because of eight existing
warnings promoted to errors in unrelated project-catalog tests, skills code,
and the large TUI message enum. It produced no diagnostic in the changed
production identity/protocol/session/server files. This pre-existing lint debt
is not a Milestone 003 blocker and is preserved for its owning work.

## 4. Security, migration, and compatibility disposition

- IDs, session IDs, and directory locators are bounded and control-character
  checked at the core boundary.
- A valid ID is not treated as authorization; the resolver validates lifecycle
  and durable membership only.
- No filesystem scan or cwd lookup is used to establish project authority.
- No historical project/session/directory columns were removed or rewritten;
  compatibility projections are derived only after canonical resolution.
- Unresolved, archived, mismatched, missing, and ambiguous contexts fail before
  executable session creation or turn binding.

Remaining compatibility fields and owners:

- `session.project_id`, `session.workspace_id`, and `session.directory`: owned
  by session storage compatibility until all clients consume `SessionBindingDto`
  and migration evidence supports removal.
- `ServerState.project_dir` and single-project route locator fields: owned by
  Project Catalog Milestone 004, which owns the full project-scoped REST/WS
  migration and default-locator removal criteria.
- Legacy `CoreRequest::SessionList.project_id` directory input: owned by the
  daemon compatibility adapter until clients send stable catalog IDs.

## 5. Future-plan disposition

Domain Identity 003 is removed from dependency-ready and active-closure work,
and is recorded under recently closed work in `plans/registry.md`.

- Runtime Assets 003 remains ready; it was already independently unblocked and
  is not changed by this closure.
- Project Catalog 003 remains blocked on Runtime Assets 003.
- Project Catalog 004 remains not ready because it still depends on the
  Project Catalog 1–3 and Runtime Assets interfaces; this closure does not
  implement its broader protocol/catalog surface.
- Multi-Project TUI 001 remains blocked on Project Catalog 004 and Runtime
  Assets refresh/activation; it must consume the canonical context contract.
- Session Projections 001 is partially unblocked: its Domain Identity leg is
  cleared, but it remains blocked on Project Catalog 004 and Multi-Project TUI
  001. Its blocker text was updated accordingly.
- Domain Identity Milestone 004 remains not started; no dependency-ready plan
  was invented. Its next handoff should define migration evidence and legacy
  removal criteria.

## 6. Closure decision

Milestone 003 is closed. The original corrective finding is preserved
immutably, the corrective implementation and evidence are linked, the
remaining compatibility surface has named owners, and downstream status was
updated without claiming that unrelated catalog, TUI, runtime-asset, or
environment-restricted work is complete.
