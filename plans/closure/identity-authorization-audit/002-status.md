# Identity, Authorization, and Audit Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/identity-authorization-audit/002-transport-authentication-principal-binding.md`

Source subsystem roadmap:

- `plans/subsystems/identity-authorization-audit-roadmap.md#M002--transport-authentication-and-principal-binding`

Repository baseline reviewed: `d1e2deaacf3e4ffc8c677c80ffd880881426d0eb`

Implementation commits:

- `d1e2deaa` — M002 transport authentication and principal binding, migration v52, executable auth matrices, restart/revocation/expiry tests, identity/server/core architecture docs.

## 1. Executive finding

M002 is closed. Every accepted connection resolves to a canonical
`AuthenticatedPrincipal` from trusted transport evidence and carries that
immutable principal through client/request context, while personal-local
startup stays login-free. Personal tokens (`cggt_<token_id>.<secret>`)
verify against a SHA-256 digest store with create/revoke/expire semantics;
`ClientRegistry` binds one immutable principal per connection; HTTP/WS
fail closed; the local Unix socket binds `LocalOwner` with no login; the
legacy global bearer remains only as a bootstrap compatibility credential
mapping to `LocalOwner`. Projection contexts use the bound canonical
principal (never the `"authenticated-remote"` synthetic, never a
payload-supplied value). Secrets are absent from logs/events. No
unresolved high, medium, or low M002 finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Auth result/session + token storage semantics on M001 types (work package A) | `crates/codegg-core/src/transport_auth.rs`: `AuthenticatedPrincipal`, `RequestAuthorityContext`, `AuthMethod`, `TransportClass`; `migrate_v52` (`personal_auth_token`), `STORAGE_LAYOUT_VERSION` 52 | pass | Immutable bindings; request DTOs remain locators (protocol has no principal fields). |
| Personal-token create/revoke/expire/verify without logging secrets (work package B) | `PersonalTokenStore::{create_personal_token, verify_personal_token, verify_for_client, revoke_personal_token}`; SHA-256 digest store, constant-time compare, `redact_presented_token`, `Debug` omits digest | pass | Plaintext returned once; only digest persists; revocation monotonic via `COALESCE`; expiry checked on every verification. |
| HTTP/WS + `ClientRegistry` binding; replace synthetic projection identity (work package C) | `src/core/client_registry.rs`: `register_with_principal`/`set_principal` (immutable)/`principal_for`; `src/server/middleware/auth.rs::resolve_bearer_principal`; `src/server/ws.rs::validate_ws_auth` + per-connection registration; `ProjectionAccessContext::from_canonical_principal`; `CoreDaemon::{request_authority_for_client, projection_access_for_client}` | pass | Distinct tokens bind distinct principals; projection uses canonical id (`LocalOwner` → `"local-user"`), never `"authenticated-remote"`. |
| Local IPC/inproc/stdio binding; zero-login LocalOwner (work package D) | `src/core/transport/daemon_socket.rs` ClientHello binds `LocalOwner` via `bind_local_owner` (pool-backed `ensure_local_owner`, fallback to anonymous local); `RequestAuthorityContext::local` fallback for inproc/stdio/`local-daemon` | pass | No login ceremony; socket file remains user-scoped; `SO_PEERCRED` per-connection UID check explicitly deferred as future hardening. |
| Global-bearer disposition + fail-closed tests (work package E) | `AuthenticatedPrincipal::bootstrap_global_bearer` + `is_bootstrap_compatibility`; `is_personal_token_presentation` routing; server docs removal condition (delete shared secret once personal tokens issued) | pass | Global bearer always maps to `LocalOwner`; never masquerades as distinct identities; no-credential → 503, wrong/unknown/revoked/expired → 401. |
| LocalOwner resolution test | `local_owner_binding_needs_no_login` | pass | `bind_local_owner` idempotent, no credential. |
| Token create/revoke/expire/restart tests | `personal_token_create_verify_round_trip`, `personal_token_revoke_fails_new_authentication`, `personal_token_expiry_fails_closed`, `personal_token_lifecycle_survives_restart` (file-DB close/reopen/remigrate) | pass | Restart-safe create→verify→revoke→verify across two reopens. |
| Wrong-token timing-safe negative | `personal_token_wrong_secret_fails_closed_timing_safe` (constant-time `ct_eq` over digests) | pass | Tampered/unknown/empty presentations fail closed. |
| Network no-token fail closed | `network_no_token_fails_closed` (`tests/identity_m002_transport_auth.rs`) | pass | No credential → 503; wrong bearer and unknown personal token → 401. |
| Distinct clients/principals | `distinct_clients_bind_distinct_principals`, `two_remote_clients_authenticate_as_different_principals` | pass | Alice/Bob bind distinct canonical ids end-to-end through `resolve_bearer_principal`. |
| Principal cannot be spoofed in payload | `principal_binding_is_immutable_for_connection`, `client_payload_cannot_select_its_principal` | pass | `set_principal` rejects rebinding; daemon trusts closure `client_id`, not `Subscribe.client_id` or `ClientHello` name. |
| Projection context uses bound principal | `projection_context_uses_bound_canonical_principal`, `daemon_projection_context_uses_bound_principal` | pass | Bound remote principal yields canonical id; local yields `"local-user"`; synthetic never produced. |
| Compatibility-token disposition | `bootstrap_global_bearer_maps_to_local_owner_only`, `authenticated_principal_is_immutable_and_secret_free` | pass | Bootstrap maps to `local-owner`/`local-user`, flagged compat; personal-token prefix never matches global path. |
| Secrets absent from events/logs | `token_record_debug_omits_digest`, `redact_presented_token_never_echoes_secret`, `auth_events_carry_no_secrets` | pass | `Debug` omits digest; redaction preserves kind only; principal JSON has no secret-bearing names. |
| Disabled principal cannot authenticate | `disabled_principal_cannot_authenticate` | pass | `set_principal_status(Disabled)` → `PrincipalNotActive` on next verification. |
| Migration v52 additive/idempotent | `migrate_v52` (`CREATE TABLE IF NOT EXISTS` + indexes) + `storage_migrations` suite green | pass | No existing table altered; forward and restart safe. |

## 3. Production implementation evidence

- `crates/codegg-core/src/transport_auth.rs` (new, ~1100 lines): canonical
  auth types, `PersonalTokenStore` daemon-owned SQLite service, local-owner
  resolver, constant-time verification, redaction, 15 focused tests
  (lifecycle, contention-free revocation, expiry, restart, isolation,
  negatives, projection convergence, secret-negative).
- `crates/codegg-core/src/session/schema.rs`: additive `migrate_v52`
  creating `personal_auth_token` (digest/owner/expiry/revocation, indexes);
  wired into the version dispatcher (`51 → 52`).
- `crates/codegg-core/src/storage/mod.rs`: `STORAGE_LAYOUT_VERSION` 51 → 52.
- `crates/codegg-core/Cargo.toml`: `subtle = "2"` for constant-time digest
  comparison (boundary-clean: no UI/server/plugin deps).
- `crates/codegg-core/src/lib.rs`: `pub mod transport_auth;`
  (boundary-clean).
- `crates/codegg-core/src/projection_replay/context.rs`:
  `from_canonical_principal` convergence constructor; legacy
  `with_projects` retained for historical contexts only.
- `crates/codegg-core/src/team.rs` module docs: M002 contract now points at
  `transport_auth` instead of "not implemented".
- `src/core/client_registry.rs`: `principal: Option<AuthenticatedPrincipal>`
  on `ConnectedClient`; `register_with_principal`, immutable
  `set_principal`, `principal_for`; 2 new binding tests.
- `src/server/middleware/auth.rs`: `resolve_bearer_principal` (personal
  token → distinct principal; legacy bearer → `LocalOwner` bootstrap;
  fail-closed 503/401); per-request principal in Axum extensions;
  auth-disabled still carries explicit `LocalOwner`.
- `src/server/ws.rs`: async `validate_ws_auth` with pool + connection id;
  `/ws`, `/tui`, `/core` all validate, bind via `for_connection`, register
  in `ClientRegistry`, and unregister on close.
- `src/server/mod.rs`: `pub mod middleware` so integration tests exercise
  the production resolver.
- `src/core/transport/daemon_socket.rs`: ClientHello binds `LocalOwner`
  (pool-backed `ensure_local_owner`, warn-and-fallback without pool) and
  registers with principal; payload supplies only display name.
- `src/core/daemon.rs`: `request_authority_for_client` and
  `projection_access_for_client` (registry principal or `LocalOwner`
  fallback); artifact-read path now uses the bound principal instead of a
  synthetic local context.
- `src/core/mod.rs` inproc/stdio paths: unchanged code, now covered by the
  documented `LocalOwner` fallback (zero-login preserved).
- `Cargo.toml`: `[[test]] identity_m002_transport_auth` (`required-features
  = ["server"]`); `tests/identity_m002_transport_auth.rs` (7 integration
  tests across the HTTP/WS seam and registry).
- `scripts/check_project_catalog_invariants.py`: expected
  `STORAGE_LAYOUT_VERSION` 50 → 52 (guard was stale since M001).
- Docs: `architecture/identity.md` (M002 section + login-free preamble),
  `architecture/server.md` (auth + operator token lifecycle + bootstrap
  removal condition), `architecture/core.md` (registry principal),
  `src/server/middleware/auth.rs` rustdoc (resolution order + removal).

Distinguished as absent (downstream milestones, not M002 scope): OIDC/device
login, project authorization decisions and attribution propagation (M003),
node PKI, audit store (M004/M005), `SO_PEERCRED` per-connection UID
validation (explicitly deferred hardening, not a correctness gap for the
declared single-user-socket trust model).

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib transport_auth
cargo test --lib core::client_registry
cargo test --features server --test identity_m002_transport_auth
cargo test --test storage_migrations
cargo test -p codegg-core --lib
cargo test --features server --lib server::
cargo fmt --all -- --check
cargo clippy -p codegg-core --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
bash scripts/check-core-boundary.sh
python3 scripts/check_project_catalog_invariants.py --verbose
bash scripts/verify.sh quick
```

The plan's literal `cargo test --workspace auth` / `--workspace transport`
name non-existent workspace crates; the equivalent narrowest-owner
invocations above (`-p codegg-core transport_auth`, `--lib
core::client_registry`, `--features server --test
identity_m002_transport_auth`) are the justified substitutes and are
recorded here without concealment.

### Results

- `cargo test -p codegg-core --lib transport_auth`: pass, 15 passed
  (round-trip, tamper negative, revoke, expiry, distinct, disabled,
  projection convergence, restart across two file-DB reopens, redaction,
  immutability).
- `cargo test --lib core::client_registry`: pass, 8 passed (existing 6 +
  immutability + register-with-principal).
- `cargo test --features server --test identity_m002_transport_auth`:
  pass, 7 passed (fail-closed, distinct remotes, bootstrap disposition,
  spoof negative, projection binding, revoke-new-auth, secret-negative).
- `cargo test --test storage_migrations`: pass, 4 passed.
- `cargo test -p codegg-core --lib`: pass, 560 passed, 0 failed.
- `cargo test --features server --lib server::`: pass, 25 passed.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy -p codegg-core --all-targets -- -D warnings`: pass
  (one M002 `type_complexity` lint found during development — extracted
  `PersonalTokenRow` alias — fixed before final run).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  pass, 0 warnings.
- `bash scripts/check-core-boundary.sh`: pass.
- `python3 scripts/check_project_catalog_invariants.py --verbose`: pass,
  7/7 (including `STORAGE_LAYOUT_VERSION is 52`).
- `bash scripts/verify.sh quick`: pass (`==> Quick verification passed.`).

All evidence above is local execution and is labeled accordingly. No hosted
`CI / verify` run is attached; routine CI remains the operator action per
the closed development-verification roadmap.

## 5. Invariant review

- Network listeners fail closed: no credential → 503; wrong/unknown/
  revoked/expired/disabled → 401; verified by `network_no_token_fails_closed`
  and revocation/expiry/disabled tests.
- Payload cannot select principal: protocol has no principal fields;
  `Subscribe.client_id` and `ClientHello` name are ignored for authority;
  `set_principal` rejects rebinding; daemon resolves from registry closure
  id.
- LocalOwner only by trusted local policy: Unix-socket/stdio/inproc bind
  `LocalOwner` without credential; network bootstrap maps to `LocalOwner`
  but is flagged compat and never yields distinct identities.
- Secrets never in logs/events: digests omitted from `Debug`; plaintext
  returned once and never persisted; redaction helper preserves kind only;
  principal JSON has no secret-bearing names.
- Binding immutable for connection/auth session: no setters on
  `AuthenticatedPrincipal`; `for_connection` preserves identity while
  assigning ownership; registry rejects second distinct binding.
- Provider credentials separate: untouched; token store is team-identity
  only and never interprets provider records.

## 6. Failure and recovery review

- Duplicate delivery/idempotency: token ids are UUIDs; creation inserts once;
  revocation uses `COALESCE(revoked_at, ?)` so repeats converge; bootstrap
  uses `INSERT OR IGNORE` via `ensure_local_owner`.
- Revocation races: revocation fails *new* authentication immediately;
  existing connections keep their bound principal until they disconnect/
  re-auth by explicit contract (documented; no indefinite grant is minted
  after revocation because every new verification re-reads the row).
- Expiry: absolute `expires_at` compared on every verification; past-expiry
  tokens fail closed; no refresh extends a live token.
- Daemon restart: file-DB close/reopen/remigrate preserves tokens and
  revocations (`personal_token_lifecycle_survives_restart`); `ensure_local_owner`
  reconverges.
- Storage failure: verification maps pool errors to rejection (fail closed);
  HTTP surfaces 401/503 rather than anonymous success.
- Malformed input: non-`cggt_` shapes, missing separators, empty/long parts,
  unknown ids, control-char labels all rejected with typed errors.
- Bounded behavior: labels bounded (200), secrets fixed 32 bytes, digests
  fixed 64 hex, token ids bounded, registry bounded by connections, no new
  unbounded retention.

## 7. Migration and compatibility review

- Additive migration v52 (`CREATE TABLE IF NOT EXISTS` +
  `IF NOT EXISTS` indexes) inside the existing transactional
  `migrate_and_record` harness; forward and restart safe.
- `STORAGE_LAYOUT_VERSION` 51 → 52. No existing table altered; no data
  backfill; no rollback beyond the standard SQLite restore story.
- Legacy global bearer compatibility: still accepted when configured, but
  bound to `LocalOwner` with `is_bootstrap_compatibility() == true` and
  documented removal condition (delete the shared secret once personal
  tokens are issued). Personal-token presentations never match the global
  path (`is_personal_token_presentation` routing).
- Projection compatibility: `"local-user"` retained for `LocalOwner` via
  the M001 adapter; `"authenticated-remote"` is never produced on the M002
  path and remains classified compat by `is_compatibility_projection` for
  historical contexts only.
- No provider-auth migration; no protocol wire break (principal travels in
  registry/extensions, not in DTOs).

## 8. Security review

- Authorization not enforced by design (M003 owns it); this milestone binds
  authentication only and documents the non-claim. Capability evaluation is
  untouched.
- Constant-time token verification (`subtle::ct_eq` over hex digests);
  length mismatch fails without content disclosure beyond length.
- Fail-closed defaults on every listener (HTTP middleware, WS upgrade,
  token store, disabled-principal, missing pool rows).
- Privilege boundaries: only `Active` principals verify; `Disabled`
  principals fail; `LocalOwner` is an explicit principal, not a bypass;
  bootstrap compat cannot yield distinct team identities.
- Denial-of-service bounds: bounded labels/secrets/ids, indexed lookups,
  bounded WS queues and rate limits unchanged, no new network or spawn
  surface.
- `codegg-core` boundary guard passes; only `subtle` added (no
  UI/server/plugin deps).

## 9. Documentation and operations

Updated:

- `architecture/identity.md` — M002 section (principal/context types, token
  lifecycle, binding, convergence, bootstrap disposition, trust limits) and
  login-free preamble.
- `architecture/server.md` — auth resolution order, bootstrap removal
  condition, operator personal-token lifecycle (create → present →
  revoke/expire → delete shared secret).
- `architecture/core.md` — `ClientRegistry` principal ownership.
- `crates/codegg-core/src/team.rs` module docs — M002 contract now
  implemented in `transport_auth`.
- `src/server/middleware/auth.rs` rustdoc — resolution order, fail-closed
  codes, secret discipline.
- `plans/implementation/identity-authorization-audit/002-transport-authentication-principal-binding.md`
  — marked closed, linking this record.
- `plans/implementation/identity-authorization-audit/003-daemon-authorization-and-attribution.md`
  — unblocked to ready for handoff.
- `plans/subsystems/identity-authorization-audit-roadmap.md` — M002 closed,
  M003 ready.
- `plans/registry.md` — Identity row advanced to M003 ready; M003
  registered dependency-ready; M003 blocker row removed; M002 recorded
  under closure evidence.

Operator note: after upgrading, existing databases migrate to v52
automatically on next daemon start. Issue personal tokens per device;
revoke on loss; delete `server.token` / `CODEGG_SERVER_TOKEN` once every
operator holds a personal token. Personal-local daemons need no action and
no login ceremony.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None. | — | — |

There are no unresolved critical, high, medium, or low M002 findings.
`SO_PEERCRED` per-connection UID validation is deferred hardening (not a
correctness gap for the declared user-scoped-socket trust model) and needs
no corrective plan. OIDC/device login, M003 authorization, and audit remain
downstream scope, not M002 defects.

## 11. Roadmap disposition

Milestone closed and the next dependency may proceed. The registry audit
found exactly one registered plan whose hard dependency is now satisfied:
M003 (daemon authorization and attribution), whose sole blocker was M002
closure. M003 is registered `ready` in the same commit. M004 remains
blocked on M003; M005 on M004; presence M001 remains blocked on identity
M003; collaboration M001 remains blocked on identity M005 + presence M003.
No corrective pass is required and no new dependency-ready plan was created
beyond the M003 unblock.

## 12. Registry updates

Included in the closure commit alongside this record:

- M002 source plan marked closed, linking this record.
- Roadmap milestone table: M002 `closed` with closure link; M003 `blocked`
  → `ready`.
- M003 implementation plan: `blocked` → `ready for handoff` (sole blocker
  M002 now closed; `AuthenticatedPrincipal`/`ClientRegistry`/
  `RequestAuthorityContext`/projection-convergence seams already landed as
  stable in this milestone).
- Registry active-subsystem row: Identity current milestone M002 ready →
  M003 ready.
- Registry dependency-ready table: M002 row replaced by the M003 row
  (daemon authorization and attribution; M002 closure is the satisfied
  dependency).
- Registry blocked-work table: M003 row removed; M004/M005, presence, and
  collaboration rows retained unchanged.
- Registry closure-evidence table: Identity M002 row added pointing at this
  record.
