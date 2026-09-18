# Provider /connect Restoration M002 — Provider Catalog and Neutral Provisioning

Status: blocked on M001 closure

Corrective roadmap:
plans/subsystems/provider-connect-restoration-corrective-addendum.md

Historical implementation:
plans/implementation/provider-connections/002-eggpool-connect-workflow.md

Historical closure:
plans/closure/provider-connections/002-status.md

Repository baseline reviewed: 5af6766a04d6326ba1e7bb3695b2f0422399e9ee

## 1. Objective

Generalize the Eggpool-specific create/provisioning vertical into the canonical
provider-connection provisioning service, backed by one pre-credential provider
definition catalog. Retain Eggpool as a named local/shared proxy preset and reuse the
existing v26 transaction, compensation, health/catalog and lifecycle machinery.

This milestone owns service/protocol/provider-layer convergence. It does not yet own
the final TUI restoration.

## 2. Defects to correct

- Runtime `ProviderRegistry` contains only instantiated providers, so it cannot list
  providers before the user supplies credentials.
- The supported default provider set is encoded across
  `register_builtin_with_config`, `builtin_registration_order()`,
  `credential_capability_for()` and provider constructors.
- `ProviderConnectionFactory` supports only part of the normal provider set.
- Create protocol/service names and error taxonomy are Eggpool-specific despite the
  underlying v26 journal already persisting `provider_kind`.
- `src/core/eggpool.rs` owns cross-store transaction/idempotency/cancellation
  behavior that is useful for every durable provider connection.
- Eggpool normalization/probe policy is mixed with generic provisioning.

## 3. Provider definition catalog

Add a non-secret `ProviderDefinition` / `ProviderCatalog` layer in
`codegg-providers`. It is metadata + construction policy, not a registry of live
instances.

Each definition must provide enough typed data to answer:

- stable provider ID and display name;
- whether CodeGG can onboard it with the currently implemented local form;
- accepted credential capability/kind;
- endpoint policy: fixed/default, optional override, required endpoint, or compatible
  proxy/custom endpoint;
- provider construction strategy;
- model validation/discovery strategy;
- optional presentation hints needed by the TUI, without putting secrets or
  authorization policy in presentation metadata.

Refactor normal startup registration and durable connection construction to consume
the same definition/builders where practical. At minimum, add exhaustive coverage
tests proving every ID in `builtin_registration_order()` has an explicit setup
disposition and every connectable definition has an executable durable factory.

Do not simply expose `ProviderRegistry::list()`; that would omit unconfigured
providers and recreate the defect.

## 4. Supported/default provider policy

The setup catalog should include the normal default providers that CodeGG can safely
construct from typed local credentials. The current built-in registration set is the
starting census.

Providers requiring auth/config that is not representable by the current safe form
must remain known but `connectable = false` until their typed onboarding fields are
implemented. Do not fake support by coercing cloud signing, OAuth/device, account-ID
or other multi-field authentication into one API-key box.

Add explicit entries for:

- Eggpool, as an OpenAI-compatible proxy preset with user endpoint, existing TLS
  normalization and default port 11300;
- generic OpenAI-compatible/custom upstream, requiring an endpoint and supported
  API-key/bearer credential kind.

Where a normal built-in uses a specialized provider implementation (for example
custom headers/requests), its durable connection builder must use that implementation
rather than incorrectly coercing it to generic OpenAI-compatible transport.

## 5. Generic provisioning service

Replace the command-level Eggpool create authority with provider-neutral types such as
`CreateProviderConnectionRequest` and `ProviderConnectionProvisioner`.

Preserve the proven M002 sequence:

validate/normalize → staged journal → operation-owned protected credential write →
bounded probe/model discovery → one final transaction publishing connection, health,
catalog and committed provisioning state.

Preserve:

- operation IDs/idempotency;
- cancellation;
- ownership-aware compensation;
- restart reconciliation;
- duplicate/equivalent submission detection;
- no network I/O inside the final SQLite transaction;
- local-only secret-bearing create boundary;
- redacted stable failure codes.

Existing `provider_provisioning.provider_kind` is already generic. Prefer no schema
migration. Add one only if a provider-neutral invariant truly cannot be represented by
the current rows.

## 6. Validation/probe strategies

Use a typed strategy selected by provider definition.

For ordinary providers, construct the provider through the canonical definition/factory
and call its `Provider::models()` behind the existing operation cancellation and
overall timeout boundary. Normalize returned `ModelInfo` into the existing bounded
connection catalog.

For Eggpool and arbitrary OpenAI-compatible endpoints, retain/reuse the current strict
compatible `/models` probe if it provides stronger redirect/body/model-count bounds
than a generic provider call. Rename/extract it so the machinery is not branded as
Eggpool when it is protocol-generic.

Provider-specific errors must map into a bounded generic provisioning reason taxonomy.
Do not expose response bodies, credentials or raw transport errors.

## 7. Eggpool reuse boundary

Research confirms Eggpool's application `connect` command mutates Eggpool TOML and
coordinates Eggpool reload/restart behavior. Its unpublished
`eggpool-connect` and `eggpool-client-config` crates are not the correct
downstream API for CodeGG provider onboarding.

Therefore:

- do not add a dependency on unpublished Eggpool application crates;
- reuse the connection/probe/normalization/lifecycle ideas already incorporated in
  CodeGG and extract them into generic CodeGG provider machinery;
- keep Eggpool as a provider definition/preset using the same compatible transport as
  any local proxy;
- if a genuinely reusable provider-template crate is later published by Eggpool, it
  may replace duplicated static metadata in a separate dependency review, but this
  corrective milestone must not block on it.

## 8. Protocol and compatibility

Add generic core request/response variants while retaining the old
Eggpool-named local request only as a temporary compatibility adapter if any active
caller/tests require it. New production TUI code must use the generic request.

Remote WebSocket denial must be defined by secret-bearing provider-create semantics,
not by the word Eggpool. The guard must fail if a future provider create variant is
accidentally exposed remotely.

Existing provider connection IDs, revisions, scopes, health rows and session
selections remain unchanged.

## 9. Tests

Add:

- provider-definition coverage against `builtin_registration_order()`;
- connectable-definition → durable-factory coverage;
- ordinary direct-provider fake transport provisioning success;
- Eggpool-compatible fake server success through the same generic service;
- invalid credential, unavailable, timeout, cancellation and oversized model cases;
- duplicate create and restart reconciliation retained from M002;
- secret-free protocol/debug/storage assertions;
- remote secret-bearing create denial for the generic variant;
- compatibility test for existing Eggpool durable rows resolving through the new
  factory.

No live provider network is required for closure.

## 10. Verification

- providers crate unit tests, including catalog/factory coverage
- provider-connections core tests
- protocol tests
- storage migration tests
- focused generic provisioning fake-server tests
- existing Eggpool M002/M004 regression tests
- auth tests after M001
- `cargo fmt --all -- --check`
- strict workspace Clippy
- `scripts/verify.sh quick`
- `git diff --check`

## 11. Acceptance

Close only when one ordinary direct provider and Eggpool can both be provisioned by
the same generic service/protocol authority; every default built-in has an explicit
setup disposition; every connectable definition is actually buildable; and all M002
secret/transaction/recovery invariants still pass.

A provider shown as connectable but not usable through the durable factory is a
closure blocker.
