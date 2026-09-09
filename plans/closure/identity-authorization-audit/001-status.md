# Identity, Authorization, and Audit Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/identity-authorization-audit/001-principal-membership-capability-domain.md`

Source subsystem roadmap:

- `plans/subsystems/identity-authorization-audit-roadmap.md#M001--principal-membership-role-and-capability-domain`

Repository baseline reviewed: `b5653b81acfbabd3b25517bb828f5f679d1c407f`

Implementation commits:

- `b0cae1e2` — M001 principal/membership/capability domain, migration v51, executable matrix, restart/contention tests, identity architecture docs.

## 1. Executive finding

M001 is closed. The durable canonical principal/project-membership domain and
deterministic role-to-capability expansion are implemented in
`codegg-core::team`, persisted through additive migration v51, and proven by
focused lifecycle, contention, restart, isolation, and negative tests. The
four initial roles expand centrally and monotonically to 21 semantic
capabilities; LocalOwner is an explicit deterministic principal, not a bypass;
no transport authentication or request-time authorization is claimed. No
unresolved high, medium, or low M001 finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Principal kinds/records (work package A) | `team.rs`: `PrincipalKind` (Human/ServiceAccount/Node/LocalOwner), `PrincipalRecord`, `TeamStore::create_principal` | pass | Typed `PrincipalId` reused; display names bounded, control chars rejected. |
| Semantic capability vocabulary (work package A) | `team.rs`: `Capability` (21 verbs), `Capability::parse` fail-closed, `CapabilitySet` deterministic dedup/sort | pass | Names are operation-oriented (`session.create`, `git.write`); no handler or frontend role names. |
| Role expansion + membership state/revision (work package B) | `ProjectRole::capabilities`, `role_capability_matrix`, `role_capability_rows`, `MembershipState`, revision-checked `update_membership` | pass | Viewer 6 / Contributor 14 / Maintainer 19 / Owner 21, strictly monotonic; only `Active` grants capabilities. |
| Durable store + LocalOwner bootstrap + migration (work package C) | `migrate_v51` (`principal`, `project_membership`), `STORAGE_LAYOUT_VERSION` 51, `ensure_local_owner` (`INSERT OR IGNORE` on `"local-owner"`) | pass | Restart-safe and idempotent; concurrent bootstrap converges on one row. |
| Service/query APIs + executable matrices + transport contract (work package D) | `TeamStore` query surface, `effective_capabilities`, `has_capability`, matrix helpers, module-level M002 contract docs | pass | Capability sets are data; request DTOs remain locators (no caller-supplied authority). |
| Role/capability matrix test | `membership_role_capability_matrix_is_deterministic_and_monotonic` | pass | Sizes, monotonicity, spot checks, 84-row executable matrix, set determinism. |
| Principal ID validation test | `principal_id_validation_rejects_paths_before_ownership` | pass | Empty/path/whitespace/invalid rejected; `local-owner` and fresh UUID accepted. |
| Membership create/update/remove test | `membership_create_update_remove_lifecycle` | pass | Viewer create, promote, suspend (empty caps), revoke (empty caps), conflict on re-create, revision-checked re-grant. |
| Concurrent stale revision test | `membership_stale_revision_cannot_restore_revoked_authority` | pass | Stale Owner re-grant after revocation returns `RevisionConflict`; row stays revoked; `has_capability` false. |
| Restart persistence test | `membership_restart_persistence_survives_reopen` | pass | File DB close/reopen/remigrate preserves membership and LocalOwner. |
| LocalOwner bootstrap test | `principal_local_owner_bootstrap_is_deterministic` | pass | Deterministic id, kind, active status, idempotent double-bootstrap, projection adapter mapping. |
| Project isolation test | `membership_project_isolation_scopes_queries` | pass | Cross-project grants absent; per-project and per-principal listings scoped. |
| Malformed/unknown capability negatives | `membership_unknown_capability_fails_closed`, `membership_role_parse_fails_closed_on_unknown` | pass | Unknown capabilities/roles rejected; empty capability list is a valid empty set. |
| Principal lifecycle test | `principal_lifecycle_create_get_disable` | pass | Create/get/disable with revision; stale status write conflicts. |
| No-secret-material test | `principal_records_contain_no_secret_material` | pass | Principal and membership JSON free of secret/token/password/api_key/bearer/credential. |
| Compatibility projections documented | `adapt_principal_to_projection_id`, `is_compatibility_projection`, identity.md section | pass | Synthetic projection strings classified; adapter is one-way diagnostic-only. |

## 3. Production implementation evidence

- `crates/codegg-core/src/team.rs` (new, ~1100 lines): canonical domain
  types, `TeamStore` daemon-owned SQLite service, future M002 transport
  contract documentation, 12 focused tests.
- `crates/codegg-core/src/session/schema.rs`: additive `migrate_v51`
  creating `principal` and `project_membership` with CHECK constraints and
  lookup indexes; wired into the version dispatcher.
- `crates/codegg-core/src/storage/mod.rs`: `STORAGE_LAYOUT_VERSION` 50 → 51.
- `crates/codegg-core/src/lib.rs`: `pub mod team;` (boundary-clean: only
  `identity`, `error::StorageError`, `session::schema`, `sqlx`, `serde`,
  `thiserror` used).
- `crates/codegg-core/src/agent_convergence.rs`: migration-version assertion
  now references `STORAGE_LAYOUT_VERSION` instead of hardcoding 50, so the
  v51 bump (and future additive migrations) do not break unrelated suites.
- `architecture/identity.md`: team-domain section recording principals,
  roles, the 21 capabilities, revision semantics, secret-free records,
  compatibility projections, and the explicit non-claim of transport
  enforcement.

Distinguished as absent (downstream milestones, not M001 scope): token
generation/authentication, request middleware, audit store, OIDC, presence,
chat, and any request-time authorization enforcement.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core principal
cargo test -p codegg-core membership
cargo test -p codegg-core team
cargo test -p codegg-core --lib
cargo fmt --all -- --check
cargo clippy -p codegg-core --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
bash scripts/check-core-boundary.sh
bash scripts/verify.sh quick
```

### Results

- `cargo test -p codegg-core principal`: pass, 5 passed (id validation,
  role-parse negative lives under membership filter; lifecycle, bootstrap,
  compatibility, no-secret tests).
- `cargo test -p codegg-core membership`: pass, 7 passed (role parse,
  capability negatives, matrix, lifecycle, stale-revision, isolation,
  restart).
- `cargo test -p codegg-core team`: pass, 12 passed (union of the above).
- `cargo test -p codegg-core --lib`: pass, 545 passed, 0 failed. One
  pre-existing hardcoding (`agent_convergence` asserting migration version
  50) was found by this run and corrected to reference
  `STORAGE_LAYOUT_VERSION`; the full suite is green after the fix.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy -p codegg-core --all-targets -- -D warnings`: pass (two
  M001 lints found during development — eager `ok_or_else` and test
  `to_string` — fixed before final run).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  pass, 0 warnings.
- `bash scripts/check-core-boundary.sh`: pass.
- `bash scripts/verify.sh quick`: pass (`==> Quick verification passed.`).

All evidence above is local execution and is labeled accordingly. No hosted
`CI / verify` run is attached; routine CI remains the operator action per
the closed development-verification roadmap.

## 5. Invariant review

- Paths never define identity: every store API takes typed `PrincipalId` /
  `ProjectId`; the lexical contract rejects path-like input before
  ownership (`principal_id_validation_rejects_paths_before_ownership`).
- LocalOwner is explicit, not a bypass: deterministic `"local-owner"` record
  with kind `LocalOwner`; authorization-shaped queries (`has_capability`,
  `effective_capabilities`) receive it like any principal.
- Roles expand centrally: `ProjectRole::capabilities` is the single
  authority; the executable matrix test pins sizes and monotonicity.
- Unknown input fails closed: unknown roles and capabilities error; empty
  capability list is the empty set; non-active memberships grant nothing.
- Membership is project scoped: primary key `(project_id, principal_id)`;
  isolation test proves no cross-project leakage in grants or listings.
- No credential secret in membership records: schema has no secret column;
  serialization test asserts the absence of secret-bearing field names.

## 6. Failure and recovery review

- Duplicate delivery/idempotency: bootstrap uses `INSERT OR IGNORE`;
  remigration is idempotent (restart test remigrates before reading).
- Stale generation: every membership and principal-status mutation requires
  the current revision; mismatches return typed `RevisionConflict` /
  `PrincipalRevisionConflict` and change nothing.
- Revocation races: revoked rows are retained, re-creation returns
  `MembershipConflict`, and a stale pre-revocation snapshot cannot restore
  authority (contention test).
- Daemon restart: file-DB close/reopen/remigrate preserves principals,
  memberships, revisions, and effective capabilities (restart test).
- Malformed input: unknown roles/capabilities, empty/overlong/control-char
  display names, and path-like identities are rejected with typed errors.
- Bounded behavior: display names bounded (200), capability sets bounded
  (21 closed variants), no unbounded retention introduced.

## 7. Migration and compatibility review

- Additive migration v51 (`CREATE TABLE IF NOT EXISTS` + `IF NOT EXISTS`
  indexes) inside the existing transactional `migrate_and_record` harness;
  forward and restart safe.
- `STORAGE_LAYOUT_VERSION` 50 → 51. No existing table altered; no data
  backfill; no rollback beyond the standard SQLite restore story.
- No existing global bearer/provider credential record is reinterpreted as a
  human principal: `create_membership` requires an existing principal row,
  and provider-credential types are untouched.
- Existing string principal fields (`ProjectionPrincipalId` synthetics,
  `principal_identity` strings) remain compatibility projections until M003;
  classified by `is_compatibility_projection` and bridged one-way by
  `adapt_principal_to_projection_id`.

## 8. Security review

- Authorization is not enforced in this milestone by design; the store is
  daemon-owned infrastructure and capability sets are data. The module docs
  and identity architecture doc state the non-claim explicitly.
- No secret material enters the new tables, records, DTOs, logs, or tests
  (schema inspection + serialization test).
- Privilege boundaries: only `Active` memberships of `Active` principals
  grant capabilities; suspended/revoked/unknown/distinct-project pairs yield
  the empty set. Stale re-grants fail closed.
- Denial-of-service bounds: bounded display names, closed capability enum,
  indexed lookups, no new network or spawning surface.
- `codegg-core` boundary guard passes; no UI/server/plugin/auth imports or
  dependencies added.

## 9. Documentation and operations

Updated:

- `architecture/identity.md` — team-domain section (principals, roles,
  21 capabilities, revision semantics, secret-free records, compatibility
  projections, M002 transport contract, explicit non-claim).
- `crates/codegg-core/src/team.rs` module docs — design notes and future
  transport contract for M002 implementers.
- `plans/implementation/identity-authorization-audit/001-principal-membership-capability-domain.md`
  — marked closed, linking this record.
- `plans/subsystems/identity-authorization-audit-roadmap.md` — M001 closed,
  M002 ready.
- `plans/implementation/identity-authorization-audit/002-transport-authentication-principal-binding.md`
  — unblocked to ready for handoff.
- `plans/registry.md` — Identity row advanced to M002 ready; M002
  registered dependency-ready; M002 blocker row removed; M001 recorded
  under closure evidence.

Operator note: after upgrading, existing databases migrate to v51
automatically on next daemon start. `TeamStore::ensure_local_owner` is safe
to call on every start. No operator action is required for M001 alone;
team authentication behavior is unchanged until M002 lands.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None. | — | — |

There are no unresolved critical, high, medium, or low M001 findings.

## 11. Roadmap disposition

Milestone closed and the next dependency may proceed. The registry audit
found exactly one registered plan whose hard dependency is now satisfied:
M002 (transport authentication and principal binding), whose sole blocker
was M001 closure. M002 is registered `ready` in the same commit. M003
remains blocked on M002; M004 on M003; M005 on M004; presence M001 remains
blocked on identity M003; collaboration M001 remains blocked on identity
M005 + presence M003. No corrective pass is required and no new
dependency-ready plan was created beyond the M002 unblock.

## 12. Registry updates

Included in the closure commit alongside this record:

- M001 source plan marked closed, linking this record.
- Roadmap milestone table: M001 `closed` with closure link; M002 `ready`.
- M002 implementation plan: `blocked` → `ready for handoff` (sole blocker
  M001 now closed; local/HTTP/WebSocket/stdio seams already identified as
  stable in the plan's readiness section).
- Registry active-subsystem row: Identity current milestone M001 ready →
  M002 ready.
- Registry dependency-ready table: M001 row replaced by the M002 row
  (transport authentication and principal binding; M001 closure is the
  satisfied dependency).
- Registry blocked-work table: M002 row removed; M003–M005, presence, and
  collaboration rows retained unchanged.
- Registry closure-evidence table: Identity M001 row added pointing at this
  record.
