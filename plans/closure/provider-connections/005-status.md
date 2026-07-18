# Provider Connections Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connections/005-corrective-lifecycle-rotation.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connections-roadmap.md#milestone-5--corrective-lifecycle-rotation-health-and-closure`

Repository baseline reviewed: `b0e8c13` (`main`; the plan's recorded
`213272c` baseline was stale because subsequent storage work had already
landed; the implementation uses additive storage migration v31).

Implementation commits or pull requests:

- `0eadc85` — implement the provider-connection lifecycle corrective pass,
  including lifecycle storage, rotation/refresh coordination, protocol and
  TUI surfaces, guards, documentation, and focused tests.

## 1. Executive finding

The corrective capability is complete and the Provider Connections and
Eggpool roadmap can close. The implementation preserves daemon ownership and
secret references while adding atomic staged rotation, revision-safe cache
invalidation, bounded/coalesced refresh, explicit lifecycle/tombstone/purge
semantics, selected-session lifecycle projection, typed turn-submit failures,
local-only masked rotation input, and deterministic lifecycle coverage.

The original Milestone 004 plan is superseded by this executed corrective
pass. Team authorization and cross-node secret distribution remain explicitly
deferred scope, not closure blockers.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Seven-state lifecycle and all 49 transitions | `lifecycle_transition_matrix_covers_all_49_pairs`; `cargo test -p codegg-core provider_connections` | pass | Idempotent transitions and revision checks are covered. |
| Tombstone, restore, reference blockers, and purge | Core store tests; migration v31; `check_provider_connections_tombstone_compat.sh` | pass | Selected-session, provisioning-operation, and active-runtime blockers are typed. |
| Atomic staged credential/endpoint rotation | `rotation_commits_new_revision_and_removes_only_previous_credential`; invalid-endpoint rollback assertion | pass | Staged credential is removed on pre-commit failure; old binding is removed only after commit. |
| Runtime revision invalidation and in-flight pinning seam | `ConnectionManager::invalidate_revision`, `resolve_with_runtime_reference`, RAII runtime lease, manager cache tests | pass | Compatibility `resolve` remains available; new lifecycle-aware callers use the lease-aware seam. |
| Refresh success, unchanged catalog, single-flight, cancellation/backoff seams | `refresh_coalesces_and_preserves_revision_for_unchanged_catalog`; `RefreshError`; bounded semaphore, cancellation, backoff, and background loop code | pass | Unchanged normalized catalogs retain the connection revision and last-good rows. |
| Session lifecycle projection and no silent fallback | `tests/session_selection.rs`; daemon `SessionLifecycleGet` and `TurnSubmit` state gate | pass | Disabled/tombstoned/missing states remain explicit. |
| Additive protocol and remote secret denial | Protocol tests; WS guard; provider lifecycle CoreClient harness | pass | Rotate secret staging/begin is local-only and remote WebSocket denied. |
| `/connections` lifecycle controls and masked input | TUI connection-selection actions, lifecycle command runner, connect rotation flow, full workspace tests | pass | The list surface renders redacted status; actions are routed through CoreClient. |
| Deterministic fake-daemon lifecycle flow | `tests/provider_connections_lifecycle.rs` | pass | Secret stage → rotate → disable → delete → restore → purge ordering is asserted. |
| Formerly flaky provisioning test | 50 exact iterations with `--test-threads=14`, all passing | pass | Fake listener binds before address publication, uses yield-only bounded readiness, and accepted streams are blocking. |
| Rotation and refresh repeat stability | 50 exact iterations of each new rotation and refresh test, all passing | pass | Local fake Eggpool servers; no live service dependency. |
| Migration and compatibility | `tests/storage_migrations.rs`; `STORAGE_LAYOUT_VERSION = 31`; additive v31 schema | pass | Existing provider rows remain compatibility projections; legacy resolution remains explicit. |
| Documentation and static guards | Architecture updates; both provider guards; core boundary, ownership, cwd, discovery, catalog, scheduler, and git guards | pass | No new secret backend or authorization inference was introduced. |

## 3. Production implementation evidence

- `codegg-core` owns the seven-state lifecycle, revision-safe transitions,
  tombstones, reference records, audit records, purge blockers, and runtime
  leases. Metadata remains credential-free.
- `EggpoolProvisioner` owns staged credential rotation, bounded probes,
  cancellation/status maps, single-flight refresh, global caps, deterministic
  backoff/jitter, last-good catalog preservation, provisioning reconciliation,
  and exact credential cleanup.
- `codegg-protocol` adds redacted lifecycle, rotation, refresh, purge, and
  session-projection DTOs and requests. `src/server/ws.rs` denies remote
  secret-bearing operations.
- `CoreDaemon` routes lifecycle operations, starts optional background refresh
  only when configured, invalidates provider revisions after commit, and
  rejects unusable selected connections before turn execution.
- The TUI adds lifecycle actions, confirmations, purge controls, refresh and
  masked local-only rotation input. Secret bytes are not retained in ordinary
  dialog state or snapshots.
- Architecture documents and provider static guards describe the resulting
  ownership, migration, redaction, and recovery contracts.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test -p codegg-core provider_connections
rtk cargo test -p codegg-protocol
rtk cargo test --test session_selection
rtk cargo test --test storage_migrations
rtk cargo test --test provider_connections_lifecycle
rtk cargo test -p codegg --lib core::eggpool -- --test-threads=1
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk cargo test --workspace --all-features -- --test-threads=14
rtk bash scripts/check_provider_connections_m4_coverage.sh
rtk bash scripts/check_provider_connections_tombstone_compat.sh
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_discovery_invariants.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk python3 scripts/check_scheduler_bypass.py
rtk python3 scripts/check_git_forbidden_patterns.py
rtk git diff --check
```

The required repeat commands were also run with `CARGO_BUILD_JOBS=1`:

```bash
# 50 iterations: successful_provision_persists_redacted_connection_and_catalog
# 50 iterations: rotation_commits_new_revision_and_removes_only_previous_credential
# 50 iterations: refresh_coalesces_and_preserves_revision_for_unchanged_catalog
```

### Results

- Workspace check: pass.
- Workspace clippy: pass after correcting three unrelated pre-existing
  test-only clippy findings in `crates/codegg-core/tests/project_catalog.rs`.
- Full workspace all-features suite: **8,008 passed, 10 ignored, 0 failed**
  across 112 suites.
- Focused provider/lifecycle suites and all three 50-iteration loops: pass.
- Provider lifecycle and tombstone static guards: pass.
- Core-boundary, cwd, ownership, discovery, project-catalog, scheduler, git
  policy, formatting, and diff checks: pass.
- The first sandboxed Eggpool attempt was environment-blocked because
  localhost binding is forbidden there; the same suite was rerun with the
  required local-server permission and passed.

## 5. Invariant review

- Plaintext credentials are confined to `SecretInput`/`SecretInputRef`
  staging and credential-store probe operations; metadata, DTOs, TUI state,
  audit rows, and errors contain only references or redacted diagnostics.
- Connection IDs remain distinct from provider implementation IDs, and scope
  metadata is not treated as authorization.
- In-flight providers retain their captured `Arc` and revision. New
  resolutions use committed revisions after exact cache invalidation.
- Rotation, refresh, and lifecycle failures preserve the prior active
  revision/catalog unless the operator explicitly changes lifecycle state.
- Disable, tombstone, and missing credentials never silently select a fallback
  connection or account; turn submission returns a typed connection-state
  error.
- Probes and background refresh are bounded, cancellable, capped, and opt-in;
  daemon startup does not perform provider I/O.
- Secret-bearing remote operations are rejected before execution, and audit
  metadata excludes secret material.

## 6. Failure and recovery review

- Duplicate provisioning remains conflict/idempotency guarded; staged
  provisioning references are removed on completion and reconciled after
  restart.
- Rotation uses expected revisions, staged bindings, transaction commit, and
  post-commit old-binding cleanup. Invalid endpoint and probe failures leave
  the old revision intact.
- Refresh coalesces concurrent callers, propagates cancellation, applies
  bounded backoff, and keeps last-good catalog rows on failure.
- Tombstone and restore are revisioned and restart-safe; purge enumerates
  selected-session, provisioning-operation, and active-runtime blockers.
- Malformed endpoints, oversized/invalid provider responses, remote secret
  carriage, and unusable lifecycle states are rejected with bounded errors.

## 7. Migration and compatibility review

Migration v31 is additive and creates lifecycle, references, tombstones, and
audit tables without rewriting the historical provider connection schema.
`STORAGE_LAYOUT_VERSION` is 31 and migration tests pass. Existing legacy
provider/model configuration remains readable through `LegacyResolution`;
this pass records removal criteria but does not perform an ambiguous automatic
migration. Protocol changes are additive and redacted.

## 8. Security review

Credential material never enters SQLite, protocol DTOs, TUI snapshots, audit
metadata, or diagnostic strings. Rotation cleanup is scoped to the exact prior
provider/account binding. Endpoint validation rejects userinfo, query,
fragment, invalid schemes, and TLS-policy mismatches. Remote WebSocket secret
operations are denied. Team/project authorization remains an explicit future
seam and is not inferred from scope labels.

## 9. Documentation and operations

Updated: `architecture/auth.md`, `core.md`, `protocol.md`, `provider.md`,
`session.md`, `storage.md`, and `tui.md`; `AGENTS.md` now lists the provider
guards. Operators have explicit connect → rotate → refresh → disable →
delete → restore → purge paths, typed status/error responses, and reference
blocker diagnostics. The two provider static guards are intended for CI.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Existing compatibility callers of `ConnectionManager::resolve` do not automatically acquire a runtime lease; new lifecycle-aware callers must use `resolve_with_runtime_reference`. | A future provider-runtime call site could bypass active-runtime purge blocking if it uses the compatibility method. | Route future durable-connection provider invocation through the lease-aware method; add a call-site guard when that runtime path is wired. |
| low | Role-based team/project authorization and cross-node secret distribution remain out of scope. | Scope labels do not independently authorize access. | Address in the later identity/authorization and distributed-runtime plans. |
| low | Built-in-agent generated-output drift (`general` prompt) and 758 bare Tokio test annotations remain pre-existing repository guard findings. | Unrelated CI hygiene debt; full tests and clippy are green. | Resolve in the agent-assets/Tokio-flavor maintenance work; not part of provider lifecycle semantics. |

## 11. Roadmap disposition

Milestone 005 is **closed** and closes the Provider Connections and Eggpool
subsystem roadmap. The original Milestone 004 plan is superseded; its
corrective-pass closure record remains as historical evidence of why Milestone
005 was created. No provider-connections downstream plan is blocked on this
milestone, and no unrelated plan is unblocked by it.

The existing blocked work remains unchanged:

- Multi-Project TUI 001 remains blocked on Project Catalog 004.
- Session Projections 001 remains blocked on Project Catalog 004 and
  Multi-Project TUI 001.

Runtime Assets 004 and Project Catalog 003 remain independently dependency-
ready; this closure does not alter their status.

## 12. Registry updates

- Mark source plan 005 `closed` and link this closure record.
- Mark source plan 004 `superseded by 005`; retain its historical closure
  record unchanged.
- Mark roadmap Milestone 4 `superseded` and Milestone 5 `closed`; set the
  provider-connections roadmap status to `closed`.
- Remove provider-connections 004/005 from the dependency-ready and active
  closure tables in `plans/registry.md`.
- Add provider-connections 005 to recently closed work with the closure
  commit, and leave the two unrelated blocked rows unchanged.
