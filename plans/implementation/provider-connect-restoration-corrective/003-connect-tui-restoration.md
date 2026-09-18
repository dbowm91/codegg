# Provider /connect Restoration M003 — Provider-Neutral TUI Restoration

Status: ready (unblocked by M002 closure at
plans/closure/provider-connect-restoration-corrective/002-status.md)

Corrective roadmap:
plans/subsystems/provider-connect-restoration-corrective-addendum.md

Historical closure:
plans/closure/provider-connections/002-status.md

Repository baseline reviewed: 5af6766a04d6326ba1e7bb3695b2f0422399e9ee

## 1. Objective

Restore `/connect` as a provider-selection workflow driven by the provider setup
catalog from M002. Eggpool must appear as one selectable upstream, not as the command's
hard-coded meaning.

The primary clean-user interaction is:

install → run `codegg` → `/connect` → choose provider → enter required credential
(and endpoint only when that provider needs one) → validate/save.

## 2. Current regression

`App::open_connect_dialog()` currently builds a one-entry list containing Eggpool.
`ConnectDialog` then unconditionally traverses host, port and TLS steps before the
API key. The completion request is Eggpool-specific.

This is the UI manifestation of the M002 regression. The M002 closure's focused form
tests did not assert that previously supported provider choices remained discoverable.

## 3. Catalog loading

The TUI must not maintain its own provider-name/auth-field allowlist.

Expose a secret-free daemon/core operation that returns the M002 setup catalog or a
bounded presentation projection of it. The response includes stable provider ID,
display name, connectability, form kind/field requirements and non-secret endpoint
defaults.

The dialog may open in a short loading state and populate from the response. Failure
to load must show an actionable bounded error and must not fall back to an Eggpool-only
list.

## 4. Form behavior

The first screen is a provider list. Support existing keyboard navigation
(arrows/j/k/Enter/Esc) and the component mouse/hit-test path so a user can click a row
and click actionable form controls.

Render only fields required by the chosen typed form:

- ordinary API-key provider: secret input, optional display/account label if retained;
- endpoint-capable provider: endpoint plus secret;
- Eggpool: endpoint/host with default port 11300 and the existing TLS policy controls,
  plus secret and scope;
- generic OpenAI-compatible: required endpoint plus accepted API-key/bearer choice if
  M002 exposes both safely;
- provider types not yet safely onboardable: omitted from the selectable list or
  visibly disabled with a concise reason, according to M002 metadata.

Do not build an arbitrary remote JSON-schema form engine. Use a small typed form enum
with exhaustive rendering/validation.

## 5. Secret handling

Retain the proven `SecretInput` behavior:

- masked rendering and paste support;
- no insertion into prompt history, command text, toast, generic TUI snapshots or
  debug output;
- clear secret buffers on submit, cancellation, error and dialog close;
- only the trusted local core request carries the plaintext secret;
- completion events/results are secret-free.

M001 makes the first protected write self-initializing for a fresh local profile.

## 6. Completion and model visibility

Submit the generic create-provider request from M002. Preserve operation IDs,
cancellation and stale-completion protection.

On success, refresh the existing connections/model projections rather than creating a
second TUI-only provider list. `/connections` remains the management surface for
durable connections. Model selection remains the already-closed provider-connection
selection subsystem.

Do not implicitly switch the current session unless that was already the documented
generic `/connect` behavior; if an automatic selection is desired later, plan it
separately.

## 7. Regression tests

Add an app-level TUI harness that covers the missing integration boundary:

1. start with a clean config/credential directory and no master-key env vars;
2. invoke `/connect` through normal slash-command routing;
3. receive a catalog with at least an ordinary provider and Eggpool;
4. select the ordinary provider by keyboard, paste a secret, complete fake validation,
   and observe one durable connection;
5. reopen `/connect`, select Eggpool (including endpoint/TLS fields), and complete
   against a deterministic compatible fake server;
6. repeat provider-row selection through mouse hit testing;
7. cancel from provider selection and secret entry and prove no credential/journal
   orphan;
8. verify secret text does not appear in rendered/debug/history snapshots.

Add a structural test that fails if `open_connect_dialog` (or successor) hard-codes
only Eggpool or constructs a provider list independent of the setup catalog.

## 8. Documentation

Update:

- architecture/provider.md
- architecture/auth.md
- architecture/protocol.md
- architecture/tui.md
- architecture/command.md
- README first-run/provider onboarding section

Documentation must describe Eggpool as an optional upstream proxy and clearly separate
`/connect` (add/configure) from `/connections` (inspect/manage/select existing
connections).

## 9. Verification

- focused connect dialog/component tests
- app-level slash-command/provider connection integration test
- provider connection core/protocol tests
- auth/key bootstrap tests from M001
- `cargo fmt --all -- --check`
- strict workspace Clippy
- `scripts/verify.sh quick`
- `git diff --check`

No live external provider or Eggpool installation is required; use deterministic local
fake endpoints.

## 10. Acceptance

Close only when the normal typed `/connect` path presents multiple supported
providers from canonical metadata, ordinary direct-provider setup requires no Eggpool
fields, Eggpool remains selectable and functional as a proxy provider, keyboard and
mouse paths both work, and a clean local profile needs no preconfigured encryption
environment variable.
