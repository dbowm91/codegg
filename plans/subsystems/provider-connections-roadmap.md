# Provider Connections and Eggpool Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md` — provider connection, secret reference, scope
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`

Related ADRs:

- None required initially. The canonical specification already chooses daemon-owned provider connections with secret references and scoped sharing.

## 1. Purpose and ownership boundary

This subsystem owns durable provider-connection identity, endpoint and capability metadata, credential references, provider health/model discovery, connection lifecycle, scope, and the `/connect` operator workflow. Eggpool is the first explicit shared connection type and should be implemented through the generic OpenAI-compatible provider machinery where protocol-compatible.

It consumes typed project/provider identities, the existing provider registry, authentication and credential-store primitives, config loading, session model selection, and protocol/TUI command surfaces. It must not own model routing policy inside Eggpool, team membership enforcement, general project catalog behavior, or a replacement secret-management product.

## 2. Work classification

### Invariants

- Provider credentials are daemon-owned and never duplicated into frontend state.
- Secrets never appear in project configuration, protocol events, logs, diagnostics, chat, or audit metadata.
- Provider connection IDs are stable and distinct from provider implementation IDs.
- Connection scope is explicit: personal, project, or deployment.
- Existing direct provider configuration remains functional during migration.
- Health/model discovery is bounded and cannot stall daemon startup indefinitely.

### Capabilities

- `/connect` offers Eggpool with host, default port `11300`, TLS policy, API key, display name, and scope.
- Several sessions and TUIs can share one connection.
- Operators can inspect redacted health, endpoint, model catalog, scope, and diagnostics.
- Credentials can be rotated or removed without rewriting every session.

### Infrastructure

- `ProviderConnection` domain model and store.
- Secret-reference integration over existing credential storage.
- Connection manager and provider-instance cache.
- Protocol DTOs/events and session selection by connection ID.
- Compatibility adapter for legacy provider config.

### Polish

- Clear connection testing and failure diagnostics.
- Health refresh and stale-state indicators.
- Configuration migration tooling and documentation.

## 3. Non-goals

- Reimplementing Eggpool routing, accounting, fallback, or model policy.
- Building team authorization in this phase; scopes must be authorization-ready but enforcement lands with the principal model.
- Replacing all environment-variable provider credentials immediately.
- Storing plaintext API keys in SQLite.
- Exposing provider secrets to ACP or remote TUI clients.
- Creating provider-specific session types.

## 4. Current state

`crates/codegg-providers` already provides a unified `Provider` trait, `ProviderRegistry`, OpenAI-compatible provider support, model discovery and ping seams, and authentication primitives. `auth_types.rs` includes `Credential`, `AuthConfig`, `AuthResolver`, encrypted/stored credential support, masking, and conventional environment fallback. This is a strong implementation base.

Provider registration is currently implementation/config oriented: providers are registered under provider IDs, credentials are resolved from environment/config/store, and sessions select models by provider/model strings. There is no durable connection record representing one configured endpoint and scope, no shared connection identity, and no session reference to a connection ID.

The server provider route lists registered providers, while the TUI/provider UX does not yet provide the required Eggpool connection workflow. Legacy config can contain direct API key/base URL data, so migration and redaction require explicit review.

## 5. Target architecture

Introduce a durable `ProviderConnection` record with:

- `ProviderConnectionId`;
- provider kind (`eggpool`, built-in, generic OpenAI-compatible, etc.);
- display name;
- normalized endpoint and TLS policy;
- `SecretRef` or credential-store account reference;
- scope and owner/project references;
- provider capability summary;
- health state, last probe time, and bounded diagnostics;
- discovered model catalog revision;
- lifecycle state (active, disabled, credential-missing, deleted).

A daemon-owned connection manager resolves records to provider instances, caches safe runtime clients, refreshes health/model metadata, and invalidates instances on credential rotation or endpoint change. Sessions refer to a connection ID plus model ID; compatibility model strings are resolved through a migration adapter.

Eggpool should be a named preset over generic compatible transport, with normalized URL construction and default port 11300. Endpoint probing must be explicit, timed out, cancellable, and redacted.

## 6. Dependency graph

```text
Milestone 1: provider connection domain, storage, and secret references
        |
        v
Milestone 2: Eggpool connect workflow and bounded probing
        |
        +--> Milestone 3: session selection and compatibility migration
        |           |
        |           v
        `--> Milestone 4: lifecycle, rotation, health, and closure
```

- Milestone 1 has a hard dependency on Domain Identity Milestone 1 for `ProviderConnectionId` and project scope types.
- Milestone 2 has a hard dependency on Milestone 1.
- Milestone 3 has hard dependencies on Milestones 1–2 and an interface dependency on session DTO evolution.
- Milestone 4 has hard dependencies on Milestones 2–3.

## 7. Milestones

### Milestone 1 — Durable connection and secret-reference foundation

Class: infrastructure

Objective: create stable provider connection records, stores, scopes, and daemon runtime resolution without adding Eggpool UI yet.

Dependencies: hard on typed identity foundation.

Deliverable boundary: core/domain types, additive schema, store, secret reference adapter using existing credential storage, connection manager seam, redacted DTOs, and compatibility import from current provider config.

User or operator value: provider endpoints become reusable daemon resources instead of frontend/config fragments.

Exit conditions:

- records persist stable IDs and never persist plaintext secrets;
- personal/project/deployment scopes serialize and validate;
- redacted list/get APIs expose no credential material;
- existing configured providers can be represented through compatibility records or adapters;
- concurrent resolution of one connection reuses or safely constructs one runtime instance.

Deferred work: Eggpool UX and session migration.

### Milestone 2 — Eggpool `/connect` workflow

Class: capability

Objective: add an explicit, tested Eggpool connection flow with default port 11300 and bounded validation.

Dependencies: hard on Milestone 1.

Deliverable boundary: command/dialog flow, host/port/TLS/API-key/display-name/scope inputs, endpoint normalization, credential-store write, timed health/model probe, rollback on failure, and structured diagnostics.

Exit conditions:

- omitted port resolves to 11300;
- explicit ports and TLS policies normalize deterministically;
- invalid credentials and unavailable endpoints return actionable redacted errors;
- successful connect creates one durable record and discoverable model catalog;
- cancelling the workflow creates no partial plaintext or orphaned active record.

Deferred work: broad provider-management UI.

### Milestone 3 — Session and model selection by connection

Class: infrastructure

Objective: move session/provider runtime selection from provider implementation IDs and frontend-owned credentials to stable connection IDs.

Dependencies: hard on Milestones 1–2.

Deliverable boundary: session fields/DTOs, model catalog projection, selection commands, runtime resolution, migration/fallback for legacy `provider/model` values, and multi-TUI concurrency.

Exit conditions:

- several sessions and clients share one Eggpool connection safely;
- connection/model selection remains project-correct;
- legacy sessions resolve or produce actionable migration diagnostics;
- disabling/deleting a connection does not silently select another credentialed endpoint.

Deferred work: role-based access enforcement.

### Milestone 4 — Rotation, health, deletion, and closure

Class: capability

Objective: complete connection lifecycle correctness and closure evidence.

Dependencies: hard on Milestones 2–3.

Deliverable boundary: credential rotation, instance invalidation, health refresh, model revision changes, disable/delete semantics, active-session behavior, documentation, and closure tests.

Exit conditions:

- rotation takes effect for new requests without exposing secrets;
- in-flight requests have defined behavior and do not switch credentials mid-request;
- deletion/disable is explicit and recoverable where policy allows;
- health probes are bounded and do not overload Eggpool;
- all Phase 2 exit criteria are evidenced.

## 8. Cross-cutting requirements

### Storage and migration

Use additive schema and stable IDs. Credential material remains in the existing protected credential store or a clearly abstracted secret backend; records store only references. Legacy config remains readable until explicit removal criteria are met.

### Protocol and compatibility

Expose redacted connection summaries, health, model catalogs, create/test/rotate/disable/delete operations, and capability flags. Never serialize resolved credentials. Old model strings require deterministic compatibility resolution.

### Security and authorization

Validate endpoint schemes, hosts, ports, redirects, and TLS policy. Prevent secret logging in HTTP client errors. Future authorization must be able to restrict project/deployment connections.

### Concurrency, cancellation, and recovery

Connection creation and rotation are transactional. Concurrent probes and model refreshes should coalesce. Runtime instance replacement uses generation/version checks. Restart reconstructs instances lazily from records and secret references.

### Observability and audit

Record connection ID, provider kind, endpoint authority, scope, health outcome, duration, and credential source class—not secret values. Provide seams for later audit actor attribution.

### Performance and resource use

Do not probe every connection synchronously during daemon startup. Use lazy or bounded background health checks and cap model catalog size/payload.

### Documentation and operations

Update provider, auth, config, protocol, session, server, and TUI docs. Document Eggpool endpoint examples and rotation behavior.

## 9. Verification strategy

Use fake OpenAI-compatible/Eggpool test servers, timeout and redirect tests, encrypted credential fixtures, concurrent session tests, migration fixtures, redaction snapshots, rotation races, daemon restart tests, and model-catalog bounds.

## 10. Risks and decision points

- Existing credential-store encryption depends on master-key configuration. The plan must preserve current failure behavior and avoid silent plaintext fallback.
- Eggpool API details may evolve. Keep the preset narrow and transport-compatible rather than spreading assumptions.
- Session migration can be ambiguous when several connections share one provider ID. Require explicit selection rather than guessing.
- If secret backends need pluggability beyond current storage, record an ADR before broadening scope.

## 11. Completion definition

This roadmap closes when Eggpool and other endpoints can be represented as reusable, daemon-owned, secret-safe provider connections; sessions select them by stable identity; health/model discovery is bounded; lifecycle operations are correct; and legacy configuration has an explicit migration path.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/provider-connections/001-connection-foundation.md` | `plans/closure/provider-connections/001-status.md` | — |
| 2 | not started | — | — | — |
| 3 | not started | — | — | Milestones 1–2 closure |
| 4 | not started | — | — | Milestones 2–3 closure |
