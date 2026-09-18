# Provider /connect Restoration Corrective Addendum

Status: active

Repository baseline reviewed: 5af6766a04d6326ba1e7bb3695b2f0422399e9ee

Related closed work:

- plans/subsystems/provider-connections-roadmap.md
- plans/implementation/provider-connections/002-eggpool-connect-workflow.md
- plans/closure/provider-connections/002-status.md
- plans/closure/provider-connections/003-status.md
- plans/closure/provider-connections/004-status.md
- plans/closure/provider-auth-capability-010-status.md
- plans/closure/tool-surface-upstream-compatibility/009-status.md

## 1. Purpose and corrective trigger

Restore the provider-neutral `/connect` contract that existed before the Eggpool
connection restructuring while retaining the useful connection/provisioning machinery
introduced by that work.

The intended product boundary is:

- `/connect` is CodeGG's provider onboarding surface. It lists the supported
  default upstream providers, lets the user select one in the TUI, collects only the
  credentials/configuration required by that provider, validates the connection, and
  persists a durable provider connection.
- Eggpool is one selectable upstream provider, primarily for developers or teams that
  run Eggpool as a local/shared proxy. Its default port, endpoint and TLS policy are
  provider-specific form details, not the semantics of `/connect`.
- CodeGG should reuse the generic transaction, credential, health/model discovery,
  idempotency and lifecycle machinery that was developed around Eggpool instead of
  maintaining a second provider-onboarding stack.
- The ordinary first-run path must not require a user to pre-create
  `CODEGG_MASTER_KEY`.

Current code violates the first and fourth points. `App::open_connect_dialog()`
constructs a one-item Eggpool provider list and `ConnectDialog` makes host/port/TLS
steps unconditional. The create protocol is named `CreateEggpoolConnectionRequest`
and `src/core/eggpool.rs` owns otherwise reusable provisioning mechanics. The
credential write path fails when no environment-provided master key exists.

## 2. Historical contract and why this is corrective

The original provider-connections roadmap states that Eggpool is the first explicit
shared connection type and should be implemented through generic OpenAI-compatible
provider machinery. M002's objective was to add an Eggpool `/connect` workflow, not
replace provider selection with Eggpool.

The M002 closure proved the Eggpool vertical thoroughly and recorded that existing
provider/auth behavior was preserved, but its TUI verification exercised the Eggpool
form and did not assert the provider-neutral catalog that preceded it. That allowed an
additive milestone to become a replacement UI without a regression failure.

This addendum does not reopen the durable connection, selection, lifecycle or rotation
work that is already closed. It restores the missing onboarding abstraction on top of
those foundations.

## 3. Current-state findings

1. Durable storage is already generic. Migration v26's
   `provider_provisioning` table persists `provider_kind`, endpoint, TLS policy,
   scope and opaque secret references. No replacement schema is required.
2. `ProviderConnectionStore`, health/catalog tables, session selection and lifecycle
   records already support a durable connection independently of Eggpool.
3. The providers crate has two related but divergent authorities:
   - `ProviderRegistry` contains executable provider instances after credentials are
     resolved.
   - `builtin_registration_order()`, `credential_capability_for()` and registration
     branches describe the supported default provider set.
   A runtime `ProviderRegistry` cannot by itself drive `/connect`, because an
   unconfigured provider has no instance to list.
4. `ProviderConnectionFactory` currently builds only the subset represented by its
   connection-kind enum. Several providers supported by normal startup registration
   therefore cannot yet be created as durable connections by `/connect`.
5. Every provider implements `Provider::models()`, giving the generalized
   provisioning path a provider-owned validation/model-discovery seam. The stricter
   existing Eggpool/OpenAI-compatible `/models` probe can remain the strategy for
   local/custom compatible endpoints.
6. Eggpool's own interactive `connect` implementation is coupled to Eggpool TOML
   mutation and reload/restart classification. The unpublished
   `eggpool-connect` / `eggpool-client-config` crates target Eggpool client
   configuration rather than CodeGG provider onboarding. CodeGG should not depend on
   those application-internal surfaces.
7. CodeGG's credential store is already encrypted and writes mode-0600 files on Unix,
   but `codegg_config::encryption::get_master_key()` only consults environment
   variables. This makes the nominal first-run TUI flow fail at credential commit.

## 4. Target architecture

Introduce one provider-definition/catalog layer in `codegg-providers` that describes
providers before credentials exist. It is separate from the runtime instance registry
but must be the shared source used by both startup registration and durable connection
construction.

A definition should carry only non-secret executable metadata needed by consumers:
provider ID/display name, credential capability, connection builder/family, endpoint
policy/default, connectability, and model-probe strategy. Do not create a second
hand-maintained provider-name list.

The intended flow is:

`/connect`
→ daemon/provider setup catalog
→ provider-specific form
→ generic create-provider-connection request
→ shared provisioning service
→ protected credential write
→ provider construction
→ bounded model/health validation
→ existing v26 finalization
→ durable connection exposed to `/connections` and model selection.

Eggpool is represented as a named OpenAI-compatible/proxy preset with its existing
normalization rules (including default port 11300) and strict catalog probe. It does
not own the generic command, request or provisioner names.

## 5. Invariants

- No API key enters the normal chat buffer, slash-command text, argv, logs, debug
  output, SQLite rows or non-secret protocol responses.
- Remote secret-bearing provider creation remains denied unless a separately reviewed
  secure remote secret transport is introduced.
- Provider setup metadata never grants authorization. Scope and authorization remain
  daemon-owned.
- Existing configured/env providers continue to work while durable connection
  onboarding is generalized.
- The runtime provider instance registry and the setup catalog cannot silently drift;
  coverage tests must fail when a default provider is added without an explicit
  onboarding disposition.
- Unsupported auth families (cloud signing, OAuth/device flows, multi-field account
  credentials) are not shown as connectable until CodeGG has a typed safe form and
  factory for them.
- Eggpool continues to be usable as an ordinary upstream proxy without being required
  for any other provider.
- No CodeGG implementation depends on unpublished Eggpool application crates.
- Existing v24/v26/v27 provider-connection data remains readable.

## 6. Milestones

### M001 — First-run credential encryption bootstrap

Status: ready

Plan:
plans/implementation/provider-connect-restoration-corrective/001-first-run-credential-key-bootstrap.md

Make protected credential writes usable on a clean personal installation without a
manual master-key environment variable, while preserving explicit environment key
compatibility and fail-closed handling of pre-existing encrypted material.

### M002 — Provider catalog and provider-neutral provisioning

Status: blocked on M001 closure

Plan:
plans/implementation/provider-connect-restoration-corrective/002-provider-catalog-and-neutral-provisioning.md

Create the pre-credential provider definition catalog, converge startup registration
and durable connection construction on it, generalize the Eggpool provisioner/request
into provider-neutral services, and retain Eggpool as a compatible-proxy preset.

### M003 — Restore the provider-neutral /connect TUI

Status: blocked on M002 closure

Plan:
plans/implementation/provider-connect-restoration-corrective/003-connect-tui-restoration.md

Drive the TUI from daemon-owned setup metadata, restore provider selection and
provider-specific credential forms, include keyboard and mouse interaction, and add an
end-to-end regression proving Eggpool is one option rather than the command itself.

## 7. Exit conditions

This corrective work is closed only when a clean user profile can launch CodeGG, run
`/connect`, choose a supported default provider, paste the required credential,
complete validation, and use the resulting durable connection without pre-setting a
CodeGG encryption variable.

The same flow must allow choosing Eggpool and supplying its proxy endpoint/key. A test
must prove at least one ordinary direct provider and Eggpool through the same generic
create/provisioning authority.

The setup catalog must be guarded against drift from the supported default provider
registration set, and provider secrets must retain the existing local-only/redaction
properties.

## 8. Deferred / non-goals

- Reimplementing Eggpool routing, accounting, fallback or team policy in CodeGG.
- Making Eggpool a required local daemon.
- Adding OAuth/cloud-signing onboarding without typed credential/form support.
- Redesigning provider session selection or connection lifecycle already closed in
  provider-connections M003/M004.
- Generic marketplace/provider-plugin discovery.
- Remote secret entry over the public WebSocket.
