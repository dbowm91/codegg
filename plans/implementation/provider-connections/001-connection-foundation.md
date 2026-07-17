# Provider Connections Milestone 001 — Durable Connection Foundation

Status: implemented

Implementation commit: `bccca00` (`feat: add durable provider connections foundation`)

Closure record:

- `plans/closure/provider-connections/001-status.md`

Repository baseline: `f203ed9` (typed identity foundation implementation and closure committed; subsequent planning-only commits do not alter implementation state)

Source roadmap:

- `plans/subsystems/provider-connections-roadmap.md#milestone-1--durable-connection-and-secret-reference-foundation`

Long-term requirements:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`

Applicable ADRs:

- None. Stop if a new general secret-storage architecture or materially different provider ownership model becomes necessary.

Primary class: infrastructure

## 1. Objective

Introduce durable, daemon-owned provider connection records, scopes, secret references, storage, and runtime resolution seams while preserving existing provider registration/configuration behavior. This milestone does not yet implement the Eggpool `/connect` UI; it creates the stable substrate that workflow will use.

## 2. Why this milestone is ready

The hard dependency is now closed:

- Domain Identity Milestone 001 closed with `ProviderConnectionId`, `ProjectId`, and compatible typed-ID contracts in `f203ed9`.

Implementation may begin from the reviewed `f203ed9` baseline. The agent must
not substitute arbitrary strings or path-valued project IDs.

## 3. Current implementation evidence

- `crates/codegg-providers` exposes a unified `Provider` trait, `ProviderRegistry`, built-in and config registration, OpenAI-compatible providers, model discovery, and ping/health seams.
- `crates/codegg-providers/src/auth_types.rs` already defines `Credential`, `AuthConfig`, `AuthResolver`, masking, encrypted/stored credential support, and a `CredentialStore` seam.
- Provider registry entries are keyed by implementation/provider ID, not durable configured connection identity.
- Sessions and model selection primarily use `provider/model` strings and runtime registry lookup.
- Provider server routes expose registered providers but not durable connection lifecycle or scope.
- Legacy config can contain inline/encrypted API key and endpoint data.

At handoff, the known gap was the absence of a stable `ProviderConnection`
record, explicit scope, secret-reference adapter, daemon connection manager,
and connection-identity compatibility seam. Those infrastructure gaps are
closed by implementation commit `bccca00`; Eggpool UI, session migration,
authorization, health probing, and rotation remain the explicit follow-up
scope described below.

## 4. Invariants that must not regress

- Plaintext credentials never enter SQLite connection records, protocol DTOs, logs, diagnostics, or frontend state.
- Existing environment/config provider registration keeps working.
- Provider implementation ID and connection ID remain distinct concepts.
- Scope is explicit and serializable.
- Runtime provider construction is daemon-owned.
- Health/model probing is not added to synchronous daemon startup in this milestone.
- No team authorization is claimed merely because scopes exist.

## 5. Scope

### In scope

- `ProviderConnection` domain record and lifecycle state.
- `ProviderScope` with personal, project, and deployment variants.
- Stable `ProviderConnectionId` use.
- Endpoint/TLS/provider-kind metadata model.
- `SecretRef` or account-reference adapter over the existing credential store.
- Additive storage schema and store/service APIs.
- Daemon connection manager seam that resolves records to provider instances lazily.
- Redacted internal/protocol summary types sufficient for later UI work.
- Legacy config compatibility/import adapter design and initial implementation where safe.
- Focused tests and architecture docs.

### Explicitly out of scope

- Eggpool command/dialog and endpoint probing.
- Session migration to connection IDs.
- Team role/capability enforcement.
- OIDC or human authentication.
- Replacing the existing credential store.
- Removing direct provider config/environment behavior.
- Periodic health checks and credential rotation UX.

## 6. Required production changes

### Core/domain

Define records/enums for:

- provider connection ID;
- provider kind/preset;
- endpoint and TLS policy;
- display name;
- secret reference/account ID;
- scope and owner/project references;
- lifecycle state;
- capability/model/health summary placeholders;
- record generation/revision for runtime invalidation.

The record must not contain resolved secret material.

### Storage and migrations

Add an additive, idempotent schema migration for provider connections. Index scope/project/owner and lifecycle fields. Store endpoint metadata and secret reference only. Define store methods for create/get/list/update/disable/delete metadata, with uniqueness and optimistic revision semantics where appropriate.

Do not migrate every legacy config entry automatically if credential/reference mapping is ambiguous. A compatibility adapter may synthesize ephemeral legacy connections or explicitly import only when safe.

### Protocol and DTOs

Add redacted connection summary/detail DTOs and capability flags only if needed for service tests or future compatibility. Never serialize secret references that reveal protected storage layout beyond opaque IDs.

### Runtime and concurrency

Create a daemon-owned connection manager that:

- loads metadata lazily;
- resolves credentials through existing auth/credential abstractions;
- constructs compatible provider instances;
- caches by connection ID plus record revision;
- coalesces concurrent construction where practical;
- invalidates safely on metadata/credential generation change;
- returns typed unavailable/credential-missing/disabled errors.

No network probe should occur simply from listing or hydrating records.

### Frontend or operator surface

No end-user connect flow yet. Provide redacted service list/get behavior and developer/operator diagnostics suitable for the next milestone.

### Security and authorization

- Ensure all `Debug`, serde, and error paths redact secret material.
- Validate endpoint scheme/authority metadata but defer live SSRF/redirect probe policy to the Eggpool milestone.
- Treat scope as metadata only until authorization lands.
- Protect secret reference lookup from accidental cross-connection fallback.

### Documentation and static guards

Update provider/auth/config architecture. Add tests or guards ensuring connection DTOs and records do not contain common secret fields or resolved credential strings.

## 7. Ordered work packages

### Work package A — Connection domain contract

Intent: define stable metadata and scope before persistence.

Required changes:

- add domain records/enums;
- define endpoint normalization representation without network I/O;
- define redacted summary conversion;
- define revision/lifecycle behavior.

Acceptance evidence:

- serde/validation tests;
- debug/redaction snapshots;
- project-scoped records require a project ID while personal/deployment records validate appropriately.

### Work package B — Secret reference integration

Intent: reuse protected credential storage without embedding secrets.

Required changes:

- define opaque `SecretRef`/account-reference adapter;
- map existing `CredentialStore` records to connections;
- establish missing/expired/master-key error behavior;
- prevent plaintext fallback during connection persistence.

Acceptance evidence:

- encrypted/stored credential fixtures;
- database/config snapshots contain no secret;
- missing master key follows current explicit failure semantics.

### Work package C — Storage and service layer

Intent: persist connection metadata and expose deterministic lifecycle APIs.

Required changes:

- additive schema/store;
- create/get/list/update/disable/delete metadata methods;
- uniqueness and revision handling;
- restart hydration tests;
- compatibility seam for legacy configured providers.

Acceptance evidence:

- idempotent migration;
- concurrent create/update behavior defined;
- archived/disabled records do not resolve as active.

### Work package D — Daemon runtime resolution seam

Intent: construct provider instances from connection identity rather than frontend credentials.

Required changes:

- connection manager in daemon runtime dependencies;
- lazy credential resolution and provider construction;
- cache/invalidation keyed by record revision;
- typed errors and redacted diagnostics;
- preserve existing registry path through compatibility adapter.

Acceptance evidence:

- multiple callers resolving one connection receive equivalent safe runtime behavior;
- disabled/missing-credential connections fail actionably;
- no startup-wide probe or secret exposure.

## 8. Failure, cancellation, restart, and contention semantics

- Connection record creation is transactional with secret-reference creation; if either fails, no active orphan record remains.
- A failed runtime construction leaves metadata intact and reports health/unavailable state without deleting credentials.
- Concurrent construction for one revision coalesces or safely duplicates without corrupting cache state.
- Daemon restart lazily reconstructs runtime providers from durable records.
- Record revision change invalidates future resolutions; in-flight provider requests keep the instance/credential captured at request start.
- Delete/disable semantics must be explicit; this milestone may restrict hard delete when referenced.

## 9. Compatibility and migration

- Existing `register_builtin_with_config` and environment fallback remain operational.
- Define how legacy provider IDs map to synthesized or imported connections, but do not guess among several endpoints with the same provider ID.
- String model selection remains unchanged in this milestone.
- Any additive DTOs require capability negotiation and redacted fixtures.
- Document explicit removal criteria for inline credential config.

## 10. Required tests

### Focused unit tests

- scope validation;
- endpoint/TLS metadata normalization;
- redacted summary/debug output;
- lifecycle/revision transitions;
- secret reference round trips without secret values.

### Integration tests

- SQLite store CRUD and indexes;
- credential-store resolution;
- daemon manager lazy construction using fake providers;
- several consumers resolving one connection.

### Restart and recovery tests

- migration idempotence;
- restart hydration and lazy cache rebuild;
- interrupted create/update leaves no active partial record.

### Contention and cancellation tests

- concurrent create of equivalent records;
- concurrent resolution/invalidation;
- update while requests are in flight.

### Security and negative tests

- secret strings absent from serialized records/DTOs/log-like errors;
- missing master key, expired credential, invalid secret reference;
- disabled/deleted record resolution;
- no cross-account credential fallback.

### Migration and compatibility tests

- existing config/provider registry tests remain green;
- compatibility adapter for one unambiguous legacy provider;
- ambiguous legacy configurations remain explicit and non-destructive.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers
cargo test -p codegg-core provider
cargo test -p codegg-protocol
cargo test provider
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Add focused migration and daemon connection-manager integration targets and run them explicitly.

## 12. Documentation updates

- `architecture/provider.md`;
- provider auth/credential architecture documentation;
- storage migration index;
- protocol capability notes if DTOs are added;
- config compatibility and secret-redaction guidance.

## 13. Acceptance criteria

- Durable provider connection metadata exists under stable IDs and explicit scope.
- Plaintext credentials are absent from connection storage and transport.
- Daemon runtime can resolve one record to a provider instance lazily.
- Existing provider registration/config continues working.
- Restart, concurrent resolution, disabled state, and missing credential behavior are tested.
- The next Eggpool workflow can create/test a connection without defining a new storage model.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- typed provider/project IDs are unavailable;
- existing credential storage cannot support opaque references without plaintext fallback;
- implementing this requires a general external secret-manager plugin system;
- legacy provider migration is ambiguous and would silently change endpoints/credentials;
- scope expands into Eggpool networking, session migration, or team authorization;
- provider construction cannot be daemon-owned without a canonical runtime dependency change requiring an ADR.

## 15. Closure evidence required

- implementation commit(s);
- schema/migration evidence;
- secret redaction matrix covering storage, DTOs, errors, and debug output;
- connection lifecycle and runtime-resolution tests/results;
- compatibility evidence for existing providers;
- exact commands run;
- documented blockers/deferred Eggpool/session work;
- closure recommendation.

## 16. Handoff notes

- Domain Identity Milestone 001 is closed; this plan is dependency-ready from
  the `f203ed9` baseline.
- Reuse existing auth primitives rather than creating parallel credential types without evidence.
- Do not add synchronous network probes to daemon startup.
- Treat planning-only commits after the baseline as non-production changes, but inspect current `main` before implementation.
