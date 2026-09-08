# Provider Connections Milestone 010 — Auth Capability Matrix and Stored Bearer Closure

Status: ready for handoff

Repository baseline: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`

Source roadmap/addendum:

- `plans/subsystems/provider-auth-capability-closure-addendum.md#4-milestone-010--auth-capability-matrix-and-stored-bearer-closure`

Predecessor evidence:

- `plans/closure/provider-connections/009-status.md`
- `architecture/auth.md`
- `architecture/provider.md`

Long-term requirements:

- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool`
- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`

Applicable ADRs: none.

Primary class: invariant / capability / polish

## 1. Objective

Make CodeGG's provider authentication support explicit and executable by introducing a provider-registration capability matrix and completing stored `BearerToken` resolution for providers whose registration/transport path already accepts a full `Credential`, while making incompatible combinations fail explicitly and preserving all existing API-key, secret-reference, expiry, and redaction behavior.

## 2. Why this milestone is ready

The auth domain already models the necessary concepts. `Credential` carries `CredentialKind::{ApiKey, BearerToken}` and optional expiry. The credential store persists kind metadata. Provider registration already converges through `resolve_provider_credential` and three factory families:

- full-credential registration;
- API-key-string registration;
- base-URL/config-aware registration.

The remaining mismatch is concrete: `AuthConfig::Stored` and no-auth store fallback currently query only `CredentialKind::ApiKey`, so stored bearer credentials are invisible even to full-credential provider paths.

External-command and OAuth modes are already deliberately disabled and documented as unsupported. There is no need to design either to close this milestone.

## 3. Current implementation evidence

At minimum inspect:

- `crates/codegg-providers/src/auth_types.rs` (`Credential`, `CredentialKind`, `AuthConfig`, `AuthResolver`, `ResolverContext`, `CredentialStore`);
- provider registration helpers in `crates/codegg-providers/src/provider/`;
- factory implementations for OpenAI-compatible, Anthropic-compatible/API-key-only, native OpenAI/Anthropic/Google/OpenRouter, OpenCode Go, General Compute, MiniMax, OpenCode Zen, and other built-ins;
- authorization header construction in each representative transport;
- durable connection secret-reference resolution and rotation paths;
- auth CLI/store tests;
- `architecture/auth.md`, `architecture/provider.md`, configuration docs/examples.

The baseline architecture documentation identifies representative families:

- full `Credential` registration for several OpenAI-compatible providers including Mistral, Groq, DeepInfra, Cerebras, Cohere, Together, Perplexity, xAI, Venice, OpenCode Go, and General Compute;
- API-key-only registration for OpenCode Zen and MiniMax;
- config/base-URL-aware registration for Anthropic, OpenAI, Google, and OpenRouter.

Do not assume the config-aware family has uniform bearer support. Inspect each factory/transport and classify it from actual code.

## 4. Invariants that must not regress

- API-key env/config/store resolution order remains unchanged unless the capability selector requires an explicit kind choice at the store step.
- Secrets remain masked in `Debug`, errors, logs, protocol output, and tests.
- Master-key requirements and encrypted-store behavior remain unchanged.
- Expired credentials fail before provider transport invocation.
- Full-credential providers preserve the credential kind rather than downcasting to a string and reconstructing an API key.
- API-key-only providers never reinterpret a bearer token as an API key.
- Unsupported auth is a typed/auth-specific failure, not a silent fallback to a different credential source that changes operator intent.
- `ExternalCommand` and `OAuthDevice` remain `Unsupported` for this milestone.
- Adding provider config continues to obey the current registration/fallback contract unless an independent defect is proven; this plan does not redesign auto-registration.
- Durable provider connections continue storing only secret references/metadata.

## 5. Scope

### In scope

- Build a data/table-backed capability classification for all built-in provider registration paths.
- Define which credential kinds each provider/factory accepts.
- Thread the accepted credential-kind predicate/capability into centralized credential resolution.
- Make stored bearer credentials resolvable for compatible providers.
- Make stored bearer credentials explicitly rejected for incompatible providers.
- Preserve kind through representative compatible transports and prove authorization behavior with fake/local HTTP tests when feasible.
- Test expiry/redaction for bearer records.
- Keep disabled auth variants explicitly unsupported and document them truthfully.
- Reconcile auth/provider/config documentation.

### Explicitly out of scope

- OAuth device/PKCE implementation.
- External credential helper execution.
- Consumer-session token reuse or scraping.
- Team identity/OIDC login.
- A new keyring backend.
- Provider routing or model selection changes.
- General provider transport normalization.
- Migrating existing stored API keys to bearer tokens.

## 6. Required production changes

### Core/domain

Introduce one capability concept at provider registration/resolution, for example:

```rust
CredentialCapability {
    ApiKeyOnly,
    ApiKeyOrBearer,
    NoAuth,
}
```

The exact type/name may differ. It must be defined in the provider/auth owner, not duplicated per factory.

The store lookup should select records according to the target capability. For a full-credential provider, `Stored` may retrieve either accepted kind according to an unambiguous selection rule. If multiple account records of different kinds can exist under the same provider/account identity, the resolver must not guess nondeterministically; inspect store uniqueness semantics and either enforce one record per binding or return an actionable ambiguity/error.

For API-key-only providers, a matching stored bearer record should produce an explicit incompatibility error when it is the selected binding. Do not filter it away and then report generic `NotFound`.

### Storage and migrations

No schema migration should be needed because stored records already contain credential kind. Confirm existing serialization is forward/backward compatible.

If uniqueness constraints currently prevent replacing an API key with a bearer record under the same provider/account, define replacement semantics using existing store APIs rather than schema expansion unless truly necessary.

### Protocol and DTOs

Do not expose secret/kind details beyond current redacted metadata contracts unless the operator already sees credential kind. If adding redacted `credential_kind` to an auth status/diagnostic DTO materially helps supportability, it must contain no secret-derived information and remain backward-compatible/additive.

### Runtime and concurrency

Credential resolution remains synchronous/owned as currently designed. Provider instance construction may cache a resolved credential according to existing connection generation/rotation semantics. Do not make an in-flight request switch credentials mid-request.

Rotation/re-resolution must preserve credential kind and invalidate/rebuild runtime instances through the existing provider-connection lifecycle.

### Frontend or operator surface

`codegg auth status` may continue showing metadata only. If it already shows kind, ensure bearer records are represented correctly. Avoid adding secret-entry UX for arbitrary bearer tokens unless the existing `set-key` command's naming becomes misleading; a small generalized `set-credential --kind` CLI is out of scope unless necessary to create supported records for users. Test fixtures can exercise store behavior directly.

If bearer storage is user-reachable only through another existing connection flow, document that rather than expanding CLI scope automatically.

### Security and authorization

- Never print bearer values.
- Preserve fixed-width masking.
- Do not accept bearer tokens in query parameters or URLs.
- Authorization headers must be constructed by provider transport using the credential contract; tests should inspect headers only with synthetic sentinel secrets and never log real credentials.
- Expired bearer records are rejected before network access.
- No fallback from an explicitly selected stored bearer binding to an unrelated env API key unless the existing explicit precedence contract intentionally says so; operator intent must be preserved.

### Documentation and static guards

Update `architecture/auth.md` with an executable support matrix or link to tests/table, and remove the statement that all stored bearer tokens are unsupported once compatible provider paths are implemented. Keep ExternalCommand/OAuth limitations explicit.

No new static guard is required unless one simple guard can prevent reintroducing global API-key-only filtering in the central stored-credential path.

## 7. Ordered work packages

### Work package A — Build the provider/auth capability matrix

Intent: classify actual support before changing resolution.

Required actions:

1. Enumerate every built-in provider registration path.
2. For each, identify factory helper, underlying transport/provider type, expected auth header/credential behavior, and accepted kinds.
3. Classify API-key-only, API-key-or-bearer, no-auth/optional-auth, or unsupported/special.
4. Add a table-driven test/data representation close to provider registration so new providers must choose a capability deliberately.

Acceptance evidence:

- complete matrix covering every built-in registration branch;
- no unclassified provider silently inherits bearer support.

### Work package B — Generalize centralized stored credential resolution

Intent: let the resolver honor provider capability without decentralizing lookup.

Required changes:

- extend resolver context/call to know accepted credential kinds;
- replace hard-coded store `ApiKey` predicate with capability-aware selection;
- preserve existing env/config API-key priority;
- make incompatible selected stored kind an explicit auth error;
- preserve expiry handling and source attribution.

Acceptance evidence:

- existing API-key resolution-order tests remain green;
- stored bearer resolves for a compatible capability;
- same record fails explicitly for API-key-only capability;
- expired bearer produces expired failure.

### Work package C — Preserve kind through provider registration/transport

Intent: ensure support is end-to-end rather than resolver-only.

Required changes:

- representative full-credential providers receive the original `CredentialKind`;
- transport builds the documented authorization form correctly;
- API-key-only helpers keep rejecting bearer credentials;
- inspect config-aware providers individually and wire their declared capability.

Acceptance evidence:

- fake/local transport tests for at least one representative full-credential provider and one API-key-only provider;
- no kind-downcast/reconstruction bug.

### Work package D — Durable connection/rotation integration

Intent: preserve existing connection semantics.

Required actions:

- verify secret references resolve the correct kind after restart;
- verify rotation from API key to bearer or bearer to API key only when target provider accepts the new kind;
- incompatible rotation fails before commit and leaves old credential/runtime valid;
- in-flight request retains captured credential revision.

Acceptance evidence:

- focused connection lifecycle tests for kind-preserving resolution/rotation or a clear statement that existing store binding API cannot expose cross-kind rotation without separate UI work.

### Work package E — Documentation and truthfulness cleanup

Intent: match configuration surface to executable support.

Required changes:

- update auth architecture matrix;
- mark ExternalCommand/OAuth as reserved/unsupported in user config docs;
- document which provider families accept stored bearer tokens;
- ensure errors recommend supported alternatives without suggesting consumer-session token hacks.

Acceptance evidence: docs and table-driven tests agree.

## 8. Failure, cancellation, restart, and contention semantics

Missing credential: preserve existing typed missing-credential behavior.

Incompatible credential kind: return a distinct actionable auth error; do not contact provider transport and do not silently reinterpret/fallback contrary to explicit binding.

Expired credential: fail before network access.

Master key unavailable: preserve current store/decryption failure semantics and never fall back to plaintext persistence.

Rotation failure: existing provider runtime/credential revision remains active. New credential is not committed partially.

Restart: durable secret reference resolves the stored kind deterministically and reconstructs compatible provider instance lazily.

Concurrent requests: each request uses the credential/runtime revision it captured under existing connection lifecycle semantics; kind changes do not mutate an in-flight request.

## 9. Compatibility and migration

No existing API-key config needs migration. Existing encrypted/stored API keys must behave exactly as before.

`AuthConfig::Stored` becomes more capable for provider paths that support bearer tokens. This is backward-compatible.

For API-key-only providers, reporting `UnsupportedCredentialKind`/equivalent instead of generic not-found when a bearer record is explicitly bound is an intentional diagnostic improvement.

ExternalCommand/OAuth config files continue parsing but fail explicitly at runtime as documented. Do not remove the enum variants in this milestone.

## 10. Required tests

### Focused unit tests

- capability table covers every provider registration branch;
- stored API key resolves under ApiKeyOnly and ApiKeyOrBearer;
- stored bearer resolves under ApiKeyOrBearer;
- stored bearer rejected under ApiKeyOnly with typed error;
- expired bearer rejected;
- debug/error formatting masks synthetic bearer secret;
- resolver source remains `UserStore` for successful stored bearer.

### Integration tests

- representative full-credential provider sends expected synthetic authorization header from stored bearer;
- representative API-key-only provider never reaches transport with bearer;
- config-aware provider family tests according to actual declared support;
- durable connection resolves stored bearer after restart if that path is supported.

### Restart and recovery tests

- store reopen/decrypt retains credential kind;
- provider connection rebuild retains kind and expiry semantics.

### Contention and cancellation tests

No new auth-level cancellation contract. Run existing connection rotation/in-flight request tests if kind can change during rotation.

### Security and negative tests

- no plaintext secret in `Debug`, errors, logs/test snapshots;
- unsupported ExternalCommand/OAuth remain fail-closed;
- incompatible kind causes zero outbound request;
- missing master key never writes plaintext fallback.

### Migration and compatibility tests

- existing API-key configuration/env/store precedence unchanged;
- legacy provider config remains functional.

## 11. Required verification commands

Select exact current modules at implementation head. Expected commands:

```bash
cargo test -p codegg-providers --lib auth_types
cargo test -p codegg-providers provider
cargo test -p codegg --lib auth
cargo test -p codegg provider_connections

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

No external provider credentials or real network servers are required; use synthetic/fake transports.

## 12. Documentation updates

- `architecture/auth.md` — capability matrix, stored bearer support, unsupported variants.
- `architecture/provider.md` — provider registration capability choice.
- Config/provider docs/examples where auth variants are shown.
- `RELEASING.md` is unrelated and should not be touched.

## 13. Acceptance criteria

- Every built-in provider registration path has an explicit credential capability.
- Existing API-key behavior and precedence remain green.
- Stored bearer token works end-to-end for at least one representative full-credential provider path and all paths classified compatible have equivalent contract coverage.
- API-key-only providers reject bearer tokens explicitly and before network access.
- Expiry, encryption, rotation, restart, and redaction semantics remain correct.
- ExternalCommand/OAuth stay explicitly unsupported.
- No consumer-session/app-token integration is added.
- Documentation matches the executable matrix.

## 14. Stop conditions

Stop and report when:

- a provider's public transport auth semantics cannot be determined from current implementation/docs and bearer support would be speculative;
- supporting bearer requires undocumented consumer-session token reuse;
- stored credential schema actually cannot retain kind without migration larger than expected;
- provider connection rotation ownership must be redesigned;
- the implementation starts adding OAuth/external-command machinery;
- a provider requires a credential type beyond the existing modeled kinds and needs a separate architecture decision.

## 15. Closure evidence required

- implementation commits/PRs;
- final provider × credential capability matrix;
- list of central resolver changes;
- API-key compatibility test outcomes;
- stored-bearer compatible/incompatible/expired test outcomes;
- representative transport header test using synthetic secret;
- durable connection/restart/rotation evidence where touched;
- secret-redaction negative evidence;
- explicit confirmation ExternalCommand/OAuth remain unsupported;
- formatting/lint/quick verification outcomes;
- unresolved provider-specific auth limitations.

## 16. Handoff notes

Do not infer “BearerToken” support merely because an HTTP API uses an `Authorization: Bearer ...` header. Many APIs colloquially call their long-lived key a bearer token while CodeGG's `CredentialKind` distinction may encode different operator/storage semantics. Classify based on the provider factory/transport contract and preserve existing API-key behavior.

The main design requirement is centralized kind-aware resolution. Avoid per-provider store lookup branches.
