# Provider /connect Restoration M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connect-restoration-corrective/002-provider-catalog-and-neutral-provisioning.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connect-restoration-corrective-addendum.md#m002--provider-catalog-and-provider-neutral-provisioning`

Repository baseline reviewed: `d1d2c722`

Implementation commits or pull requests:

- `b532ef84` — feat(providers): catalog and neutral provisioning (provider-connect M002)
- `189eb37b` — fix(providers): namespace rotation/refresh credentials by connection binding

## 1. Executive finding

M002 is complete. One ordinary direct provider (`openai`, static
`Provider::models()` path with no network) and Eggpool (strict
compatible `/models` probe against a loopback fake server) are both
provisioned by the same generic
`ProviderConnectionProvisioner::create_connection` service and the same
`CoreRequest::ProviderConnectionCreate` protocol authority. Every ID in
`builtin_registration_order()` (17/17) has an explicit setup disposition
in the new pre-credential catalog, every connectable definition builds
through the canonical durable builder, and all M002-era
secret/transaction/recovery invariants still pass with no schema
migration. No unresolved findings; no corrective pass required.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Non-secret `ProviderDefinition`/`ProviderCatalog` layer in `codegg-providers`, separate from the live-instance registry | `crates/codegg-providers/src/setup_catalog.rs`: `ProviderDefinition`, `provider_setup_catalog()`, `setup_definition()`; typed `SetupEndpointPolicy` (Fixed/OptionalOverride/RequiredEndpoint/ProxyPreset), `SetupConstruction`, `SetupProbeStrategy` | pass | ID/display/connectable/capability/endpoint/construction/probe/env-hint/description; no secrets or authz |
| Startup registration and durable construction converge on the same definitions/builders | `credential_capability_for()` delegates to the catalog; fixed base-URL constants owned by the catalog and referenced by `additional.rs`; `ProviderConnectionFactory::build` delegates to `build_durable_provider()` | pass | Single construction policy; see §3 |
| Exhaustive coverage: every `builtin_registration_order()` ID has an explicit setup disposition | `catalog_covers_builtin_registration_order_with_explicit_disposition` (17/17 connectable, capability agrees with registration matrix) | pass | — |
| Exhaustive coverage: every connectable definition has an executable durable factory | `every_connectable_definition_builds_through_the_durable_builder` (20/20 incl. eggpool/custom/azure; bearer disposition pinned per definition) | pass | — |
| Pinned capability matrix (no silent bearer inheritance) | `catalog_capability_matrix_is_pinned` (20 rows) | pass | — |
| Specialized implementations keep their builders (no generic coercion) | `specialized_builders_keep_their_implementation_identity` (xai, opencode_go, minimax, openrouter, zen, mistral, anthropic, openai, google, azure, eggpool) + `legacy_and_catalog_kinds_resolve_through_durable_factory` (13 stored kinds incl. `other:*`) | pass | xAI custom config, Go affinity header, MiniMax/OpenRouter/Zen native transports preserved |
| Eggpool as OpenAI-compatible proxy preset (user endpoint, TLS normalization, default port 11300) | Catalog `eggpool` entry (`ProxyPreset{11300}`); preset normalization reuses host/port/TLS logic with per-preset port; `omitted_port_uses_eggpool_default_and_v1_path` et al. still green | pass | — |
| Generic OpenAI-compatible/custom upstream (required endpoint, API-key/bearer) | Catalog `custom` entry (`RequiredEndpoint`, `ApiKeyOrBearer`); `generic_custom_compatible_provision_succeeds` through the generic service | pass | No 11300 default imposed on custom URLs |
| Generic provisioning service (`CreateProviderConnectionRequest`, `ProviderConnectionProvisioner`) with the M002 sequence | `ProviderConnectionProvisioner::create_connection` (`src/core/eggpool.rs`): validate/normalize → staged journal → operation-owned credential write → bounded probe → one final transaction | pass | `EggpoolProvisioner` retained as a compatibility alias |
| Operation IDs/idempotency, cancellation, ownership-aware compensation, restart reconciliation, duplicate detection | Unchanged machinery, now provider-keyed (`provider_kind` in duplicate check and staged row; idempotency hashes provider+endpoint+scope); tests below | pass | — |
| No network I/O inside the final SQLite transaction | Both probe strategies run before `finalize()`; `finalize()` is pure SQLite | pass | — |
| Local-only secret-bearing create boundary | `CreateProviderConnectionRequest.credential: SecretInput`; remote WS denies via `is_secret_bearing()` | pass | — |
| Redacted stable failure codes | `error_code()` extended (`unsupported_provider`, `unsupported_credential_kind`); probe taxonomy unchanged; `direct_probe_error_mapping_is_bounded` | pass | — |
| No schema migration | `provider_provisioning.provider_kind` already generic; staged/final rows bind the storage key (`eggpool`/`openai`/…/`openai_compatible`/`other:{id}`) | pass | `STORAGE_LAYOUT_VERSION` untouched |
| Typed probe strategy per definition; ordinary = `Provider::models()` with cancel/timeout; compatible = strict `/models` probe, de-branded | `probe_direct_models()` + `Compatible*` aliases in `providers::eggpool`; `SetupProbeStrategy` per definition | pass | Direct errors map Auth→`authentication_failed`, Timeout→`probe_timeout`, else `unsupported_api` |
| No dependency on unpublished Eggpool application crates | No new dependencies; Eggpool ideas extracted into generic CodeGG machinery | pass | — |
| Generic core request/response; Eggpool-named request only as compat adapter | `CoreRequest::ProviderConnectionCreate`, `CoreResponse::ProviderConnectionCreated` (+ `ProviderSetupList`/`ProviderSetupList` secret-free catalog); `From<CreateEggpoolConnectionRequest>` adapter; `create()` delegates to `create_connection()` | pass | TUI untouched (M003 owns restoration) |
| Remote denial by secret-bearing semantics; guard fails on future variants | `CoreRequest::is_secret_bearing()`; `ws.rs` denies via the helper; `secret_bearing_variants_are_denied` pins the set | pass | — |
| Existing IDs/revisions/scopes/health/session selections unchanged | Storage keys additive (`other:{id}` reads via existing `Other` parsing); legacy `eggpool`/`openai_compatible` rows resolve through the new factory | pass | — |
| Rotation/refresh credential namespace follows the stored binding | `189eb37b`: staged/read/cleanup paths use `binding.provider_ref` (identical for Eggpool rows); `rotation_uses_the_connection_credential_namespace` | pass | Follow-up coherence fix inside this milestone |

## 3. Production implementation evidence

Ownership:

- `codegg-providers::setup_catalog` owns pre-credential definitions, fixed
  URL constants, and `build_durable_provider()`.
  `provider_core::credential_capability_for` reports the catalog; the
  `additional` fixed-URL constructors reference catalog constants;
  `connection::ProviderConnectionFactory` delegates builds to the catalog.
  `scripts/check-core-boundary.sh` passes (no new core/UI coupling).
- `src/core/eggpool.rs` owns the generic provisioning service
  (`ProviderConnectionProvisioner`, neon alias `EggpoolProvisioner`),
  catalog-driven normalization (`normalize_generic()`), endpoint
  validation (`validate_user_endpoint()` without preset port/path
  assumptions), probe dispatch, and the unchanged v26 journal/finalize
  shape with provider-keyed rows.
- `codegg-protocol::provider` owns `CreateProviderConnectionRequest`
  (redacted `Debug`), `ProviderTlsPolicy`/`ProviderConnectionScope`/
  `ProviderCredentialKind`, the `CreateEggpoolConnectionResult` result
  alias, and secret-free `ProviderSetupEntryDto`.
- `codegg-protocol::core` owns `ProviderConnectionCreate`,
  `ProviderSetupList`, and `is_secret_bearing()`. `daemon_providers.rs`
  serves both plus the secret-free catalog projection;
  `daemon_family.rs` routes them to Providers; `authorization/policy.rs`
  authorizes `provider_connection_create` (ProjectConfigure) and
  `provider_setup_list` (enumeration, no capability).

Storage keys: `eggpool`, `openai`, `anthropic`, `google`,
`azure_openai`, `openai_compatible` (generic custom), `other:{id}` for
every other catalog provider. `descriptor_for()` maps `Other(id)` to the
catalog builder instead of erroring; legacy rows are unaffected.

Docs: `architecture/provider.md` (catalog + neutral provisioning),
`architecture/protocol.md` (generic create/setup list, semantic denial),
`.opencode/skills/provider-auth/SKILL.md` (catalog ownership + hard rule).

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-providers --locked
cargo test -p codegg-protocol --locked
cargo test -p codegg --lib core::eggpool
cargo test -p codegg --lib core::provider_connections
cargo test -p codegg-core authorization
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test -p codegg --lib --locked --features server,plugins,lsp-test-support -- --test-threads=1
cargo fmt --all -- --check
git diff --check
bash scripts/check-core-boundary.sh
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --locked --features server,plugins,lsp-test-support -- -D warnings
scripts/verify.sh quick
```

### Results

All local. `codegg-providers`: 159 passed (8 new catalog tests).
`codegg-protocol`: 188 passed (3 new: secret-bearing guard, generic
request secrecy/round-trip, setup-list round-trip). `core::eggpool`: 22
passed (11 new generic: direct success, Eggpool-preset success, custom
success, invalid credential, unreachable, oversized, cancellation,
restart reconciliation, typed rejections, error mapping, rotation
namespace). `core::provider_connections`: 4 passed (1 new: 13 stored
kinds resolve with identity intact). `codegg-core authorization`: 21
passed. Full workspace: 4750 lib + all integration targets, 0 failed.
Feature-gated root lib: 4795 passed. fmt, diff-check, core-boundary,
both Clippy configurations, and `verify.sh quick` (fmt/agent-schema/
core-boundary/sandbox/execution-ownership/TUI-authority + workspace
check) all pass.

Deviation from the plan's §10 command list: Clippy ran without
`--all-features` (plus the repo-standard
`server,plugins,lsp-test-support` set) per `AGENTS.md` — `--all-features`
drags in `lsp-real-server-tests`, which need installed language servers.
Timeout-through-service is covered at the probe layer (existing
`cancellation_and_overall_timeout_are_bounded`) plus the
`WORKFLOW_TIMEOUT` wrapper and mapping test rather than a dedicated
15s+ service test, keeping the suite bounded; see §10.

No live provider network was used; compatible-probe paths ran against
deterministic loopback fake servers and the direct path ran offline.

## 5. Invariant review

- No secret in chat buffer/argv/logs/SQLite/protocol: `SecretInput`
  stays redacted in `Debug`/snapshots (asserted); staged/final SQLite
  rows scanned for the test secret (asserted clean); credential bytes
  only touch `store.put` inputs and the encrypted store file.
- Explicit env keys retain precedence (untouched resolution path).
- Existing ciphertext never re-encrypted; orphaned stores still fail
  closed via M001 semantics.
- Fixed-endpoint definitions reject caller endpoint/port/TLS material;
  unknown providers and bearer-for-`ApiKeyOnly` fail before any write.
- Setup metadata grants nothing: catalog endpoint is presentation-only;
  scope/authorization stay daemon-owned; `ProviderSetupList` is
  secret-free and remotely admissible by construction.
- No unpublished Eggpool crate dependency added.
- v24/v26/v27 rows remain readable (no migration; `Other` parsing
  pre-exists).

## 6. Failure and recovery review

- Duplicate/equivalent submissions: provider-keyed active check plus
  provider-keyed idempotency journal → `connection_conflict` (asserted
  for ordinary and Eggpool-preset paths, including cross compat-adapter
  duplicates).
- Restart: staged/probing rows reconcile to `failed`/`daemon_restarted`
  with operation-owned credential cleanup before the next create
  (asserted with a manually staged `openai` row).
- Cancellation: in-flight compatible probe cancels, compensates the
  operation-owned credential, leaves no active row (asserted).
- Invalid credential/unreachable/oversized: bounded codes
  (`authentication_failed`, `endpoint_unreachable`,
  `catalog_oversized`), staged credential removed, `failed` row recorded
  (asserted).
- Rotation namespace: staged/read/cleanup follow the stored binding
  (asserted for a `custom` row end-to-end).

## 7. Migration and compatibility review

No SQLite migration. No config-schema change. Staged/final rows reuse
the v26 shape with provider-keyed values. The Eggpool-named protocol
request/response and `EggpoolProvisioner` name remain as compatibility
adapters over the generic service; existing TUI code paths are
untouched and keep working (TUI restoration is M003). Remote clients see
two additive request variants and two additive response variants plus
one additive authorization operation; the secret-bearing newcomer is
denied remotely by the semantic guard.

## 8. Security review

- Credential writes remain operation-owned with exact-binding
  compensation; reconciliation cleans staged bindings by stored ref.
- Endpoint validation rejects userinfo/query/fragment/control chars and
  path traversal on every path (preset, user URL, fixed); TLS/scheme
  consistency enforced by `Endpoint::new` plus preset policy checks.
- Error taxonomy carries no bodies, URLs, headers, credentials, or
  transport detail (asserted for auth failure and typed rejections).
- `is_secret_bearing()` is the single remote-denial predicate;
  `secret_bearing_variants_are_denied` fails until future secret-bearing
  variants join it.

## 9. Documentation and operations

- `architecture/provider.md`, `architecture/protocol.md`,
  `.opencode/skills/provider-auth/SKILL.md` updated (see §3).
- Operator view: `/connect` behavior is unchanged until M003; durable
  rows now report `provider_kind` per implementation (`openai`,
  `other:mistral`, …) in existing list surfaces.
- Guards: existing `check-core-boundary`/`check_master_key_resolver`
  plus the new coverage tests (`catalog_covers_…`,
  `every_connectable_definition_builds…`,
  `legacy_and_catalog_kinds_resolve…`, `secret_bearing_variants_are_denied`).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No dedicated 15s+ timeout-through-service test | Timeout boundary exists (`WORKFLOW_TIMEOUT` wraps both probe strategies) and is covered at the probe layer plus mapping test; suite stays bounded | None for closure; M003 harness may add a short-timeout service case if it makes timeouts configurable |
| low | Rotation/refresh validation still uses the compatible probe for all rows | Ordinary rows rotate/refresh with namespace-correct credentials but Eggpool-transport validation; failures are closed (CAS revision, no commit) | None for closure; per-provider rotation validation is unregistered future work, not M003 scope |
| — | None blocking | — | — |

## 11. Roadmap disposition

Milestone M002 closed. Its hard dependent,
`plans/implementation/provider-connect-restoration-corrective/003-connect-tui-restoration.md`
(M003), is unblocked to `ready`. No corrective pass required.

## 12. Registry updates

- `plans/registry.md`: M002 `ready` → `closed` (this record,
  implementations `b532ef84` + `189eb37b`); subsystem row M002 ready →
  M001+M002 closed, M003 ready; M003 `blocked` → `ready`.
- `plans/subsystems/provider-connect-restoration-corrective-addendum.md`:
  M002 `ready` → closed; M003 `blocked on M002 closure` → `ready`.
- `plans/implementation/provider-connect-restoration-corrective/002-*.md`:
  `ready` → `implemented`.
- `plans/implementation/provider-connect-restoration-corrective/003-*.md`:
  `blocked on M002 closure` → `ready`.
