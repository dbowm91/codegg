# Provider Connection Qualification Corrective Milestone 010 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connect-qualification-wire-dispatch-corrective-addendum/010-provider-connection-qualification-semantics.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connect-qualification-wire-dispatch-corrective-addendum.md#milestone-010--provider-connection-qualification-semantics`

Repository baseline reviewed: `d85ed67bef970cfe99e320a7876e7a51373b7e37`

Implementation commits:

- `d2c82e08` — typed qualification axes, migration v69, revision-scoped
  inference feedback, truthful `/connect` messaging, guard, docs, and this
  closure record.

## 1. Executive finding

The milestone's capability boundary is complete. Catalog discovery and
credential verification are now separate facts in the type system, in durable
storage, on every provisioning/lifecycle write path, in operator-visible
messaging, and in the static guard set. Provisioning no longer treats a
successful `Provider::models()` call as proof that a credential was accepted;
a first-class `unverified` state exists for "configured, not yet proven";
a genuine typed 401/403 is the only thing that can mark a credential rejected;
and no code path anywhere issues an inference request to validate a credential.

The original defect — false-positive verification for static/local catalogs,
best-effort compatible discovery, and publicly readable `/models` endpoints —
is closed structurally rather than by convention: the credential verdict is
derived only through `CredentialVerification::from_probe`, whose
`CatalogOnly → Unverified` mapping is a pure function with a table-driven test
over the whole setup catalog, plus a repository guard.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Distinguish catalog discovery from credential verification in the type system | `crates/codegg-providers/src/qualification.rs` — `CatalogOutcome` (no authentication variant exists), `CredentialVerification`, `CredentialEvidence`, `ProbeQualification` | pass | Catalog reachability and credential validity are not representable in one enum. |
| A public/static catalog must not fail provisioning because it cannot validate a key | `setup_catalog.rs` renamed `DirectModels` → `ProviderCatalog`, documented as catalog-only; `probe_provider_catalog` returns `Unverified` | pass | Static-catalog provisioning keeps succeeding (pre-existing `generic_direct_provider_provision_succeeds_without_network` still green). |
| Typed probe strategy carrying what it may prove | `SetupProbeStrategy::credential_evidence()` → `CredentialEvidence` | pass | `ProviderCatalog` = `CatalogOnly`, `AuthenticatedCompatibleCatalog` = `Authenticated`. |
| Provisioning uses only the typed classification | `CredentialVerification::from_probe`; `map_provider_error_reason` uses `error_class()`, no string matching | pass | Typed 401/403 covered via `error_class() == "auth"`, not just the `Auth` variant. |
| Additive persistence for a distinct credential axis | migration v69 `ALTER TABLE provider_connection_health ADD COLUMN credential_status TEXT NOT NULL DEFAULT 'unverified' CHECK (...)` + index; `STORAGE_LAYOUT_VERSION` 68 → 69 | pass | No table rebuild; conservative default for historical rows. |
| Backward/forward-compatible wire shape | `ConnectionHealthDto.credential_status: Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]` | pass | Older clients unchanged. |
| Runtime feedback from a real authenticated request, revision-scoped | `src/core/provider_qualification.rs` `ProviderConnectionCredentialReporter`; `WHERE connection_id = ? AND revision = ?` | pass | Stale-revision verdicts match no row and are discarded. |
| Transient/retryable failure must not become auth-failed | `InferenceCredentialOutcome::Inconclusive` writes no SQL; refresh failure uses `CASE WHEN ? THEN ? ELSE credential_status END` | pass | Only `RefreshError::CredentialRejected` (typed 401/403) moves the axis. |
| Rotation/refresh do not fabricate or erase verdicts | rotation re-derives from the new probe; refresh preserves the axis unless it ran an authenticated probe | pass | Refresh cannot clear a durable `verified` or `authentication_failed`. |
| No provider-specific string matching in the agent loop | `src/agent/provider_qualification.rs` classifies from `AppError`/`ProviderError::error_class()` only | pass | No provider names, prefixes, or response-body sniffing. |
| No billable dummy completion to validate a credential | no new inference request exists on any provisioning path | pass | The only inference on the connection is a real user turn. |
| Operator messaging states the two axes separately | `connect_success_message` (`src/tui/components/dialogs/connect.rs`) + `command_dispatch.rs` call site | pass | `credential verified` / `no credential required` / `catalog loaded … credential not yet verified`. |
| Secret-free | no secret crosses the reporter boundary; credential material stays in the credential store | pass | Existing `storage_rows_contain` secret-leak assertions still green. |
| Focused guard fails if provisioning re-equates `models()` with verification | `scripts/check_provider_qualification.py` (4 checks + `--self-test`), wired into `scripts/verify.sh quick` | pass | Self-test confirms it detects the pre-M010 shape. |
| Migration is additive and safe on existing data | `legacy_health_rows_migrate_to_the_conservative_unverified_axis` | pass | Legacy row lands `unverified`; CHECK rejects out-of-contract values. |
| Docs match the new contract | `architecture/provider.md`, `architecture/storage.md`, `docs/providers.md`, `.opencode/skills/provider-auth/SKILL.md` | pass | Skill + architecture doc updated together per repo convention. |

## 3. Production implementation evidence

**New modules**

- `crates/codegg-providers/src/qualification.rs` — the typed qualification
  model and the single place a probe may yield a credential verdict.
- `src/agent/provider_qualification.rs` — terminal-turn credential
  observation (`InferenceCredentialOutcome`, `ProviderCredentialObserver`).
- `src/core/provider_qualification.rs` — the durable, revision-scoped writer.

**Provisioning (`src/core/eggpool.rs`)**

- `ProbeResult` carries a `credential: CredentialVerification`.
- `probe_provider_catalog` (renamed from `probe_direct_models`) always yields
  `Unverified` on success; a typed provider auth error still propagates as
  `ProbeReason::AuthenticationFailed` and fails provisioning.
- The strict compatible probe yields `Verified` — it is a genuinely
  authenticated, non-billable metadata request.
- `finalize`, rotate, and refresh write the credential axis; refresh failure
  moves it only on `RefreshError::CredentialRejected` (new variant, error code
  `credential_authentication_failed`, included in the backoff set so a bad key
  cannot cause unbounded refresh retries).

**Runtime seam**

`daemon_turns` now retains the selected connection from the lifecycle gate it
already performed, and installs a reporter for that exact revision into the
turn (`TurnRunInput` → `AgentLoopBuildInput` → `AgentLoop::set_credential_observer`).
`stream_with_retry` reports the terminal outcome via a thin wrapper around the
existing retry body; intermediate retryable failures are not reported, and
cancellation is inconclusive.

**Storage** — migration v69, additive, `STORAGE_LAYOUT_VERSION = 69`.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets -- -D warnings
cargo test -p codegg-providers --lib qualification
cargo test -p codegg-providers --lib setup_catalog
cargo test -p codegg --lib core::eggpool::tests
cargo test -p codegg --lib provider_qualification
cargo test -p codegg --lib connect_message_tests
python3 scripts/check_provider_qualification.py --verbose
python3 scripts/check_provider_qualification.py --self-test
python3 scripts/check_project_catalog_invariants.py
bash scripts/verify.sh quick
```

### Results

All local; no CI run was triggered by this closure.

| Check | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass (exit 0) |
| `codegg-providers` qualification unit tests | 8 passed |
| `codegg-providers` setup-catalog tests | 15 passed (incl. 2 new catalog-wide qualification guards) |
| `codegg` provisioning tests (`core::eggpool::tests`) | 28 passed (incl. 5 new M010 tests) |
| `codegg` inference-outcome classification tests | 3 passed |
| TUI connect message tests | 4 passed |
| `check_provider_qualification.py` | 4/4 pass; `--self-test` ok |
| `check_project_catalog_invariants.py` | 7/7 pass (layout marker tracks v69) |
| `verify.sh quick` | pass |

## 5. Invariant review

- Catalog reachability is never reported as credential validity. The
  `CatalogOutcome` type has no authentication variant, so the conflation is
  not expressible.
- A credential is marked rejected only by a typed authentication error.
  Transport, timeout, rate-limit, circuit, cancellation, and storage failures
  are all inconclusive and write nothing.
- Verdicts are scoped to the connection revision that produced them; a
  rotation invalidates them by construction.
- No provider is billed to validate a credential — no inference request exists
  on any provisioning or refresh path.
- No secret reaches the reporter boundary, the durable credential axis, or any
  operator-visible message.
- Existing lifecycle invariants are untouched: provisioning still fails closed
  without a usable catalog, rotation still commits one transaction, and
  refresh remains single-flight and bounded.

## 6. Failure and recovery review

- Credential-status write failures are logged and do not fail a turn: the
  reporter is health telemetry, never durable session state.
- An inconclusive outcome is a no-op by construction, so a network blip cannot
  erase a verdict.
- A verdict that arrives after rotation is discarded (zero rows affected) and
  logged at debug.
- Migration is additive and idempotent (`add_column_ignore_duplicate`), so a
  partially-applied v69 is safe to re-run.

## 7. Migration and compatibility review

- Additive `ALTER TABLE ... ADD COLUMN` only. No table rebuild, no data
  rewrite, no historical row promoted. `status IN ('healthy','unhealthy')` is
  unchanged, so existing health queries and indexes keep working.
- Conservative mapping: every pre-existing row becomes `unverified`, because
  no v68 column recorded authenticated evidence. Nothing is globally degraded
  — `status` (health) is untouched, only the new axis is introduced.
- `ConnectionHealthDto` gained an optional field with `serde(default)`, so old
  and new peers interoperate in both directions.

## 8. Security review

- No secret material is read, copied, or logged by any new code path. The
  reporter receives only a connection id and a revision.
- The credential verdict is derived exclusively from the typed provider error
  class; no response body, header, or provider-specific string is inspected.
- The TUI message includes only the display name, endpoint, model count, and a
  credential-status keyword.

## 9. Documentation and operations

- `architecture/provider.md` — new "Connection Qualification Semantics (M010)"
  section with the typed model, storage contract, revision-safety rules, and
  per-path write policy.
- `architecture/storage.md` — layout version 69 and the v69 migration note.
- `docs/providers.md` — user-facing explanation of what `/connect` does and
  does not prove, with the three toast variants.
- `.opencode/skills/provider-auth/SKILL.md` — ownership map row plus hard
  rules 7 and 8, guard and test commands.
- New guard `scripts/check_provider_qualification.py` in `verify.sh quick`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The plan's original `CatalogOutcome::available(models)` shape carried the model list; the landed `Available` variant carries `model_count`/`revision`/`duration_ms` instead | none | Deliberate: the bounded model list is persisted to `provider_connection_models` at commit and the summary already counts rows, so keeping models out of the outcome type avoided a second ownership of the catalog. Recorded here rather than silently. |
| low | Pre-existing connections created through the authenticated compatible probe are `unverified` until their next authenticated probe or inference | Cosmetic only | Accepted conservative mapping; nothing is degraded and the first real request promotes them. |
| low | Hosted CI has not been observed for this commit | Evidence gap | Run `CI / verify` on the pushed branch and record the run id here if it fails. |

No critical, high, or medium findings remain.

## 11. Roadmap disposition

Milestone closed; M011 is now eligible for a dependency audit.

## 12. Registry updates

- `plans/registry.md`: M010 `ready` → `closed`; M011 remains `blocked` with a
  narrowed blocker (its CodeGG M010 dependency is now discharged; the EggPool
  shared-provider-profile M001 accepted closure revision is still outstanding).
- Roadmap milestone table: M010 → `closed` with this closure record.
- New follow-up work created by this closure: none.