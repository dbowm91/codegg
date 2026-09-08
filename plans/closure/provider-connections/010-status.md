# Provider Connections Milestone 010 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connections/010-auth-capability-matrix-and-stored-bearer-closure.md`

Source subsystem roadmap:

- `plans/subsystems/provider-auth-capability-closure-addendum.md#4-milestone-010--auth-capability-matrix-and-stored-bearer-closure`

Repository baseline reviewed: `5b31fc1012fe77ad40efc5099ee4d8e91046cd25` (pre-work HEAD)

Implementation commits or pull requests:

- provider-auth M010 implementation + closure + registry updates in the commit carrying this record (see `git log --oneline -- plans/closure/provider-connections/010-status.md`).

## 1. Executive finding

M010 is strictly closed. Provider authentication support is now explicit and
executable: every built-in registration branch has a declared
`CredentialCapability`, stored `BearerToken` records resolve end-to-end for
compatible full-credential paths, incompatible combinations fail with typed
actionable errors before any network I/O, expiry/redaction/rotation/restart
semantics are preserved and tested, and documentation matches the executable
matrix. `ExternalCommand` and `OAuthDevice` remain explicitly unsupported. No
consumer-session/app-token integration was added.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Every built-in registration path has explicit capability | `credential_capability_for` + `builtin_registration_order()` in `crates/codegg-providers/src/provider_core.rs`; `capability_matrix_covers_every_registration_branch` (17 branches) | pass | 11 `ApiKeyOrBearer`, 6 `ApiKeyOnly`; unknown ids default conservative `ApiKeyOnly` |
| New providers must choose deliberately | table-driven test fails on unclassified branch; docs in `architecture/auth.md` + `architecture/provider.md` | pass | No silent bearer inheritance |
| Centralized kind-aware resolution | `ResolverContext.capability` set from matrix in `resolve_provider_credential`; no per-factory store branches | pass | Env/config priority unchanged; only store step is capability-filtered |
| Stored bearer resolves for compatible | `stored_bearer_resolves_under_compatible_capability`, `stored_bearer_resolves_for_compatible_provider` (xai), `fallback_bearer_resolves_under_compatible` | pass | Kind preserved, source `UserStore` |
| Stored bearer rejected for incompatible with typed error | `stored_bearer_rejected_under_api_key_only_with_typed_error`, `fallback_bearer_rejected_under_api_key_only`, `stored_bearer_rejected_for_api_key_only_providers` (all 6 `ApiKeyOnly` ids) | pass | `AuthError::Unsupported`, not `NotFound`; no secret in message |
| Expired bearer fails before transport | `expired_bearer_fails_before_transport`, `stored_expired_bearer_rejected_for_compatible_provider` | pass | `AuthError::Expired`, secret not leaked |
| Expired API key fails before transport | `expired_api_key_fails_before_transport` | pass | New enforcement; previously unchecked in resolver (see §7) |
| API-key behavior unchanged | `stored_api_key_resolves_under_both_capabilities`, `stored_api_key_still_resolves_for_api_key_only_provider`, existing precedence tests green | pass | Full providers test suite 127/127 |
| Kind preserved through registration/transport | `simple_with_credential_preserves_*`, `openai_compatible_factory_preserves_bearer_kind`, `stored_bearer_reaches_transport_with_bearer_header` (fake TCP capture, sentinel `m010-sentinel-bearer`) | pass | `Authorization: Bearer …` for either kind on compatible transport |
| API-key-only never reaches transport | Resolver returns `Unsupported` before provider construction; `ensure_api_key_credential_rejects_bearer_token`; registration skip path | pass | Zero outbound by construction; no bearer reinterpretation |
| Config-aware family classified | All of anthropic/openai/google/openrouter declared `ApiKeyOnly` with rationale (String contract; Bearer-wire ≠ bearer lifecycle) | pass | Conservative; future migration needs separate decision |
| Durable kind-preserving resolution | `adapter_round_trip_preserves_kind_and_expiry_without_exposing_secret`, `bearer_resolves_after_store_reopen_for_compatible_connection` | pass | Restart/reopen retains kind+expiry |
| Durable incompatible rejection | `openai_and_anthropic_reject_bearer_tokens_like_existing_registration`, `incompatible_kinds_reject_bearer_before_network` (openai/anthropic/google/azure) | pass | `UnsupportedCredentialKind`, no secret |
| Compatible durable accepts bearer | `compatible_connection_accepts_bearer_and_preserves_kind` | pass | `OpenAiCompatible` builds with bearer |
| Rotation fails before commit | `validate_rotation_kind` + `rotation_guard_fails_before_commit_for_incompatible_kind` | pass | Caller validates before store overwrite; factory re-validates at build |
| In-flight retains captured revision | `in_flight_instance_retains_captured_credential_across_failed_rotation` + existing `ConnectionManager` cache tests | pass | Old `Arc` untouched by failed rotation |
| Redaction | `credential_debug_masks_secret`, `bearer_debug_masks_secret`, error-message assertions (no sentinel in `Display`) | pass | Fixed 16-bullet mask; no prefix/suffix |
| ExternalCommand/OAuth unsupported | `external_command_and_oauth_remain_unsupported` | pass | Both resolver arms + `ExternalCommandProvider::fetch` return `Unsupported` |
| Docs match matrix | `architecture/auth.md` (capability section, matrix table, store/Durable updates), `architecture/provider.md` (capability wiring), `codegg.example.jsonc` stored comment | pass | Removed “all stored bearer unsupported” claim |
| No consumer-session tokens | No new auth variants, no session-reuse code, no scraping | pass | Out-of-scope items untouched |
| No schema migration | `StoredCredentialRecord` serialization unchanged; kind already persisted | pass | `store_reopen_retains_kind_and_expiry` proves compat |

## 3. Production implementation evidence

Ownership: `codegg-providers` (`auth_types`, `provider_core`, `connection`).

- `crates/codegg-providers/src/auth_types.rs`
  - New `CredentialCapability::{ApiKeyOnly, ApiKeyOrBearer}` (default
    `ApiKeyOnly`) with `accepts()`/`as_str()`, plus
    `incompatible_credential_message()` (no secret material).
  - `ResolverContext.capability` (default `ApiKeyOnly`).
  - `CredentialStore::find_record()` (deterministic exact-match metadata;
    one record per binding via `put`-replace) and
    `CredentialStore::get_credential()` (full `Credential` with kind+expiry;
    `Ok(None)` without master key, matching `get_plaintext` semantics).
  - `AuthResolver::resolve`: `Stored` arm and no-auth fallback store step
    now check expiry → `Expired`, capability → `Unsupported`, then decrypt
    to a kind-preserving `Credential` (`UserStore`). Env/config/legacy
    priority order unchanged.
- `crates/codegg-providers/src/provider_core.rs`
  - `credential_capability_for()` executable matrix + `builtin_registration_order()`
    (17 ids in registration order). Compatible: mistral, groq, deepinfra,
    cerebras, cohere, together, perplexity, xai, venice, opencode_go,
    generalcompute. Incompatible (`ApiKeyOnly`): anthropic, openai, google,
    openrouter, opencode_zen, minimax. Unknown → `ApiKeyOnly`.
  - `resolve_provider_credential` sets `capability` from the matrix
    (centralized; helpers add no per-provider branches).
  - `ensure_api_key_credential` retained as defense-in-depth.
- `crates/codegg-providers/src/connection.rs`
  - `capability_for_provider_kind()` (durable mirror: only
    `OpenAiCompatible` is `ApiKeyOrBearer`) and `validate_rotation_kind()`
    pre-commit guard.
  - `ProviderConnectionFactory` behavior unchanged in structure: native
    kinds `require_api_key`, compatible preserves `Credential` — now with
    explicit matrix and guard.
- `crates/codegg-providers/src/lib.rs`, `src/auth/mod.rs`: re-export new
  capability APIs.
- Transports untouched (no normalization): `OpenAiCompatibleProvider`
  already sends `Authorization: Bearer …` from
  `authorization_header_value()` for either kind; `AnthropicProvider`
  (`x-api-key`), `GoogleProvider` (`x-goog-api-key`), `AzureProvider`
  (`api-key`) remain `String`-contract `ApiKeyOnly`. OpenAI-native,
  OpenRouter, and Zen wire Bearer for long-lived keys but keep the
  `ApiKeyOnly` factory contract for this milestone (documented).

Final provider × credential capability matrix:

| Provider | Helper / durable kind | Capability |
|---|---|---|
| anthropic | `register_config_provider` / `Anthropic` | `ApiKeyOnly` |
| openai | `register_config_provider` / `OpenAi` | `ApiKeyOnly` |
| google | `register_config_provider` / `Google` | `ApiKeyOnly` |
| openrouter | `register_config_provider` | `ApiKeyOnly` |
| opencode_zen | `register_api_key_provider` | `ApiKeyOnly` |
| minimax | `register_api_key_provider` | `ApiKeyOnly` |
| mistral, groq, deepinfra, cerebras, cohere, together, perplexity, xai, venice, opencode_go, generalcompute | `register_credential_provider` / `OpenAiCompatible` | `ApiKeyOrBearer` |

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-providers --lib auth_types
cargo test -p codegg-providers --lib provider_core
cargo test -p codegg-providers --lib connection
cargo test -p codegg-providers --lib openai_compatible
cargo test -p codegg-providers --lib
cargo test -p codegg --lib auth
cargo test --test provider_connections_lifecycle
cargo test -p codegg --lib core::provider_connections
cargo fmt --all -- --check
cargo clippy -p codegg-providers --all-targets --all-features -- -D warnings
scripts/verify.sh quick
bash scripts/check_provider_connections_m4_coverage.sh
bash scripts/check_provider_connections_tombstone_compat.sh
```

### Results

All local; no external provider credentials or real network servers used
(synthetic sentinels + loopback TCP capture only).

```text
cargo test -p codegg-providers --lib auth_types        14 passed
cargo test -p codegg-providers --lib provider_core     32 passed
cargo test -p codegg-providers --lib connection        14 passed
cargo test -p codegg-providers --lib openai_compatible 10 passed
cargo test -p codegg-providers --lib                  127 passed, 0 failed
cargo test -p codegg --lib auth                        34 passed
cargo test --test provider_connections_lifecycle        1 passed
cargo test -p codegg --lib core::provider_connections  3 passed
cargo fmt --all -- --check                             passed
cargo clippy -p codegg-providers --all-targets --all-features -- -D warnings  passed
scripts/verify.sh quick                               passed (fmt, agent check, core boundary, sandbox, execution-ownership, workspace check --locked)
check_provider_connections_m4_coverage.sh             ok
check_provider_connections_tombstone_compat.sh        ok
```

No hosted `CI / verify` run: the addendum requires only the bounded local
contract and no auth-specific CI lane; closure does not depend on external
operational evidence.

## 5. Invariant review

- API-key env/config/store resolution order unchanged except the store step
  now applies the capability selector (explicitly required by the plan).
- Secrets masked in `Debug`, errors, logs, protocol output, tests: `Debug`
  uses fixed mask; `incompatible`/`Expired`/`NotFound` messages carry only
  provider/account/capability labels; capture tests use sentinels and assert
  absence from error strings.
- Master-key/encrypted-store behavior unchanged: `put` still requires key;
  `get_credential` returns `Ok(None)` without key like `get_plaintext`;
  `Stored` without key preserves historical `NotFound` (not a new
  fallback); no plaintext persistence added.
- Expired credentials fail before transport: both `Stored` and fallback
  return `Expired` from metadata before decryption/network, for both kinds.
- Full-credential providers preserve kind (no downcast to string +
  `api_key` reconstruction): resolver returns full `Credential`;
  `register_credential_provider` forwards it; transport uses
  `authorization_header_value()`.
- API-key-only providers never reinterpret bearer as API key:
  resolver-level `Unsupported` + `ensure_api_key_credential` defense.
- Unsupported auth is typed (`Unsupported`), not silent fallback.
- `ExternalCommand`/`OAuthDevice` remain `Unsupported`.
- Registration/fallback contract otherwise unchanged; no auto-registration
  redesign.
- Durable connections store only secret references/metadata; descriptors
  serialize secret-free (existing + retained tests).

## 6. Failure and recovery review

- Missing credential: `NotFound` (Stored) / `Ok(None)` (fallback) preserved.
- Incompatible kind: `Unsupported` with actionable message suggesting an
  API-key credential for the same binding; no transport contact; no
  fallback to unrelated env key (explicit binding preserved).
- Expired: `Expired` before decrypt/network for both arms and both kinds.
- Master key missing: `get_credential` → `Ok(None)`; `Stored` → `NotFound`
  (historical), fallback → `Ok(None)` so env/config still work; never
  plaintext fallback.
- Rotation failure: `validate_rotation_kind` fails before store overwrite;
  factory `UnsupportedCredentialKind` fails before network at build;
  `ConnectionManager` revision cache keeps old `Arc` valid for in-flight
  requests (tested).
- Restart: store reopen retains kind/expiry (tested); connection rebuild is
  lazy per `(connection_id, revision)` with single-flight coalescing
  (existing manager tests).
- Contention: no new auth cancellation contract; credential captured at
  resolution/construction; kind changes never mutate in-flight requests.
- Malformed input: descriptor validation (secret-bearing URLs, empty ids)
  unchanged and tested.

## 7. Migration and compatibility review

- No schema migration: `StoredCredentialRecord` already carried `kind` +
  `expires_at`; serialization unchanged; reopen test proves
  forward/backward compat.
- Existing API-key config/env/store precedence unchanged; legacy
  `api_key`/`encrypted_api_key` still resolve via the single path
  (existing tests green).
- `AuthConfig::Stored` is more capable for `ApiKeyOrBearer` paths
  (backward-compatible expansion).
- `ApiKeyOnly` + explicitly bound bearer now reports `Unsupported` instead
  of generic `NotFound`: intentional diagnostic improvement per plan.
- `ExternalCommand`/`OAuthDevice` configs still parse but fail at runtime
  as documented; variants retained.
- One behavior tightening: the resolver previously ignored `expires_at`
  for stored records (both kinds); it now returns `Expired` before
  transport. This is the plan-required invariant (“expired credentials fail
  before provider transport invocation”), not a migration; no stored format
  change is involved. No evidence of relied-upon expired-credential success
  was found (durable adapter already enforced expiry).

## 8. Security review

- Bearer values never printed: fixed-width `mask_secret`, `Debug` masking
  for both kinds, error strings asserted secret-free, CLI `status` shows
  kind/expiry metadata only, `set-key` never echoes.
- No bearer in query params/URLs: descriptor validation rejects
  userinfo/query/fragment; transports put credentials only in headers.
- Authorization headers built by transport from the `Credential` contract;
  tests inspect headers with synthetic sentinels only.
- Expired rejected before network; incompatible causes zero outbound.
- No fallback from an explicit stored bearer binding to unrelated env key.
- `get_plaintext` retained for non-provider callers (eggpool uses `|_| true`);
  provider paths use kind-aware `get_credential`/`find_record`.

## 9. Documentation and operations

- `architecture/auth.md`: capability-scoped stored bearer semantics,
  resolver order with `ctx.capability`, registration matrix table,
  `CredentialCapability`/`get_credential`/`find_record` API, `ResolverContext`
  capability field, durable `capability_for_provider_kind` +
  `validate_rotation_kind` + revision-cache note; removed the “all stored
  bearer unsupported” statement; kept ExternalCommand/OAuth limitations.
- `architecture/provider.md`: capability wiring per helper, matrix
  ownership, centralized-selection rule.
- `codegg.example.jsonc`: `stored` comment now states bearer scope
  (11 compatible providers), typed rejection, and expiry behavior.
- `codegg auth status` already showed `api_key`/`bearer` kind metadata
  correctly; no CLI scope expansion (per plan, test fixtures exercise the
  store directly).
- No new static guard: the table-driven
  `capability_matrix_covers_every_registration_branch` test is the
  executable guard against reintroducing global API-key-only filtering or
  unclassified providers (allowed by the plan).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

Provider-specific limitations carried forward (not defects): OpenAI-native,
OpenRouter, and Zen wire `Authorization: Bearer …` for long-lived API keys
but remain `ApiKeyOnly` by factory contract; claiming short-lived bearer
lifecycle for them needs a separate architecture decision and factory
migration (plan stop condition respected). Config-only `String` factories
(`sap_ai_core`, `zenmux`, `kilo`, `vercel_ai_gateway`) are not auto-registered
and inherit the conservative `ApiKeyOnly` default if ever wired.

## 11. Roadmap disposition

- Milestone M010 closed; the provider-auth-capability-closure addendum meets
  its completion definition (executable matrix, compatible stored-bearer
  support, explicit incompatible rejection, unchanged API-key behavior,
  redaction, truthful docs).
- Next dependency: none registered. The dependency audit (registry Blocked
  work + subsystem dependency graphs) found no registered future plan with a
  hard or interface dependency on provider-auth M010; therefore no future
  plan was unblocked or had its status changed.
- Deferred OAuth device/PKCE and external-command work remains intentionally
  unregistered and requires independent product/security justification per
  the addendum.
- Predecessor M009 closure remains immutable and unaffected.

Final recommendation: **closed**.

## 12. Registry updates

- `plans/registry.md`: provider-auth row `active/M010 ready` → `closed/M010
  closed`; remove M010 from dependency-ready plans; record M010 under
  recently closed control points.
- `plans/subsystems/provider-auth-capability-closure-addendum.md`: status
  `active` → `closed`; M010 status `ready` → `closed`.
- `plans/implementation/provider-connections/010-auth-capability-matrix-and-stored-bearer-closure.md`:
  status `ready for handoff` → `implemented`.
