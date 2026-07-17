# Provider Connections Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connections/002-eggpool-connect-workflow.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connections-roadmap.md#milestone-2--eggpool-connect-workflow`

Repository baseline reviewed: implementation and closure commits recorded
below

Implementation commits or pull requests:

- M2 implementation commit — daemon-owned Eggpool probe/provisioning,
  secret-safe protocol, TUI workflow, migration v26, tests, and docs.
- M2 closure commit — this evidence record, roadmap/registry updates, and the
  dependency-ready Milestone 003 handoff plan.

## 1. Executive finding

Milestone 2 is closed. CodeGG now provides a local `/connect` Eggpool flow
that validates and normalizes an endpoint, stores the API key only in the
existing protected credential store, performs a bounded local/fake-server
compatible model probe, and atomically publishes one active durable
connection with health and model-catalog metadata. The operation is
cancellable, restart-reconciled, duplicate-protected, and exposed through
redacted daemon-owned protocol/list operations.

The TUI supports the personal-local path and explicitly explains that project
scope requires authoritative project context. Session provider/model
selection is unchanged by design and remains Milestone 3.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Eggpool `/connect` collects host, port, TLS, API key, display name, and scope | `src/tui/components/dialogs/connect.rs`; `src/tui/app/mod.rs`; TUI connect tests | pass | Personal scope is the safe available context; service DTOs validate project/deployment scopes for trusted callers. |
| Omitted port resolves to `11300` | `src/core/eggpool.rs::omitted_port_uses_eggpool_default_and_v1_path`; provider normalization tests | pass | TUI displays `11300`; daemon normalization also defaults when the field is absent. |
| Explicit ports/TLS normalize deterministically | `tls_and_explicit_port_matrix_is_deterministic`; endpoint validation tests | pass | Conflicting embedded/explicit ports and TLS contradictions fail before network I/O. |
| Secret input is masked and redacted | `SecretInput`, `EggpoolApiKey`, custom request `Debug`, TUI mask/clear paths, redaction tests | pass | The local IPC request is the explicitly trusted secret-bearing boundary; responses/events/TUI completion commands are secret-free. |
| Protected credential-store write | `EggpoolProvisioner::create_inner`; `CredentialStore::put`; fake workflow test | pass | Master key is required; account binding is operation-owned and UUID-backed. |
| Durable provisioning/health/catalog state | migration v26; `provider_provisioning`, `provider_connection_health`, `provider_connection_models`; storage migration test | pass | No key, ciphertext, or authorization-header columns exist. |
| Bounded health/model probe | `codegg_providers::EggpoolProbe`; local fake-server success, bounds, timeout, cancellation tests | pass | Redirects are disabled; request/overall time, response bytes, model count, and model-field size are capped. |
| Actionable redacted failures | Stable provider/daemon reason enums and mapping test | pass | Auth, unreachable, timeout, TLS, redirect, unsupported API, invalid JSON, empty, oversized, and cancelled outcomes are bounded. |
| Successful finalize publishes one active connection and catalog | `successful_provision_persists_redacted_connection_and_catalog` | pass | Final SQLite transaction writes active metadata, health, catalog, and committed provisioning state together. |
| Failure/cancellation compensation | `cancellation_compensates_operation_owned_credential`; probe-failure compensation path; storage-write compensation | pass | Compensation removes only the operation-owned credential locator and never rolls back a committed active row. |
| Restart reconciliation | `reconcile_once`; v26 staged/probing journal and `daemon_restarted` transition | pass | First use reconciles stale staged/probing rows without probing every active connection at startup. |
| Duplicate/equivalent submission handling | Fake workflow duplicate assertion; unique idempotency key and active-equivalence checks | pass | Equivalent active or staged/probing/committed work returns a typed conflict. |
| Daemon-owned protocol operations | `CoreRequest`/`CoreResponse` provider contracts; `CoreDaemon` handlers; TUI `CoreClient` path | pass | Create, cancel, status, list, and model operations are daemon-owned; session selection is unchanged. |
| Unsafe remote secret transport denied | `src/server/ws.rs`; protocol/server architecture docs | pass | Remote core WebSocket returns `secret_operation_remote_denied` for create. |
| TUI cancellation and stale completion safety | registered command task, close cancellation, operation-ID matching in `command_dispatch`, TUI tests | pass | Late completion is ignored unless it matches the still-open dialog operation. |
| Existing provider/auth behavior preserved | provider compatibility filter and auth/provider suites | pass | No legacy env/config registration or model-selection path was removed. |

## 3. Production implementation evidence

### Provisioning sequence and state diagram

```text
local TUI form
    │ secret-bearing create over local authenticated IPC
    ▼
validate + normalize ──invalid──► no write
    │
    ▼
staged journal row
    │ operation-owned credential-store write
    ▼
probing ──auth/unreachable/TLS/timeout/bounds/cancel──► compensate → failed/cancelled
    │ bounded /models response
    ▼
one SQLite transaction:
  provider_connections(active)
  provider_connection_health(healthy + catalog revision)
  provider_connection_models(bounded revisioned rows)
  provider_provisioning(committed)
    │
    ▼
redacted result: connection summary + model DTOs

daemon restart/first use:
  staged/probing → remove only operation-owned credential → failed(daemon_restarted)
```

The network probe is never held inside the final SQLite transaction. The
credential write and journal precede the probe; finalize is atomic; every
failure path has an ownership-aware compensation path.

### Ownership boundaries

- `crates/codegg-providers/src/eggpool.rs` owns the bounded HTTP probe,
  redirect policy, response parsing, model limits, stable digest, and
  redacted provider failure categories.
- `src/core/eggpool.rs` owns endpoint/TLS/scope normalization, credential
  storage, provisioning state, cancellation, duplicate checks, finalization,
  compensation, restart reconciliation, and redacted projections.
- `crates/codegg-protocol/src/provider.rs` owns bounded secret input and
  secret-free result DTOs.
- `src/core/daemon.rs` is the daemon authority for create/cancel/status/list/
  models. `src/server/ws.rs` blocks remote secret-bearing create.
- `src/tui/` owns only form state, masking, operation submission, cancellation,
  and redacted result rendering; it does not write credentials or construct a
  provider.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo check -p codegg --lib
rtk cargo test -p codegg-providers eggpool
rtk cargo test -p codegg-core provider_connections
rtk cargo test -p codegg-protocol
rtk cargo test --test storage_migrations
rtk cargo test -p codegg --lib core::eggpool -- --test-threads=1
rtk cargo test -p codegg --lib connect
rtk cargo test --test tui connect_dialog
rtk cargo test provider
rtk cargo test -p codegg --lib auth
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk git diff --check
```

### Results

- Formatting and root library check: pass.
- Eggpool provider probe suite: pass; deterministic fake-server success,
  auth, unsupported, invalid JSON, empty, oversized, timeout, cancellation,
  redirect, normalization, digest, and redaction cases are covered.
- Core Eggpool workflow suite: 8 tests pass, including normalization/reason
  mapping, fake-server success, duplicate conflict, cancellation
  compensation, and secret-safe credential persistence.
- Core provider-connection focused suite: 6 passed.
- Protocol suite: pass.
- Storage migration integration: 3 passed, including injected migration
  failure/resume and v26 secret-free table assertions.
- Root `connect` filter: 12 passed; root `provider` compatibility filter: 51
  passed; focused root auth filter: 25 passed; TUI connect filter: 2 passed.
- All-target workspace clippy with `-D warnings`: pass.
- Core-boundary and daemon-cwd guards: pass.
- Diff whitespace check: pass.

The broader `rtk cargo test auth` name filter was started but did not complete
because it selected unrelated integration binaries in this environment; the
focused root `--lib auth` suite is the recorded compatibility evidence.

The deterministic local fake server is the only network fixture. No live
Eggpool or external network is required. A full interactive CoreClient/TUI
terminal harness was not added; the daemon fake-server workflow and focused
TUI form/task tests are the closure substitute and are explicitly retained as
a follow-up hardening opportunity.

## 5. Invariant review

- Plaintext API keys exist in bounded secret-input memory, the local IPC
  request, the outbound authorization header, and the protected credential
  write path only. They are absent from SQLite, provider errors, redacted
  responses, completion commands, TUI snapshots, config writes, and logs.
- Stable `ProviderConnectionId` remains distinct from provider kind and the
  operation-owned credential account ID.
- Existing configured providers and legacy session model selection remain
  available; this milestone creates a connection but does not select it for a
  session.
- Scope is validated metadata. Personal/project/deployment DTOs are accepted
  only after typed validation; no scope string grants authorization.
- Active daemon startup does not synchronously probe every connection.
- TUI close clears the secret and cancels the operation; operation-ID matching
  prevents a stale completion from mutating a later dialog.

## 6. Failure and recovery review

| Failure or race | Behavior | Evidence |
|---|---|---|
| Invalid endpoint/scope/display name | Reject before journal or credential write | Core normalization tests |
| Credential store/master-key failure | Failed provisioning outcome; no active row | `create_inner`, typed error mapping |
| Auth/availability/TLS/timeout/redirect | Stable reason code; operation-owned compensation | Provider probe + daemon mapping tests |
| Invalid/unsupported/empty/oversized catalog | Stable bounded reason; no raw body in error | Provider fake-server tests |
| Cancel before/during probe | Cancelled result and owned credential cleanup | Core cancellation test |
| SQLite state-write/finalize failure | Compensation path removes owned binding; journal remains inspectable | Storage error paths and journal schema |
| Crash/restart in staged/probing | Reconciliation removes owned binding and marks `daemon_restarted` | `reconcile_once` and v26 schema |
| Equivalent concurrent/duplicate create | Active/idempotency checks and unique key return conflict | Success/duplicate workflow test |
| Cancel racing final commit | Final transaction is definitive; post-commit close does not roll back active connection | Transaction ordering and operation-ID UI guard |
| Stale TUI completion | Ignored unless operation ID matches current dialog | `TuiCommand` operation ID and dispatch guard |

## 7. Migration and compatibility review

- Storage layout advances from v25 to v26 through an additive, idempotent
  migration. Existing v24 provider rows and v25 identity tables remain
  untouched.
- `provider_provisioning` contains only operation, endpoint, scope, lifecycle,
  and opaque credential locator metadata. Health and model rows are keyed by
  connection/revision and use bounded fields.
- Existing environment/config provider registration, auth behavior, and
  `provider/model` session selection remain unchanged.
- New protocol variants are additive; existing clients do not need to
  understand provisioning unless they invoke it.
- No session fields are migrated to connection IDs in this milestone.

## 8. Security review

- `SecretInput`, `CreateEggpoolConnectionRequest`, `EggpoolApiKey`, and probe
  diagnostics redact `Debug`/`Display` output. The TUI renders a fixed mask and
  clears its secret buffer on submission, error, and close.
- SQLite has no plaintext, ciphertext, API-key, or header columns. The
  credential store remains the existing encrypted mode-`0600` protected
  store and requires a master key for writes.
- Endpoint validation rejects unsupported schemes, userinfo, query/fragment,
  control characters, path traversal (including encoded traversal), invalid
  or conflicting ports, and TLS-policy contradictions.
- Redirects are disabled; HTTP response/error bodies are bounded and raw
  reqwest/serde errors never cross the redacted error boundary.
- Secret-bearing create is denied on the remote core WebSocket. Follow-up
  status/list/model operations are secret-free.
- Compensation proves ownership through the generated account locator before
  removing a credential; it cannot target another active connection's account.

## 9. Documentation and operations

Updated:

- `architecture/provider.md` — Eggpool probe and daemon connection ownership;
- `architecture/auth.md` — protected-store prerequisites and secret boundary;
- `architecture/protocol.md` — provider DTOs and local-only create operation;
- `architecture/tui.md` and `architecture/command.md` — `/connect` flow,
  masking, cancellation, scope, and session-selection boundary;
- `architecture/server.md` — remote secret-operation denial;
- `architecture/storage.md`, `architecture/jobs.md`, and
  `architecture/workspace_services.md` — v26 layout references and tables;
- source implementation plan, subsystem roadmap, registry, and this closure
  record.

Operational recovery is automatic on daemon first use for stale staged/probing
operations. The status/list/model core operations provide bounded diagnostics;
periodic health refresh and credential rotation remain Milestone 004 scope.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No full interactive CoreClient/TUI fake-server harness drives every keyboard step end to end. | The vertical service and focused TUI form/task paths are covered, but one integrated regression harness would reduce UI wiring risk. | Add the harness when Milestone 003 expands connection/model projections; do not reopen the durable provisioning boundary. |
| medium | TUI currently exposes personal-local scope only. | Project/deployment provisioning is service/protocol-ready but cannot be selected from a context without authoritative project/deployment identity. | Enable project/deployment scope when project-aware TUI/context and authorization interfaces are closed. |
| low | TLS certificate and unreachable network paths are covered by stable classification/mapping but not by live network fixtures. | Environment-independent tests avoid external network and certificate setup. | Add deterministic transport seams or local TLS fixtures in later probe-hardening work if the provider crate exposes them. |

No critical or high-severity finding remains. The medium findings are bounded
evidence/context limitations explicitly allowed by the source plan's stop
conditions and do not invalidate the local personal connection capability.

## 11. Roadmap disposition

Milestone 2 is closed and the next hard dependency is unlocked. Milestone 3,
“Session and model selection by connection,” has been authored and registered
as dependency-ready. Milestone 4 remains not started and blocked on Milestone
3. Runtime Assets 001 and Project Catalog 001 remain independently ready.
Multi-Project TUI 001 and Session Projections 001 remain blocked on their
existing catalog/identity/TUI dependencies; this closure does not unlock them.

## 12. Registry updates

- Source plan `002-eggpool-connect-workflow.md` is marked `implemented` and
  links this closure record.
- Provider Connections Milestone 2 is removed from the dependency-ready table
  and recorded under recently closed work.
- Provider Connections Milestone 3 is registered as the sole newly unlocked
  provider-connections handoff plan.
- The provider-connections roadmap pointer advances to Milestone 3 and marks
  Milestone 2 closed.
- No unrelated blocked plan is changed.
