# Provider Connections Milestone 002 — Eggpool `/connect` Workflow

Status: implemented

Implementation commit: `8c1675c`

Closure record:

- `plans/closure/provider-connections/002-status.md`

Repository baseline: `9dcde707f6fe001cc6d73e7f562ccccf9f782f1a` (`main`; Provider Connections Milestone 001 is closed; later planning-only commits do not alter the production baseline)

Production implementation baseline:

- `bccca00` — durable, secret-safe provider connection domain/storage and daemon connection manager.
- `f203ed9` — typed `ProviderConnectionId`, `ProjectId`, and scope identity primitives.

Source roadmap:

- `plans/subsystems/provider-connections-roadmap.md#milestone-2--eggpool-connect-workflow`

Long-term requirements:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool`
- `plans/001-terminology-and-domain-model.md` — ProviderConnection, SecretRef, scope, lifecycle, capability/model catalog.
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`

Applicable closure evidence:

- `plans/closure/provider-connections/001-status.md`
- `plans/closure/domain-identity/001-status.md`

Applicable ADRs:

- None. The canonical specification already decides that Eggpool is a daemon-owned provider connection, defaults to port `11300`, uses secret references, and should reuse compatible provider transport rather than creating a parallel runtime.

Primary class: capability

## 1. Objective

Implement an explicit, secure, cancellable Eggpool connection workflow through `/connect` that accepts host, optional port, TLS policy, API key, optional display name, and connection scope; validates and normalizes the endpoint; stores the credential in the existing protected credential store; performs a bounded health/model probe; and atomically exposes one durable active connection with a discoverable model catalog.

The milestone succeeds when a user can connect CodeGG to an Eggpool instance—defaulting to port `11300`—without editing config files or exposing secrets to frontend state, logs, protocol payloads, diagnostics, or project files.

This milestone creates and validates connections. It does not yet migrate sessions to select providers by `ProviderConnectionId`; that remains Milestone 003.

## 2. Why this milestone is ready

The hard dependency is closed:

- Provider Connections Milestone 001 added `ProviderConnection`, `ProviderConnectionStore`, personal/project/deployment scope, validated endpoint/TLS metadata, opaque secret bindings, the existing credential-store adapter, revisioned lifecycle operations, and daemon-owned lazy provider construction.

The current TUI already registers `/connect`, but it has no complete workflow. The current auth CLI and credential store already support secret-safe API-key writes, and the provider abstraction already exposes `ping`, `models`, and `discover_models` seams. The milestone can therefore deliver a vertical user capability without redesigning credential storage or provider transport.

## 3. Current implementation evidence

At the repository baseline:

- `ProviderKind::Eggpool` is represented in durable connection metadata.
- `ProviderConnectionId`, endpoint normalization, `TlsPolicy`, `ProviderScope`, lifecycle/revision fields, `SecretRef`, and `SecretBindingLocator` are persisted without plaintext credential material.
- `ProviderConnectionStore` provides additive SQLite persistence, uniqueness, CRUD, lifecycle transitions, deletion, and optimistic revision checks.
- `CredentialStoreAdapter` resolves exact provider/account bindings through the existing encrypted credential store. Missing master key, missing account, expired credential, and invalid binding are typed failures.
- `ProviderConnectionFactory` can construct native and generic OpenAI-compatible providers, while unsupported kinds fail closed.
- `ConnectionManager` resolves and caches provider instances by connection ID/revision and coalesces concurrent construction.
- Connection creation currently stores metadata independently from credential creation and does not probe endpoints.
- No health/model metadata persistence, provisioning transaction coordinator, Eggpool-specific factory preset, core create/test operation, protocol request, or TUI dialog exists yet.
- `src/tui/command.rs` registers `/connect` with the description “Connect provider,” but there is no completed Eggpool dispatch/dialog flow.
- `src/auth/cli.rs` intentionally notes that richer `/connect` TUI flows may build on the same credential-store API.
- Sessions still select provider/model through legacy runtime/config paths; this milestone must not silently change session selection.

Known gap: the user cannot create and validate an Eggpool connection through CodeGG, and there is no crash-safe transaction spanning credential write, durable metadata, probing, model catalog publication, and cancellation cleanup.

## 4. Invariants that must not regress

- Plaintext API keys MUST exist only in bounded secret-input memory and the protected credential-store write path.
- API keys MUST NOT be placed in TUI application snapshots, normal command history, protocol events, logs, error strings, SQLite rows, project configuration, chat, audit metadata, or serialized diagnostics.
- Provider connection IDs remain distinct from provider implementation IDs and credential account IDs.
- Existing environment/config provider registration and direct provider behavior remain functional.
- Connection scope is metadata and future authorization context; it MUST NOT grant access merely because a scope value parses.
- Omitted Eggpool port resolves to `11300` exactly.
- Endpoint normalization is deterministic and rejects embedded credentials, query strings, fragments, unsupported schemes, invalid ports, and TLS-policy contradictions.
- Redirects, timeouts, oversized responses, and model catalogs are bounded.
- Daemon startup MUST NOT synchronously probe all connections.
- A failed, timed-out, or cancelled connect attempt MUST NOT leave an active connection, plaintext data, or an unrecoverable credential/metadata orphan.
- An in-flight probe uses one immutable endpoint/credential generation and does not switch credentials mid-request.
- Concurrent equivalent connect attempts must not create duplicate active records.
- Frontends invoke daemon-owned connection services; they do not instantiate provider clients or persist credentials directly.

## 5. Scope

### In scope

- Eggpool preset/factory integration over the existing compatible provider transport.
- `/connect` command routing and an interactive TUI flow or equivalent frontend-neutral form state.
- Input collection for host, optional port, TLS policy, API key, optional display name, and scope.
- Secure API-key entry that is masked, not echoed, and not retained in generic prompt history.
- Local validation and deterministic endpoint construction with default port `11300`.
- A daemon/core create-and-test service and typed protocol/transport operations.
- Protected credential-store write using a deterministic, connection-owned binding.
- Provisioning lifecycle/journal sufficient for crash-safe recovery across SQLite and credential-store operations.
- Bounded authentication/health/model discovery probe against Eggpool.
- Redacted structured results and diagnostics.
- Durable health result and bounded model catalog or catalog revision sufficient for listing immediately after success.
- Cancellation and compensating cleanup.
- Duplicate/equivalent connection detection and explicit reuse/conflict behavior.
- List/get visibility sufficient to confirm the new connection and models.
- Documentation and deterministic fake-Eggpool tests.

### Explicitly out of scope

- Migrating sessions or model selection to `ProviderConnectionId`.
- General create/edit UI for every provider kind.
- Credential rotation, disable/delete UX, periodic health refresh, or long-term stale health policy.
- Team role enforcement or project-membership authorization.
- Eggpool routing, accounting, model alias policy, fallback, compression, or load-balancing internals.
- Starting, installing, or managing an Eggpool daemon.
- Storing credentials in SQLite or project files.
- Automatic import of ambiguous legacy provider config.
- Probing all connections at daemon startup.
- A new secret backend or replacement encryption system.

## 6. Required production changes

### Core/domain

Add a frontend-neutral provisioning service owned by daemon/core. It should accept a secret-bearing request only at an internal trusted boundary and return a fully redacted result.

Suggested contracts, adapted to repository conventions:

- `CreateEggpoolConnectionRequest` containing host, optional port, TLS policy, display name, scope, and a non-serializable or explicitly secret-classified API-key envelope;
- normalized `EggpoolConnectionSpec` with endpoint, provider kind, scope, account locator, and display metadata;
- `ConnectionProvisioningId` or operation identifier for cancellation/recovery;
- provisioning state and outcome types;
- `ConnectionProbeResult` with bounded health status, duration, capability summary, model catalog, catalog revision/digest, and redacted diagnostics;
- `CreateConnectionResult` exposing only connection summary and catalog metadata.

Do not derive a connection ID or credential binding from host/path text. Generate the `ProviderConnectionId` through the typed identity layer. A credential account locator may be deterministically associated with the generated connection ID, but it must remain a non-secret implementation detail.

### Eggpool provider preset

Implement Eggpool as a narrow preset over generic OpenAI-compatible transport when API-compatible. The preset must centralize, rather than spread, assumptions such as:

- provider kind and implementation ID;
- endpoint path normalization;
- bearer/API-key header behavior;
- health endpoint strategy;
- model discovery endpoint and parsing;
- request/response limits;
- redirect policy;
- timeouts;
- capability mapping.

If Eggpool requires a small incompatible endpoint behavior, isolate it in the preset/factory layer. Do not fork the entire provider runtime.

### Endpoint input and normalization

The workflow must accept:

- host as hostname, IP literal, or complete allowed HTTP(S) origin;
- optional port;
- TLS policy: required, optional/derived, or disabled according to existing enum semantics;
- optional display name;
- scope: personal, project, or deployment.

Rules:

- when no port is supplied, use `11300`;
- reject a port embedded in the host plus a conflicting explicit port;
- reject userinfo, query, fragment, path traversal, unsupported schemes, empty host, control characters, invalid IPv6 syntax, and out-of-range ports;
- normalize IPv6 literals with brackets;
- normalize trailing slash/path according to the Eggpool preset without duplicating API path segments;
- TLS-required produces HTTPS only; TLS-disabled produces HTTP only; optional/derived behavior must be deterministic and documented;
- do not resolve DNS during local validation.

Project scope must require an explicit stable `ProjectId` supplied by an authoritative context or entered as a validated ID. Until Domain Identity Milestone 003 provides project-aware frontend context, the workflow may default to personal scope and must fail actionably rather than derive project scope from cwd or directory.

### Secret input and credential write

- Use a dedicated secret-input component or prompt mode with masking and paste support.
- Never insert the API key into the ordinary chat prompt buffer, command string, command recall, toast, debug representation, or generic TUI state snapshot.
- Clear/drop secret buffers promptly after handoff; avoid unnecessary clones.
- Write the credential through the existing encrypted `CredentialStore`/adapter and preserve master-key failure behavior.
- Use an exact provider/account binding reserved for this connection; never fall back to another account.
- Redact all credential-store errors before returning them to protocol/UI layers.

### Provisioning transaction and recovery

SQLite and the credential store are separate durability domains, so the workflow needs an explicit transaction coordinator rather than claiming impossible atomicity.

Implement a crash-recoverable sequence such as:

1. validate and normalize all non-secret inputs;
2. allocate connection/provisioning IDs and create a durable non-active provisioning record or journal entry;
3. write the credential under the reserved binding;
4. construct an ephemeral provider instance from the exact staged spec/credential generation;
5. execute bounded health and model probes;
6. in one SQLite transaction, persist the active connection metadata, health/model catalog result, revision, and clear/finalize provisioning state;
7. publish redacted success to the caller and invalidate/prime manager cache as appropriate.

On validation, credential-write, probe, persistence, or cancellation failure:

- no active connection may be visible;
- remove the staged credential when safe and owned by the operation;
- remove or mark failed the provisioning record with bounded diagnostics;
- retain enough non-secret recovery state to reconcile a process crash;
- never delete a preexisting credential or connection not uniquely owned by this operation.

At daemon startup or first provisioning-service use, reconcile stale provisioning operations. Recovery must deterministically finalize a fully committed success or remove/tombstone operation-owned staged state. It must not probe every active connection.

### Health and model probing

Use a fakeable probe interface and explicit bounds. At minimum:

- connect timeout;
- request timeout;
- overall workflow deadline;
- cancellation token;
- redirect limit or redirects disabled by default;
- response-byte limit;
- model-count limit;
- per-model string/metadata limits;
- JSON nesting/parse safety through existing libraries;
- concurrency cap/coalescing for identical connection/probe operations.

Authentication failure, unreachable endpoint, TLS failure, timeout, invalid JSON, unsupported API, empty model catalog, and oversized catalog must produce distinct redacted reason codes.

The probe must not include the API key, authorization header, full response body, or secret-derived fingerprint in diagnostics. Endpoint diagnostics should prefer normalized authority and bounded path class rather than arbitrary URL echoing.

A successful workflow persists enough catalog metadata to immediately list discovered models and a deterministic catalog revision/digest. Do not persist unbounded provider payloads.

### Storage and migrations

Add additive storage after migration v24 for the minimum health/catalog/provisioning state required by this milestone. This may extend provider-connection tables or add normalized supporting tables, but must preserve Milestone 001 rows and semantics.

Required persisted properties:

- connection ID/revision relation;
- provisioning operation state and timestamps;
- last explicit probe outcome and bounded diagnostic reason;
- health status and completion time;
- bounded model entries or a bounded serialized catalog with deterministic revision;
- indexes for connection, provisioning status, health, and catalog lookup;
- no credential value, ciphertext, header, or secret-derived data.

Migration is additive, idempotent, and rollback-safe. Do not probe endpoints during migration.

### Protocol and DTOs

Add typed daemon protocol operations for the workflow. The exact request split may follow existing `CoreRequest` conventions, but must support:

- begin/create Eggpool connection;
- cancel provisioning;
- receive provisioning progress or bounded status polling;
- receive redacted success/failure;
- list/get redacted connection summaries;
- list discovered models for a connection.

Secret handling requires special care:

- the API key may traverse local authenticated IPC only through a request field explicitly classified as secret and excluded from `Debug`, tracing, event replay, snapshots, and error serialization;
- remote network use of secret-bearing create operations must remain disabled or require an explicit secure capability and transport policy; do not accidentally expose it through unauthenticated WebSocket/SSE routes;
- provisioning progress events contain no secret and are bounded;
- large catalogs remain bounded or use handles if needed.

Do not change session DTOs or provider/model selection in this milestone.

### TUI and operator surface

Implement `/connect` as an asynchronous, cancellable dialog/workflow using existing spawn-and-complete and stale-request patterns.

Minimum UX:

1. choose Eggpool;
2. enter host;
3. optionally enter port, default shown as `11300`;
4. select TLS policy;
5. enter masked API key;
6. optionally enter display name;
7. choose scope, defaulting safely for personal-local mode;
8. review redacted normalized endpoint/scope;
9. submit and show bounded validation/probe progress;
10. show success with connection ID/display name and discovered model count, or actionable redacted failure.

Requirements:

- keyboard cancellation works at every pre-commit and probe stage;
- closing the dialog invalidates stale completions;
- resize/render loop remains responsive;
- API key never appears in `App`, remote snapshots, toasts, info dialogs, command history, panic messages, or test snapshots;
- repeated submit is disabled or deduplicated while one operation is active;
- personal-local mode remains usable without team login;
- project scope is unavailable with an explanation when no stable project context exists.

A noninteractive CLI path MAY be added for automation, but must read secrets from stdin/file descriptor or protected environment input and never accept a key in argv by default.

### Security and authorization

- Validate all input before network activity.
- Enforce loopback/private/public endpoint policy only if existing configuration defines one; do not invent broad SSRF policy silently. At minimum expose a policy seam and prevent unsupported schemes/local file access.
- DNS rebinding and redirect changes must not bypass the validated scheme/host/TLS policy. Revalidate redirect targets or disable redirects.
- Project/deployment scope is recorded but not treated as authorization until the principal subsystem lands.
- Secret-bearing operations are denied on transports that cannot provide the required confidentiality/authentication boundary.
- Audit seams record actor placeholder/local owner, connection ID, scope, endpoint authority, outcome, duration, and model count—not the key.

### Documentation and static guards

Update at minimum:

- `architecture/provider.md`;
- `architecture/auth.md`;
- `architecture/protocol.md`;
- `architecture/tui.md`;
- `architecture/storage.md`;
- command documentation and user-facing Eggpool examples.

Add or extend redaction/static tests so secret-classified request fields cannot derive `Debug` output and cannot enter remote TUI/session projection types.

## 7. Ordered work packages

### Work package A — Eggpool preset and normalization

Intent: establish one tested, reusable Eggpool provider construction and endpoint contract.

Required changes:

- add Eggpool preset/factory integration;
- implement host/port/TLS normalization with default `11300`;
- define bounded probe interface and reason codes;
- add local validation and redacted display types.

Acceptance evidence:

- omitted port produces the documented normalized endpoint;
- explicit port/TLS combinations are deterministic;
- malformed/secret-bearing URLs fail before network I/O;
- Eggpool provider construction uses the exact staged credential and endpoint.

### Work package B — Provisioning state and persistence

Intent: create crash-safe coordination across connection metadata, credential storage, and model/health state.

Required changes:

- add provisioning operation records/state;
- add health/model catalog persistence and migration;
- define operation-owned credential binding;
- implement finalize, compensate, and startup reconciliation paths;
- integrate revision/cache invalidation.

Acceptance evidence:

- successful finalize exposes one active connection and catalog;
- every injected failure point leaves no active partial record;
- stale provisioning recovers after restart;
- no preexisting credential is deleted by compensation;
- v24 data migrates unchanged.

### Work package C — Bounded probe service

Intent: validate authentication and discover models without unbounded resource use or secret leakage.

Required changes:

- implement explicit timeout/cancellation/redirect/size/model-count limits;
- add fake server and fake clock/cancellation seams;
- map provider errors into bounded redacted reason codes;
- calculate deterministic catalog revision;
- coalesce or cap concurrent equivalent probes.

Acceptance evidence:

- valid fake Eggpool returns healthy result and bounded model catalog;
- invalid key, unavailable endpoint, timeout, TLS error, invalid payload, redirect, and oversized catalog are distinct and redacted;
- authorization header and response bodies are absent from diagnostics/log capture;
- cancellation stops the probe and runs compensation.

### Work package D — Core protocol operations

Intent: make provisioning daemon-owned and transport-safe.

Required changes:

- add core request/response and progress contracts;
- implement secret-safe request formatting and tracing exclusions;
- add cancellation/status/list/get/model operations;
- restrict secret-bearing operations to approved authenticated/confidential transports;
- keep session selection unchanged.

Acceptance evidence:

- local TUI can complete the workflow solely through `CoreClient`;
- secret values never appear in serialized responses/events or `Debug` output;
- duplicate operation IDs are idempotent or produce typed conflicts;
- reconnect/status behavior is deterministic for in-progress provisioning.

### Work package E — TUI `/connect` workflow

Intent: deliver the user-visible Eggpool connection capability.

Required changes:

- implement dialog/form state and validation;
- implement dedicated masked secret input;
- use registered asynchronous tasks and stale request IDs;
- support submit, cancellation, progress, success, and failure;
- refresh redacted connection/model views after success;
- add help and command documentation.

Acceptance evidence:

- keyboard-driven happy path creates one connection;
- omitted port path uses `11300`;
- cancel/close leaves no active or orphaned operation;
- stale completion after dialog close is ignored;
- TUI snapshots and logs contain no test API key;
- connection and discovered model count are visible after success.

### Work package F — Recovery, redaction, and closure hardening

Intent: prove the capability under failures and prevent later secret regressions.

Required changes:

- add failpoint/restart tests across provisioning steps;
- add redaction snapshots and log-capture assertions;
- add concurrency/duplicate tests;
- document recovery and operational behavior;
- add static/source guards for secret-bearing DTOs and remote exposure where practical.

Acceptance evidence:

- all required failure points have deterministic cleanup/recovery evidence;
- repeated equivalent submissions do not create duplicate active records;
- full required verification matrix passes;
- documentation matches final semantics.

## 8. Failure, cancellation, restart, and contention semantics

- Local validation failure performs no credential-store or SQLite write.
- Credential-store failure leaves the provisioning record failed/cleanable and creates no active connection.
- Probe failure compensates operation-owned credential state and removes/tombstones non-active metadata; no active connection is exposed.
- SQLite finalize failure compensates the staged credential when ownership is certain and leaves a recoverable provisioning record.
- Process crash at any step is resolved by startup/first-use reconciliation using provisioning state and deterministic credential ownership.
- User cancellation before final commit produces no active connection. Cancellation racing with final commit returns a definitive committed-success or cancelled-clean result, never both.
- Concurrent identical submissions are deduplicated by an idempotency/equivalence key or one succeeds while the other receives a typed existing/conflict result.
- Probes are concurrency-limited globally and coalesced per operation/spec where safe.
- Timeout/cancellation drops the provider/client generation used by the operation; it does not alter existing active connections.
- A successful active connection is not rolled back because the TUI disconnects after commit; reconnect/list reveals the result.
- No database transaction remains open across network I/O.

## 9. Compatibility and migration

- Preserve all Milestone 001 provider-connection rows and APIs.
- Preserve existing environment/config registration and current model selection behavior.
- Do not automatically import legacy config as Eggpool unless the user explicitly runs the workflow and supplies/chooses the endpoint and credential binding.
- Additive protocol operations must not require existing clients to understand provisioning events unless they invoke the capability.
- Connection health/model state is versioned by connection revision or explicit catalog revision.
- Do not write connection IDs into sessions yet.
- Document how Milestone 003 will consume the connection/model catalog without committing that migration prematurely.

## 10. Required tests

### Focused unit tests

- host/port/TLS normalization including default `11300`;
- IPv4, IPv6, hostname, and complete-origin inputs;
- conflicting embedded/explicit ports;
- display-name/scope validation;
- secret-bearing URL rejection;
- provisioning state transitions and invalid transitions;
- catalog bounds and deterministic revision;
- redacted error/display/debug behavior.

### Provider and probe tests

- fake Eggpool success with model discovery;
- invalid API key;
- unavailable/refused endpoint;
- connect timeout and response timeout;
- TLS mismatch/certificate failure;
- redirects disabled or revalidated;
- invalid JSON and incompatible API response;
- oversized response and model-count limits;
- empty catalog policy;
- cancellation during connect, response, and parse;
- no secret in captured logs/errors.

### Storage and migration tests

- v24-to-new-version migration;
- clean schema migration;
- idempotent migration rerun;
- injected migration failure rollback;
- provisioning/health/catalog round trips;
- active connection uniqueness;
- no plaintext/ciphertext/header values in SQLite;
- existing Milestone 001 rows retain behavior.

### Integration tests

- full local CoreClient create workflow;
- TUI workflow happy path;
- personal scope happy path;
- project scope with explicit valid `ProjectId`;
- project scope rejected when no authoritative project context exists;
- success immediately appears in connection/model listing;
- existing environment-configured provider still works after Eggpool connect.

### Restart and recovery tests

Inject process-style interruption after:

- provisioning row creation;
- credential write;
- successful probe before finalize;
- catalog write before activation/finalization;
- active commit before UI response.

For each case, prove deterministic recovery and absence of an orphan active record or unintended credential deletion.

### Contention and cancellation tests

- two equivalent simultaneous connect attempts;
- two distinct Eggpool endpoints;
- cancel racing with probe success;
- cancel racing with final commit;
- TUI close followed by stale completion;
- global probe concurrency cap;
- operation status after frontend disconnect/reconnect.

### Security and negative tests

- API key absent from protocol responses/events, `Debug`, tracing, panic text, TUI snapshots, command history, SQLite, and docs fixtures;
- secret-bearing operation denied over unsafe/unapproved transport;
- endpoint rejects file/unix/other schemes, userinfo, query, fragment, invalid ports, and control characters;
- redirect cannot escape validated policy;
- error bodies are bounded and not surfaced raw;
- compensation cannot delete another connection’s credential;
- no synchronous startup probe.

### Compatibility tests

- existing provider and auth suites pass;
- `register_builtin` and `register_builtin_with_config` behavior remains intact;
- legacy model/session behavior is unchanged;
- Milestone 001 manager cache/revision tests pass;
- protocol unknown-request compatibility remains explicit.

## 11. Required verification commands

The implementation agent must adapt exact filters to current test names while preserving this coverage. At minimum run:

```bash
rtk cargo fmt --all -- --check

rtk cargo test -p codegg-providers connection
rtk cargo test -p codegg-providers eggpool
rtk cargo test -p codegg-core provider_connections
rtk cargo test -p codegg-protocol provider
rtk cargo test --lib core::provider_connections
rtk cargo test --test storage_migrations
rtk cargo test provider
rtk cargo test auth
rtk cargo test connect

rtk cargo test -p codegg-providers
rtk cargo test -p codegg-core
rtk cargo test -p codegg-protocol

rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk git diff --check
```

Run the TUI and transport-focused suites added by the implementation. Use only local deterministic fake servers; closure must not depend on a live Eggpool endpoint or external network.

## 12. Documentation updates

- Document `/connect` Eggpool step-by-step flow.
- Document host forms, default port `11300`, TLS behavior, and scope behavior.
- Document master-key/credential-store prerequisites.
- Document explicit probe bounds, cancellation, provisioning recovery, and redacted diagnostics.
- Document what success stores and what it never stores.
- Document the distinction between creating a connection and selecting it for a session.
- Update protocol and storage schema references.
- Add operator recovery guidance for stale failed provisioning operations if any manual action remains possible.

## 13. Acceptance criteria

- `/connect` offers an Eggpool workflow with host, optional port, TLS policy, API key, optional display name, and scope.
- Omitted port resolves to `11300` and is shown in the redacted review/success result.
- Invalid credentials, unavailable endpoints, TLS failures, timeouts, incompatible responses, and oversized catalogs produce actionable redacted diagnostics.
- A successful workflow stores the API key only in the protected credential store and creates exactly one durable active connection.
- A successful workflow persists/exposes a bounded discovered model catalog and deterministic revision.
- Cancellation or any injected failure creates no active partial connection, plaintext leak, or unrecoverable operation-owned credential orphan.
- Restart reconciliation resolves every staged provisioning state deterministically.
- Concurrent equivalent submissions do not create duplicate active records.
- The TUI remains responsive and ignores stale completions.
- Existing providers and legacy session/model selection remain functional.
- No secret appears in protocol outputs, logs, events, TUI snapshots, SQLite, project config, or diagnostics.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- Eggpool’s current API is not sufficiently OpenAI-compatible and supporting it would require a separate broad provider runtime rather than a narrow preset;
- a live external Eggpool instance is required to verify correctness instead of a deterministic fake server;
- secure secret input cannot be kept out of ordinary TUI/prompt/remote snapshot state;
- the only implementation would transmit API keys over an unauthenticated or unencrypted remote transport;
- crash-safe provisioning requires replacing the credential store or introducing a new general secret backend;
- project scope can only be implemented by deriving a project ID from cwd/path;
- session/model selection migration is required to demonstrate connection creation, expanding into Milestone 003;
- periodic health/lifecycle management or general provider UI becomes necessary, expanding into Milestone 004;
- credential compensation cannot prove operation ownership before deletion.

## 15. Closure evidence required

The closure record must include:

- implementation commits;
- final provisioning sequence/state diagram;
- migration version and health/catalog/provisioning schema summary;
- requirement-to-evidence matrix;
- TUI happy-path evidence using a deterministic fake Eggpool server;
- default-port `11300` evidence;
- explicit port/TLS normalization matrix;
- failure-reason matrix for auth, availability, TLS, timeout, invalid payload, redirect, and bounds;
- cancellation and stale-completion evidence;
- restart/failpoint recovery matrix for every provisioning stage;
- concurrent duplicate-submission evidence;
- catalog bounds and revision evidence;
- proof that no secret entered protocol output, logs, snapshots, SQLite, or config;
- existing provider/auth compatibility suite outcomes;
- exact verification commands run and results;
- documentation updates;
- unresolved findings classified by severity;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 16. Handoff notes

- Provider Connections Milestone 001 is closed at `bccca00`; reuse its domain/store/manager/credential adapter rather than creating parallel connection types.
- `/connect` already exists in the command registry as a placeholder. Preserve command discoverability and integrate through existing asynchronous TUI patterns.
- The current auth CLI accepts secret text as an internal function parameter and reads stdin for CLI use. Do not pass Eggpool keys in command-line argv or ordinary slash-command arguments.
- Use a deterministic local fake Eggpool/OpenAI-compatible server. Do not add live-network tests.
- Personal scope is the safe default before project-aware TUI context exists. Project scope requires an explicit validated `ProjectId`; never infer it from path.
- Do not change session model/provider selection in this milestone.
- Do not probe active connections synchronously during daemon startup.
- Preserve unrelated user changes and inspect current `main` before implementation.
