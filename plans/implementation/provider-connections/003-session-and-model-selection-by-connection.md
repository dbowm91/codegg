# Provider Connections Milestone 003 — Session and Model Selection by Connection

Status: implemented

Implementation commit: `efe1995`

Closure record:

- `plans/closure/provider-connections/003-status.md`

Repository baseline: `8c1675c` (Provider Connections Milestone 002 closure; see `plans/closure/provider-connections/002-status.md`)

Source roadmap:

- `plans/subsystems/provider-connections-roadmap.md#milestone-3--session-and-model-selection-by-connection`

Long-term requirements:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`

Applicable closure evidence:

- `plans/closure/provider-connections/001-status.md`
- `plans/closure/provider-connections/002-status.md`

Primary class: infrastructure

## 1. Objective

Move session/provider runtime selection from frontend-owned provider/model
strings to stable daemon-owned `ProviderConnectionId` plus model ID, while
preserving deterministic compatibility for legacy sessions and leaving
credential ownership in the connection manager.

This milestone consumes the durable Eggpool connection and revisioned model
catalog created by Milestone 002. It must not add a second provider runtime or
silently select a different credentialed endpoint.

## 2. Why this milestone is ready

- Provider Connections Milestone 001 is closed with durable connection,
  secret-reference, scope, store, and manager contracts.
- Provider Connections Milestone 002 is closed with local `/connect`, bounded
  probing, health/model catalog persistence, cancellation, restart
  compensation, and redacted protocol/listing seams.
- Session and model selection remains on the legacy path, which is the exact
  bounded gap this milestone owns.

The project-aware TUI and catalog tracks are related but not hard blockers for
personal-local connection selection. Project-correct selection must use the
authoritative project/session context when it becomes available; this plan
must not derive a project identity from a filesystem path.

## 3. Current implementation evidence

- `ProviderConnectionId` and connection revisions are durable and typed.
- `ProviderConnectionSummaryDto` and bounded model catalog DTOs are exposed
  by core operations.
- `ConnectionManager` resolves provider instances by connection ID/revision.
- Sessions still persist/select legacy provider/model values and have no
  connection reference or migration diagnostic.
- The provider-connections roadmap reserves role-based authorization and
  lifecycle/rotation work for later milestones.

## 4. Invariants that must not regress

- A session resolves only the selected connection ID and model revision; it
  never receives a different credentialed endpoint through fallback.
- Provider implementation IDs remain distinct from durable connection IDs.
- Existing legacy sessions continue to load and produce either a deterministic
  compatibility resolution or an actionable migration diagnostic.
- Connection scope is enforced as context, not treated as authorization by
  string parsing alone.
- Model catalogs are bounded, revision-aware, and not synchronously probed on
  daemon startup.
- Multi-client/session selection is revision-safe and does not mutate another
  session's selection.
- Resolved secrets remain inside daemon/provider runtime boundaries.

## 5. Scope

### In scope

- Session storage fields and protocol DTOs for optional connection ID,
  selected model ID, and catalog/revision diagnostics.
- Daemon-owned selection/listing commands and runtime resolution.
- Legacy `provider/model` compatibility adapter and explicit migration/failure
  diagnostics.
- Connection/model projection to local and remote clients without secrets.
- Project/session context validation and concurrent client behavior.
- Additive migration, documentation, and closure evidence.

### Explicitly out of scope

- Credential rotation, health refresh scheduling, disable/delete UX, or
  role-based team authorization (Milestone 004).
- General provider-management UI or new provider-specific session types.
- Inferring project IDs from cwd, path, repository name, or UI labels.
- Changing Eggpool routing, accounting, fallback, alias, or load-balancing
  policy.
- Removing legacy configuration before deterministic compatibility evidence is
  complete.

## 6. Required production changes

### Core/domain and storage

- Add optional typed connection/model selection fields with additive schema
  migration and revision/foreign-key validation.
- Define an explicit legacy-resolution outcome: resolved connection, unresolved
  legacy provider, ambiguous match, disabled connection, or missing credential.
- Keep historical fields readable until removal criteria are approved.

### Protocol and runtime

- Add redacted connection/model selection requests and responses.
- Route every selected request through the daemon `ConnectionManager` using the
  selected connection revision; do not construct providers in the TUI.
- Return catalog revision and bounded diagnostics when a selected model is
  unavailable or stale.

### Frontends and concurrency

- Add local/remote selection views that show endpoint authority, display name,
  scope, health, and model IDs only.
- Ensure two sessions can select different models on one connection and two
  clients cannot overwrite each other's selection through stale revisions.
- Keep project scope unavailable or explicitly unresolved until authoritative
  project context is present.

### Security and compatibility

- Verify scope and session/project ownership at the daemon boundary.
- Never expose credential locators, resolved secrets, provider headers, or
  encrypted payloads in selection DTOs or diagnostics.
- Preserve `register_builtin`, environment/config providers, and legacy model
  behavior while emitting migration telemetry without secret values.

## 7. Ordered work packages

### Work package A — Selection model and migration

Add typed optional connection/model fields, additive migration, validation,
and deterministic legacy compatibility outcomes.

Evidence: old rows migrate unchanged; new rows round-trip; ambiguous and
disabled cases are explicit and non-fallbacking.

### Work package B — Daemon selection service

Implement daemon-owned list/select/resolve operations over connection and model
catalog revisions, including stale revision conflicts and bounded errors.

Evidence: selected requests use the exact connection ID/revision; no provider
construction or credential resolution occurs in frontend code.

### Work package C — TUI/remote projections

Expose redacted connection/model selection to local and remote clients, with
project/session context checks and multi-client concurrency behavior.

Evidence: two sessions share one connection safely, remote snapshots contain
no secret material, and project mismatch is actionable.

### Work package D — Compatibility and closure hardening

Exercise legacy providers, connection disable/missing-credential behavior,
catalog revision changes, restart reconstruction, and stale selections.

Evidence: compatibility suites pass, no silent credential fallback exists, and
the next lifecycle milestone can consume one stable selection contract.

## 8. Failure, cancellation, restart, and contention semantics

- A stale selection revision returns a conflict and leaves the stored
  selection unchanged.
- A missing, disabled, or credential-missing connection returns a typed
  diagnostic; it never chooses another connection.
- A removed model returns a catalog-revision diagnostic and preserves the
  user's explicit connection choice.
- Daemon restart reconstructs connection runtime lazily from durable metadata
  and the protected credential store.
- Concurrent updates use expected revisions; independent sessions do not
  contend on one mutable global selection.
- Frontend disconnect does not mutate durable session selection.

## 9. Compatibility and migration

- The migration is additive and preserves all Milestones 001–002 tables.
- Existing `provider/model` strings remain readable through a deterministic
  adapter until an explicit removal plan is accepted.
- No automatic import is performed when multiple connection records could
  match a legacy provider/endpoint.
- Protocol additions remain optional for existing clients; unknown selection
  variants fail with bounded, explicit errors.

## 10. Required tests

### Focused and migration tests

- typed selection validation, catalog revision checks, and additive schema;
- legacy provider/model resolution, ambiguity, disabled, and missing-secret
  cases;
- no credential material in selection serialization or diagnostics.

### Integration and concurrency tests

- two sessions and two clients sharing one connection with distinct models;
- stale update conflict and project-context mismatch;
- daemon restart/lazy reconstruction;
- catalog revision change and removed-model diagnostics;
- existing provider/auth compatibility suites.

### Security and negative tests

- no silent fallback to another connection or credential;
- no path-derived project identity;
- remote projection remains secret-free;
- legacy configuration remains functional during migration.

## 11. Required verification commands

```bash
rtk cargo fmt --all -- --check
rtk cargo test -p codegg-core provider_connections
rtk cargo test -p codegg-protocol
rtk cargo test provider
rtk cargo test auth
rtk cargo test connect
rtk cargo test --test storage_migrations
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk git diff --check
```

## 12. Documentation updates

- Update provider, session, protocol, storage, TUI, and compatibility docs.
- Document legacy-resolution diagnostics and the distinction between creating
  a connection and selecting it for a session.
- Document project-context and multi-client concurrency behavior.

## 13. Acceptance criteria

- Sessions can select a stable connection ID and model without frontend-owned
  credentials.
- Legacy sessions resolve deterministically or show actionable diagnostics.
- Several sessions/clients share one connection without cross-session
  selection or credential leakage.
- Disabled/deleted/missing connections never silently select another endpoint.
- Catalog and selection revisions prevent stale overwrite.
- Existing provider/auth behavior remains functional.

## 14. Stop conditions

The agent must stop and report if:

- session migration requires changing canonical project identity ownership;
- compatibility would require choosing among multiple credentialed endpoints;
- secure remote selection cannot remain secret-free;
- rotation/health/deletion or authorization work becomes necessary to prove
  session selection, expanding into Milestone 004.

## 15. Closure evidence required

Include implementation commits, migration and protocol compatibility evidence,
legacy-resolution matrix, multi-session/client concurrency evidence, stale
revision behavior, project-context evidence, secret-redaction proof, exact
verification commands, documentation updates, unresolved findings, and a
roadmap/registry disposition for Milestone 004.

## 16. Handoff notes

- Reuse the closed Milestones 001–002 connection/domain/probe services.
- Do not place API keys in session rows, selection DTOs, TUI snapshots, or
  command arguments.
- Keep test fixtures deterministic and local; do not require a live Eggpool.
- Coordinate with Project Catalog and Multi-Project TUI plans where their
  interfaces are needed, without making path-derived identity assumptions.
