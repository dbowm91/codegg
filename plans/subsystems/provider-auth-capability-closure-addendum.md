# Provider Authentication Capability Closure Addendum

Status: closed

Source subsystem and predecessor evidence:

- `plans/subsystems/provider-connections-roadmap.md`
- `plans/implementation/provider-connections/009-direct-provider-session-context-corrective-pass.md`
- `plans/closure/provider-connections/009-status.md`
- `architecture/auth.md`
- `architecture/provider.md`

Long-term references:

- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool`
- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Related ADRs: none.

## 1. Trigger and purpose

The provider-connection line is operationally closed through its previous corrective work, but the current auth configuration surface is broader than the runtime capability that is actually exercised.

At baseline `15632a0483a8c4b9d573ff2ce43297b29be8f42a`:

- `AuthConfig` exposes `ApiKey`, `Stored`, `ExternalCommand`, `OAuthDevice`, and `None`;
- `CredentialKind` supports `ApiKey` and `BearerToken`;
- the encrypted `CredentialStore` can persist a credential kind and expiry metadata;
- `ExternalCommand` is intentionally disabled and returns `AuthError::Unsupported`;
- OAuth device flow is intentionally disabled and returns `AuthError::Unsupported`;
- stored credential lookup currently filters to `CredentialKind::ApiKey`, so a stored bearer-token record is treated as a miss even for providers whose transport accepts a full `Credential` envelope;
- provider factories have different credential capabilities: some accept a full `Credential`, while API-key-only providers accept a secret string and correctly cannot consume arbitrary bearer-token semantics.

The corrective objective is therefore not “implement every auth enum variant.” It is to make the provider × auth-method capability contract explicit and executable, complete the already-modeled stored bearer-token path where transports genuinely support it, and make unsupported combinations fail explicitly rather than appearing configured but silently missing.

## 2. Ownership boundary

This addendum owns:

- provider/auth capability classification;
- credential-resolution behavior for stored API keys and stored bearer tokens;
- explicit typed rejection of unsupported provider/auth combinations;
- focused provider-registration/transport tests proving the matrix;
- documentation/config truthfulness.

It does not own:

- consumer ChatGPT/Claude/Grok/Copilot session-token reuse;
- undocumented app-token scraping;
- implementation of OAuth device/PKCE without a concrete provider contract;
- external command execution without a separately justified async/timeout/security design;
- identity/team login;
- provider routing or Eggpool internals;
- a generalized secret-management product.

## 3. Invariants

- Secret values never appear in logs, protocol events, diagnostics, or closure evidence.
- Credential resolution remains centralized in `codegg-providers` rather than becoming provider-local ad hoc lookup.
- A provider that can accept a full credential may receive an API key or bearer token according to its transport contract.
- An API-key-only provider must reject a bearer token explicitly; it must not reinterpret it as an API key.
- `AuthConfig::ExternalCommand` and `AuthConfig::OAuthDevice` remain explicit unsupported states unless a separate future plan activates them.
- Stored credential expiry remains enforced.
- Legacy env/config API-key paths remain compatible.
- Durable provider connections continue storing secret references, not plaintext secrets.

## 4. Milestone 010 — Auth capability matrix and stored bearer closure

Class: invariant / capability / polish.

Status: closed.

Closure record:

- `plans/closure/provider-connections/010-status.md`

Implementation plan:

- `plans/implementation/provider-connections/010-auth-capability-matrix-and-stored-bearer-closure.md`

Objective:

Create an executable provider/auth capability matrix and make stored bearer tokens work only for provider registration paths that accept them, while all unsupported combinations fail with typed actionable diagnostics.

Deliverable boundary:

- capability inventory for every built-in provider-registration family;
- resolver/store lookup generalized to select credential kinds according to the target registration capability instead of globally hard-coding `ApiKey`;
- full-credential provider paths preserve `CredentialKind` through registration and request authorization;
- API-key-only paths reject stored bearer credentials explicitly;
- disabled ExternalCommand/OAuth remain truthful and covered;
- auth docs/config examples distinguish supported, unsupported, and reserved modes;
- focused secret-redaction and expiry tests.

Exit conditions:

- provider × auth-method behavior is represented by tests/data rather than only prose;
- stored API-key behavior remains unchanged;
- a stored bearer token resolves and reaches at least one representative full-credential provider factory without kind loss;
- a stored bearer token against an API-key-only provider yields explicit unsupported/invalid auth diagnostics, not `NotFound` caused by an unrelated API-key predicate;
- expired bearer credentials fail as expired and never reach transport;
- ExternalCommand and OAuth configurations fail explicitly as unsupported;
- no plaintext secret appears in debug/log/error output;
- documentation matches executable support.

## 5. Verification strategy

Use resolver/unit tests in `codegg-providers`, representative provider-registration tests for each registration family, fake HTTP transports where necessary to inspect authorization behavior without external network access, credential-store fixtures with API-key/bearer/expired records, and existing provider/connection redaction tests.

Broad verification remains the repository's normal bounded local contract; no auth-specific CI lane is required.

## 6. Completion definition

This addendum closes when M010 has an accepted closure record demonstrating the executable matrix, stored bearer-token support on compatible provider paths, explicit rejection on incompatible paths, unchanged API-key behavior, secret redaction, and truthful documentation.

Future OAuth/external-command work remains deferred and requires independent product/security justification.
