# Provider Connections Milestone 004 — Lifecycle, Rotation, Health, and Closure

Status: ready for handoff

Repository baseline: `3ce0a7ea7c1a8baa41a2618eb293291435e9f9f0` (`main`; planning-only commits after this baseline do not alter production behavior)

Production implementation baseline:

- `bccca00` — durable provider connections, secret references, revisioned lifecycle, and lazy daemon manager.
- `8c1675c` — Eggpool `/connect` provisioning, bounded probe, redacted model catalog, and cancellation cleanup.
- `213783e` — session/model selection by stable connection ID and catalog revision.

Source roadmap:

- `plans/subsystems/provider-connections-roadmap.md#milestone-4--rotation-health-deletion-and-closure`

Long-term requirements:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool`
- `plans/001-terminology-and-domain-model.md` — ProviderConnection, SecretRef, lifecycle, revision, model catalog, scope, and session selection.
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`

Applicable closure evidence:

- `plans/closure/provider-connections/001-status.md`
- `plans/closure/provider-connections/002-status.md`
- `plans/closure/provider-connections/003-status.md`

Applicable ADRs:

- None. The canonical documents already decide daemon ownership, secret references, bounded probes, revisioned selection, and no silent fallback. Stop for an ADR if implementation requires a new secret backend or materially changes connection lifecycle semantics.

Primary class: capability

## 1. Objective

Complete provider-connection lifecycle correctness and close the provider/Eggpool roadmap by implementing secret-safe credential rotation, bounded health/model refresh, explicit enable/disable/delete behavior, active-session diagnostics, runtime-instance invalidation, and operator-facing lifecycle controls.

The milestone succeeds when new requests adopt a rotated credential/endpoint revision without changing credentials mid-request, health/model refresh is bounded and coalesced, disabling/deleting a connection never silently falls back to another endpoint, active sessions receive actionable state, and all Phase 2 acceptance criteria have closure evidence.

This milestone includes the related verification debt recorded by Milestone 003: an end-to-end fake-daemon/TUI lifecycle harness, richer stale-state reconciliation outcomes, the `SessionSelectionDto` size warning if lifecycle additions would worsen it, and the flaky Eggpool provisioning timing test.

## 2. Why this milestone is ready

All hard dependencies are closed:

- Milestone 001 provides durable revisioned connection records, secret references, lifecycle state, and daemon runtime caching.
- Milestone 002 provides provisioning transactions, health/model persistence, bounded Eggpool probes, and local-only secret transport.
- Milestone 003 provides session selection by connection/model revision and typed no-fallback compatibility outcomes.

No later project, identity, or TUI roadmap dependency is required for personal/local lifecycle correctness. Project/deployment scopes remain metadata and authorization-ready; role enforcement remains a later identity/authorization phase.

## 3. Current implementation evidence

At the repository baseline:

- `ProviderConnectionStore` persists stable IDs, scope, endpoint/TLS metadata, lifecycle, revisions, opaque secret bindings, health, model catalogs, and provisioning state.
- `ConnectionManager` lazily constructs and caches provider instances by connection ID/revision and can invalidate cached revisions.
- Eggpool provisioning stores credentials through the protected `CredentialStore`, probes with bounded timeouts, persists redacted health/model data, and cleans up cancelled/failed provisioning.
- Sessions carry optional `provider_connection_id`, connection revision, model catalog revision, and selected model ID.
- `SelectionService` rejects stale revisions/catalogs and disabled/deleted connections without silent fallback.
- `/connections` provides redacted selection UI, while `/connect` provisions a new Eggpool connection.
- There is no complete rotation transaction, lifecycle-management UI, bounded refresh coordinator, periodic/backoff policy, active-session lifecycle projection, or deletion reference policy.
- Milestone 003 recorded that stale-revision outcomes omit current selection fields, `SessionSelectionDto` may need boxing/splitting, and there is no true keyboard-driven fake-daemon TUI harness.
- `core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog` intermittently fails under broad parallel load despite passing focused runs.

## 4. Invariants that must not regress

- Plaintext credentials remain confined to bounded secret-input memory and protected credential-store operations.
- Secrets never enter SQLite connection metadata, protocol DTOs, TUI snapshots, logs, diagnostics, project configuration, chat, or audit metadata.
- A connection ID and provider implementation ID remain distinct.
- In-flight requests retain the provider instance/revision captured at request start.
- New requests use the latest successfully committed active revision.
- Rotation or refresh failure leaves the previous valid active connection usable unless the operator explicitly disables it.
- Disable/delete never silently selects another connection or credential.
- Health/model probing remains explicit or bounded background work and cannot stall daemon startup.
- Concurrent rotation/refresh/lifecycle operations are revision-safe and coalesced where appropriate.
- Scope metadata does not itself grant authorization.

## 5. Scope

### In scope

- Credential and endpoint rotation transaction.
- Revisioned runtime cache invalidation and in-flight pinning semantics.
- On-demand health/model refresh.
- Bounded optional background refresh policy with backoff/jitter and global/per-endpoint concurrency caps.
- Model-catalog diffing and deterministic revision changes.
- Enable/disable, soft-delete/tombstone, restore where policy allows, and explicit purge eligibility.
- Active-session behavior and typed lifecycle diagnostics.
- Additive protocol operations/DTOs for rotate, refresh, enable/disable, delete/restore, and status.
- `/connections` lifecycle actions and local-only secret input.
- End-to-end fake-daemon/TUI harness for connect/select/refresh/rotate/disable/delete flows.
- Stale-state reconciliation improvements.
- Provider-related flaky-test stabilization.
- Closure matrix for the complete provider-connections roadmap.

### Explicitly out of scope

- Team ACL enforcement or project membership policy.
- Replacing the credential store or introducing a pluggable secret backend without an ADR.
- Reimplementing Eggpool routing, accounting, or fallback.
- Generic provider marketplace/configuration UI beyond lifecycle actions for durable connections.
- Automatic migration of ambiguous legacy provider configs.
- Cross-node secret distribution.
- Project catalog/TUI multi-project work.

## 6. Required production changes

### Rotation transaction

Add a daemon-owned rotation workflow supporting credential-only rotation and endpoint/TLS/display metadata changes where allowed.

Required sequence:

1. validate connection ID, expected revision, lifecycle, and requested metadata;
2. accept secret input through the existing local-only secret transport;
3. write a staged credential record under a new opaque binding/account reference without replacing the active binding;
4. construct/probe a staged provider instance with bounded timeout and cancellation;
5. discover and validate a bounded model catalog;
6. transactionally update connection metadata/binding/health/catalog and increment revision;
7. invalidate future resolution of older cached revisions;
8. preserve any already-captured old runtime for in-flight requests;
9. retire/remove the prior credential only after the new revision commits and according to an explicit cleanup policy;
10. on any pre-commit failure, remove staged metadata/credential and retain the prior active revision.

Do not overwrite the active credential in place before validation. Rotation errors must be typed and redacted.

### Runtime revision and in-flight behavior

- A request/turn resolves a connection once and pins the returned runtime plus revision for its lifetime.
- Cache invalidation prevents new resolutions from returning the old revision after successful rotation/endpoint update.
- An in-flight request using the old revision may complete normally unless explicitly cancelled for security policy; document the chosen default.
- A failed or cancelled rotation does not invalidate the active runtime.
- Concurrent rotations use expected revision and one winner; stale callers receive current redacted state.
- Restart reconstructs only the committed active revision lazily.

### Health and model refresh coordinator

Implement a daemon-owned single-flight refresh path per connection with:

- explicit manual refresh operation;
- optional bounded background refresh disabled or conservative by default;
- connection and global concurrency caps;
- connect/read/overall timeouts;
- exponential backoff with jitter after failures;
- no synchronous startup probe;
- cancellation;
- redirect/TLS/endpoint policy identical to provisioning;
- redacted error classification;
- bounded model count, field lengths, and payload bytes;
- deterministic catalog normalization/digest/revision;
- no catalog revision change when normalized content is unchanged;
- health timestamps/status transitions and stale threshold;
- coalescing of concurrent selection/UI refresh requests.

A failed refresh updates bounded health diagnostics according to policy but must not erase the last valid model catalog or active runtime.

### Lifecycle state machine

Define and enforce an explicit state machine over active, disabled, credential-missing, provisioning/rotating, deleted/tombstoned, and error/stale states.

Required behavior:

- disable prevents new provider resolution and new selections;
- existing sessions retain their explicit selection but resolve to typed disabled/unavailable status rather than fallback;
- enable requires a valid credential/binding and may optionally require a successful probe according to documented policy;
- delete defaults to logical tombstone/soft delete so references remain explainable;
- restore is supported when metadata/credential policy allows;
- hard purge is an explicit administrative operation only when no session/reference/provisioning/runtime dependency remains, or otherwise returns typed blockers;
- credential removal transitions affected connections to credential-missing without deleting identity/history;
- lifecycle transitions use expected revision and are idempotent where appropriate.

### Session and selection behavior

Extend selection outcomes/projections so clients can reconcile lifecycle changes without guessing.

At minimum:

- stale revision/catalog outcomes include current redacted connection revision, catalog revision, and current selected model/connection state where bounded;
- selected sessions surface disabled, deleted, credential-missing, unhealthy/stale, or removed-model state;
- removed models preserve the connection choice and require explicit replacement;
- active sessions do not silently clear or change selection;
- turn submission fails with a typed actionable connection-state error before model invocation when the selected connection is unusable;
- no implicit selection of another active connection occurs.

If lifecycle additions enlarge `SessionSelectionDto`, box or split the large variant rather than adding more unboxed payload weight.

### Protocol and DTOs

Add redacted additive operations equivalent to:

- connection get/detail/status;
- rotate begin/cancel/status/result;
- health/model refresh begin/cancel/status/result;
- enable/disable;
- delete/restore;
- optional purge with blockers;
- lifecycle event/projection updates.

Secret-bearing rotate input remains local-only and must be rejected over disallowed remote transports using the existing Eggpool secret-transport policy.

DTOs may contain stable IDs, display name, endpoint authority, TLS policy, scope label, lifecycle/health state, revisions, model summaries, durations, timestamps, and bounded error codes/messages. They must not contain secret references where unnecessary, credentials, encrypted payloads, auth headers, or secret-derived fingerprints.

### TUI/operator surface

Extend the existing `/connections` dialog rather than creating disconnected provider-management state.

Required actions:

- inspect redacted connection detail/status;
- manual health/model refresh;
- rotate credential and optionally endpoint metadata through masked/local-only input;
- enable/disable;
- delete/restore with confirmation and reference blockers;
- show active session/reference counts and lifecycle consequences;
- show last successful probe, stale state, current revision, catalog revision, and bounded diagnostics;
- handle stale revisions by applying current state from the outcome or refreshing automatically;
- never place secret input into normal command history, prompt text, remote snapshots, or reusable TUI state.

Add a deterministic fake-daemon/CoreClient harness that drives keyboard/action flows through connect, select, refresh, rotate, disable, and delete/restore without requiring a real terminal service or external Eggpool.

### Flaky-test and verification closure

Investigate and fix `core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog` under broad parallel load.

The fix must address synchronization/timing ownership rather than increasing arbitrary sleeps. Preferred approaches include explicit fake-server readiness barriers, deterministic clocks, operation completion signaling, or isolated test resources/ports.

Review provider-related warnings and test debt recorded by prior closures:

- `SessionSelectionDto` large enum variant;
- stale outcome reconciliation fields;
- TUI fake-daemon lifecycle coverage.

### Security and authorization

- Exact provider/account binding remains mandatory; no fallback account lookup.
- Rotation cleanup must not delete an unrelated credential record.
- Endpoint changes re-run all scheme/host/port/TLS/redirect validation.
- Secret input and errors remain redacted, including failure bodies and URLs.
- Lifecycle operations retain capability/authorization seams for later team roles but do not infer access from connection scope.
- Audit-ready events may record actor seam, connection ID, action, revisions, endpoint authority, outcome, and duration, never secrets.

### Documentation

Update at least:

- `architecture/provider.md`;
- `architecture/auth.md`;
- `architecture/session.md`;
- `architecture/protocol.md`;
- `architecture/storage.md`;
- `architecture/tui.md`;
- `architecture/core.md`;
- operator examples for Eggpool connect/rotate/refresh/disable/delete.

Document the complete lifecycle state machine, in-flight revision semantics, background refresh defaults, backoff/caps, model revision rules, deletion/reference behavior, and legacy compatibility removal criteria.

## 7. Ordered work packages

### Work package A — Lifecycle state machine and reference policy

Intent: define transitions and blockers before adding UI.

Required changes:

- formalize states/transitions/outcomes;
- implement revision-safe enable/disable/delete/restore/purge eligibility;
- add session/reference counting and blocker details;
- preserve tombstones and diagnostics.

Acceptance evidence:

- transition table tests cover valid/invalid/idempotent/stale cases;
- selected sessions never fall back;
- delete/restore preserves explainable identity/history.

### Work package B — Rotation transaction and runtime invalidation

Intent: rotate safely without disrupting in-flight work.

Required changes:

- staged credential/binding;
- bounded staged probe/catalog;
- atomic revision commit;
- old-runtime pinning/new-runtime adoption;
- cleanup/rollback and restart behavior.

Acceptance evidence:

- failed/cancelled rotation retains old active revision;
- new requests use new revision after commit;
- in-flight request finishes on captured old revision;
- concurrent rotations yield one winner and typed stale outcome.

### Work package C — Health/model refresh coordinator

Intent: provide bounded fresh state without startup stalls or probe storms.

Required changes:

- single-flight per connection;
- global/per-endpoint caps;
- timeout/backoff/jitter/cancellation;
- catalog normalization/revision diff;
- stale/health persistence and last-good preservation.

Acceptance evidence:

- concurrent refresh coalesces;
- unchanged catalog keeps revision;
- failed refresh keeps last-good catalog/runtime;
- repeated failures honor backoff and do not overload fake Eggpool.

### Work package D — Session/protocol reconciliation

Intent: make lifecycle changes visible and actionable to selected sessions/clients.

Required changes:

- richer current-state stale outcomes;
- selected-session lifecycle projection;
- turn-submit typed failures;
- additive lifecycle protocol operations/events;
- box/split large DTO variant if needed.

Acceptance evidence:

- old clients remain decodable;
- no secret fields serialize;
- disabled/deleted/credential-missing/removed-model states are explicit;
- no silent connection/model mutation.

### Work package E — TUI lifecycle controls and end-to-end harness

Intent: close the user-facing lifecycle workflow.

Required changes:

- extend `/connections` dialog/actions;
- masked local-only rotation input;
- confirmations and blocker rendering;
- refresh progress/cancel/stale handling;
- fake-daemon/CoreClient interaction harness.

Acceptance evidence:

- deterministic keyboard/action tests cover connect/select/refresh/rotate/disable/delete/restore;
- secret input never enters normal TUI snapshots/history;
- stale response reconciles without blind overwrite.

### Work package F — Flaky-test, docs, and roadmap closure

Intent: produce trustworthy final evidence.

Required changes:

- fix provisioning timing race deterministically;
- run full provider/session/protocol/TUI/security matrix;
- update architecture and operator docs;
- create complete Phase 2 closure matrix and legacy-removal criteria.

Acceptance evidence:

- repeated broad-load test runs no longer reproduce the provider timing failure;
- provider-focused clippy/tests are clean or remaining unrelated findings are explicit;
- every roadmap exit criterion has evidence.

## 8. Failure, cancellation, restart, and contention semantics

- Rotation validation failure makes no persistent change.
- Cancellation before rotation commit removes staged credential/metadata and preserves the active revision.
- Cancellation after commit returns/reconstructs the committed result; cleanup is retryable/idempotent.
- Crash with staged rotation/provisioning is recovered from durable operation state and either completed safely or rolled back; no staged secret becomes active accidentally.
- Concurrent rotations/endpoint updates use expected revision; one wins, others receive current state.
- Refresh cancellation keeps last-good health/catalog and records cancellation separately from endpoint failure.
- Concurrent refresh calls single-flight and share one result.
- Disable is immediate for new resolutions; in-flight requests follow documented captured-runtime semantics.
- Delete/tombstone prevents new resolution/selection but preserves references and diagnostics.
- Hard purge fails with explicit blockers while sessions, credentials, operations, or runtime references remain.
- Restart reconstructs committed lifecycle/revision state lazily and does not probe synchronously.
- Credential-store record missing on restart yields credential-missing state without selecting another account.

## 9. Compatibility and migration

- Existing provider registry/environment/config paths remain readable.
- Existing durable connections and sessions migrate additively; do not rewrite or clear selections silently.
- Any schema additions are nullable/defaulted and idempotent.
- Existing `/connect` and `/connections` behavior remains available with additive lifecycle actions.
- Legacy provider/model strings continue through `LegacyResolution` until explicit removal criteria are accepted.
- Soft-deleted connections remain resolvable for historical diagnostics but not runtime execution.
- Scope remains metadata-only pending later authorization work.
- No secret backend replacement is introduced.

## 10. Required tests

### Focused unit tests

- lifecycle transition table and invalid transitions.
- expected-revision conflicts and idempotent transitions.
- catalog normalization/revision stability.
- refresh backoff/jitter/cap calculations with deterministic clock/RNG seams.
- redacted DTO serialization and absence of secret fields.
- purge blocker calculation.

### Integration tests

- successful credential rotation and endpoint rotation.
- invalid credential/unavailable endpoint rotation rollback.
- in-flight old revision plus new-request new revision.
- concurrent rotation winner/stale loser.
- health/model refresh success, unchanged catalog, changed catalog, failure, stale, cancellation, and coalescing.
- disable/enable/delete/restore with selected sessions.
- credential deletion transitions to credential-missing.
- turn submission against each unusable lifecycle state.
- multiple sessions sharing one connection.

### Restart and recovery tests

- crash/failpoints at each staged rotation boundary.
- committed revision survives restart and old staged revision does not activate.
- interrupted refresh preserves last-good catalog.
- tombstone/restore and reference counts survive restart.
- no startup probe.

### Contention and cancellation tests

- many concurrent refresh callers result in one probe.
- global/per-connection probe caps.
- rotation vs refresh race resolves by revision/generation.
- disable/delete during in-flight request follows documented semantics.
- cancellation cleanup is idempotent.

### TUI/protocol tests

- fake-daemon/CoreClient lifecycle flow.
- stale revision response includes current bounded state.
- rotate secret rejected over disallowed remote transport.
- dialog state contains no secret after submit/cancel/close.
- old protocol fixtures still decode.
- lifecycle DTO large-variant warning is resolved if touched.

### Security and negative tests

- secret absent from SQLite, logs, DTO JSON, errors, snapshots, debug output, and test artifacts.
- credential cleanup never deletes unrelated provider/account records.
- endpoint userinfo/query/fragment/redirect/TLS violations rejected.
- no fallback account/connection after disable/delete/missing credential.
- bounded response/error body handling.

### Flake and scale tests

- repeat provisioning/rotation/refresh tests under broad parallel load.
- deterministic fake-server readiness without sleeps.
- model-catalog maximum bounds.
- probe-storm/backoff simulation.

## 11. Required verification commands

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test -p codegg-providers
rtk cargo test -p codegg-providers connection
rtk cargo test -p codegg-core provider_connections
rtk cargo test -p codegg-core session::legacy_resolution
rtk cargo test -p codegg-protocol
rtk cargo test --test session_selection
rtk cargo test --test session_crud
rtk cargo test --test storage_migrations
rtk cargo test -p codegg --lib core::eggpool
rtk cargo test -p codegg --lib connect
rtk cargo test -p codegg --lib session_selection
rtk cargo test -p codegg --lib tui::
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk git diff --check
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 rtk cargo test --workspace --all-features -- --test-threads=14
```

Additionally run the formerly flaky provisioning test repeatedly under parallel load using the repository's accepted test tooling, record iteration count and environment, and avoid hiding failures through arbitrary sleep increases.

## 12. Documentation updates

- Complete provider connection lifecycle state machine.
- Explain staged rotation and old/new revision behavior.
- Document health/model refresh scheduling, caps, backoff, stale thresholds, and manual refresh.
- Document disable/delete/restore/purge and selected-session consequences.
- Document local-only secret transport and redaction.
- Document operator workflows and recovery from missing credentials/unavailable endpoints.
- Record legacy config/model-string removal prerequisites.

## 13. Acceptance criteria

- Credential/endpoint rotation is staged, probed, atomic, revisioned, and rollback-safe.
- In-flight requests keep their captured revision; new requests use the new committed revision.
- Health/model refresh is bounded, cancellable, coalesced, and startup-independent.
- Failed refresh retains the last valid catalog/runtime.
- Disable/delete/credential-missing never trigger silent fallback.
- Selected sessions expose actionable lifecycle/model state.
- Soft delete/restore and purge blockers are explicit and restart-safe.
- `/connections` supports lifecycle actions without exposing secrets.
- End-to-end fake-daemon/TUI lifecycle coverage exists.
- The provisioning timing test is deterministically stabilized.
- All Phase 2 roadmap exit criteria have closure evidence.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- rotation requires plaintext secrets in SQLite, protocol, normal TUI state, or logs;
- safe transactionality cannot be achieved with the existing credential store without a secret-backend redesign;
- lifecycle semantics would silently change session selection or introduce fallback;
- team authorization is required to define personal/project/deployment access;
- Eggpool routing/accounting behavior must be reimplemented;
- hard purge would orphan session/history references;
- background refresh would require unbounded startup or probe behavior;
- work expands into distributed secret transport or project/TUI roadmap scope.

## 15. Closure evidence required

The closure record must contain:

- exact implementation commit(s);
- complete lifecycle transition matrix;
- staged rotation sequence and failpoint results;
- in-flight/new-request revision evidence;
- refresh bounds/backoff/coalescing/catalog-revision evidence;
- selected-session disable/delete/missing-credential behavior;
- redaction matrix across storage/protocol/TUI/logs/errors;
- fake-daemon/TUI lifecycle harness results;
- formerly flaky test root cause, fix, and repeated-load evidence;
- compatibility/migration review;
- full verification log with pass/fail counts;
- complete Phase 2 requirement-to-evidence matrix and roadmap disposition.

## 16. Handoff notes

- Treat `3ce0a7e` as the reviewed production baseline; inspect current `main` before editing.
- A prior closure incorrectly stated that this plan was already authored; current repository state contained no plan, so this file is the authoritative Milestone 004 handoff.
- Preserve the existing credential store and local-only secret transport.
- Use deterministic fake-server readiness and clocks instead of arbitrary sleeps.
- Follow the repository's resource-conscious test configuration.
- Scope metadata is not authorization; leave role enforcement for the later identity/ACL roadmap.
