# Provider Connections Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connections/001-connection-foundation.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connections-roadmap.md#milestone-1--durable-connection-and-secret-reference-foundation`

Repository baseline reviewed: `bccca00` (`main` implementation commit)

Implementation commits or pull requests:

- `bccca00` — add secret-safe provider connection domain/storage, credential-store compatibility, daemon lazy resolution, tests, and architecture documentation.

## 1. Executive finding

Milestone 1 is closed as an infrastructure milestone. Durable provider
connection metadata now has stable typed identity, explicit scope, validated
endpoint/TLS metadata, opaque credential references, SQLite persistence,
optimistic revision/lifecycle APIs, and a daemon-owned lazy runtime manager.
Existing environment/config provider registration remains intact. The next
Eggpool workflow can build on this storage and runtime substrate without
inventing a second connection model.

This closure does not claim the user-visible Eggpool `/connect` flow, session
selection by connection ID, team authorization, periodic health checks, or
credential rotation UX; those are later roadmap milestones and remain
explicitly deferred by the source plan.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Durable connection record and typed ID | `crates/codegg-core/src/provider_connections.rs`; `ProviderConnectionId`; core focused tests | pass | Provider implementation identity remains distinct from durable connection identity. |
| Personal, project, and deployment scope | `ProviderScope`; scope validation and SQLite round-trip tests | pass | Project scope requires `ProjectId`; scope remains metadata and grants no authorization. |
| Endpoint/TLS metadata validation without network I/O | `Endpoint::new`; endpoint normalization/negative tests | pass | Rejects unsupported schemes, userinfo, query/fragment, and TLS-policy mismatch. |
| Secret-safe persistence and redacted views | `SecretRef`, `SecretBindingLocator`, custom `Debug`, `ProviderConnectionDetail`, provider adapter tests | pass | SQLite and descriptors contain no resolved plaintext; detail omits the opaque reference. |
| Existing credential-store compatibility | `CredentialStoreAdapter`; `ProviderConnectionFactory`; 8 focused provider tests | pass | Exact provider/account lookup; missing, expired, and missing-master-key cases are typed failures. |
| Additive/idempotent migration and CRUD | migration v24 in `crates/codegg-core/src/session/schema.rs`; `ProviderConnectionStore` tests; `storage_migrations` | pass | Includes scope/state/revision indexes, uniqueness, CRUD, disable/delete, and optimistic conflict handling. |
| Daemon-owned lazy provider construction | `src/core/provider_connections.rs`; `CoreRuntimeDeps::with_jobs`; 3 manager tests | pass | Cache is keyed by connection ID/revision; `OnceCell` coalesces concurrent construction; production SQLite wiring installs the manager. |
| Native/provider compatibility factory | `ProviderConnectionFactory` tests | pass | OpenAI, Anthropic, Google, Azure OpenAI, and generic OpenAI-compatible construction are supported; unknown future kinds fail typed rather than falling back. |
| Preserve legacy registration/config behavior | existing provider suite and `cargo test provider` | pass | No legacy registry/config path was removed or implicitly migrated. |
| Protocol/UI integration | redacted core detail seam; no route/UI changes | pass | No protocol DTO or `/connect` surface was needed for this infrastructure-only milestone; later UI work has a stable redacted service seam. |

## 3. Production implementation evidence

### Core/domain and storage

- Added `codegg_core::provider_connections` with provider kind, endpoint,
  TLS policy, scope, lifecycle, revision, opaque secret binding, redacted
  summary/detail, and typed store errors.
- Added migration v24 for `provider_connections`, scope/state/revision
  indexes, and equivalent-record uniqueness. The row stores only metadata and
  non-secret credential-store locators.
- Added async SQLite create/get/list/list-by-scope/update/transition/disable/
  delete APIs with optimistic revision checks.

### Credential and provider runtime

- Added `codegg_providers::connection` as a compatibility adapter over the
  existing encrypted `CredentialStore`.
- Added a side-effect-free descriptor/factory seam covering the current
  native and OpenAI-compatible provider paths. Credential resolution happens
  only during manager resolution; construction does not probe endpoints.
- Added `ConnectionManager` under daemon core with revision-keyed caching,
  concurrent construction coalescing, invalidation, and typed disabled,
  missing, unavailable, and construction failures.
- Production `CoreRuntimeDeps::with_jobs` installs the manager when the
  existing credential store can be opened; legacy/test constructors remain
  source-compatible and do not acquire a user credential store.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo test -p codegg-providers
rtk cargo test -p codegg-providers connection
rtk cargo test -p codegg-core provider
rtk cargo test -p codegg-core provider_connections
rtk cargo test -p codegg-core
rtk cargo test -p codegg-protocol
rtk cargo test provider
rtk cargo test --lib core::provider_connections
rtk cargo test --test storage_migrations
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk git diff --check
```

### Results

- Formatting: pass.
- `codegg-providers`: 58 passed; focused connection filter: 8 passed.
- `codegg-core`: 177 passed; provider filter: 6 passed; connection-domain
  filter: 6 passed.
- `codegg-protocol`: 75 passed.
- Root provider compatibility filter: 50 passed.
- Root daemon manager filter: 3 passed.
- Storage migration integration test: 1 passed, including rerun after a
  deliberately injected mid-migration failure.
- All-target workspace clippy with `-D warnings`: pass.
- Core boundary and daemon-cwd guards: pass.
- Diff whitespace check: pass.
- A full all-features workspace test run was not required after the focused
  matrix and was not claimed as closure evidence.

## 5. Invariant review

- Plaintext credentials never enter the connection row, redacted detail,
  provider descriptor, or manager diagnostics. The provider test fixture
  verifies that the in-memory secret is absent from serialized metadata.
- Existing environment/config provider registration remains available and its
  provider-focused suite passes.
- Durable `ProviderConnectionId` and provider implementation IDs are distinct
  fields and are mapped explicitly at the manager boundary.
- Scope is serialized and validated, but no scope is treated as an
  authorization grant.
- Provider construction is daemon-owned through `CoreRuntimeDeps` and the
  manager; direct legacy registration remains a compatibility path.
- No synchronous health/model probe is performed by migration, listing,
  hydration, manager creation, or cache invalidation.

## 6. Failure and recovery review

- Equivalent scoped records fail the SQLite uniqueness constraint and are
  reported as a typed conflict.
- Concurrent updates use expected revisions; stale writers receive the
  current revision conflict rather than overwriting metadata.
- Concurrent construction for one connection revision is coalesced by
  `OnceCell`; invalidation removes cached revisions, while in-flight callers
  retain the instance captured for their request.
- Restart reconstruction is lazy: durable rows are read only when resolved,
  and production runtime dependencies rebuild the manager from the catalog
  and existing credential store.
- Metadata creation does not mutate the credential store. This makes the
  metadata transaction atomic without creating credential orphans; credential
  creation remains the existing explicit encrypted-store operation.
- Disabled, credential-missing, missing-account, expired, invalid-reference,
  and missing-master-key paths are typed and actionable. Unknown provider
  kinds fail closed with a construction error.

## 7. Migration and compatibility review

- Migration v24 is additive and idempotent; the existing v1–v23 migration
  chain remains intact.
- Existing `register_builtin`, `register_builtin_with_config`, environment
  fallback, inline/encrypted config, and user-store behavior remain unchanged.
- Legacy configuration is not automatically imported when endpoint/account
  identity would be ambiguous. The new descriptor/factory is the explicit
  compatibility seam for unambiguous future imports.
- No session model-string migration or protocol version change was made.
- Hard delete is revision-guarded and currently removes metadata only; the
  separately owned credential record is not guessed at or deleted implicitly.

## 8. Security review

- Endpoint parsing rejects credentials in URLs, query/fragment material,
  unsupported schemes, and TLS-policy mismatches before persistence.
- Credential lookup matches provider and account exactly and never falls back
  to another account. Existing encrypted-store master-key semantics are
  preserved.
- `SecretRef` has redacted `Debug`/`Display` behavior; redacted summaries and
  descriptors omit resolved secrets and encrypted payloads.
- Errors expose only provider/account identifiers and bounded reasons; no
  credential value, prefix, suffix, or length is logged or serialized.
- Scope remains metadata-only until the later authorization milestone.

## 9. Documentation and operations

Updated:

- `architecture/provider.md` — durable connection ownership, caching, and
  legacy registry compatibility;
- `architecture/auth.md` — opaque references and exact credential lookup;
- `architecture/config.md` — explicit legacy configuration compatibility;
- `architecture/storage.md` — migration v24 and connection schema;
- `plans/implementation/provider-connections/001-connection-foundation.md` —
  implemented status and closure link.

Operational/static evidence is recorded in section 4. No user-facing command,
health-probe scheduler, or rotation recovery procedure is introduced in this
milestone.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Eggpool `/connect` UI, endpoint probing, health/model refresh, session connection-ID migration, and team authorization are not implemented. | The milestone provides infrastructure rather than the final user capability. | Implement in the next provider-connections milestones; do not reopen this foundation scope. |
| low | `ProviderKind::Other` is persisted safely but has no default construction factory. | Future provider presets require an explicit compatibility factory. | Add the factory when the provider preset is accepted; unknown kinds currently fail closed. |

No critical or high-severity finding remains. The unresolved items are either
explicit source-plan non-goals or bounded extension points and do not prevent
closure of this infrastructure milestone.

## 11. Roadmap disposition

Milestone 1 is closed and its infrastructure dependency may be consumed by
the next Eggpool workflow milestone. The provider-connections roadmap remains
active because the user-visible `/connect` capability and later session,
health, rotation, and authorization work are not part of this closure.

## 12. Registry updates

- Mark the source implementation plan `implemented` and link this closure
  record.
- Mark Provider Connections Milestone 1 `closed` in
  `plans/subsystems/provider-connections-roadmap.md`.
- Remove Milestone 1 from the active dependency-ready plan table.
- Record this closure under recently closed work in `plans/registry.md`.
- Advance the provider-connections roadmap pointer to the unregistered
  Milestone 2 Eggpool connect workflow; no new handoff plan is claimed here.
