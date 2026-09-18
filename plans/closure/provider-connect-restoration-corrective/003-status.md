# Provider /connect Restoration M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connect-restoration-corrective/003-connect-tui-restoration.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connect-restoration-corrective-addendum.md#m003--restore-the-provider-neutral-connect-tui`

Repository baseline reviewed: `2d288406`

Implementation commits or pull requests:

- `5cc460c0` — feat(tui): provider-neutral /connect restoration (provider-connect M003)

## 1. Executive finding

M003 is complete. `/connect` is a provider-neutral onboarding surface
again: the dialog opens in a loading state, renders one selectable row per
secret-free catalog entry served by `CoreRequest::ProviderSetupList`, and
branches into a small typed form (fixed secret-only, optional endpoint
override, required endpoint, Eggpool host/port/TLS proxy preset) with an
explicit API-key/bearer choice exactly when the catalog admits both.
Submit sends the generic `ProviderConnectionCreate` request; success
refreshes the existing connections/model projections. The Eggpool-only
provider list, the unconditional host/port/TLS interrogation, and the
`EggpoolConnectionCreate` send path are gone from the dialog, and a
structural regression test fails if provider-name branching or an
Eggpool-only fallback reappears. Secret handling reuses the masked
`SecretInput` path with clearing on submit, cancel, error, and close; no
plaintext reaches snapshots, completions, or debug output. No unresolved
findings; no corrective pass required.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| No TUI provider allowlist; provider list comes from the setup catalog | `ProviderInfo::from_setup_dto()` is the only list constructor; `connect_tui_has_no_eggpool_only_hardcode` scans the dialog source for provider-name branches, Eggpool-only list builders, and Eggpool send paths | pass | — |
| Dialog opens in loading state with empty list; bounded error on catalog failure, no Eggpool-only fallback | `ConnectDialog::new_loading()`, `ConnectSetupLoaded` completion, `apply_connect_error()`; `loading_state_has_no_eggpool_fallback` | pass | Disabled `connectable=false` rows render with reason, not selectable (`disabled_rows_are_visible_but_not_selectable`) |
| Fixed providers need only a secret; ordinary providers never see Eggpool host/port/TLS | `ConnectFormKind::Fixed` + `advance_from_provider_selection()`; `ordinary_fixed_provider_skips_eggpool_fields` | pass | — |
| Optional-override providers accept a blank endpoint | `ConnectFormKind::OptionalOverride`; `optional_override_accepts_blank_endpoint` | pass | — |
| Required-endpoint providers validate without preset port assumptions | `ConnectFormKind::RequiredEndpoint`; `required_endpoint_provider_validates_without_preset_port` | pass | — |
| Eggpool keeps host/default-port-11300/TLS plus secret and explicit scope | `ConnectFormKind::EggpoolProxy` (`proxy_preset`); `eggpool_preset_keeps_host_port_tls_flow` | pass | Scope stays explicit; no silent inheritance |
| Dual-kind providers (api_key+bearer) force an explicit choice; single-kind pins the admitted kind | `needs_credential_choice()`, `move_to_credential_kind_step()`, `cycle_credential_kind()`; `fixed_dual_kind_provider_requires_credential_choice` | pass | Kind pins from live focus; no stale cross-provider selection |
| Stale-completion safety on the catalog load | `ConnectSetupLoaded` carries the connection/dialog `operation_id`; late failure/success ignored after close | pass | Same operation-ID discipline as the rotation editor |
| Keyboard parity (up/down + j/k + Enter/Esc) and clickable provider rows incl. scroll offset | `advance_from_provider_selection` on Enter; `hit_test_provider_row()` + modal mouse routing; `provider_rows_hit_test_with_scroll`; harness keyboard + mouse flows | pass | — |
| Success/cancel/error clear the secret; rotation-editor path untouched | `clear_secret()` on submit/cancel/error/close; `rotation_editor_closes_from_secret_entry`; `secret_is_masked_rendered_and_forgotten_on_clone` | pass | — |
| `/connections` and provider-connection selection untouched | No changes outside the Connect dialog render/input paths; `tests/tui` 165 green | pass | — |
| Success refreshes existing connections/model projections | `ProviderConnectionFinished` triggers connections + models refresh through the normal completion path | pass | Asserted in harness via completion handling |
| Docs: Eggpool as optional upstream, `/connect` vs `/connections`, typed-form catalog table, README first-run section | `architecture/provider.md`, `auth.md`, `protocol.md`, `tui.md`, `command.md`, `README.md` (this commit) | pass | — |

## 3. Production implementation evidence

Ownership:

- `src/tui/components/dialogs/connect.rs` owns the catalog-driven dialog:
  `ProviderInfo::from_setup_dto()` (DTO is the authority; legacy
  `auth_modes` stays consistent — `[None]` when API key is not admitted),
  `ConnectFormKind` (Fixed/OptionalOverride/RequiredEndpoint/EggpoolProxy),
  loading/error state, typed advance/next/cycle transitions,
  `hit_test_provider_row()`, masked review rendering, secret-free
  snapshots, and `clear_secret()`.
- `src/tui/app/mod.rs` owns dialog lifecycle: `open_connect_dialog()`
  opens `new_loading` and issues `CoreRequest::ProviderSetupList` as an
  operation-owned async task (stale completions ignored);
  `handle_connect_send()` builds generic `ProviderConnectionCreate` from
  the live focus snapshot plus catalog-pinned credential kind and
  per-form endpoint/port/TLS; mouse selection routes into the Connect
  dialog; dialog close clears the secret.
- `src/tui/app/commands.rs` owns `TuiCommand::ConnectSetupLoaded` and
  `ProviderConnectionFinished`; the Eggpool-named completion stays only
  for the pre-existing rotation-compat path.
- `src/tui/runtime/command_dispatch.rs` owns completion handling and
  `apply_connect_error()` (bounded, actionable, secret-free).

Secret boundary: plaintext travels only in the trusted local
`ProviderConnectionCreate` request built at submit time; completion
events, snapshots, toasts, and `Debug` impls are secret-free (asserted).
Remote transport denial is unchanged M002 machinery
(`is_secret_bearing()` + `secret_bearing_variants_are_denied`).

Docs: `architecture/provider.md` (M003 `/connect` subsection),
`architecture/auth.md` (`/connect` secret handling + managed-key
bootstrap note), `architecture/protocol.md` (catalog-driven selection →
generic create), `architecture/tui.md` (connect.rs line +
`/connect` vs `/connections`), `architecture/command.md` (command rows),
`README.md` (first-run `/connect` onboarding, Eggpool as optional
upstream).

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib connect_restoration --locked
cargo test -p codegg --test connect_tui_restoration --locked
cargo test -p codegg --test tui --locked
cargo test -p codegg-providers --locked
cargo test -p codegg-protocol --locked
cargo fmt --all -- --check
git diff --check
cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/verify.sh quick
```

### Results

All local. New unit suite: 11 passed (loading, catalog order, fixed,
dual-kind, required, optional, Eggpool preset, disabled rows, masked
render/clone, hit-test, rotation close). New integration harness
(`FakeConnectDaemon` serving the real `provider_setup_catalog()`): 2
passed (structural no-hardcode guard; keyboard openai flow + Eggpool
host/port/TLS flow + mouse select + cancel-from-secret + secret-free
snapshots). `tests/tui`: 165 passed. `codegg-providers`: 159 passed.
`codegg-protocol`: 188 passed. fmt, diff-check, strict Clippy, and
`verify.sh quick` (fmt/agent-schema/core-boundary/sandbox/
execution-ownership/TUI-authority + workspace check) all pass.

Deviation from the plan's §11 command list: Clippy ran without
`--all-features` per `AGENTS.md` — it drags in `lsp-real-server-tests`,
which need installed language servers. The full workspace test sweep was
replaced by the five focused suites above plus `verify.sh quick`'s
workspace check; the touched crates (root TUI, providers, protocol) are
fully covered. No live provider network was used; compatible-probe paths
run through the M002 fake-server machinery, and the new harness asserts
request shapes rather than re-probing.

## 5. Invariant review

- No secret in prompt history, command text, toasts, snapshots, logs, or
  `Debug` output: secret renders masked, completion payloads carry no
  secret, `clear_secret()` runs on submit/cancel/error/close (asserted,
  including `Clone` forgetting).
- TUI holds no provider-name allowlist and no Eggpool-only fallback; the
  catalog DTO is the single authority (structural guard fails the build
  otherwise).
- Fixed-endpoint providers cannot receive caller endpoint/port/TLS; the
  Eggpool proxy preset keeps default port 11300 without leaking it to
  other providers (submit builder branches on `ConnectFormKind`).
- Stale async completions (catalog load, provision) cannot corrupt a
  closed or repurposed dialog (operation-ID checks).
- `/connections`, provider-connection selection, and the rotation editor
  are behavior-preserving (existing suites green, rotation close test).
- `codegg-core` boundary untouched (TUI-only + test changes outside
  core; `check-core-boundary.sh` passes via `verify.sh quick`).

## 6. Failure and recovery review

- Catalog load failure: dialog shows a bounded actionable error with the
  list empty; retry re-issues `ProviderSetupList`; Enter/click cannot
  advance while loading or errored (asserted).
- Provisioning failure: `apply_connect_error()` surfaces the daemon's
  bounded code, keeps the dialog open for correction, and clears the
  secret buffer.
- Cancellation mid-provision: in-flight request cancels through the
  existing daemon cancellation path; dialog returns to selection with no
  secret retained.
- Daemon restart mid-dialog is M002 machinery (staged rows reconcile to
  failed with credential compensation); the TUI side treats the
  orphaned operation as a stale completion.
- Duplicate submissions: daemon-side provider-keyed idempotency is
  unchanged; the TUI disables double-submit via the in-flight state.

## 7. Migration and compatibility review

No SQLite migration, no config-schema change, no protocol change (M003
only consumes M002's `ProviderSetupList` / `ProviderConnectionCreate`).
The Eggpool-named request/completion remain as compatibility adapters;
existing CLI/env-var onboarding flows are untouched. Remote frontends see
no new variants; the secret-bearing create stays locally issued and
remotely denied by the M002 semantic guard.

## 8. Security review

- Plaintext secret exists only in the in-memory `SecretInput` buffer and
  the locally issued create request; every other surface (render,
  snapshots, completions, errors, `Debug`) is masked or secret-free by
  test.
- Endpoint validation stays daemon-side (`validate_user_endpoint`);
  the TUI performs no URL trust decision beyond blank-vs-present for
  required endpoints.
- Disabled catalog rows cannot be selected by keyboard or mouse
  (both paths check `is_selectable()`).
- One Clippy-driven hardening inside this milestone: legacy
  `auth_modes` for non-API-key kinds is `[None]`, keeping
  `supports_api_key()` consistent with `requires_api_key`.

## 9. Documentation and operations

- `architecture/provider.md`, `auth.md`, `protocol.md`, `tui.md`,
  `command.md`, and `README.md` updated (see §3).
- Operator view: a clean install runs `codegg` → `/connect` → choose
  provider → paste credential (+ endpoint only when needed); the first
  protected write bootstraps the managed key with no preconfigured
  encryption variable (M001 machinery, unchanged).
- Guards: the new structural test (`connect_tui_has_no_eggpool_only_hardcode`)
  plus existing `check-core-boundary` / TUI-authority guards.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No dedicated no-`CODEGG_MASTER_KEY` end-to-end run in this milestone | First-run bootstrap is M001-verified machinery and unchanged; the M003 harness injects the provisioner rather than re-proving key bootstrap | None for closure; clean-host qualification owns the live run (see §11) |
| low | No TUI-level remote-denial test for the newly issued create path | Denial is protocol-level M002 machinery (`secret_bearing_variants_are_denied` pins the variant set including `ProviderConnectionCreate`); the TUI only issues it over the local boundary | None for closure |
| — | None blocking | — | — |

## 11. Roadmap disposition

Milestone M003 closed. The provider-connect restoration corrective
subsystem (M001+M002+M003) has no further registered milestones; the
subsystem roadmap moves to closed. Its hard dependent,
`plans/implementation/self-contained-installation-corrective/002-runtime-resolution-and-clean-host-qualification.md`
(M002, unregistered), has its `/connect` closure precondition satisfied
by this record but remains blocked on its own M001 — no registered plan
changes state in this commit beyond M003 itself.

## 12. Registry updates

- `plans/registry.md`: M003 `ready` → `closed` (this record,
  implementation `5cc460c0`); subsystem row M001+M002 closed, M003 ready
  → subsystem closed with M001+M002+M003 closed; M003 moved to recently
  closed work.
- `plans/subsystems/provider-connect-restoration-corrective-addendum.md`:
  M003 `ready` → closed; subsystem status → closed.
- `plans/implementation/provider-connect-restoration-corrective/003-*.md`:
  `ready` → `implemented`.
